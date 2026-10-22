# United native WhatsApp bridge

Small Go helper that links a WhatsApp companion device with
[`whatsmeow`](https://github.com/tulir/whatsmeow), writes new message previews
to `events.jsonl`, and exposes QR/status files for the WPF host.

The host only uses this helper for **background previews** while the WhatsApp
WebView is hibernated. Opening a chat wakes the normal WebView, where reading
and replying still happen on the official WhatsApp Web UI.

## Build

Requirements: Go 1.24+ and a C compiler (the SQLite driver uses CGO).

```powershell
.\tools\build-native.ps1
```

Or manually:

```powershell
cd native\whatsapp-bridge
$env:CGO_ENABLED = '1'
go build -trimpath -ldflags "-s -w" -o United.WhatsAppBridge.exe .
```

The resulting `United.WhatsAppBridge.exe` is copied by the app project when it
exists and is included in the release zip. The binary itself is ignored by git.

## Runtime files

All files live in `%LOCALAPPDATA%\United\WhatsAppNative`:

| File | Purpose |
|---|---|
| `session.db` | whatsmeow companion-device session |
| `events.jsonl` | append-only message previews consumed by United |
| `status.json` | pairing/connection state |
| `qr.png` | current pairing QR (RGBA PNG for WPF) |
| `bridge.pid` | helper pid, used to clean up after a crash |
| `bridge.log` | helper stdout/stderr |

The helper exits automatically when its parent United process exits.

## Risk

`whatsmeow` is an unofficial WhatsApp client library. Linking a device this way
is not endorsed by WhatsApp and can lead to account restrictions. Use it only
with an account you are willing to link this way. The normal WhatsApp WebView
remains available and is used whenever the bridge is missing, unpaired, or
unavailable.
