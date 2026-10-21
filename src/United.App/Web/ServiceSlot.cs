using System.Diagnostics;
using System.IO;
using System.Text;
using System.Text.Json;
using System.Windows;
using System.Windows.Controls;
using Microsoft.Web.WebView2.Core;
using Microsoft.Web.WebView2.Wpf;
using United.App.Diagnostics;
using United.Core.Lifecycle;
using United.Core.Services;

namespace United.App.Web;

/// <summary>
/// One hosted messenger webview: a WPF WebView2 control created against the shared
/// CoreWebView2Environment with a dedicated profile (isolated cookies/sessions inside
/// one shared browser process). Owns its lifecycle state and memory policy application.
/// </summary>
public sealed class ServiceSlot : IDisposable
{
    private static readonly object EnvLock = new();
    private static Task<CoreWebView2Environment>? _envTask;

    private readonly Grid _host;
    private WebView2? _wv;
    private bool _disposedFully;
    private bool _shown;
    private Thickness _paneInsets;
    private bool _inboxFocus;
    private string? _inboxFocusCss;
    private bool _darkTheme;

    public ServiceDefinition Def { get; }
    public ServiceState State { get; private set; } = ServiceState.NotCreated;
    public string LastTitle { get; private set; } = "";
    public string? LastError { get; private set; }

    public event Action<ServiceSlot>? StateChanged;
    public event Action<ServiceSlot, string>? TitleChanged;
    public event Action<ServiceSlot, bool>? NavigatingChanged;
    public event Action<ServiceSlot, string>? Failed;
    public event Action<ServiceSlot, string>? PopupRequested;
    public event Action<ServiceSlot>? RecreateRequested;

    /// <summary>Set by the app; returns the notification event args when a web notification fired.</summary>
    public event Action<ServiceSlot, CoreWebView2NotificationReceivedEventArgs>? WebNotification;
    /// <summary>Live chat-list changes, observed after the initial page snapshot.</summary>
    public event Action<ServiceSlot, string, string, DateTimeOffset, string?, int?>? LiveActivityObserved;

    /// <summary>Exact unread count read from the service's own in-page badges.</summary>
    public event Action<ServiceSlot, int>? UnreadCountObserved;

    public ServiceSlot(ServiceDefinition def, Grid host)
    {
        Def = def;
        _host = host;
    }

    public bool IsLive => _wv?.CoreWebView2 != null;
    public string Id => Def.Id;
    public int BrowserProcessId
    {
        get
        {
            var c = _wv?.CoreWebView2;
            return c != null ? (int)c.BrowserProcessId : 0;
        }
    }

    /// <summary>The single shared environment: one user data folder => one browser process tree.</summary>
    public static Task<CoreWebView2Environment> GetEnvironmentAsync()
    {
        lock (EnvLock)
        {
            _envTask ??= CreateEnvironmentAsync();
            return _envTask;
        }
    }

    public static string UserDataFolder =>
        Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "United", "WebData");

    private static async Task<CoreWebView2Environment> CreateEnvironmentAsync()
    {
        Directory.CreateDirectory(UserDataFolder);
        // Evergreen runtime, no bundled browser. Keep hidden and minimised service
        // renderers on their normal timer cadence so chat-list observers and the
        // services' own notification callbacks keep firing while another service is
        // in front. None of these flags expose remote debugging.
        var options = new CoreWebView2EnvironmentOptions
        {
            AdditionalBrowserArguments =
                "--disable-background-timer-throttling " +
                "--disable-backgrounding-occluded-windows " +
                "--disable-renderer-backgrounding " +
                "--disable-features=CalculateNativeWinOcclusion",
        };
        var env = await CoreWebView2Environment.CreateAsync(
            browserExecutableFolder: null, userDataFolder: UserDataFolder, options);
        AppLog.Info($"WebView2 environment created (UDF: {UserDataFolder})");
        return env;
    }

    public async Task CreateAsync()
    {
        if (_wv != null || _disposedFully) return;
        SetState(ServiceState.Starting);

        var wv = new WebView2
        {
            HorizontalAlignment = HorizontalAlignment.Stretch,
            VerticalAlignment = VerticalAlignment.Stretch,
            Margin = _shown || !Def.KeepRenderingWhenHidden ? _paneInsets : ParkedInsets,
            // Only the active slot is shown. A collapsed WebView2 keeps sockets and service
            // workers alive, but some services stop producing DOM until they are rendered —
            // those are kept visible and parked outside the pane instead (Def.KeepRenderingWhenHidden).
            Visibility = _shown || Def.KeepRenderingWhenHidden ? Visibility.Visible : Visibility.Collapsed,
        };
        _host.Children.Add(wv);

        try
        {
            var env = await GetEnvironmentAsync();
            var options = env.CreateCoreWebView2ControllerOptions();
            options.ProfileName = SanitizeProfileName(Def.Id); // per-service isolation

            await wv.EnsureCoreWebView2Async(env, options);
            if (_disposedFully)
            {
                wv.Dispose();
                return;
            }

            _wv = wv;
            Configure(wv.CoreWebView2);
            ApplyTheme(_darkTheme);
            wv.CoreWebView2.Navigate(Def.Url);
            LastError = null;
            SetState(ServiceState.Ready);
        }
        catch (Exception ex)
        {
            AppLog.Exception(ex, $"creating webview for {Def.Id}");
            _host.Children.Remove(wv);
            try { wv.Dispose(); } catch { /* best effort */ }
            _wv = null;
            LastError = ex.Message;
            SetState(ServiceState.Failed);
            Failed?.Invoke(this, ex.Message);
        }
    }

    private static string SanitizeProfileName(string id)
    {
        // Profile names become folder names; keep them conservative.
        var chars = id.Where(char.IsLetterOrDigit).Select(char.ToLowerInvariant).ToArray();
        return chars.Length > 0 ? new string(chars) : "profile";
    }

    private void Configure(CoreWebView2 c)
    {
        var s = c.Settings;
        s.IsStatusBarEnabled = false;
        s.AreDevToolsEnabled = Debugger.IsAttached; // diagnostics only while debugging
        s.IsGeneralAutofillEnabled = false;         // no credential/autofill storage in our profiles
        s.IsPasswordAutosaveEnabled = false;
        s.AreBrowserAcceleratorKeysEnabled = true;  // F5 / Ctrl+R / zoom stay native
        s.IsBuiltInErrorPageEnabled = true;
        if (!string.IsNullOrWhiteSpace(Def.UserAgent))
            s.UserAgent = Def.UserAgent!;

        c.DocumentTitleChanged += (_, _) =>
        {
            LastTitle = c.DocumentTitle ?? "";
            TitleChanged?.Invoke(this, LastTitle);
        };

        c.NavigationStarting += (_, e) =>
        {
            NavigatingChanged?.Invoke(this, true);
            try
            {
                if (IsBlockedNavigation(e.Uri))
                {
                    e.Cancel = true;
                    NavigatingChanged?.Invoke(this, false);
                }
            }
            catch (Exception ex)
            {
                AppLog.Exception(ex, $"navigation hook {Def.Id}");
            }
        };

        c.NavigationCompleted += (_, e) =>
        {
            NavigatingChanged?.Invoke(this, false);
            if (!e.IsSuccess)
            {
                LastError = e.WebErrorStatus.ToString();
                // Keep the state Ready for transient failures (offline blips) — the built-in
                // error page is shown and the user can retry; only hard creation failures
                // move to Failed.
            }

            if (_inboxFocus) _ = ApplyInboxFocusAsync();
            if (e.IsSuccess) _ = InstallLiveActivityObserverAsync(c);
        };

        c.WebMessageReceived += (_, e) => OnWebMessage(c, e);

        c.NewWindowRequested += (_, e) =>
        {
            try
            {
                if (IsExternal(Def, e.Uri) && !IsAuthHost(e.Uri))
                {
                    e.Handled = true;
                    OpenInSystemBrowser(e.Uri);
                    return;
                }

                // In-scope popups (OAuth / secondary flows) open in a lightweight popup
                // window that shares this service's profile (same cookies/session).
                e.Handled = true;
                PopupRequested?.Invoke(this, e.Uri);
            }
            catch (Exception ex)
            {
                AppLog.Exception(ex, $"new window hook {Def.Id}");
            }
        };

        c.PermissionRequested += (_, e) => PermissionBroker.Handle(Def, e);

        c.NotificationReceived += (_, e) =>
        {
            AppLog.Info($"WebView2 NotificationReceived fired [{Def.Id}]");
            WebNotification?.Invoke(this, e);
        };

        c.ProcessFailed += (_, e) =>
        {
            AppLog.Warn($"process failed [{Def.Id}] kind={e.ProcessFailedKind} reason={e.Reason}");
            if (e.ProcessFailedKind == CoreWebView2ProcessFailedKind.BrowserProcessExited)
            {
                SetState(ServiceState.Failed);
                LastError = "browser process exited";
                RecreateRequested?.Invoke(this);
            }
            else
            {
                // Render process crash / frame failure: reloading recovers the page;
                // the profile (session) is untouched.
                try { c.Reload(); } catch { /* the recreate path covers hopeless cases */ }
            }
        };
    }

    /// <summary>Chat-list landmarks per service. Null means the service has no list we watch.</summary>
    private sealed record LiveChatSpec(
        string Root, string[] Rows, string? RowFilter = null, bool RequireTime = false,
        bool PublishUnread = true, bool DiscordDm = false, bool WhatsApp = false,
        int SettleMs = 10000);

    private static LiveChatSpec? LiveChatSpecFor(string id) => id.ToLowerInvariant() switch
    {
        "telegram" => new("#column-left", new[] { ".chatlist-chat" }),

        // WhatsApp Web rebuilt its chat list without stable href/data-id values. The
        // conversation title (from the titled span, not the row's hidden unread label)
        // is the only durable identity left, so it also serves as the snapshot key.
        "whatsapp" => new(
            "#pane-side",
            new[] { "[data-testid=\"cell-frame-container\"]" },
            WhatsApp: true,
            // The chat list routinely appears ~15 s after navigation on a cold start;
            // priming must finish only after the list has stopped filling in.
            SettleMs: 12000),

        // Discord: every DM/channel row is an anchor to /channels/@me/<numeric id>; the
        // data-list-item-id values are not numeric (uid_11___friends, uid_11___nitro, ...),
        // so the href is the stable key. The bare /channels/@me list page is excluded.
        "discord" => new(
            "body",
            new[] { "a[href^=\"/channels/@me/\"]" },
            "^/channels/@me/\\d+",
            DiscordDm: true,
            // Discord's SPA can finish rendering its DM list well after 10 s on a
            // cold start; a short settle floods the inbox with every old preview.
            SettleMs: 30000),

        // Messenger thread links exist in several landmarks; match real thread ids only.
        "facebook" or "messenger" => new(
            "body",
            new[] { "a[href*=\"/t/\"]" },
            "/t/\\d+"),

        // Instagram labels its DM inbox "Thread list" (localized). Rows are buttons, not
        // links, and the inbox also contains a Notes tray whose rows have no timestamp.
        // The sidebar badge comes from the page title, so no count is published here.
        "instagram" => new(
            "body",
            new[] { "a[href*=\"/direct/t/\"]", "[role=\"button\"]" },
            null,
            RequireTime: true,
            PublishUnread: false),

        // Threads DMs live at /messages/ and each conversation is an anchor to
        // /messages/t/<id>; the row also ends with a relative timestamp.
        "threads" => new(
            "body",
            new[] { "a[href*=\"/messages/t/\"]" },
            "/messages/t/\\d+",
            RequireTime: true,
            PublishUnread: false),

        _ => null,
    };

    /// <summary>
    /// Installs a read-only MutationObserver over the service's own chat list. Only preview
    /// text changes that happen after the initial snapshot are reported; the initial list is
    /// never imported, no tokens are read, and nothing is ever sent.
    /// </summary>
    private async Task InstallLiveActivityObserverAsync(CoreWebView2 core)
    {
        try
        {
            if (!Uri.TryCreate(core.Source, UriKind.Absolute, out var source)) return;
            if (IsExternal(Def, source.ToString())) return; // only the service's own pages

            var spec = LiveChatSpecFor(Def.Id);
            if (spec == null) return;

            var js = """
                (() => {
                  if (window.__unitedLiveObserver) return 'exists';
                  window.__unitedLiveObserver = true;
                  const rootSel = __ROOT__;
                  const rowSels = __ROWS__;
                  const rowFilter = __FILTER__;
                  const requireTime = __REQUIRE_TIME__;
                  const publishUnread = __PUBLISH_UNREAD__;
                  const discordDm = __DISCORD__;
                  const whatsapp = __WHATSAPP__;
                  const settleMs = __SETTLE__;
                  const service = __SERVICE__;
                  // WebView2 serializes postMessage objects to JSON. A lone surrogate in a
                  // chat title (common with truncated emoji) makes .NET's strict JSON
                  // reader reject the whole message, so normalize strings first.
                  const wellFormed = value =>
                    typeof value === 'string' && value.toWellFormed
                      ? value.toWellFormed() : value;
                  let lastUnread = -1;
                  let lastRows = -1;
                  // Do not treat the first populations of the list as new messages. A slow
                  // SPA can mount its list long after the global settle window; two
                  // periodic scans with a populated list are required before emitting.
                  let primed = false;
                  let populationScans = 0;
                  const install = () => {
                    const root = document.querySelector(rootSel);
                    if (!root) return false;
                    const previous = new WeakMap();
                    const startedAt = Date.now();
                    // Service pages rebuild their row elements on every update, so a WeakMap
                    // alone would treat every re-render as a brand-new row and stay silent.
                    // Snapshots are also kept under a stable key (href, list id, or the row
                    // title) so a re-created row is still compared against its last text.
                    const snapshots = new Map();
                    const scan = (emit) => {
                      let totalUnread = 0;
                      let matchedRows = 0;
                      const seen = new Set();
                      for (const sel of rowSels) {
                        let rows = [];
                        try { rows = Array.from(root.querySelectorAll(sel)); } catch (e) { continue; }
                        for (const row of rows) {
                          if (seen.has(row)) continue;
                          seen.add(row);
                          if (rowFilter) {
                            const keys = [row.getAttribute('data-list-item-id'), row.getAttribute('href')]
                              .filter(Boolean);
                            const re = new RegExp(rowFilter);
                            if (!keys.some(k => re.test(k))) continue;
                          }
                          let rowText = row.innerText || '';
                          let title = '';
                          let bodyParts = [];
                          let rowUnread = 0;
                          if (whatsapp) {
                            // The title span's own title attribute is clean. innerText can
                            // start with a localized "N unread messages" accessibility line.
                            const titleEl = row.querySelector('[data-testid="cell-frame-title"]');
                            const titleSpan = titleEl ? titleEl.querySelector('span[title]') : null;
                            title = (titleSpan
                              ? (titleSpan.getAttribute('title') || titleSpan.textContent || '')
                              : '').trim().replace(/\s+/g, ' ').slice(0, 160);
                            if (!title && titleEl) {
                              const lines = (titleEl.innerText || '').split('\n')
                                .map(x => x.trim()).filter(Boolean)
                                .filter(x => !/^\d+\s+(pesan|messages?)\b.*(belum dibaca|unread)/i.test(x))
                                .filter(x => !/^\d+$/.test(x));
                              title = (lines[0] || '').slice(0, 160);
                            }
                            const bodyEl = row.querySelector('[data-testid="cell-frame-secondary"]');
                            const bodySpan = bodyEl ? bodyEl.querySelector('span[title]') : null;
                            let rawBody = bodySpan
                              ? (bodySpan.getAttribute('title') || bodySpan.textContent || '')
                              : (bodyEl ? bodyEl.innerText : '');
                            rawBody = (rawBody || '')
                              .replace(/^[\s"\u201c\u201d\u200e\u200f]+|[\s"\u201c\u201d\u200e\u200f]+$/g, '')
                              .replace(/\s+/g, ' ').trim();
                            if (!title || !rawBody) continue;
                            matchedRows++;
                            const badge = row.querySelector('[data-testid="icon-unread-count"]');
                            const badgeText = badge ? (badge.innerText || '').trim() : '';
                            if (/^\d+$/.test(badgeText)) rowUnread = parseInt(badgeText, 10);
                            else if (badge || row.querySelector('[data-testid*="unread" i]')) rowUnread = 1;
                            bodyParts = [rawBody];
                            rowText = rawBody;
                          } else {
                            rowText = row.innerText || '';
                            let parts = rowText.split('\n').map(x => x.trim()).filter(Boolean);
                            // Instagram stamps every conversation with a relative time; the
                            // Notes tray above the list has none. Separators and the stamp are
                            // dropped so the body stays a message preview.
                            if (requireTime && !parts.some(p => /^\d+\s*[smhdwy]$/i.test(p))) continue;
                            if (requireTime)
                              parts = parts.filter(p => p !== '·' && !/^\d+\s*[smhdwy]$/i.test(p));
                            if (parts.length < 2) continue;
                            matchedRows++;
                            // Discord DMs have a vertical unread pill, not a number. The
                            // numeric badges inside a DM row are server-tag icons and must
                            // never be counted as unread (that produced a fake badge of 2).
                            if (discordDm) {
                              const mentions = Array.from(row.querySelectorAll('[class*="numberBadge"]'))
                                .map(e => (e.innerText || '').trim())
                                .filter(t => /^\d+$/.test(t))
                                .reduce((a, t) => a + parseInt(t, 10), 0);
                              if (mentions > 0) rowUnread += mentions;
                              else if (row.querySelector('[class*="unreadPill"]')) rowUnread += 1;
                            } else {
                              const marker = row.querySelector(
                                '[class*="numberBadge" i],[class*="unread" i],[data-testid*="unread" i]');
                              const label = row.getAttribute('aria-label') || '';
                              if (marker) {
                                const badge = (marker.innerText || '').trim();
                                if (badge) rowUnread += /^\d+$/.test(badge) ? parseInt(badge, 10) : 1;
                              } else if (/unread|new message|belum dibaca/i.test(label) ||
                                         /unread message|belum dibaca|pesan baru/i.test(rowText)) {
                                rowUnread += 1;
                              }
                            }

                            title = parts[0].slice(0, 160);
                            bodyParts = parts.slice(1);
                            if (discordDm) {
                              // aria-label is the stable DM name; innerText can start with
                              // a presence line or a server-tag caption.
                              const aria = row.getAttribute('aria-label') || '';
                              const name = aria.match(/^(.*?)\s*\(direct message\)/i);
                              if (name && name[1].trim()) title = name[1].trim().slice(0, 160);
                              // "Server Tag: X" and its value are profile chrome, not a message.
                              const tagAt = parts.findIndex(p => /^server tag:/i.test(p));
                              const tagValue = tagAt >= 0 ? parts[tagAt + 1] : null;
                              bodyParts = bodyParts.filter(p => p !== title &&
                                !/^server tag:/i.test(p) && (!tagValue || p !== tagValue));
                            }
                            // Discord repeats the preview line in two layout nodes.
                            bodyParts = bodyParts.filter((p, i) => i === 0 || p !== bodyParts[i - 1]);
                          }
                          totalUnread += rowUnread;
                          const body = bodyParts.join(' ').slice(0, 400);
                          const snapshot = title + '\u001f' + body;
                          // WhatsApp rows carry an aria-label that includes the changing
                          // unread text; only the cleaned conversation title is stable.
                          const key = whatsapp ? title
                            : (row.getAttribute('href') || row.getAttribute('data-list-item-id') ||
                               row.getAttribute('aria-label') || title);
                          const prior = snapshots.has(key) ? snapshots.get(key)
                                      : (previous.has(row) ? previous.get(row) : undefined);
                          // A non-emitting scan must only seed rows it has never seen. If it
                          // overwrote an existing snapshot, a mutation queued at the same moment
                          // would find prior === snapshot and stay silent (seen on Threads).
                          if (emit || prior === undefined)
                          {
                            snapshots.set(key, snapshot);
                            previous.set(row, snapshot);
                          }
                          if (!emit || prior === snapshot || !body) continue;
                          // Ignore everything the page emits while it is still rendering the
                          // initial list; only changes after it settles are real activity.
                          if (Date.now() - startedAt < settleMs) continue;
                          if (!primed) continue;
                          // A newly mounted, already-read row is usually virtualizer churn,
                          // not a new message. New unread rows still pass.
                          if (prior === undefined && rowUnread === 0) continue;
                          if (/^(you|anda|kamu|vos|vous|du|ich)\s*:/i.test(body)) continue;
                          // Group membership/name changes and deleted-message placeholders
                          // are service events, not incoming messages.
                          if (/(joined using this group's invite link|joined via invite link|changed the group name|mengubah nama grup|bergabung via tautan undangan|this message was deleted|pesan ini telah dihapus)/i.test(body)) continue;
                          // Threads previews own activity as "You sent ..."/"Sent a post".
                          if (/^you (sent|shared)\b/i.test(body) || /^sent a (post|message)$/i.test(body)) continue;
                          // Presence lines ("Active 7m ago", "1 active today") are not messages.
                          if (/^active\b.*\b(ago|today|now)\b/i.test(body) || /^\d+\s+active\b/i.test(body)) continue;
                          window.chrome.webview.postMessage({
                            kind: 'chat-preview', service,
                            title: wellFormed(title), body: wellFormed(body),
                            target: wellFormed((row.getAttribute('href') || '').slice(0, 600)),
                            unread: publishUnread ? rowUnread : null
                          });
                        }
                      }
                      // Discord's own Direct Messages icon is the authoritative unread
                      // count; per-row pills are the fallback while the list is rendered.
                      let unreadReliable = true;
                      if (discordDm) {
                        const home = document.querySelector(
                          '[data-list-item-id="guildsnav___home"] [class*="unreadCount"]');
                        const homeText = home ? (home.innerText || '').trim() : '';
                        if (/^\d+$/.test(homeText)) totalUnread = parseInt(homeText, 10);
                        else unreadReliable = matchedRows > 0;
                      }
                      if (publishUnread && unreadReliable && totalUnread !== lastUnread) {
                        lastUnread = totalUnread;
                        window.chrome.webview.postMessage({
                          kind: 'unread-count', service, count: Math.min(totalUnread, 999)
                        });
                      }
                      if (matchedRows !== lastRows) {
                        lastRows = matchedRows;
                        window.chrome.webview.postMessage({ kind: 'row-count', service, count: matchedRows });
                      }
                      if (!primed && !emit && matchedRows > 0) {
                        populationScans++;
                        if (populationScans >= 2) primed = true;
                      }
                    };
                    scan(false);
                    let pending = false;
                    new MutationObserver(() => {
                      if (pending) return;
                      pending = true;
                      setTimeout(() => { pending = false; scan(true); }, 400);
                    }).observe(root, { childList: true, subtree: true, characterData: true });
                    setInterval(() => scan(false), 10000);
                    return true;
                  };
                  if (install()) return 'installed';
                  const mountObserver = new MutationObserver(() => {
                    if (!install()) return;
                    mountObserver.disconnect();
                    window.chrome.webview.postMessage({ kind: 'observer-ready', service });
                  });
                  mountObserver.observe(document.documentElement, { childList: true, subtree: true });
                  return 'waiting-for-chat-list';
                })();
                """
                .Replace("__ROOT__", JsonSerializer.Serialize(spec.Root))
                .Replace("__ROWS__", JsonSerializer.Serialize(spec.Rows))
                .Replace("__FILTER__", spec.RowFilter == null ? "null" : JsonSerializer.Serialize(spec.RowFilter))
                .Replace("__REQUIRE_TIME__", spec.RequireTime ? "true" : "false")
                .Replace("__PUBLISH_UNREAD__", spec.PublishUnread ? "true" : "false")
                .Replace("__DISCORD__", spec.DiscordDm ? "true" : "false")
                .Replace("__WHATSAPP__", spec.WhatsApp ? "true" : "false")
                .Replace("__SETTLE__", spec.SettleMs.ToString(System.Globalization.CultureInfo.InvariantCulture))
                .Replace("__SERVICE__", JsonSerializer.Serialize(Def.Id));

            var result = await core.ExecuteScriptAsync(js);
            AppLog.Info($"live observer [{Def.Id}] {result}");
        }
        catch (Exception ex)
        {
            AppLog.Exception(ex, $"installing live observer [{Def.Id}]");
        }
    }

    private void OnWebMessage(CoreWebView2 core, CoreWebView2WebMessageReceivedEventArgs e)
    {
        try
        {
            if (!Uri.TryCreate(e.Source, UriKind.Absolute, out var source)) return;
            if (IsExternal(Def, source.ToString())) return;

            using var message = JsonDocument.Parse(StripInvalidSurrogates(e.WebMessageAsJson));
            var root = message.RootElement;
            if (!root.TryGetProperty("kind", out var kind)) return;
            if (!root.TryGetProperty("service", out var serviceElement) ||
                !string.Equals(serviceElement.GetString(), Def.Id, StringComparison.OrdinalIgnoreCase)) return;

            switch (kind.GetString())
            {
                case "unread-count":
                    if (root.TryGetProperty("count", out var countElement) &&
                        countElement.TryGetInt32(out var count))
                    {
                        AppLog.Info($"unread count [{Def.Id}] {count}");
                        UnreadCountObserved?.Invoke(this, count);
                    }
                    break;

                case "chat-preview":
                    var title = root.TryGetProperty("title", out var titleElement) ? titleElement.GetString() : null;
                    var body = root.TryGetProperty("body", out var bodyElement) ? bodyElement.GetString() : null;
                    var target = root.TryGetProperty("target", out var targetElement) ? targetElement.GetString() : null;
                    int? unread = null;
                    if (root.TryGetProperty("unread", out var unreadElement) &&
                        unreadElement.TryGetInt32(out var rowUnread))
                        unread = rowUnread;
                    if (string.IsNullOrWhiteSpace(title) || string.IsNullOrWhiteSpace(body)) return;
                    LiveActivityObserved?.Invoke(this, title, body, DateTimeOffset.Now, target, unread);
                    break;

                case "observer-ready":
                    AppLog.Info($"live observer [{Def.Id}] installed-late");
                    break;

                case "row-count":
                    if (root.TryGetProperty("count", out var rowCount) && rowCount.TryGetInt32(out var rows))
                        AppLog.Info($"live rows [{Def.Id}] {rows}");
                    break;
            }
        }
        catch (Exception ex) when (ex is JsonException or InvalidOperationException)
        {
            // A service can post a truncated emoji or a null where a number was
            // expected. Drop that one message instead of turning it into an app error.
            AppLog.Warn($"ignored malformed live observer message [{Def.Id}] {ex.GetType().Name}");
        }
        catch (Exception ex)
        {
            AppLog.Exception(ex, $"reading live observer message [{Def.Id}]");
        }
    }

    /// <summary>
    /// Replaces lone UTF-16 surrogates before strict JSON parsing. Paired
    /// surrogates (real emoji and supplementary characters) are preserved.
    /// </summary>
    private static string StripInvalidSurrogates(string text)
    {
        StringBuilder? cleaned = null;
        for (var i = 0; i < text.Length; i++)
        {
            var current = text[i];
            if (char.IsHighSurrogate(current) &&
                i + 1 < text.Length && char.IsLowSurrogate(text[i + 1]))
            {
                cleaned?.Append(current).Append(text[i + 1]);
                i++;
                continue;
            }

            if (char.IsSurrogate(current))
            {
                if (cleaned == null)
                {
                    cleaned = new StringBuilder(text.Length);
                    cleaned.Append(text, 0, i);
                }
                cleaned.Append('\uFFFD');
                continue;
            }

            cleaned?.Append(current);
        }
        return cleaned?.ToString() ?? text;
    }

    // OAuth providers used by the bundled services. These hosts stay in-app so the
    // callback lands in the same WebView2 profile (cookies/session) instead of a
    // separate system-browser cookie jar. Custom services can add providers through
    // authPopupHosts in services.json.
    private static readonly string[] CommonAuthHosts =
    {
        "accounts.google.com",
        "accounts.youtube.com",
        "appleid.apple.com",
        "login.microsoftonline.com",
        "login.live.com",
        "steamcommunity.com",
        "steampowered.com",
    };

    public static bool IsAuthHost(string url)
    {
        if (!Uri.TryCreate(url, UriKind.Absolute, out var u)) return false;
        if (u.Scheme is not ("https" or "http")) return false;
        foreach (var domain in CommonAuthHosts)
            if (HostMatches(u.Host, domain))
                return true;
        return false;
    }

    /// <summary>Navigation policy: everything outside the service's own domains (and known auth hosts)
    /// goes to the system browser instead of hijacking the messenger tab.</summary>
    private bool IsBlockedNavigation(string uri)
        => Uri.TryCreate(uri, UriKind.Absolute, out var u) &&
           u.Scheme is "https" or "http" &&
           IsExternal(Def, uri) &&
           !IsAuthHost(uri);

    public static bool IsExternal(ServiceDefinition def, string url)
    {
        if (!Uri.TryCreate(url, UriKind.Absolute, out var u)) return false;
        if (u.Scheme is not ("https" or "http")) return false;

        var host = u.Host;
        foreach (var list in new[] { def.OriginHosts, def.AuthPopupHosts })
            foreach (var domain in list)
                if (HostMatches(host, domain))
                    return false;

        return true;
    }

    private static bool HostMatches(string host, string domain)
        => string.Equals(host, domain, StringComparison.OrdinalIgnoreCase) ||
           host.EndsWith("." + domain, StringComparison.OrdinalIgnoreCase);

    public static void OpenInSystemBrowser(string url)
    {
        try
        {
            Process.Start(new ProcessStartInfo(url) { UseShellExecute = true });
        }
        catch (Exception ex)
        {
            AppLog.Exception(ex, $"open external {url}");
        }
    }

    /// <summary>
    /// Shows or hides this slot. Panel.ZIndex does not reorder hosted (HWND) WebView2
    /// surfaces, so switching services must toggle Visibility — exactly one slot visible.
    /// </summary>
    public void Show(bool visible)
    {
        _shown = visible;
        if (_wv == null) return;
        if (visible)
        {
            _wv.Visibility = Visibility.Visible;
            _wv.Margin = _paneInsets;
            try { _wv.Focus(); } catch { /* focus is best effort */ }
        }
        else if (Def.KeepRenderingWhenHidden)
        {
            _wv.Visibility = Visibility.Visible;
            _wv.Margin = ParkedInsets;
        }
        else
        {
            _wv.Visibility = Visibility.Collapsed;
        }
    }

    /// <summary>Insets that move a background surface out of the pane without collapsing it,
    /// so Chromium keeps rendering it (see <see cref="ServiceDefinition.KeepRenderingWhenHidden"/>).</summary>
    private Thickness ParkedInsets =>
        new(_paneInsets.Left - 100_000, _paneInsets.Top, _paneInsets.Right + 100_000, _paneInsets.Bottom);

    /// <summary>Insets the hosted surface; the unified inbox uses the left inset to
    /// place the chat beside its conversation list.</summary>
    public void SetPaneInsets(double left, double top, double right, double bottom)
    {
        _paneInsets = new Thickness(left, top, right, bottom);
        if (_wv != null)
            _wv.Margin = _shown || !Def.KeepRenderingWhenHidden ? _paneInsets : ParkedInsets;
    }

    /// <summary>
    /// Makes the web content follow United's light/dark theme where the service
    /// supports prefers-color-scheme, and paints the pre-load background in the
    /// same surface colour so switching services does not flash white.
    /// Services with their own explicit theme setting can still override this.
    /// </summary>
    public void ApplyTheme(bool dark)
    {
        _darkTheme = dark;
        if (_wv != null)
        {
            try
            {
                _wv.DefaultBackgroundColor = dark
                    ? System.Drawing.Color.FromArgb(0x11, 0x1B, 0x21)
                    : System.Drawing.Color.FromArgb(0xF5, 0xF5, 0xF5);
            }
            catch { /* best effort */ }
        }

        var c = _wv?.CoreWebView2;
        if (c == null) return;
        try
        {
            c.Profile.PreferredColorScheme = dark
                ? CoreWebView2PreferredColorScheme.Dark
                : CoreWebView2PreferredColorScheme.Light;
        }
        catch (Exception ex)
        {
            AppLog.Exception(ex, $"theme [{Def.Id}]");
        }
    }

    /// <summary>
    /// In the unified inbox the host already provides the conversation list, so the
    /// service's own list/navigation column is hidden and only its conversation is
    /// shown. This is a cosmetic stylesheet; it never reads or sends messages.
    /// </summary>
    public void SetInboxFocusMode(bool enabled)
    {
        var css = enabled ? InboxFocusCss(Def.Id) : null;
        var next = enabled && css != null;
        if (_inboxFocus == next && _inboxFocusCss == css) return;

        _inboxFocus = next;
        _inboxFocusCss = css;
        _ = ApplyInboxFocusAsync();
    }

    private async Task ApplyInboxFocusAsync()
    {
        var c = _wv?.CoreWebView2;
        if (c == null) return;
        try
        {
            var css = _inboxFocusCss;
            var js = css == null
                ? "document.getElementById('__united_inbox_focus')?.remove();"
                : "(function(){let s=document.getElementById('__united_inbox_focus');" +
                  "if(!s){s=document.createElement('style');s.id='__united_inbox_focus';" +
                  "(document.head||document.documentElement).appendChild(s);}" +
                  "s.textContent=" + JsonSerializer.Serialize(css) + ";})();";
            await c.ExecuteScriptAsync(js);
        }
        catch (Exception ex)
        {
            AppLog.Exception(ex, $"inbox focus css [{Def.Id}]");
        }
    }

    /// <summary>
    /// Styles that collapse the service's own conversation list in the inbox pane.
    /// Only stable, service-specific landmarks are targeted; unknown services keep
    /// their original layout so nothing is ever hidden blindly.
    /// </summary>
    private static string? InboxFocusCss(string serviceId) => serviceId.ToLowerInvariant() switch
    {
        // WhatsApp keeps its list in #side and uses empty "drawer" overlays whose
        // 1px borders would otherwise remain as vertical rules. Only collapse the
        // chrome once a real conversation (#main) is open, so a failed reveal
        // leaves the service's own list usable instead of showing a blank pane.
        "whatsapp" =>
            ".two:has(#main) #side{display:none!important}" +
            ".two:has(#main) div:has(> #side){display:none!important}" +
            ".two:has(#main) > header:first-child{display:none!important}" +
            ".two:has(#main) [data-testid=\"drawer-left\"]," +
            ".two:has(#main) [data-testid=\"drawer-middle\"]{display:none!important}" +
            ".two:has(#main) #main{width:100%!important;flex:1 1 auto!important}",

        // Telegram Web K: hide the chat list only after a conversation is loaded
        // (.bubbles is the message history), then let the center column fill.
        "telegram" =>
            "#column-left:has(+ #column-center .bubbles){display:none!important}" +
            "#column-center:has(.bubbles){width:100%!important;flex:1 1 auto!important;min-width:0!important}",

        // Discord uses hashed class names; the stable fragments are content_,
        // sidebar_, page_ and chatContent_. Parking the DM list off-screen (instead
        // of display:none) keeps its 340px box measurable, so Discord's virtualizer
        // keeps every row mounted — the inbox observer stays live while a DM is open.
        "discord" =>
            "div[class*=\"content_\"]:has(main[class*=\"chatContent_\"]) > div[class*=\"sidebar_\"]{" +
            "position:fixed!important;left:-1200px!important;top:0!important;" +
            "width:340px!important;height:100vh!important;opacity:0!important;" +
            "display:flex!important;pointer-events:none!important}" +
            "div[class*=\"content_\"]:has(main[class*=\"chatContent_\"]){grid-template-columns:1fr!important}" +
            "div[class*=\"content_\"]:has(main[class*=\"chatContent_\"]) div[class*=\"page_\"]{grid-column:1/-1!important;width:100%!important}" +
            "main{min-width:0!important}",

        // Messenger's chat list is the navigation landmark that owns thread
        // links. Only hide it when the thread pane actually has a composer.
        "facebook" or "messenger" =>
            "div[role=\"navigation\"]:has(a[href*=\"/t/\"]):has(~ div[role=\"main\"] [contenteditable=\"true\"]){display:none!important}" +
            "div[role=\"main\"]:has([contenteditable=\"true\"]){width:auto!important;max-width:none!important;flex:1 1 auto!important}",

        // Instagram labels its DM inbox "Thread list" (localized).
        "instagram" =>
            "div[role=\"navigation\"]:has(h2)," +
            "div[role=\"navigation\"][aria-label*=\"thread\" i]{display:none!important}",

        _ => null,
    };

    /// <summary>
    /// Best effort: clicks the conversation whose title matches in the service's own
    /// list so the unified inbox opens it directly. Navigation only — the user replies
    /// in the service's native editor; no message is ever sent by this method.
    /// </summary>
    public async Task<bool> TryRevealChatAsync(string title)
    {
        var c = _wv?.CoreWebView2;
        if (c == null || string.IsNullOrWhiteSpace(title)) return false;

        var result = await ExecuteRevealAsync(c, title);
        if (WasClicked(result)) return true;

        // A freshly created slot is still navigating; retry once after it settles.
        var tcs = new TaskCompletionSource<bool>(TaskCreationOptions.RunContinuationsAsynchronously);
        void OnNav(object? _, CoreWebView2NavigationCompletedEventArgs __) => tcs.TrySetResult(true);
        c.NavigationCompleted += OnNav;
        try
        {
            if (await Task.WhenAny(tcs.Task, Task.Delay(5000)) == tcs.Task)
            {
                await Task.Delay(400);
                result = await ExecuteRevealAsync(c, title);
            }
        }
        finally
        {
            c.NavigationCompleted -= OnNav;
        }

        return WasClicked(result);
    }

    /// <summary>
    /// Opens the exact conversation by its stable href (Discord /channels/@me/&lt;id&gt;,
    /// Threads /messages/t/&lt;id&gt;, …). Falls back to navigation inside the service's
    /// own origin when the row is not currently rendered. Read-only: only opens the chat.
    /// </summary>
    public async Task<bool> TryRevealTargetAsync(string target)
    {
        var c = _wv?.CoreWebView2;
        if (c == null) return false;

        target = (target ?? "").Trim();
        if (target.Length is 0 or > 600 || target.Contains('\n') || target.Contains('\r')) return false;

        var result = await ExecuteRevealTargetAsync(c, target);
        if (WasClicked(result)) return true;

        // Relative paths only; never let a stored value turn into javascript: or an
        // external host. IsExternal reuses the same navigation policy as the webview.
        if (!target.StartsWith('/') || target.StartsWith("//")) return false;
        if (!Uri.TryCreate(Def.Url, UriKind.Absolute, out var baseUri) ||
            !Uri.TryCreate(baseUri, target, out var full) ||
            IsExternal(Def, full.ToString()))
            return false;

        AppLog.Info($"reveal target [{Def.Id}] navigate {full}");
        c.Navigate(full.ToString());
        return true;
    }

    private async Task<string> ExecuteRevealTargetAsync(CoreWebView2 c, string target)
    {
        try
        {
            var literal = JsonSerializer.Serialize(target);
            var js = $$"""
                       (() => {
                         try {
                           const wanted = {{literal}};
                           const el = Array.from(document.querySelectorAll('a[href]'))
                             .find(a => a.getAttribute('href') === wanted);
                           if (!el) return 'notfound';
                           el.scrollIntoView({ block: 'center' });
                           try { el.focus(); } catch (e) {}
                           const r = el.getBoundingClientRect();
                           const b = {
                             bubbles: true, cancelable: true, view: window, button: 0,
                             clientX: r.left + Math.max(24, r.width * 0.55),
                             clientY: r.top + r.height / 2
                           };
                           try {
                             el.dispatchEvent(new PointerEvent('pointerdown',
                               { ...b, buttons: 1, pointerId: 1, pointerType: 'mouse', isPrimary: true }));
                           } catch (e) {}
                           el.dispatchEvent(new MouseEvent('mousedown', { ...b, buttons: 1 }));
                           try {
                             el.dispatchEvent(new PointerEvent('pointerup',
                               { ...b, buttons: 0, pointerId: 1, pointerType: 'mouse', isPrimary: true }));
                           } catch (e) {}
                           el.dispatchEvent(new MouseEvent('mouseup', { ...b, buttons: 0 }));
                           el.dispatchEvent(new MouseEvent('click', { ...b, buttons: 0 }));
                           return 'clicked-target';
                         } catch (e) { return 'error'; }
                       })();
                       """;
            var result = await c.ExecuteScriptAsync(js);
            AppLog.Info($"reveal target [{Def.Id}] {JsonSerializer.Serialize(target)} -> {result}");
            return result ?? "";
        }
        catch (Exception ex)
        {
            AppLog.Exception(ex, $"reveal target [{Def.Id}]");
            return "error";
        }
    }

    private static bool WasClicked(string? result)
        => !string.IsNullOrEmpty(result) &&
           result.Trim().Trim('"').StartsWith("clicked", StringComparison.OrdinalIgnoreCase);

    private async Task<string> ExecuteRevealAsync(CoreWebView2 c, string title)
    {
        try
        {
            // JsonSerializer produces a valid JS string literal (quotes/newlines escaped).
            var literal = JsonSerializer.Serialize(title.Trim().ToLowerInvariant());
            var js = $$"""
                       (() => {
                         try {
                           const wanted = {{literal}};
                           if (!wanted) return 'skip';
                           const selectors = [
                             '[data-testid="cell-frame-container"]',
                             '[data-testid="chat-list-item"]',
                             '[role="listitem"]',
                             '.chatlist-chat',
                             '[data-list-item-id^="private-channels-"]',
                             'a[href*="/direct/t/"]',
                             'a[href*="/messages/t/"]'
                           ];
                           const fire = (el) => {
                             // Click the row itself, in its text/preview area. Picking a
                             // child control can hit the avatar (WhatsApp opens contact
                             // info/media) or a trailing action. The row's own handler opens
                             // the conversation; no second synthetic click is sent.
                             const target = el;
                             try { target.focus(); } catch (e) {}
                             const r = target.getBoundingClientRect();
                             const b = {
                               bubbles: true, cancelable: true, view: window, button: 0,
                               clientX: r.left + Math.max(24, r.width * 0.55),
                               clientY: r.top + r.height / 2
                             };
                             try {
                               target.dispatchEvent(new PointerEvent('pointerdown',
                                 { ...b, buttons: 1, pointerId: 1, pointerType: 'mouse', isPrimary: true }));
                             } catch (e) {}
                             target.dispatchEvent(new MouseEvent('mousedown', { ...b, buttons: 1 }));
                             try {
                               target.dispatchEvent(new PointerEvent('pointerup',
                                 { ...b, buttons: 0, pointerId: 1, pointerType: 'mouse', isPrimary: true }));
                             } catch (e) {}
                             target.dispatchEvent(new MouseEvent('mouseup', { ...b, buttons: 0 }));
                             target.dispatchEvent(new MouseEvent('click', { ...b, buttons: 0 }));
                           };
                           const seen = new Set();
                           let fallback = null, fallbackCount = 0;
                           const rowTitle = (el) => {
                             // WhatsApp prepends a hidden localized unread label to the
                             // row text. The titled span is the conversation name.
                             if (el.matches('[data-testid="cell-frame-container"]')) {
                               const span = el.querySelector(
                                 '[data-testid="cell-frame-title"] span[title]');
                               const name = span
                                 ? (span.getAttribute('title') || span.textContent || '')
                                 : '';
                               if (name.trim()) return name.trim().toLowerCase();
                             }
                             return (el.innerText || el.textContent || '').trim()
                               .split('\n')[0].trim().toLowerCase();
                           };
                           for (const sel of selectors) {
                             for (const el of document.querySelectorAll(sel)) {
                               if (seen.has(el)) continue;
                               seen.add(el);
                               const text = (el.innerText || el.textContent || '').trim();
                               if (!text) continue;
                               const firstLine = rowTitle(el);
                               if (firstLine === wanted) {
                                 el.scrollIntoView({ block: 'center' });
                                 fire(el);
                                 return 'clicked-exact';
                               }
                               if (text.toLowerCase().includes(wanted)) { fallback = el; fallbackCount++; }
                             }
                           }
                           if (fallbackCount === 1 && fallback) {
                             fallback.scrollIntoView({ block: 'center' });
                             fire(fallback);
                             return 'clicked-unique';
                           }
                           return fallbackCount === 0 ? 'notfound' : 'ambiguous';
                         } catch (e) { return 'error'; }
                       })();
                       """;
            var result = await c.ExecuteScriptAsync(js);
            AppLog.Info($"reveal chat [{Def.Id}] {JsonSerializer.Serialize(title)} -> {result}");
            return result ?? "";
        }
        catch (Exception ex)
        {
            AppLog.Exception(ex, $"reveal chat [{Def.Id}]");
            return "error";
        }
    }

    /// <summary>Applies the memory governor's decision to this slot (and mirrors it in state).</summary>
    public void ApplyMemory(MemoryAction action)
    {
        var c = _wv?.CoreWebView2;
        if (c == null) return;

        switch (action)
        {
            case MemoryAction.None:
                c.MemoryUsageTargetLevel = CoreWebView2MemoryUsageTargetLevel.Normal;
                SetState(ServiceState.Ready);
                break;
            case MemoryAction.Lower:
                // Documented behaviour: scripts keep running and network connections stay alive —
                // notifications continue to work while caches are trimmed.
                c.MemoryUsageTargetLevel = CoreWebView2MemoryUsageTargetLevel.Low;
                SetState(ServiceState.Suspended);
                break;
            case MemoryAction.Hibernate:
                Hibernate();
                break;
        }
    }

    /// <summary>Disposes the current controller and creates a fresh one on the same profile
    /// (used after a browser-process failure). Session data survives on disk.</summary>
    public async Task RecreateAsync()
    {
        CloseControl();
        if (State is not (ServiceState.Failed or ServiceState.Hibernated or ServiceState.NotCreated))
            SetState(ServiceState.Hibernated); // Ready/Suspended -> Hibernated (valid edge)
        await CreateAsync();                  // Hibernated/Failed -> Starting -> Ready
    }

    public void Reload()
    {
        if (_wv?.CoreWebView2 != null)
        {
            _wv.CoreWebView2.Reload();
        }
        else if (State is ServiceState.Failed or ServiceState.Hibernated)
        {
            _ = CreateAsync();
        }
    }

    /// <summary>Disposes the controller (renderers freed) while the profile/session stays on disk.</summary>
    public void Hibernate()
    {
        if (_wv == null) return;
        CloseControl();
        SetState(ServiceState.Hibernated);
    }

    /// <summary>Full teardown (service removed or app exit).</summary>
    public void Dispose()
    {
        _disposedFully = true;
        CloseControl();
        if (State != ServiceState.Disposed) SetState(ServiceState.Disposed);
    }

    private void CloseControl()
    {
        var wv = _wv;
        if (wv == null) return;
        _wv = null;
        try { _host.Children.Remove(wv); } catch { /* already gone */ }
        try { wv.Dispose(); } catch { /* best effort */ }
    }

    private void SetState(ServiceState next)
    {
        if (next == State) return; // no-op: prevents feedback loops from memory policy
        if (!ServiceStateMachine.CanTransition(State, next)) return;
        State = next;
        StateChanged?.Invoke(this);
    }
}
