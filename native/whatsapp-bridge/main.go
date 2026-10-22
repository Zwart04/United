// United native WhatsApp bridge.
//
// This is deliberately small: it links a WhatsApp companion device, writes new
// message previews to an append-only JSONL file, and exposes pairing state plus
// a QR PNG for the WPF host. It never stores or sends message bodies itself.
package main

import (
	"context"
	"encoding/json"
	"flag"
	"fmt"
	"os"
	"os/signal"
	"path/filepath"
	"strings"
	"sync"
	"syscall"
	"time"

	"image"
	"image/draw"
	"image/png"

	"github.com/skip2/go-qrcode"
	"go.mau.fi/whatsmeow"
	"go.mau.fi/whatsmeow/store/sqlstore"
	"go.mau.fi/whatsmeow/types"
	"go.mau.fi/whatsmeow/types/events"
	waLog "go.mau.fi/whatsmeow/util/log"
	"golang.org/x/sys/windows"

	_ "github.com/mattn/go-sqlite3"
)

type messageEvent struct {
	Type   string `json:"type"`
	Title  string `json:"title"`
	Body   string `json:"body"`
	At     string `json:"at"`
	Chat   string `json:"chat,omitempty"`
	Sender string `json:"sender,omitempty"`
}

type statusEvent struct {
	State     string `json:"state"`
	Paired    bool   `json:"paired"`
	UpdatedAt string `json:"updatedAt"`
	Error     string `json:"error,omitempty"`
	Me        string `json:"me,omitempty"`
}

type bridge struct {
	mu         sync.Mutex
	dataDir    string
	eventsPath string
	statusPath string
	qrPath     string
	status     statusEvent
	groups     map[string]string
}

func main() {
	dataDir := flag.String("data-dir", "", "directory for the WhatsApp session and bridge files")
	parentPID := flag.Int("parent-pid", 0, "exit when this process exits")
	flag.Parse()

	if *dataDir == "" {
		home, _ := os.UserHomeDir()
		*dataDir = filepath.Join(home, "AppData", "Local", "United", "WhatsAppNative")
	}
	if err := os.MkdirAll(*dataDir, 0o700); err != nil {
		fmt.Fprintf(os.Stderr, "create data dir: %v\n", err)
		os.Exit(1)
	}

	b := &bridge{
		dataDir:    *dataDir,
		eventsPath: filepath.Join(*dataDir, "events.jsonl"),
		statusPath: filepath.Join(*dataDir, "status.json"),
		qrPath:     filepath.Join(*dataDir, "qr.png"),
		groups:     make(map[string]string),
	}

	// Old events must not be replayed into a fresh host process.
	_ = os.WriteFile(b.eventsPath, nil, 0o600)
	_ = os.Remove(b.qrPath)
	b.writeStatus("starting", false, "")

	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	go b.watchParent(*parentPID, cancel)
	go func() {
		ch := make(chan os.Signal, 1)
		signal.Notify(ch, os.Interrupt, syscall.SIGTERM)
		<-ch
		cancel()
	}()

	container, err := sqlstore.New(ctx, "sqlite3",
		"file:"+filepath.Join(*dataDir, "session.db")+"?_foreign_keys=on&_busy_timeout=5000",
		waLog.Stdout("WA-DB", "WARN", true))
	if err != nil {
		b.writeStatus("error", false, "open session: "+err.Error())
		fmt.Fprintf(os.Stderr, "open session: %v\n", err)
		os.Exit(1)
	}
	defer container.Close()

	device, err := container.GetFirstDevice(ctx)
	if err != nil {
		b.writeStatus("error", false, "load device: "+err.Error())
		fmt.Fprintf(os.Stderr, "load device: %v\n", err)
		os.Exit(1)
	}

	client := whatsmeow.NewClient(device, waLog.Stdout("WA", "WARN", true))
	client.EnableAutoReconnect = true
	client.AddEventHandler(b.handleEvent(client))

	if client.Store.ID == nil {
		qrChan, err := client.GetQRChannel(ctx)
		if err != nil {
			b.writeStatus("error", false, "QR channel: "+err.Error())
			fmt.Fprintf(os.Stderr, "QR channel: %v\n", err)
			os.Exit(1)
		}
		if err := client.Connect(); err != nil {
			b.writeStatus("error", false, "connect: "+err.Error())
			fmt.Fprintf(os.Stderr, "connect: %v\n", err)
			os.Exit(1)
		}
		go b.consumeQR(ctx, qrChan)
	} else {
		b.writeStatus("starting", true, "")
		if err := client.Connect(); err != nil {
			b.writeStatus("error", true, "connect: "+err.Error())
			fmt.Fprintf(os.Stderr, "connect: %v\n", err)
			// Keep the process alive; whatsmeow's reconnect loop may recover.
		}
	}

	<-ctx.Done()
	client.Disconnect()
	b.writeStatus("stopped", client.Store.ID != nil, "")
}

func (b *bridge) consumeQR(ctx context.Context, ch <-chan whatsmeow.QRChannelItem) {
	for item := range ch {
		switch item.Event {
		case whatsmeow.QRChannelEventCode:
			if err := writeQRCode(item.Code, b.qrPath); err != nil {
				b.writeStatus("error", false, "write QR: "+err.Error())
				continue
			}
			b.writeStatus("pairing", false, "")
		case whatsmeow.QRChannelEventError:
			b.writeStatus("error", false, item.Error.Error())
		case whatsmeow.QRChannelSuccess.Event:
			_ = os.Remove(b.qrPath)
			b.writeStatus("connected", true, "")
		case whatsmeow.QRChannelTimeout.Event:
			b.writeStatus("pairing_timeout", false, "")
		default:
			// Passkey and other pairing states are not needed by this host.
		}
		select {
		case <-ctx.Done():
			return
		default:
		}
	}
}

func (b *bridge) handleEvent(client *whatsmeow.Client) func(any) {
	return func(raw any) {
		switch evt := raw.(type) {
		case *events.Connected:
			_ = os.Remove(b.qrPath)
			me := ""
			if client.Store.ID != nil {
				me = client.Store.ID.User
			}
			b.writeStatus("connected", client.Store.ID != nil, "", me)
		case *events.Disconnected:
			b.writeStatus("disconnected", client.Store.ID != nil, "")
		case *events.LoggedOut:
			_ = os.Remove(b.qrPath)
			b.writeStatus("logged_out", false, "unlinked from phone")
		case *events.PairSuccess:
			b.writeStatus("connected", true, "", evt.ID.User)
		case *events.GroupInfo:
			if evt.Name != nil && !evt.JID.IsEmpty() {
				b.mu.Lock()
				b.groups[evt.JID.String()] = evt.Name.Name
				b.mu.Unlock()
			}
		case *events.Message:
			b.handleMessage(client, evt)
		case *events.UndecryptableMessage:
			// A later retry may still produce a normal Message event.
		case *events.StreamReplaced:
			b.writeStatus("disconnected", true, "another client connected with this session")
		}
	}
}

func (b *bridge) handleMessage(client *whatsmeow.Client, evt *events.Message) {
	info := evt.Info
	if info.IsFromMe || info.Chat.IsEmpty() {
		return
	}
	if evt.IsEdit || info.Type == "reaction" || info.Type == "poll" {
		return
	}
	if info.Chat.User == "status" || info.Chat.Server == types.BroadcastServer {
		return
	}

	body := extractText(evt)
	if strings.TrimSpace(body) == "" {
		return
	}
	if len(body) > 360 {
		body = body[:359] + "..."
	}

	title := strings.TrimSpace(info.PushName)
	if info.IsGroup {
		title = b.groupName(client, info.Chat)
		if title == "" {
			title = info.Chat.User
		}
		if title == "" {
			title = "WhatsApp group"
		}
	} else if title == "" {
		title = info.Chat.User
	}
	if title == "" {
		title = "WhatsApp"
	}
	if len(title) > 140 {
		title = title[:139] + "..."
	}

	sender := info.Sender.User
	at := info.Timestamp
	if at.IsZero() {
		at = time.Now()
	}
	b.append(messageEvent{
		Type:   "message",
		Title:  title,
		Body:   body,
		At:     at.Format(time.RFC3339Nano),
		Chat:   info.Chat.String(),
		Sender: sender,
	})
}

func (b *bridge) groupName(client *whatsmeow.Client, jid types.JID) string {
	key := jid.String()
	b.mu.Lock()
	name := b.groups[key]
	b.mu.Unlock()
	if name != "" {
		return name
	}

	ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
	defer cancel()
	info, err := client.GetGroupInfo(ctx, jid)
	if err != nil || info == nil || strings.TrimSpace(info.Name) == "" {
		return ""
	}
	name = strings.TrimSpace(info.Name)
	b.mu.Lock()
	b.groups[key] = name
	b.mu.Unlock()
	return name
}

func extractText(evt *events.Message) string {
	msg := evt.Message
	if msg == nil {
		return ""
	}
	if text := msg.GetConversation(); text != "" {
		return text
	}
	if ext := msg.GetExtendedTextMessage(); ext != nil && ext.GetText() != "" {
		return ext.GetText()
	}
	if img := msg.GetImageMessage(); img != nil {
		if img.GetCaption() != "" {
			return img.GetCaption()
		}
		return "[Photo]"
	}
	if video := msg.GetVideoMessage(); video != nil {
		if video.GetCaption() != "" {
			return video.GetCaption()
		}
		return "[Video]"
	}
	if doc := msg.GetDocumentMessage(); doc != nil {
		if doc.GetCaption() != "" {
			return doc.GetCaption()
		}
		return "[Document]"
	}
	if msg.GetAudioMessage() != nil {
		return "[Voice message]"
	}
	if msg.GetStickerMessage() != nil {
		return "[Sticker]"
	}
	if msg.GetLocationMessage() != nil {
		return "[Location]"
	}
	if msg.GetContactMessage() != nil || msg.GetContactsArrayMessage() != nil {
		return "[Contact]"
	}
	if msg.GetPollCreationMessage() != nil || msg.GetPollCreationMessageV2() != nil || msg.GetPollCreationMessageV3() != nil {
		return "[Poll]"
	}
	if msg.GetProtocolMessage() != nil {
		return ""
	}
	return ""
}

// writeQRCode writes a plain RGBA PNG. go-qrcode's default output is 1bpp
// indexed, which WPF's BitmapImage decoder rejects in some Windows builds.
func writeQRCode(code, path string) error {
	qr, err := qrcode.New(code, qrcode.Medium)
	if err != nil {
		return err
	}
	src := qr.Image(420)
	dst := image.NewRGBA(src.Bounds())
	draw.Draw(dst, dst.Bounds(), src, image.Point{}, draw.Src)

	f, err := os.Create(path)
	if err != nil {
		return err
	}
	if err := png.Encode(f, dst); err != nil {
		_ = f.Close()
		return err
	}
	return f.Close()
}

func (b *bridge) append(evt messageEvent) {
	line, err := json.Marshal(evt)
	if err != nil {
		return
	}
	b.mu.Lock()
	defer b.mu.Unlock()
	f, err := os.OpenFile(b.eventsPath, os.O_CREATE|os.O_WRONLY|os.O_APPEND, 0o600)
	if err != nil {
		return
	}
	defer f.Close()
	_, _ = f.Write(append(line, '\n'))
}

func (b *bridge) writeStatus(state string, paired bool, errText string, me ...string) {
	b.mu.Lock()
	b.status = statusEvent{
		State:     state,
		Paired:    paired,
		UpdatedAt: time.Now().Format(time.RFC3339),
		Error:     errText,
	}
	if len(me) > 0 {
		b.status.Me = me[0]
	}
	payload, _ := json.MarshalIndent(b.status, "", "  ")
	b.mu.Unlock()

	tmp := b.statusPath + ".tmp"
	if os.WriteFile(tmp, payload, 0o600) == nil {
		_ = os.Rename(tmp, b.statusPath)
	}
}

func (b *bridge) watchParent(pid int, cancel context.CancelFunc) {
	if pid <= 0 {
		return
	}
	h, err := windows.OpenProcess(windows.SYNCHRONIZE, false, uint32(pid))
	if err != nil {
		return
	}
	defer windows.CloseHandle(h)
	for {
		event, err := windows.WaitForSingleObject(h, 5000)
		if err != nil || event == windows.WAIT_OBJECT_0 {
			cancel()
			return
		}
	}
}
