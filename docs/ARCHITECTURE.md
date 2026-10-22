# Arsitektur United Messenger

Dokumen ini menjelaskan bagaimana United bekerja, keputusan desain, dan batasannya. Untuk pemakaian, lihat [TUTORIAL.md](TUTORIAL.md).

## 1. Tujuan & batasan

| Prioritas | Konsekuensi desain |
|---|---|
| RAM rendah | Satu environment WebView2, satu proses browser, profil per layanan, low-memory target untuk latar, hibernate total |
| Notifikasi andal | Intersepsi `NotificationReceived` + toast WinRT sendiri; permission notifikasi diizinkan; dedupe per layanan |
| Sesi persisten | Profil WebView2 per layanan di UDF yang sama; cookie tidak bercampur |
| Startup cepat | Shell WPF ringan; layanan dibuat setelah window tampil; ikon vektor (tanpa raster) |
| Instalasi kecil | .NET + WebView2 yang sudah ada; tanpa UI framework pihak ketiga; aset ikon vektor |
| UI bersih | Sidebar minimal, tema terang/gelap, kontrol kustom sederhana |
| Tanpa backend | Semua data lokal; tidak ada server/telemetri |

Bukan tujuan: menjadi klien API, mengakali ToS, atau menjamin semua fitur situs (panggilan, DRM, dsb.) berfungsi di WebView2.

## 2. Proyek & dependensi

```
United.Core (net10.0)
  ├─ Services/ServiceDefinition, ServiceCatalog, UnreadParser
  ├─ Settings/AppSettings, SettingsStore (JSON, migrasi)
  ├─ Notifications/NotificationGate, ToastPayload
  └─ Logging abstractions
        ▲
United.Platform.Windows (net10.0-windows)
  ├─ WebView2 bootstrap? (lihat App)
  ├─ Notifications/ToastService, Shortcut/AUMID
  ├─ Tray/TrayIconService
  ├─ Startup/Autostart, StartMenuShortcut
  ├─ Activation/ProtocolRegistration, SingleInstance
  ├─ Metrics/ProcessMetricsCollector
  └─ SystemThemes/ThemeManager, ThemeWatcher
        ▲
United.App (WPF, WinExe, x64)
  ├─ App.xaml.cs (composition root)
  ├─ Shell/MainWindow, SidebarItem, IconStore
  ├─ Web/ServiceSlot, NotificationRouter, PermissionBroker, NativeWhatsAppBridge
  ├─ Views/SettingsWindow, AddServiceWindow, NativeWhatsAppPairingWindow
  ├─ Services/CustomServicesFile
  └─ Themes/, Resources/BrandIcons.xaml, Styles.xaml
```

Dependensi eksternal: `Microsoft.Web.WebView2` (NuGet) dan WinRT toast APIs. Tidak ada MVVM toolkit, tidak ada library UI tambahan.

## 3. Startup

1. `App.OnStartup` membuat `SingleInstance` named pipe. Jika bukan instance pertama, argumen `united://...`/`activate` diteruskan ke instance utama lalu proses baru keluar.
2. Mendaftarkan shortcut Start Menu (AUMID toast) dan protokol `united://` (HKCU, per user).
3. Memuat `settings.json`; menerapkan tema; memulai `ThemeWatcher`.
4. Membuat `MainWindow` (shell + sidebar kosong), menampilkan window kecuali `--minimized`.
5. `MainWindow.InitializeAsync`:
   - memulai helper native WhatsApp (jika diaktifkan); helper menulis `status.json`/`qr.png`/`events.jsonl`;
   - memakai status `paired` untuk memutuskan apakah WebView WhatsApp hibernasi;
   - memuat definisi dari `ServiceCatalog.Resolve(enabledServices, servicesJson)`,
   - membuat `ServiceSlot` untuk tiap layanan,
   - membuat WebView layanan aktif terlebih dahulu, lalu layanan lain secara bertahap (kecuali yang berstatus hibernated),
   - memasang event slot (title, navigation, notification, popup, process failure).
6. Membuat tray, metrics timer, dan server pipe untuk aktivasi.
7. Autostart disinkronkan dengan settings.

## 4. Model layanan

`ServiceDefinition` adalah data murni (immutable `init`):

- `Id`, `DisplayName`, `Url`, `Icon`
- `OriginHosts`, `AuthPopupHosts` (domain yang boleh berada di dalam app)
- `TitlePatterns` (regex unread)
- `NotificationMode` (`Host=0`, `Webview=1`, `Off=2`)
- `EnabledByDefault`, `SupportsHibernation`, `UserAgent`

`ServiceCatalog` berisi:

- `BuiltIn` (6 default, urutan sidebar awal),
- `Extras` (X, Messenger, Reddit, LinkedIn, Bluesky, YouTube — ditambahkan lewat Settings),
- `Resolve()` yang menggabungkan built-in + `services.json` + `enabledServices`.

Resolusi:

1. Semua built-in masuk kamus berdasarkan id.
2. Definisi kustom dari `services.json` divalidasi (`IsValid`: id & URL; id tanpa karakter path) lalu menimpa/masuk kamus. Definisi kustom dengan id built-in akan menimpa built-in dan dicatat sebagai masalah.
3. Urutan hasil = urutan `enabledServices` (id yang tidak punya definisi dilaporkan).
4. Definisi kustom non-built-in dengan `enabledByDefault: true` ditambahkan di akhir bila belum ada di `enabledServices`.

Migrasi settings (`SettingsVersion = 1`) menambahkan `facebook` setelah `instagram` sekali saja untuk pengguna lama; idempoten (diuji).

## 5. WebView2 & isolasi sesi

- Satu `CoreWebView2Environment` statis, satu UDF:

  ```
  %LOCALAPPDATA%\United\WebData
  ```

- Setiap layanan membuat controller dengan `ProfileName = sanitize(id)`; WebView2 memisahkan cookie/localStorage/IndexedDB per profil.
- Inactive WebView memakai `Visibility.Collapsed` (bukan `ZIndex`) agar tidak menangkap input dan tidak merender.
- `MemoryUsageTargetLevel.Low` untuk layanan latar (opsional, default aktif).
- Hibernate memanggil `ServiceSlot.Hibernate()` (menutup controller/WebView, melepas renderer) sambil mempertahankan baris sidebar dan status dorman. Resume membuat ulang controller dengan profil yang sama.
- Saat startup, layanan yang tercatat di `hibernatedServices` tetap dorman, termasuk layanan terakhir yang aktif. Uji runtime 20 Okt 2026 mengonfirmasi enam layanan dorman menghasilkan nol proses WebView2; rincian dan batas pengukuran ada di [laporan benchmark](benchmarks/2026-10-20-runtime-report.md).
- Jika proses browser mati: state `Failed` + recreate. Jika renderer crash: reload otomatis.

## 6. Navigasi & popup

`ServiceSlot.IsExternal` menganggap URL **eksternal** jika host tidak cocok dengan `OriginHosts`, `AuthPopupHosts`, maupun daftar host login umum (`CommonAuthHosts`: accounts.google.com, accounts.youtube.com, appleid.apple.com, login.microsoftonline.com, login.live.com, steamcommunity.com, steampowered.com):

- navigasi top-level eksternal → buka di browser default (tab messenger tidak dibajak),
- `NewWindowRequested` eksternal → browser default,
- `NewWindowRequested` internal (termasuk host login di atas) → popup WPF `PopupWindow` dengan profil sesi yang sama (OAuth/login); menutup popup memuat ulang slot asal,
- subresource (gambar, API) tidak dibatasi oleh kebijakan ini.

`ExternalHosts` ada di skema tetapi **belum dipakai** oleh logika navigasi (field direservasi).

## 7. Notifikasi & badge

- `PermissionBroker` mengizinkan: Notifications, Camera, Microphone, ClipboardRead, PersistentStorage; selain itu Deny.
- `NotificationRouter.OnWebNotification`:
  - `Off` → `Handled = true`.
  - `Webview` → biarkan WebView2 menampilkan.
  - `Host` → `Handled = true`; jika layanan sedang terlihat di layar, tidak ada toast (banner in-page situs yang tampil); jika tidak, `NotificationGate` melakukan dedupe berdasarkan judul+body, lalu `ToastService.Show` memakai AUMID `United.Messenger`.
- Toast payload memuat `united://open/{serviceId}`; klik ditangani `SingleInstance`/`ProtocolRegistration`.
- Badge: `UnreadParser` menjalankan `TitlePatterns` berurutan terhadap `document.title`. Group `count` → angka; match tanpa group → titik. Total unread → sidebar + tray.
- **Inbox terpadu**: `ActivityStore` (United.Core) menyimpan maksimal 300 aktivitas terbaru per grup layanan, dipersist ke JSON, dan mengisi tampilan **All messages**. Klik baris memanggil `ServiceSlot.ExecuteRevealAsync` (navigasi JS ke percakapan + klik baris chat), lalu `MainWindow` menampilkan webview layanan itu dalam mode fokus (`InboxFocusCss` menyembunyikan sidebar kontak layanan via selector ber-*guard* `:has()`), sehingga hanya satu daftar chat yang tampil dan tidak ada garis pemisah tersisa.
- Tema: `ServiceSlot.ApplyTheme`/`PopupWindow.ApplyTheme` menyetel `Profile.PreferredColorScheme` + `DefaultBackgroundColor` dari `ThemeManager.Current`; `MainWindow.OnThemeChanged` menerapkan ulang ke semua slot dan popup.

## 8. Pengaturan & file data

`SettingsStore` menulis JSON atomik (`.tmp` + move). Nilai yang disimpan: tema, startup, tray, suara, layanan aktif, versi settings, override mode notifikasi, status hibernate, layanan terakhir, auto low-memory, geometri jendela.

`CustomServicesFile` membaca/menulis `services.json` dengan:

- baca: case-insensitive, komentar & trailing comma diterima,
- tulis: camelCase, indentasi, encoder relaxed agar regex mudah dibaca,
- operasi `AddOrReplace`/`Remove` menulis ulang file.

Catatan: file yang diedit manual akan ditulis ulang (komentar hilang) saat ada operasi UI. Definisi aktif tidak di-reload sampai aplikasi restart.

## 9. Ikon

- `IconStore`: cari override `icons\<id>.<ext>` (png/jpg/jpeg/ico/bmp/gif) → brand vector → glyph globe.
- Override dinormalisasi: decode frame terbesar (ICO) / frame pertama, downscale ke ≤256 px, encode PNG, tulis atomik, hapus varian ekstensi lain.
- Brand vector: `BrandIcons.xaml` berisi `DrawingImage` dengan key `Brand.<Pascal>`; dibuat oleh `tools/convert-logos.ps1` dari `tools/logos/*.svg`.
- Tidak ada raster brand bawaan; ini menjaga ukuran aplikasi dan ketajaman DPI.

## 10. Tray, autostart, aktivasi

- Tray `NotifyIcon` menggambar ikon 32×32 (huruf U + badge merah) saat runtime; menu Open/Settings/Hibernate all/Quit.
- Autostart per user: `HKCU\...\Run` value `United Messenger` = `"<exe>" [--minimized]`.
- Shortcut Start Menu `United Messenger.lnk` diperlukan agar toast WinRT punya AUMID yang valid; dibuat ulang saat start bila hilang.
- Protokol `united://` terdaftar di HKCU; format `united://open/<id>`.
- Single instance via named pipe + mutex; launch kedua meneruskan payload.

## 11. Keputusan & trade-off

| Keputusan | Alasan | Konsekuensi |
|---|---|---|
| WebView2, bukan Electron | Runtime sudah ada di Windows, ukuran aplikasi kecil, API toast/permission memadai | Kontrol terbatas atas perilaku situs; beberapa situs memblokir UA WebView |
| Satu environment | Hemat proses/memori vs banyak environment | Semua layanan berbagi proses browser; crash browser memengaruhi semua (ada recovery) |
| Profil per layanan | Isolasi cookie/login | Satu akun per layanan; akun ganda perlu definisi duplikat |
| Data-driven service | Tambah layanan tanpa kode | Perilaku situs khusus harus diakali lewat field JSON (UA, auth hosts) |
| Toast Host default | Routing & dedupe konsisten | Perlu AUMID/shortcut valid; toast click belum teruji end-to-end |
| Ikon vektor XAML | Kecil, tajam, tanpa aset raster | Menambah brand butuh generator + rebuild; pengguna memakai PNG override |
| Native WhatsApp opsional | WebView WhatsApp menyumbang ~900 MB private; helper Go/whatsmeow hanya ~56 MB | Library tidak resmi, butuh pairing QR, berisiko pembatasan akun; fallback ke WebView |

## 12. Yang belum ada

- Installer/uninstaller (Inno Setup) dan auto-update.
- UI untuk user-agent, proxy, zoom, atau pengaturan per layanan selain JSON.
- Sinkronisasi cloud/backup otomatis.
- Dukungan multi-akun per layanan di UI.
- Pola unread Bluesky yang terverifikasi (Threads sudah terverifikasi dari judul).
- Pengukuran benchmark lengkap S0–S5 dengan akun login.

Lihat [KNOWN-LIMITATIONS.md](KNOWN-LIMITATIONS.md) untuk daftar lengkap.
