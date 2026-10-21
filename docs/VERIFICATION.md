# Verifikasi Fitur

Tanggal verifikasi: **21 Oktober 2026** · Windows 11 (build 10.0.26100), layar 1920×1080 @125%, United 0.1.0 Release build.

Dokumen ini membedakan **sudah diuji** dari **belum diuji**. Jangan mengklaim fitur yang belum masuk kolom “Lulus”.

## 1. Otomatis

| Uji | Perintah | Hasil |
|---|---|---|
| Build | `dotnet build src\United.App\United.App.csproj -c Release -r win-x64 --self-contained false -t:Rebuild` | ✅ 0 warning, 0 error |
| Unit test | `dotnet test tests\United.Core.Tests\United.Core.Tests.csproj` | ✅ 54/54 lulus |
| Normalisasi timestamp notifikasi | `NotificationTimeTests` (default, UTC, local, unspecified UTC, unspecified local) | ✅ timestamp UTC WebView2 tidak lagi mundur 7 jam |
| Migrasi Facebook | `SettingsAndLifecycleTests.Load_FromVersionlessFile_AddsFacebookOnce` | ✅ facebook disisipkan setelah instagram, idempoten |
| Facebook sudah ada | `Load_WithFacebookAlreadyPresent_DoesNotDuplicate` | ✅ tidak duplikat |
| Katalog Facebook | `BuiltIn_IncludesFacebook` | ✅ id/nama/icon/URL |
| Katalog built-in valid | `BuiltInDefinitions_AreValid` | ✅ semua lolos `IsValid` |
| Target & unread ActivityStore | `Add_StoresTargetAndUsesAuthoritativeUnread` | ✅ target tersimpan, Count mengikuti unread baris (0/2), notifikasi tetap increment |
| Judul Discord bukan badge DM | `DiscordTitleMentions_AreNotTreatedAsDmBadge` | ✅ `(29761) Discord …` tidak cocok (TitlePatterns kosong) |
| Identitas percakapan stabil | `Add_SameTarget_KeepsOneConversationWhenTitleChanges` | ✅ judul berubah, target sama → satu baris, judul terbaru, Count snapshot tidak menumpuk |
| Dedupe data lama | `Load_DeduplicatesEntriesForTheSameTarget` | ✅ dua entri target sama digabung, body terbaru, Count = max |

Unit test mencakup: settings round-trip, file korup dikarantina, migrasi, katalog/resolusi JSON, unread parser, notifikasi gate, normalisasi timestamp notifikasi, lifecycle slot state, identitas target, dan dedupe activity.

## 2. Manual / UI Automation

Skrip: `tools/verify-ui.ps1` + UI Automation (PowerShell), screenshot via `PrintWindow` (DPI-aware).

| Skenario | Bukti | Status |
|---|---|---|
| Aplikasi start, 6 layanan default (WhatsApp, Instagram, Facebook, Telegram, Discord, Threads) | log `initialized with 6 service(s)`; sidebar 6 radio | ✅ |
| Facebook tampil sebagai layanan (default + migrasi) | `images/facebook.png`; sidebar urutan ke-3 | ✅ |
| Halaman Facebook dimuat di WebView2 | halaman login Facebook tampil, judul `United · Facebook` | ✅ (login nyata belum) |
| WhatsApp official logo | sidebar + halaman QR | ✅ |
| Ganti service | Instagram → WhatsApp, state tetap | ✅ |
| Settings terbuka dari gear | screenshot `settings-general.png` (tidak disimpan di repo) | ✅ |
| Halaman Services: 6 baris, logo brand, URL, state + id, Enabled, mode | `images/services-top.png` | ✅ |
| Daftar “Add a service”: X, Messenger, Reddit, LinkedIn, Bluesky, YouTube + logo resmi | `images/services-bottom.png` | ✅ |
| Tambah layanan bawaan (X) | `settings.json` → `x`; sidebar 7; state ready | ✅ |
| Disable layanan (X) | `settings.json` tanpa `x`; sidebar kembali 6 | ✅ |
| Tambah layanan kustom (Test Chat, `https://example.com/chat`) via dialog | `services.json`, `icons/test-chat.png`, sidebar ke-7, halaman example.com | ✅ |
| Logo kustom layanan baru tampil | ikon oranye “T” di sidebar & Settings | ✅ |
| Hapus layanan kustom (Remove → Yes) | `services.json` = `[]`, `icons/test-chat.png` hilang, sidebar 6 | ✅ |
| Change logo WhatsApp (Settings) | `icons/whatsapp.png` 128×128, sidebar live berubah | ✅ (gambar uji, sudah di-reset) |
| Reset logo WhatsApp | file terhapus, logo brand kembali, log `custom logo cleared` | ✅ |
| Add custom dialog: field, preview, validasi, mode | `images/add-service.png` | ✅ |
| Context menu sidebar: Reload/Hibernate/Change logo/Reset/Open in browser | `images/context-menu.png`; 5 item | ✅ |
| Reset logo nonaktif jika tidak ada logo kustom | UIA `IsEnabled=false`; terlihat di screenshot | ✅ |
| Hibernate via context menu | menu berubah Hibernate → Resume | ✅ |
| Resume via context menu | halaman dimuat kembali; menu kembali Hibernate | ✅ |
| Hibernate via Settings + overlay | overlay “is hibernated”, tombol Resume | ✅ |
| Resume via overlay (perbaikan bug) | setelah Resume, `IsDormant=false`; menu menampilkan Hibernate | ✅ (diperbaiki 20 Okt) |
| Status hibernate tersimpan di settings | setelah perbaikan, `hibernatedServices` terisi saat aksi | ✅ |
| Services page scrollbar bertema | thumb tipis, tanpa track putih/arrow putih | ✅ |
| Teks `MutedText` wrapping | hint Services tidak terpotong | ✅ |
| Settings Performance & About | screenshot; path & metrik tampil | ✅ |
| Jendela muat di area kerja | 1475×950 fisik di 1920×1080; clamp WorkArea | ✅ |
| Context menu otomatis verify-ui | sempat gagal karena foreground/instance; berhasil manual | ⚠️ skrip perlu perbaikan robustness |
| **All messages** (`Ctrl+0`): daftar aktivitas semua layanan | daftar terisi, badge unread, klik baris membuka layanan | ✅ |
| All messages: tidak ada daftar chat dobel + tanpa garis pemisah | screenshot WhatsApp/FB/IG/Discord; hanya satu pane chat, seam bersih | ✅ |
| All messages: balas langsung di webview asli | editor chat fokus setelah reveal; tidak ada pesan terkirim otomatis | ✅ |
| Focus mode per layanan menyembunyikan sidebar kontak | WhatsApp/Facebook/Instagram/Discord; Telegram & Threads belum (logout) | ✅ / ⚠️ |
| Kembali ke mode normal memulihkan sidebar layanan | WhatsApp normal setelah keluar dari All messages | ✅ |
| Settings selalu di depan United | jendela owned + activate; screenshot Settings di depan | ✅ |
| OAuth popup in-app (Google) | `window.open(accounts.google.com)` → popup United “Gmail”, profil sama, di depan | ✅ (login penuh belum) |
| Tema United → WebView2 | light & dark: `prefersDark` sesuai, body WhatsApp ikut terang/gelap | ✅ |
| Startup dengan 6 layanan hibernated | runtime-target Release log `hibernation at startup: 6 asleep`; tidak ada `creating service webview`; proses WebView2 United = 0 setelah 65 s | ✅ (20 Okt 2026) |
| Benchmark runtime | Debug/Release 0 warning/error; 54/54 test; 3 skenario dicatat dengan raw process data | ✅ terbatas; [laporan](benchmarks/2026-10-20-runtime-report.md) |
| Observer inbox Telegram (21 Okt) | log `live rows [telegram] 12`, `unread count [telegram] 12`, `live activity observed/stored [telegram]` | ✅ |
| Observer inbox Discord (21 Okt) | 13 anchor DM di DOM saat DM terbuka (sidebar diparkir 340×720, `opacity:0`); `live rows [discord] 6` (baris ber-preview), `unread count [discord] 0` | ✅ |
| Badge Discord = ikon Direct Messages (21 Okt) | uji DOM terkontrol: badge home `3` → log `unread count [discord] 3`; dihapus → `0`; pill baris `1` → `1`; ikon Server Tag tidak dihitung | ✅ |
| Klik inbox Discord membuka DM yang sama (21 Okt) | log `reveal target [discord] "/channels/@me/<id>" -> "clicked-target"`; tidak ada baris `navigate`; URL akhir `/channels/@me/<id>` | ✅ |
| Observer Discord saat DM terbuka (21 Okt) | event preview masuk saat pane All messages menampilkan DM (bukan daftar DM): `live activity observed` + `stored`; target `/channels/@me/<id>`, Count 0 | ✅ |
| Startup Discord tanpa flood (21 Okt) | restart dengan settle 30 dtk: tidak ada `live activity observed [discord]` untuk preview lama; entri “Server Tag” tidak dibuat | ✅ |
| Priming dua sapuan (21 Okt) | build final start 10:47:30 → observer 10:47:58 → `live rows [discord] 6` 10:48:10, `unread count [discord] 0`; tidak ada event startup. Uji build CDP: setelah priming, marker DOM menghasilkan dua event `live activity observed/stored [discord]` (10:45:53, 10:45:56) | ✅ |
| Seleksi inbox tidak direbut event lain (21 Okt) | saat DM Discord terbuka, marker DOM Telegram memicu `live activity observed/stored [telegram]` tetapi **tidak** ada `reveal target [telegram]`; URL Discord tetap `/channels/@me/<id>`, sidebar tetap diparkir 340×720; UIA 70 detik kemudian masih memilih baris DM Discord | ✅ |
| Identitas percakapan berbasis href (21 Okt) | `activity.json` tanpa target duplikat (`dupTargets=0`); judul Telegram berubah tiap preview tetapi baris tetap satu per `#chat-id` | ✅ |
| Izin clipboard ditolak (21 Okt) | `navigator.clipboard.readText()` → `NotAllowedError`; log `permission [discord] ClipboardRead from discord.com: deny` | ✅ |
| Observer inbox Facebook (21 Okt) | log `live rows [facebook] 9`, event preview tersimpan | ✅ |
| Observer inbox Instagram (21 Okt) | URL awal `/direct/inbox/`; `live rows [instagram] 11`; Notes tray dikecualikan; event preview tersimpan | ✅ |
| Observer inbox Threads (21 Okt) | URL awal `/messages/` (DM web); baris `a[href*="/messages/t/"]`; `live rows [threads] 4`; event preview tersimpan | ✅ |
| Uji serentak 5 observer (21 Okt) | satu run: DM Discord, DM Instagram, inbox Facebook, DM Threads, dan chat bot Telegram → semua `live activity observed` + `stored` | ✅ |
| Observer tetap hidup saat tab lain aktif | log 5 layanan di atas muncul tanpa membuka tab-nya; Telegram diparkir di luar pane (`KeepRenderingWhenHidden`) | ✅ |
| Instagram entri palsu (status “Active …”) | filter presence + masa tenang 10 detik; 0 event palsu setelah perbaikan | ✅ |
| Race snapshot vs sapuan interval | sapuan non-emit hanya mengisi baris baru, tidak menimpa snapshot lama; event Threads yang sempat hilang langsung terkirim | ✅ (diperbaiki 21 Okt) |
| Badge unread sidebar | screenshot: WhatsApp 98, Facebook 20 `(20+)`, Telegram 12, Discord 0 (bukan 2/29759), Threads 1 (dari judul `(1) Messages • Threads`); All messages 29 (saat uji) | ✅ |
| Badge per baris vs judul | Telegram/Facebook dihitung per baris percakapan; Discord memakai ikon Direct Messages (judul `(29761)` = mention server, bukan DM); Instagram/Threads memakai angka dari `document.title` | ✅ |
| Build final portable (21 Okt) | publish 13:03 dari source final; `United.dll` 229888 byte; ZIP `build/United-portable-win-x64.zip` 6.80 MB / 21 entry, SHA-256 `31A742C053CB150FFD2D533981D8CBA0F1852998E0443125EA94892F285F89E0`; string `--remote-debugging-port`/`9222` **tidak ada** di `United.dll`/`United.exe`; port 9222 = 0 saat app berjalan; start 13:03 dari folder publish → 6 observer terpasang, WhatsApp `live rows 19`, tidak ada error/warn | ✅ |
| Payload observer rusak (21 Okt) | JS memakai `String.prototype.toWellFormed` sebelum `postMessage`; host menyanitasi lone surrogate sebelum `JsonDocument.Parse`; field angka memakai `TryGetInt32` sehingga null diabaikan; build 0 warning/0 error; run final 13:03 tanpa ERROR/WARN | ✅ kode siap; belum ada event malformed baru |
| Observer WhatsApp DOM (21 Okt) | `#pane-side` + 19 baris `cell-frame-container`; observer dipasang terlambat lewat `MutationObserver` setelah daftar muncul; log `live rows [whatsapp] 19`; judul dan preview dibaca dari span ber-`title`, bukan teks baris yang diawali label unread; dua perubahan preview nyata pukul 13:05:03 dan 13:05:16 tercatat `live activity observed` + `stored` (judul disamarkan) | ✅ |
| All six observer tetap hidup saat jendela minimized (21 Okt) | build diagnostik sementara: heartbeat 15 detik dari keenam observer terus tercatat setelah UIA melaporkan `WindowVisualState=Minimized` dan rect `-32000,-32000`; WhatsApp/Telegram `visible`, Discord/Instagram/Threads/Facebook `hidden`; build final tidak lagi memuat heartbeat | ✅ |
| Audit Settings via UI Automation (21 Okt) | 67 kontrol interaktif di General/Services/Performance/About; **0 disabled**, semua punya pattern yang sesuai; navigasi ke empat panel dan “Send test notification” berhasil | ✅ |
| Notifikasi WhatsApp live + urutan masuk (21 Okt) | log `web notification received [whatsapp]` 11:00:35, 11:00:56, 11:01:34; `activity.json` menyimpan `04:00:35+00:00`, `04:00:56+00:00`, `04:01:34+00:00` (= 11:00/11:01 WIB); saat notifikasi 11:01 masuk, baris WhatsApp terbaru naik ke posisi teratas tanpa restart/reopen | ✅ |
| Normalisasi timestamp WebView2 (21 Okt) | sebelum perbaikan notifikasi 10:50 WIB tersimpan `03:50+07:00`; setelah `NotificationTime.Normalize` tersimpan `04:00+00:00` dan tampil “now”; 14 entri WhatsApp lama dimigrasi +7 jam dan file cadangan dibuat | ✅ |
| Urutan gabungan setelah migrasi (21 Okt) | dua baris WhatsApp uji (10:54 dan 10:34) tampil di antara entri Telegram sesuai waktunya; tidak ada lagi baris WhatsApp baru bertanggal 03:xx | ✅ |

## 3. Belum diuji (jangan diklaim)

| Area | Alasan |
|---|---|
| Login nyata WhatsApp, Facebook, Instagram, Discord, Telegram, Threads | ✅ dipakai dengan akun nyata di mesin uji; layanan opsional belum |
| Login nyata X/Messenger/Reddit/LinkedIn/Bluesky/YouTube | Belum ada akun uji yang disiapkan |
| Notifikasi dari layanan nyata | Telegram dan WhatsApp: pesan nyata tercatat. Discord/Facebook/Instagram/Threads: pipeline preview diverifikasi lewat perubahan DOM terkontrol, belum dengan pesan masuk nyata dari akun lain |
| Klik toast `united://open/<id>` | Butuh toast nyata |
| Badge Bluesky | Pola judul belum diverifikasi |
| Tray menu: Open/Settings/Hibernate all/Quit via klik | Ikon & menu dibuat; klik belum diotomasi |
| Panggilan audio/video, kamera/mikrofon | Butuh akun & lawan bicara |
| Autostart setelah reboot | Butuh reboot |
| Multi-monitor/DPI campuran | Mesin uji satu monitor |
| Installer/uninstaller | Belum ada installer |
| Soak test (RAM/CPU berjam-jam) | Belum dijalankan |

## 4. Checklist uji manual dengan akun nyata

Gunakan akun uji, bukan akun produksi. Centang setelah lulus.

### WhatsApp
- [ ] Scan QR berhasil; sesi bertahan setelah Quit → buka lagi
- [ ] Kirim/terima pesan di United dan ponsel
- [x] Pesan baru masuk ke All messages secara live dan urut berdasarkan waktu (21 Okt 2026)
- [ ] Notifikasi Host muncul saat jendela di latar; klik toast membuka chat (jika didukung)
- [ ] Badge `(n)` muncul di sidebar & tray
- [ ] Hibernate → Resume; sesi tetap
- [ ] Panggilan audio (jika ingin diuji)

### Facebook
- [ ] Login berhasil dan diarahkan ke `/messages`
- [ ] Kirim/terima pesan
- [ ] Notifikasi & badge
- [ ] Popup login Facebook dari Instagram/Messenger tetap in-app

### Instagram
- [ ] Login (termasuk popup Facebook)
- [ ] DM masuk/keluar
- [ ] Notifikasi & badge

### Telegram
- [ ] Login QR/nomor
- [ ] Pesan & notifikasi
- [ ] Badge

### Discord
- [ ] Login (captcha/2FA)
- [ ] Pesan & notifikasi
- [ ] Badge

### Threads
- [ ] Login via Instagram
- [x] DM web terbuka di `/messages/`; daftar percakapan `a[href*="/messages/t/"]`
- [x] Badge `(1) Messages • Threads` terbaca sebagai 1
- [ ] Notifikasi & pesan masuk nyata dari akun lain

### Layanan opsional & kustom
- [ ] X, Messenger, Reddit, LinkedIn, Bluesky, YouTube bisa login/dipakai
- [ ] Custom service dengan login lintas domain (tambahkan `authPopupHosts`)
- [ ] Logo kustom untuk layanan bawaan + reset
- [ ] `services.json` dengan 2+ entri, `enabledByDefault=false`, Add via UI

### Sistem
- [ ] Close-to-tray, tray Quit, tray Settings
- [ ] Autostart saat sign-in (dengan/tanpa `--minimized`)
- [ ] Tema System berubah live (shell + halaman web)
- [ ] Test notification muncul
- [ ] Shortcut Ctrl+0, Ctrl+1..9, Ctrl+,, Ctrl+H, Ctrl+M
- [ ] Restart aplikasi: layanan aktif terakhir, hibernate, ukuran/posisi jendela
- [ ] Tidak ada layanan ganda setelah launch kedua (single instance)
- [ ] Log tidak berisi error berulang

## 5. Cara menjalankan ulang verifikasi UI

```powershell
powershell -ExecutionPolicy Bypass -File tools\verify-ui.ps1 `
  -Exe "src\United.App\bin\Debug\net10.0-windows10.0.19041.0\United.exe" `
  -OutDir "$env:TEMP\united-verify"
```

Catatan lingkungan uji:

- UI Automation bisa gagal membuka context menu jika jendela tidak foreground; skrip mengklik title bar dulu dan klik kanan item.
- File dialog Windows dieksekusi dengan WM_SETTEXT + WM_COMMAND (bukan SendKeys) agar tidak bergantung fokus.
- MessageBox Yes memakai `WM_COMMAND 6`.
- Jangan gunakan mouse/keyboard saat skrip berjalan.

Log: `%LOCALAPPDATA%\United\logs\united-YYYYMMDD.log`.
