# PLAN — Ultra-Lightweight Unified Desktop Messenger for Windows

> Historical design document from 20 October 2026, kept for engineering context. The shipped 0.1.0 supports six built-in services; current behavior and verification are in [README.md](README.md) and [docs/VERIFICATION.md](docs/VERIFICATION.md).
> Title (working): **United** — one window, six messengers, one browser process.
> Version: 1.0 · Date: 2026-10-20 · Target: Windows 10 (19041+) / Windows 11

---

## 0. Executive summary

The four services (WhatsApp, Telegram, Instagram, Discord) are all ordinary web apps that Microsoft Edge
(or any Chromium browser) can host. The dominant memory cost is the Chromium infrastructure (browser, GPU,
network, storage service processes), duplicated once per app in today's setup — four separate Chromium
runtimes. The single biggest win is therefore **not** a fancier app framework: it is
**one shared WebView2 browser process serving all four services with per-service profile isolation**.

**Recommended stack:** a native **.NET 10 WPF host** (custom chrome, no web frontend for the shell) +
the official **Microsoft.Web.WebView2** SDK, with:

- one `CoreWebView2Environment` and one user-data folder → **one shared browser process**;
- **one profile per service** (`CoreWebView2ControllerOptions.ProfileName`) → full session/cookie isolation
  inside that single browser process;
- **`CoreWebView2.MemoryUsageTargetLevel = Low`** on every non-focused service at all times — this API is
  documented to leave *scripts and network connections running* (notifications keep working) while trimming
  caches;
- unread counts from **`DocumentTitleChanged`** parsing (title-based, no fragile DOM scraping for V1);
- **native Windows toasts** raised by the host (WinRT `ToastNotificationManagerCompat`) with in-page
  (service-authored) notifications used where they work, plus dedupe logic;
- manual per-service **hibernation** = close the WebView controller, keep the profile (session persists);
  reload on demand.

Why not Tauri/Wails: Tauri 2's multi-webview-in-one-window is still an **unstable** feature (2026 docs),
and Wails v3's one-window-per-service model loses the coherent single-window UX and adds shell overhead per
window. The raw-WebView2 host (C++ or C#) gives us the exact APIs we need — process sharing, profiles, memory
target level, title events, permission handling — with zero framework in between.

This plan makes **no RAM-saving claims**. It defines a measurement methodology and hypotheses to validate
in milestone M0.

---

## 1. Research findings (with sources)

### 1.1 WebView2 process model: sharing one browser process

- Multiple WebView2 controls **share the same browser process** when created with the same user data folder
  (UDF) and a compatible environment configuration. Microsoft explicitly documents sharing UDFs to save
  system resources.
  Sources: [Manage user data folders](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/user-data-folder)
  ([Sharing user data folders](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/user-data-folder#sharing-user-data-folders)),
  [CoreWebView2Environment docs](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/winrt/microsoft_web_webview2_core/corewebview2environment?view=webview2-winrt-1.0.1222-prerelease)
  ("Multiple app processes can share a browser process by creating their webviews from a
  CoreWebView2Environment with the same user data folder").
- ⚠️ **Constraint:** one UDF can only be opened by **one active `CoreWebView2Environment` instance** per app
  process (a second app instance reusing the folder misbehaves; there is a lock). The app must be
  **single-instance** (named mutex), or use a second UDF for a second instance (defeats the memory goal).
  Sources: [WebView2Feedback #2503](https://github.com/MicrosoftEdge/WebView2Feedback/issues/2503),
  [Stack Overflow on multi-instance WebView2 apps](https://stackoverflow.com/questions/77801995/odd-issue-with-multiple-instances-of-an-app-that-uses-webview2-and-the-same-user).

### 1.2 Per-service isolation: multiple profiles in ONE user data folder

- WebView2 supports **multiple named profiles per UDF**: each profile has its own cookies, local storage,
  IndexedDB, permission grants, auto-fill, etc. API: `CoreWebView2ControllerOptions.ProfileName` /
  `ProfileId` at controller creation; the profile is reachable via `CoreWebView2.Profile`
  (download path, color scheme, private mode, and more are per-profile).
  Sources: [Multi-profile support](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/multi-profile-support),
  design spec [MultiProfile.md](https://github.com/MicrosoftEdge/WebView2Feedback/blob/main/specs/MultiProfile.md).
- **Consequence for this app:** WhatsApp / Telegram / Instagram / Discord can be four isolated, persistent
  sessions wrapped by **one** browser process — the isolation we need, without four Chromium trees.
  (Internet answers to "share one renderer process for multiple webviews" point at exactly this
  technique: [Microsoft Learn Q&A](https://learn.microsoft.com/en-us/answers/questions/1187995/share-one-renderer-process-for-multiple-webview2-i).)

### 1.3 Memory reduction for inactive WebViews: `MemoryUsageTargetLevel`

- `CoreWebView2.MemoryUsageTargetLevel` (`Normal` / `Low`, shipped in stable SDK) is **designed for exactly
  our case**. From the official API documentation/spec:
  - *"Scripts will not be impacted and continue to run. This is useful for inactive apps that still want to
    run scripts and/or keep network connections alive and therefore could not call `TrySuspend`."*
  - *"It is not necessary to set `CoreWebView2Controller.IsVisible = false` when calling the API."*
  - Best-effort; memory may be swapped out and swapped back on demand; set `Normal` when the view becomes
    active again (no automatic restore).
  - Do **not** mix with `TrySuspend`/`Resume`; suspending implies Low and resuming implies Normal.
  Sources:
  [MemoryUsageTargetLevel spec](https://github.com/MicrosoftEdge/WebView2Feedback/blob/main/specs/MemoryUsageTargetLevel.md),
  [.NET property docs](https://learn.microsoft.com/en-us/dotnet/api/microsoft.web.webview2.core.corewebview2.memoryusagetargetlevel?view=webview2-dotnet-1.0.4191.47),
  [Performance best practices](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/performance)
  ("Set MemoryUsageTargetLevel = Low on inactive WebViews to reduce memory usage").
- **This directly answers the project's most dangerous question:** the memory-saving mechanism that does
  *not* kill scripts or connections is Low target level (not suspension, not hiding, not renderer recycling).
  Actual savings must be measured (H2 in §12), but the mechanism exists and is supported.

### 1.4 Background behavior: throttling vs notifications

- Chromium throttles **JS timers** in hidden pages: after 5 min hidden, timers are intensively throttled to
  ~1 wakeup/minute; WebSocket *event delivery* is not throttled the same way — message arrival handlers run.
  Sources: [Chrome timer throttling blog](https://developer.chrome.com/blog/timer-throttling-in-chrome-88).
- WebView2 offers **no clean supported switch to fully disable background throttling**
  ([WebView2Feedback #1172](https://github.com/MicrosoftEdge/WebView2Feedback/issues/1172); also Tauri
  feature request [#5250](https://github.com/tauri-apps/tauri/issues/5250) referencing it).
- Because all four services push unread state through **websocket/push events**, the badge/notification
  path only needs event handlers to run — which Low target level explicitly preserves. Periodic polling
  logic inside the services may keep downsampling in the background (acceptable for V1; see §10 risks).
- `TrySuspend`/`Resume` and `IsVisible=false` are real options we deliberately **avoid** for
  notification-critical services (§5), since hidden visibility changes `document.visibilityState` to
  `hidden`, which some services treat as "leave/go idle" — a real-world flakiness source WATG/Ferdium
  also fight.

### 1.5 Web notifications in WebView2

- WebView2 exposes `PermissionKind.Notifications`; permission is managed via the **`PermissionRequested`**
  event — "Apps that would like to show notifications should handle PermissionRequested events and no
  browser permission prompt will be shown for notification requests"
  ([CoreWebView2PermissionKind](https://learn.microsoft.com/en-us/dotnet/api/microsoft.web.webview2.core.corewebview2permissionkind?view=webview2-dotnet-1.0.4129.50)).
- There are **known real-world quirks** with web notifications/push inside WebView2 (e.g.
  [WebView2Feedback #5051](https://github.com/MicrosoftEdge/WebView2Feedback/issues/5051) "Notifications do
  not work correctly"; push works in Edge but not in WebView2 cases —
  [Q&A](https://learn.microsoft.com/en-us/answers/questions/96885/web-push-working-in-edge-but-not-in-webview2)).
- **Design consequence — belt and suspenders:** the host must be able to raise its **own** Windows toasts
  (WinRT `ToastNotificationManagerCompat`, works unpackaged by creating an AUMID shortcut) driven by unread
  count deltas, independent of whether in-page notifications render. Which path is authoritative
  (service-authored vs host-generated) is a per-service config choice, settled by spike M0-4.

### 1.6 Tauri 2 status (multi-webview)

- The `Window`/`Webview` split (several webviews in one native window) is gated behind the **`unstable`
  Cargo feature**, documented as unfinished/be unstable and subject to breaking changes; stable `WebviewWindow`
  remains one-webview-per-window.
  Sources: [v2.tauri.app webview reference](https://v2.tauri.app/reference/javascript/api/namespacewebview/),
  [docs.rs tauri feature list](https://docs.rs/tauri/latest/tauri/#cargo-features),
  [Tauri v2 beta announcement](https://v2.tauri.app/blog/tauri-2-0-0-beta/).
- Verdict for this app: multiwebview Tauri = production risk on our core interaction pattern (pipeline
  switching); stable Tauri = four windows again. Either way the memory advantage does not beat raw
  WebView2, because Tauri *is* WebView2 on Windows.

### 1.7 Wails v3 status (Go)

- Wails v3 has progressed to **beta** (multi-window, first-class window API) but there is **no
  multi-webview-in-one-window** concept — services would be separate windows; also a known Windows bug with
  `Hidden:true` windows ([#4498](https://github.com/wailsapp/wails/issues/4498)).
  Sources: [Wails v3 Beta announcement](https://v3.wails.io/blog/wails-v3-beta/),
  [Multiple Windows docs](https://v3.wails.io/features/windows/multiple/).
- Verdict: promising later; window-per-service does not meet the UX goal, and its runtime (Go + WebView2)
  offers no structural memory advantage over a thin native host.

### 1.8 Native WebView2 host options

| Host tech | Notes |
|---|---|
| **C# / .NET WPF or WinForms** | Official support path, first-class, `Microsoft.Web.WebView2` package; custom `WindowChrome` gives a frameless premium shell; least implementation risk. Runtime overhead ~ tens of MB (to be measured; do not guess). |
| **C++ Win32 + WebView2 COM** | Lowest possible host footprint; all the same APIs; higher dev cost for shell chrome (GDI/D2D sidebar, DPI, theming). |
| **Rust (windows-rs + webview2-com)** | Equivalent to C++ with nicer bindings; still meaningful effort for the shell. |
| Maui/WinUI3 | Higher shell overhead (XAML island/WinUI runtime), unclear benefit for a 4-icon sidebar. |

There is no technical capability gap between these — all can create controllers, share environments, set
profiles/memory levels. The difference is shell overhead vs development cost.

### 1.9 Ferdium (open-source precedent)

- Architecture: Electron with per-service **recipes**; recipes are small JS/CSS packages handling unread
  counting, dark mode, notification interception per service — a proven pattern for our per-service config
  layer. App license **Apache-2.0**; recipes repo **MIT**
  ([ferdium-recipes](https://github.com/ferdium-ferdium-recipes), LICENSE.md in repo).
- Ferdium's RAM problem is structural (Electron runtime + one process tree per extra webview/page engine);
  users are routinely advised to enable "hibernation" to reach sane memory
  ([community thread](https://www.reddit.com/r/ferdium/comments/1jffinb/new_ferdi_migrant_high_ram_usage/)).
- License-wise we may **read** recipes to learn per-service behavior/quirks and write our own equivalent
  config code (include MIT attribution if any recipe text/selector is actually copied).

### 1.10 WATG (DavidBevi) — closest prior art to this project

- [WATG](https://github.com/DavidBevi/WATG) wraps **WhatsApp + Telegram** web in a portable Tauri app:
  toast notifications, unread in tray/titlebar, click-tray cycling, ~no framework overhead. Validates the
  overall approach and the badge-derivation approach on small scale.
- Its caveats are important primary evidence:
  - **WhatsApp does not expose notifications cleanly** and needs JS/CSS workarounds injected;
  - **WhatsApp changes internals → injected scripts break over time** (they ship side-loadable `wa.js`/`wa.css`
    for user patching);
  - testing workload is dominated by keeping up with site changes — reinforces config-driven, not
    code-driven, per-service logic.

### 1.11 Per-service compatibility in WebView2

- **WhatsApp:** As of **Nov 2025, WhatsApp's official Windows app itself migrated from native UWP to a
  WebView2 web wrapper** ([Daring Fireball](https://daringfireball.net/2025/11/meta_whatsapp_windows_shitty_web_app),
  [securityonline](https://securityonline.info/whatsapp-on-windows-11-ditches-native-uwp-for-webview2-performance-takes-a-hit/),
  [windowslatest](https://www.windowslatest.com/2025/11/22/i-tested-the-new-whatsapp-for-windows-11-a-web-wrapper-and-its-a-performance-nightmare/)).
  Two important implications: (a) `web.whatsapp.com` inside WebView2 is production-proven (it IS the
  official app now); (b) the naive wrapper reportedly holds ~400 MB in background — our project's
  performance budget exists to beat that naive baseline via process sharing + Low target level; WhatsApp
  Web keeps its connection via **service workers / persistent socket** and does not need a visible page to
  receive messages.
- **Telegram:** `web.telegram.org` works in WebView2 (WATG proves the same in Tauri = same WebView2 runtime);
  unread from title `(n) Telegram …` (verify exact format in M1).
- **Instagram:** `instagram.com` DMs work in desktop Chromium browsers; login-friction/UA acceptance inside
  specific wrappers is the main risk (assume + verify; UA quirk fallback is cheap). Title update verified
  in M1; Ferdium supports Instagram (recipe exists) so feasible.
- **Discord:** `discord.com/app` runs in any Chromium; mic/cam via PermissionRequested grants. **Known
  WebView2 bug:** `getDisplayMedia` screen-share picker dialog renders cropped/wrong
  ([WebView2Feedback #2184](https://github.com/MicrosoftEdge/WebView2Feedback/issues/2184)) — voice chat and
  screen *viewing* are fine; screen *sharing* may be degraded (cosmetic picker issue). No UA block known.

### 1.12 Telegram TDLib (native alternative)

- [TDLib](https://tdlib.github.io/td/) is a full client library; [TDLight](https://github.com/tdlight-team/old-migrated-repository)
  is a maintained fork "focused on memory footprint and performance". No reliable measurement shows TDLib
  runtime memory beats Telegram Web *for a comparable feature set* (much of the "500 MB" in their docs refers
  to **compile-time** RAM per translation unit — not runtime).
- Native Telegram would require building a bespoke chat UI over MTProto/TDLib for full personal-account
  behavior (bot API cannot read user DMs; self-bot automation violates Telegram ToS). Verdict:
  **not for MVP**; revisit only if benchmarks prove Telegram Web dominates our memory budget.

### 1.13 Security/isolation of embedded untrusted sites

- Standard model: everything remote runs *inside* the Chromium sandbox of WebView2; the shell (native code)
  must never grant web content direct bridges. Concretely: **no `AddHostObjectToScript`** to service origins
  (only to our own local config/bridge pages if ever needed); `WebMessageReceived` only from the local
  app-origin document; block/limit `window.external`; default deny unknown permissions; never whitelist dev
  features in release; disable DevTools in release builds; keep password saving/autofill off for
  messenger profiles so credentials stay in service-controlled storage, not in our config.

### 1.14 Windows 10/11 compatibility

- WebView2 Evergreen runtime is preinstalled/updated on updated Win10 (19041+) and Win11 systems (ships with
  Edge). Installer includes a runtime check + silent runtime install fallback
  ([Evergreen docs](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/distribution)).
- WPF/.NET 10 runs fine on Win10 1809+; WinToast/AUMID shortcuts work on both; DPAPI profile data exists on both.
- Single-instance + `HKCU …\Run` autostart entries are policy-safe and reversible.

---

## 2. Architecture comparison matrix

Score 1–5 (5 best). Derived from the findings above; every "memory" entry is architecture-level reasoning
(one vs many Chromium trees; API availability), **not** a benchmark claim.

| Criterion | **A. Tauri 2 (unstable multiwebview)** | **B. Tauri 2 (4 windows)** | **C. Wails v3 beta (4 windows)** | **D. .NET WPF + WebView2 (proposed)** | **E. C++ Win32 + WebView2** | **F. Rust + webview2-com** | G. Electron/Ferdium-style |
|---|---|---|---|---|---|---|---|
| Shared browser process (4 services) | 3 (same env by default, needs care) | 3 | 3 | **5 (explicit profiles/env control)** | 5 | 5 | 2 |
| Per-service session isolation | 2 (Tauri data-dir model is app-level; profiles awkward) | 2 | 2 | **5 (ProfileName)** | 5 | 5 | 3 |
| Several webviews in ONE window | 2 (unstable) | 1 | 1 | **5** | 5 | 5 | 4 (webviews cross-window OK) |
| Memory-reduction APIs (Low level, invalidation) | 2 (partial passthrough) | 2 | 2 | **5 (full SDK)** | 5 | 5 | 1 |
| Notification path control (host toasts + per-service policy) | 3 (plugin) | 3 | 3 | **5** | 4 | 4 | 3 |
| Shell thickness / overhead ("weight") | 4 | 3 | 4 | **4** | **5** | 5 | 1 |
| Dev speed / maintainability | 3 | 3 | 3 | **5** | 2 | 3 | 4 |
| API stability on our critical path | 2 (unstable flag) | 4 | 3 (beta) | **5** | 5 | 4 | 4 |
| Windows-native UX fit | 4 | 3 | 3 | **5** | 5 | 5 | 2 |
| Install size | 4 | 4 | 4 | **4** | 5 | 5 | 1 |

**Recommendation: D — .NET 10 WPF host + official WebView2 SDK** (A–C lose on our critical architecture;
E/F are acceptable second-tier choices with higher cost; G is the baseline we try to beat).

Secondary rationale for WPF over WinForms: DPI correctness, easy frameless chrome (WindowChrome), no extra
dependencies for a 44–48 px sidebar + toasts + tray. If M0 measurements show the .NET host overhead is
material (>~80 MB private) and unacceptable, the same architecture ports to **E (C++ Win32)** because the
WebView2 usage is identical — the shell is thin by design.

---

## 3. Recommended technology stack (final)

| Layer | Choice | Reason |
|---|---|---|
| Host runtime | **.NET 10 LTS, WPF** | Support through Nov 2028; first-class WebView2; no runtime bundling needed (framework-dependent bootstrap in installer; self-contained fallback optional). |
| Web engine | **Microsoft.Web.WebView2 (latest stable SDK)** + Evergreen runtime | The exact Chromium the OS already ships; no bundled Chromium (saves ~150–200 MB install vs Electron). |
| Process model | 1 Environment / 1 UDF / **4 profiles** | One browser process tree for four apps (the entire memory thesis). |
| Shell UI | Hand-written WPF: custom titlebar (`WindowChrome`), 46 px sidebar, no third-party UI kit | No heavyweight dependency; native look; cheap. |
| Toasts | `CommunityToolkit.WinUI.Notifications` (ToastNotificationManagerCompat) or direct WinRT interop | Native Action Center notifications; AUMID shortcut auto-created for app id. |
| Tray | `System.Windows.Forms.NotifyIcon` via WPF interop | Zero new dependencies. |
| Badges | `DocumentTitleChanged` parser (+or DOM probe later) | Title is exactly what services export for this purpose; no fragile scraping V1. |
| Config | JSON service descriptors + JSON `settings.json` in `%LOCALAPPDATA%\United\` | Per-service quirks live in *data*, updatable without app update (WATG lesson). |
| Autostart | `HKCU\...\CurrentVersion\Run` (toggleable) | Simple, work-Safe. |
| Installer | Inno Setup (or zip-off portable) | Small, fast, no MSIX signing burden; portable variant supported. |
| Iconography/fonts | Segoe Fluent Icons (system), inline SVG on demand | No asset bloat. |
| License hygiene | Apache-2.0/MIT attribution only if any Ferdium/WATG lines adapted | Read-derived knowledge, own code by default. |

**Explicitly chosen against:** Shipping Electron or any bundled Chromium; forcing Wails/Tauri because of
"modern framework" optics; multi-process multi-window layouts for the four services.

---

## 4. Application architecture

```
┌─────────────────────────────────────────────────────────────────────────────┐
│ United.exe (WPF host, single process, single instance mutex)                 │
│                                                                             │
│  Shell (native WPF chrome, ~500 LOC visible layer)                          │
│  ├── Sidebar (46 px): 4 service icons + unread badges + status dot           │
│  ├── ContentPanel: hosts THE webview controls (one visible at a time)        │
│  ├── Sparkle-free window drag/resize (WindowChrome); titlebar w/ global     │
│  │   unread total + service name; theme switch (auto/light/dark)            │
│  └── TrayIcon + menu (Show / per-service suspend / Quit)                    │
│                                                                             │
│  Core (no UI deps)                                                          │
│  ├── ServiceRegistry        – loads config/services/*.json                  │
│  ├── SessionHost            – CoreWebView2Environment(UDF), controllers      │
│  │                            created with ProfileName=<service>            │
│  ├── LifecycleManager       – state machine per service (see §5)             │
│  ├── MemoryGovernor         – MemoryUsageTargetLevel Normal/Low per state     │
│  ├── ReadStateWatcher       – DocumentTitle parsing → badge model             │
│  ├── Notifier               – toast policy, dedupe, suppression, click→focus  │
│  ├── PermissionBroker       – PermissionRequested policy per service          │
│  ├── WebParamConfigurer     – UA override / user-data tweaks / downloads      │
│  ├── HealthMonitor          – RenderProcessUnresponsive/Failed → recovery     │
│  └── MetricsService         – private WS of host + browser-process tree       │
│                                                                             │
│  Platform (interop)                                                         │
│  └── WinRT toasts · NotifyIcon · Run-key autostart · single-instance mutex   │
└──────────────┬──────────────────────────────────────────────────────────────┘
               │  (one shared browser process tree)
   ┌───────────▼─────────────────────────────────────────────────┐
   │ msedgewebview2.exe tree (browser+GPU+network+storage+4×renderer)│
   │  profile: whatsapp │ telegram │ instagram │ discord            │
   └───────────────────────────────────────────────────────────────┘
```

**Data flow for the notification pipeline (the critical one):**

```
service websocket → page JS event → (a) page Notification API → WebView2 permission already granted
                                    (b) page sets document.title "(3) WhatsApp"
      → DocumentTitleChanged → ReadStateWatcher: diff count
      → badge model update (sidebar + tray numeral)
      → Notifier: if configured hosting-authoritative → host toast (AC clicked → service focus)
                  elif service-authored works → host subtract (suppress duplicates)
```

---

## 5. WebView lifecycle & memory management design

Per-service state machine (LifecycleManager):

```
Created ──load──▶ Loading ──▶ Ready ⟲  (visible: active service only)
                    │
   Focus/blur events│
                    ▼
             Backgrounded(service not selected)  → MemoryUsageTargetLevel = Low
                    │                              (scripts+connections keep running)
   user clicks tray "Hibernate <svc>" (or optional auto, OFF by default)
                    ▼
             Hibernated  → controller.Close(); renderer freed; profile intact
                    │     badge shows ∅/grey; notif state for this service = suspended
   user selects ⟵── reload on demand (session intact, ~1–3 s to usable)
```

Rules encoded:

1. **Exactly one webview is `IsVisible=true`** (the selected service); others are attached but
   `IsVisible=false` *only if the service tolerates visibility change* — otherwise Low level and let the
   compositor skip them. Visibility/gone-away verification in M0 spikes (does IG/TG misbehave when
   `visibilityState=hidden`? WATG-style workarounds only if evidenced).
2. **Low level on the fly**: whenever window → tray, or another service is focused, previous services go
   Low. Foregrounding restores Normal *only for that service*. Restore must not happen automatically after
   Low→Normal only for the FOCUSED webview (docs: no auto-restore).
3. **No `TrySuspend` mixing.** Suspended ≠ running; we need scripts running for notifications.
4. **No unconditional renderer recycling**: sessions survive restart/hibernate precisely because
   controller teardown does not touch profile storage. Avoid destroy-recreate churn except:
   manual hibernate, renderer crash recovery, user "reload service".
5. **Startup ladder** (tuned): create the 4 profile controllers staggered (~120 ms apart) so first paint + a
   usable font/UI appear quickly; services marked "notify-only at startup" (settings toggle, default all)
   still start fully — they must, to receive messages.
6. **Hibernate policy**: default OFF for all (priority #2 notifications), exposed per service; if a user
   enables auto-hibernate for Discord,亦 fine (their choice; toast about trade-off in the settings text).
7. **GPU/paint pressure**: prevent re-render churn by never re-layouting webviews besides attach/detach
   (exact host geometry reuse); sidebar badges update via data binding, not DOM timers.

---

## 6. Per-platform authentication & notification strategy

| Service | Session model | Auth flow | UA / quirks | Unread source | Notification policy |
|---|---|---|---|---|---|
| WhatsApp | Profile `whatsapp` — cookies+IndexedDB persist | Link-device QR in-page (stepped auth); never automated client-side beyond QR display | Default WebView2 Edge UA (the official Windows client IS a WebView2 wrapper today — no spoof) | Title `(n) …` regex + in-page Notification API | Auto-grant `Notifications` on this host; WA-authored toasts verified in M0 → authoritative if working, else host toasts from title deltas |
| Telegram | Profile `telegram` | In-page phone/email + 2FA (user types it; no extraction) | Default UA | Title `(n) Telegram …` | Same policy; sounds allowed in foreground |
| Instagram | Profile `instagram` | In-page standard login; optional "saved browser" flows pass | Default UA; fallback UA table (Chrome/Edge versions) if login rejected | Title/dominant count probe (exact format verified in M1) | Notifications auto-grant; IG-authored preferred |
| Discord | Profile `discord` | In-page password; QR cross-device login stays in-page | Default UA; `PermissionRequested` mic/cam granted for voice | Title `(n) Discord \| …` | Auto-grant `Notifications`; authored toasts preferred |

**Hard rules (ToS/security):** no headless automation/self-bot, no token/credential harvesting, never
machine-parse message content, no MITM, QR/OTP remain user-entered inside the page. Everything dangerous
injected must stay limited to (a) title-reading only, or (b) optional cosmetic CSS per service that can be
enabled/disabled (keep-out default).

**Deduplication & reliability design:** the Notifier always knows which authority is live per service
(authored-vs-host) and switches on first observed double-toast or silence; a per-service
`notificationMode: { authored | host | off }` config with default `authored` where M0 verifies it works.
Click-to-conversation: where in-page notifications support click focusing, keep service behavior; host
toasts at minimum restore the app and foreground the right *service*; per-conversation deep links are
explicitly **out of scope V1** (none of the four exposes a stable deep link from our position without
invasive scraping).

---

## 7. UI component structure

Thin shell, zero-fat dependency tree:

```
AppShell (Window, WindowChrome, custom 36px titlebar)
├── TrayIcon                    (NotifyIcon; badge numeral; menu)
├── SidebarView (46 px)
│   ├── ServiceButton ×4        (icon, active dot, unread pill, tooltip w/ counts)
│   ├── SettingsButton
│   └── ThemeToggle (auto/light/dark)
├── ContentHost (Grid)
│   ├── WebViewContainer ×4     (swap-in Geometry; single visible)
│   └── ServiceStateOverlay     (loading / offline / crashed / hibernated card)
├── MetricsWindow (settings panel, "resource monitor")
│   └── per-service: RAM now, RAM now (Low), uptime, restarts, CPU%
└── SettingsWindow (list-detail; no tabs)
    ├── General (autostart, tray behaviour, language)
    ├── Services (open/reload per service, notification mode, hibernate)
    ├── Appearance (theme, compact density)
    └── Advanced (UA per service, downloads dir, diagnostics export)
```

Design language: rounded 6 px, 4 px grid spacing, Segoe UI Variable 12/13 px labels, `InfoCard`-style
secondary text muted, single accent color derived per-service brand for icons only, **no gradients**,
transitions only: 120 ms crossfade at service switch + 90 ms sidebar hover tint. Keyboard: `Ctrl+1..4`
service, `Ctrl+K` service palette (optional), `Ctrl+W`→tray, `Ctrl+H` hibernate, `Ctrl+,` settings.

---

## 8. Proposed project directory structure

```
united/
├── src/
│   ├── United.sln
│   ├── United.App/                  # WPF host ((entry), xaml + code-behind, thin)
│   │   ├── Shell/                   # AppShell, Sidebar, ContentHost, Overlays
│   │   ├── Views/Settings/          # metrics, settings windows
│   │   ├── Resources/               # icons.xaml, styles.xaml, theme dicts
│   │   └── app.manifest
│   ├── United.Core/                 # logic, no UI deps (unit-testable)
│   │   ├── Services/                # ServiceRegistry, ServiceDef, per-service JSON
│   │   ├── Threading/               # LifecycleManager, FocusTracker
│   │   ├── Memory/                  # MemoryGovernor, HibernatePolicy
│   │   ├── ReadState/               # TitleParser, BadgeModel, Deduper
│   │   ├── Notifier/                # ToastAuthor, ToastDispatcher, dedupe rules
│   │   ├── Permissions/             # PermissionBroker policy
│   │   ├── Config/                  # settings.json IO, defaults
│   │   └── Diagnostics/             # MetricsService, log sink
│   ├── United.Platform.Windows/     # NotifyIcon, toasts (AUMID), Run-key, DPI helpers
│   └── config/services/*.json       # whatsapp.json, telegram.json, instagram.json, discord.json
├── tests/
│   ├── United.Core.Tests/           # title parsers, state machine, dedupe, config
│   └── United.Platform.Windows.Tests/  # toast JSON, autostart guard
├── tools/
│   └── bench/                       # PowerShell measure scripts, scenarios, exporter
│       ├── measure-ram.ps1          # private WS of host + browser tree
│       └── scenarios/{s0..s5}.md
├── installer/                       # inno setup script; portable zip step
├── docs/                            # this plan, benchmark reports, service notes
│   └── benchmarks/
└── PLAN.md
```

The `United.Core`/`Platform` split keeps the (future) C++ port option realistic and makes the hot logic
(lifecycle, dedupe, parsers) pure and testable.

---

## 9. Development milestones & implementation order

### M0 — Spike & decision gate (2–3 days)
A throwaway console/WPF skeleton, no shell polish yet. Prove the four big unknowns:
1. Shared environment + 4 profiles → single `msedgewebview2.exe` tree confirmed (Task Manager + code).
2. Logged into WA/TG; **title unread deltas captured** during real messages with the window in the tray.
3. `MemoryUsageTargetLevel` Low/Normal — measured RAM swing per hidden service; check notifications still arrive.
4. Web notification authored in-page → does Windows show it after permission grant? (and AUMID need?)
**Exit criteria:** written spike notes + 1 benchmark table; the architecture (or a documented pivot) chosen
from real data.

### M1 — Shell & sessions (3–4 days)
Sidebar shell, 4 real services, theme, geometry, WindowChrome, tray show/hide, project structure set.
**Exit criteria:** switch services < 150 ms visual; sessions persist across restart and reboot; cold start
< 3 s to usable sidebar (measured, no claim blind), app icon + name correct.

### M2 — Notifications & badges (3–4 days)
Title parser per service, badge model, tray numeral, host toast author, dedupe logic, click behaviors,
per-service notification mode config, permission broker.
**Exit criteria:** toast on test message from each platform while app is in tray and window minimized;
dedupe verified in soak (no repeat toasts for the same count change); badge numbers correct within one JS
event; zero toasts during normal read activity.

### M3 — Memory governance & metrics (2–3 days)
MemoryGovernor (Low on background, Normal on focus), Hibernate per service, auto-hibernate policy setting,
metrics panel, health-based reload, crash recovery (renderer fail → card UI + restore).
**Exit criteria:** 24-h idle soak with 4 services tray'd: no OOM growth, no badge drift, RAM median token
(written up in docs/benchmarks), user-initiated hibernate releases renderer process (Task Manager verified),
restore from hibernate < 4 s with session intact.

### M4 — Media & edge-case integration (2–3 days)
Downloads UI (per-profile download folder), clipboard, emoji pickers (native), camera/mic grants &
per-service persistence, file upload picker, link routing (external links out), keyboard scan, DPI/multi
monitor, compact-width layout (min ~560 px), zoom shortcuts (Ctrl+ wheel).
**Exit criteria:** WA calls in-page (audio) hold 10-min test; Discord voice joins and speaks; screen-view
works; file at 100 MB transfers intact out through downloads path; no download dialog duplicates.

### M5 — Settings, theme, autostart, packaging (2–3 days)
Settings window (detail view), theme store restore, autostart toggle, About/diagnostics export,
installer build (Inno) + portable zip, first-run wizard (per-service links + permission explainer).
**Exit criteria:** uninstaller removes app data selection (ask), autostart reliable at boot test,
install size verified, SmartScreen honestness (unsigned note in docs).

### M6 — Test campaign & benchmark report (3–4 days)
Full ea §11 matrix executed; benchmark scenarios (§12) run & recorded; failing UX polished.
**Exit criteria:** a committed `docs/benchmarks/2026-…-report.md` with methodology + numbers (no claims).

### M7 — Hardening & polish (2 days)
Theme edge cases (high contrast), long-title crash, notification storm throttle cap (e.g. one toast per
4 s per service slider), logging, README, final code review.

Total: **~3–3.5 focused weeks** single-dev.

---

## 10. Risks, limitations, alternatives

| # | Risk | Probability | Impact | Mitigation |
|---|---|---|---|---|
| 1 | Service DOM/title auth changes break badge (esp. WhatsApp — WATG reports this history) | **High over months** | Badge/toast quality | Title-first (robust); selectors in config JSON, hot-reloadable; monitor Ferdium-recipes changes for early warning; graceful "unknown" badge triangle |
| 2 | In-page notifications don't render as OS toasts in this app (WebView2 quirk [#5051](https://github.com/MicrosoftEdge/WebView2Feedback/issues/5051)) | Medium | Higher-level experience of notifications | Host-authored toasts path present from M2 (never blocked); per-service switch; doc note |
| 3 | Instagram login friction / UA suspicion inside embedded webview | Medium | Service unavailable | UA fallback table; in-place "open in system browser to complete login then return" escape hatch; persistent cookies make suspicion rare after first pass |
| 4 | Discord screen-share picker dialog bugged in WebView2 ([#2184](https://github.com/MicrosoftEdge/WebView2Feedback/issues/2184)) | High | Feature annoyance | Cosmetic known issue; report/track; fallback "share from browser" documented; voice unaffected |
| 5 | `MemoryUsageTargetLevel Low` savings weaker than hoped for a workload | Medium | Benchmark credibility | It is a measured outcome; fallback: hibernation by default for user-chosen services; stricter process every switch |
| 6 | One UDF one instance ⇒ second launch breaks ([#2503](https://github.com/MicrosoftEdge/WebView2Feedback/issues/2503)) | Certain by design | Annoyance for user | Single-instance mutex + focus existing; documented limitation |
| 7 | Webviews' background hidden-visibility quirks per service (e.g. IG goes idle) | Medium | Badge silence | Visibility-mode toggle per service retained in design (stay-visible overlay offscreen layout for edge cases); evidence from M0 decides |
| 8 | Timer throttling obscures services that rely on polling for badge state | Medium | Delayed badge (≤1 min) | Accept small badge delay; toast authority can be authored-notification path where events push directly |
| 9 | .NET host overhead larger than ideal | Low | Benchmark optics | Thinner self-host attempted if needed (NativeAOT not available with WPF; but shell is small); documented path to C++ port |
| 10 | Upstream runtime auto-update (Evergreen) shifts behavior | Medium | Sudden regression | Pin awareness: log WebView2 version; smoke test after monthly runtime; `AdditionalBrowserArguments` pin for last-known-good options if needed |
| 11 | Legal/ToS: wrappers vs platform rules | Low | Existential | No automation, no credential interception, no client mods; official web endpoints only — the same endpoints the official WA Windows app (WebView2 wrapper) uses |
| 12 | Notification storm from group chats | Medium | UX annoyance | Rate cap + aggregate badge; per-service mute passthrough (respect in-page mute) |

**Alternative approaches explicitly parked:** TDLib-native Telegram client (§1.12); merging all four
conversations into a custom inbox (violates "native interface" requirement); per-service windows
(Wails/Tauri-stable model) — rejected on UX; Electron fork (memory anti-goal).

---

## 11. Test strategy

**Automated (CI, fast):**
- Unit: title regex parsers (golden corpus of real titles), dedupe logic, state machine transitions,
  config loader/val, toast text builder, autostart guard, single-instance guard.
- Integration (headless XUnit + WebView2 offscreen controllers): create 4 profiles with one environment;
  navigate to data: pages proving profile isolation (cookies not leaked cross-profile), Low level applied
  via reflection to idle webviews.

**Manual matrix — per service:**
| Case | WA | TG | IG | DC |
|---|---|---|---|---|
| Login fresh (QR/2FA) | ✓ | ✓ | ✓ | ✓ |
| Session survives restart, reboot | ✓ | ✓ | ✓ | ✓ |
| Toast received while tray'd, focus works | ✓ | ✓ | ✓ | ✓ |
| Incorrect-password / network down UX | ✓ | ✓ | ✓ | ✓ |
| media upload/download, clipboard, emoji | ✓ | ✓ | ✓ | ✓ |
| audio call quality, camera grant, revoke | ✓ | ✓ | n/a (DM suggests phone app) | ✓ |
| dark-mode consistency, zoom, compact | ✓ | ✓ | ✓ | ✓ |
| hibernate → memory → restore session | ✓ | ✓ | ✓ | ✓ |

**Soak & recovery:** 24 h idle (§12 S4); kill each `msedgewebview2` renderer randomly ×10 (recovery, no
badge corruption); disable network for 10 min then restore (graceful card + retry, connection restored).

**Update resilience:** monthly (or on runtime update) regression of badge formats via golden corpus +
one live login per service.

---

## 12. RAM / CPU benchmarking methodology

**What to measure (no aggregation sleight of hand):**
- **Private Working Set** (unique to that process, no double counting) and **Private Commit** of:
  the host process ; each `msedgewebview2.exe` in our tree, tagged by role
  (browser/gpu/network/storage/audio/render) + profile (via `--user-data-dir` cmdline +
  `CoreWebView2.BrowserProcessId`/`RenderProcessId` APIs).
- My honest total for summary: **∑ private working sets** (shared pages among our own processes count
  once via the roles), plus a *separate* note: machine-wide Task Manager "memory" column should not be
  added across processes (shared memory double-counting) — we report per-role detail instead.
- CPU: `GetProcessMemoryInfo` + `GetSystemTimes`/ETW context switches per scenario; timers (wakeups/s) via
  ETW if needed for "CPU wakeups" claims.

**Scenarios (scripted, reproducible):**
| ID | Scenario | Metric focus |
|---|---|---|
| S0 | Cold start → idle 10 min (4 connected) | peak, endpoint RAM |
| S1 | Focus rotation 1 min per service ×5 | switch cost, leaks |
| S2 | Minimized 30 min, incoming msgs (real test accounts) | RAM, delivery success % |
| S3 | Hibernate toggle per service, twice each | savings, restore time |
| S4 | 24-h Soak | drift, cumulative RSS, event loss |
| S5 | All four active with calls (voice test) | CPU, RAM under load |

**Method details:** median of ≥ 3 runs, 10-min warm-up, pinned `msedgewebview2` runtime version recorded,
machine specs + background apps noted, benchmark scripts committed (`tools/bench/`). **Counterfactual
baselines measured on this same machine:** user's current setup (4 separate apps), WhatsApp official
wrapper alone, WATG — so claims are always relative to *measured* baselines, never to vendor numbers.
Every claim sentence in reports must cite which scenario produced it.

---

## 13. MVP acceptance criteria

- [ ] All four services usable in one window with their **native web interfaces** (not custom inboxes).
- [ ] **Single** `msedgewebview2` browser-process tree shared by all services (verified by PID probe).
- [ ] Sessions **persistent**: restart app, reboot Windows — all four remain logged in.
- Sidebar: icons ×4 with numeric unread; active check; hover tooltips.
- [ ] Windows native toasts for incoming messages while minimized/tray'd for each service (soak-verified);
      **click** toast → app opens with that service focused.
- [ ] Per-service unread badge = title-derived (primary) — no DOM scraping required for V1.
- [ ] Tray: badge numeral, context menu (Show, hibernate toggles, notification mode, quit), minimize-to-tray.
- [ ] Background: tray'd app receives messages ≥ 30 min soak with **zero missed** on test accounts (S2).
- [ ] Memory: Low target level engages for backgrounded webviews (traceable in code path); per-service
      hibernate + auto-restore; settings panel shows live per-service RAM.
- [ ] Benchmarks (S0–S5) executed; report committed. **No unmeasured claims.**
- [ ] Startup < 3 s to interactive sidebar (measured); service switch visually instant (< 150 ms).
- [ ] Mic, camera, clipboard, emoji, file upload/download all verified per service (M4 matrix).
- [ ] Light + dark themes, system-follow; keyboard shortcuts §7 working; compact-width usable at 560 px.
- [ ] Settings: autostart, theme, per-service notification mode, hibernation, downloads dir, metrics.
- [ ] Crashed renderer → clean recovery card, no session loss; external links default to user's browser.
- [ ] Uninstall leaves a user-choice on data; app exit (real quit) frees all child processes.

---

## 14. Confirmed facts vs assumptions to verify

**Verified (source-backed):**
1. One environment + one UDF + named profiles ⇢ shared browser process + isolated sessions (§1.1–1.2). ← *architectural cornerstone, stable public API*
2. `MemoryUsageTargetLevel` Low runs scripts and keeps connections, expressly to serve apps like ours (§1.3).
3. `DocumentTitleChanged` event exists; title-based unread is exactly what Ferdium recipes/`+WATG` rely on per-service (§1.5,1.9,1.10).
4. WebView2 `PermissionRequested` centralizes notification/camera/mic grants, no browser prompt (§1.5).
5. Web-notification quirks/push inconsistency exist in the wild — belt-and-suspenders host toasts required (§1.5).
6. Tauri 2 multiwebview remains **unstable-gated** (§1.6); Wails v3 is a multi-window beta (§1.7).
7. Ferdium = Electron; app Apache-2.0, recipes MIT; structural memory behavior known (§1.9).
8. WATG validates WA+TG-in-one-Tauri-window + toast/cycle UX and documents WhatsApp fragility (§1.10).
9. WhatsApp's own Windows client moved to a WebView2 web wrapper (Nov 2025) — endpoint compatibility
   production-proven, and its naive wrapper (~400 MB background) is the baseline to beat (§1.11).
10. No supported WebView2 "disable background throttling"; Chromium throttles timers but not socket-driven event handlers (§1.4).
11. Chromium `getDisplayMedia` dialog bug in WebView2 affects Discord screen-*share* picker (cosmetic-ish) (§1.11).

**Assumptions → must be validated in M0/M1 (documented spike results will replace these lines):**
A1. Per-service **title formats** (WA/IG/TG/DC) as assumed regexes match live apps and stay stable enough month-over-month.
A2. In-page `Notification` permission granted on the host yields OS toasts under tray state, on both Win10 19041+ and Win11 (before fallback logic proves the badge path necessary anyway).
A3.服务工作/`websocket` connections of WA and TG survive the `Low` target level with **no notification breakage**, including long idle (S2/S4).
A4. Instagram login works without UA spoofing under default WebView2 Edge UA (fallback table exists if not).
A5. Discord voice in WebView2 needs no extra flags beyond permission grants (screen share assumed degraded per #2184).
A6. .NET WPF host overhead stays in the low-tens of MB private (to be measured; C++ port path documented as mitigation).
A7. Staggered controller creation reaches "sidebar interactive < 3 s" without lazy-loading pain.
A8. TDLib is not needed for our memory goal (re-revisit only if Telegram Web's share of the measured total is dominant).

---

## 15. Open questions for the owner

1. Windows-only first? (Plan assumes yes; Tauri/Wails alternatives exist if cross-platform becomes a goal — different conversation.)
2. Naming/branding preference (working title **United** is placeholder).
3. OK to require Windows 10 19041+ (pre-installed WebView2) with installer fallback for older/low-RAM systems?
4. Do you want the **benchmarks run against your actual machine** (4 apps installed?) as measured baselines — plan assumes yes; test accounts for soak tests are yours to provide.

---

*End of plan. Implementation proceeds only after M0 decision gate. No code has been written in this phase.*
