# Batasan yang Diketahui

Dokumen ini jujur tentang apa yang **belum** berfungsi, belum diuji, atau memang di luar cakupan United 0.1.0. Tanggal: **21 Oktober 2026**.

## Ringkasan status

| Area | Status |
|---|---|
| Build & unit test | ✅ 0 warning/error; 54/54 test |
| 6 layanan default + Facebook | ✅ tampil, halaman login WhatsApp/Facebook terverifikasi |
| Migrasi settings lama → Facebook | ✅ unit test |
| Tambah/hapus layanan kustom | ✅ diuji end-to-end (service uji “Test Chat”, sudah dibersihkan) |
| Ganti/reset logo | ✅ diuji lewat Settings; sidebar live |
| Add/disable layanan bawaan | ✅ diuji dengan X |
| Hibernate/resume | ✅ diuji lewat overlay dan menu klik kanan |
| Settings Services/Performance/About | ✅ screenshot & layout |
| Login akun nyata (semua layanan) | ⚠️ Telegram, Discord, Facebook, Instagram, WhatsApp dipakai dengan akun nyata di mesin uji; layanan lain belum |
| Notifikasi end-to-end dengan akun nyata | ⚠️ inbox Telegram dan WhatsApp terbukti dengan pesan nyata; Discord/Facebook/Instagram/Threads terbukti lewat perubahan DOM terkontrol, belum pesan masuk nyata |
| Klik toast (`united://`) | ❌ belum diverifikasi dengan toast nyata |
| Badge Bluesky | ❌ pola belum terverifikasi |
| Tray menu (klik item) | ❌ belum diuji otomatis |
| Panggilan suara/video | ❌ belum diuji |
| Benchmark lengkap S0–S5 | ❌ baru baseline terbatas |

## 1. Batasan produk

1. **Bukan klien resmi.** United membuka situs resmi di WebView2. semua ToS, batas perangkat, dan kebijakan layanan tetap berlaku.
2. **Tidak ada API, self-bot, atau ekstraksi token.** Aplikasi tidak membaca/menyadap sesi di luar mekanisme browser biasa.
3. **Satu profil per id layanan.** Tidak ada UI multi-akun. Akun ganda hanya mungkin dengan definisi kustom duplikat (URL sama, id berbeda) — belum diuji dengan akun nyata.
4. **Ketergantungan pada situs.** Jika situs mengubah judul, notifikasi, atau memblokir WebView2, fitur bisa berhenti sampai definisi diperbarui.
5. **Bukan pengganti aplikasi native.** Panggilan/video, DRM, dan fitur perangkat tertentu bergantung pada dukungan WebView2 dan situs; tidak ada jaminan.

## 2. Keterbatasan notifikasi

- Mode **Host** hanya bekerja bila situs memakai Web Notification API yang tertangkap WebView2. Situs yang hanya menampilkan badge in-page tidak menghasilkan toast.
- **Judul halaman** menentukan badge: Threads memakai prefiks `(n)` dari judul `Messages • Threads`, Instagram mengandalkan judul saat situs menampilkannya. Bluesky mengandalkan pola kosong; layanan kustom default `^\((?<count>\d+)\)` mungkin tidak cocok.
- Toast **tidak muncul** saat layanan sedang terlihat di layar (by design; banner in-page situs yang tampil).
- Deduplikasi memakai jeda waktu; notifikasi beruntun dengan isi sama bisa ditekan.
- **Klik toast belum diverifikasi end-to-end.** Protokol `united://open/<id>` terdaftar dan routing diimplementasikan, tetapi belum diuji dengan notifikasi nyata.
- **Timestamp notifikasi WebView2** datang sebagai jam UTC dengan `Kind=Unspecified`. Sejak 21 Okt 2026 United menormalkannya ke instant terdekat, jadi pesan baru tidak lagi mundur sebesar offset zona waktu. Entri WhatsApp lama yang tersimpan `+07:00` dimigrasi sekali; file cadangan mengikuti pola `activity.json.bak-*`.
- **All messages WhatsApp memakai notifikasi web + observer DOM.** Observer membaca judul dan preview baris di `#pane-side` hanya setelah snapshot awal. Chat yang di-mute, notifikasi WhatsApp Web yang dimatikan, atau pesan yang tidak mengubah preview tetap bisa terlewat. Notifikasi tidak membawa href, sehingga klik baris memakai pencocokan judul, bukan target exact seperti Discord.
- Windows Focus Assist, pengaturan notifikasi per aplikasi, dan mode senyap dapat menekan toast di luar kendali aplikasi.
- Suara hanya untuk toast Host; mode Webview mengikuti situs/Windows.

## 3. Keterbatasan logo

- Format didukung: PNG/JPG/JPEG/ICO/BMP/GIF. **SVG dan WebP tidak**.
- GIF animasi tidak dipertahankan; hanya frame pertama.
- Gambar besar diperkecil ke **maksimal 256 px**; gambar kecil **tidak diperbesar** (bisa buram).
- Satu logo untuk tema terang dan gelap; tidak ada varian per tema.
- Ikon brand bawaan adalah vektor XAML; menambah/menggantinya butuh source + rebuild (`tools/convert-logos.ps1`). Pengguna akhir memakai PNG override.
- Ikon tray adalah glyph **U** yang digambar runtime, bukan `united.ico`. Mengganti `united.ico` tidak mengubah tray.
- Logo manual yang diletakkan saat aplikasi berjalan mungkin belum tampil di sidebar sampai restart (Settings bisa menampilkannya lebih cepat).

## 4. Keterbatasan layanan & navigasi

- Domain di luar `originHosts`/`authPopupHosts` dibuka di **browser default**; login lintas domain bisa gagal sampai domain ditambahkan ke `authPopupHosts` (JSON) dan aplikasi di-restart.
- Field `externalHosts` ada di skema tetapi **tidak dipakai** oleh logika navigasi saat ini (direservasi). Jangan mengandalkannya.
- `userAgent` hanya bisa diubah lewat `services.json`; tidak ada UI. Belum ada jaminan situs menerima UA tersebut.
- Tidak ada pengaturan proxy, zoom per layanan, atau izin per situs di UI.
- Membuka tautan “Open in browser” memakai browser default; tidak ada integrasi kembali.
- Beberapa situs (mis. Reddit via curl) membalas 403 untuk klien non-browser; di WebView biasanya normal, tetapi bisa berubah.
- URL bawaan YouTube (`youtube.com/messages`) hanyalah halaman YouTube, bukan kotak pesan; layanan ini lebih tepat dianggap “tab YouTube”.
- Facebook/Instagram diverifikasi dengan akun nyata untuk daftar percakapan; pengiriman pesan lintas perangkat belum diuji otomatis.
- WhatsApp Web memerlukan ponsel tetap terhubung; batas perangkat WhatsApp berlaku.
- Instagram selalu membuka DM inbox lebih dulu (`/direct/inbox/`), bukan feed. Threads juga membuka DM inbox (`/messages/`), bukan feed.
- Saat DM Discord terbuka di All messages, sidebar DM diparkir di luar pane (340×720, `opacity:0`) agar Discord tetap me-render baris dan observer tetap hidup. Ini memakai sedikit CPU/GPU tambahan. Baris yang hanya berisi “Server Tag: …” tidak dianggap preview pesan. Discord tidak memberi angka unread per baris; badge memakai angka di ikon Direct Messages, dengan fallback jumlah baris ber-`unreadPill`.
- Discord memakai masa tenang **30 detik** saat start (cold start SPA bisa lambat) ditambah **priming dua sapuan periodik**: event baru tidak dikirim sampai daftar berhasil terisi pada dua sapuan interval. DM yang tiba dalam 30 detik pertama setelah aplikasi dibuka bisa tidak masuk All messages karena snapshot awal belum selesai.
- Jika DM Discord dibuka lalu tab lain diaktifkan (bukan All messages), seluruh surface Discord bisa di-collapse; daftar DM yang diparkir hanya dijamin saat mode fokus All messages. Notifikasi web Discord tetap masuk.

## 5. Keterbatasan data & keamanan

- **Izin**: notifikasi dan penyimpanan persisten diizinkan; kamera/mikrofon hanya diizinkan dari origin layanan itu sendiri (iframe pihak ketiga tidak bisa memakai kamera/mikrofon United); **clipboard-read ditolak** (paste Ctrl+V tetap berfungsi). Izin lain (lokasi, MIDI, dsb.) ditolak. Tidak ada UI untuk mencabut izin per layanan.
- Semua data ada di `%LOCALAPPDATA%\United` tanpa enkripsi khusus. Siapa pun yang dapat membaca profil Windows Anda dapat membaca sesi WebView2.
- Tidak ada backup/sinkronisasi otomatis. Salin folder saat aplikasi tertutup.
- Profil WebView2 dapat terikat pada versi/mesin; memindahkan sesi antar-PC bersifat best-effort.
- Log dapat memuat judul halaman/error; periksa sebelum membagikannya.
- Shortcut Start Menu dan entri registry per user dibuat otomatis. Belum ada uninstaller resmi.
- Single instance mencegah dua proses memakai UDF yang sama; jika proses menggantung, hentikan lewat Task Manager.

## 6. Keterbatasan memori & performa

- Semua layanan berbagi **satu proses browser**; ini menghemat RAM tetapi satu crash browser dapat memengaruhi semua layanan (aplikasi mencoba recreate/reload).
- Layanan latar memakai `MemoryUsageTargetLevel.Low`, bukan dihentikan; notifikasi/WebSocket tetap hidup dengan biaya memori tertentu.
- Slot Telegram diparkir di luar pane alih-alih di-collapse agar daftar chat tetap ter-render; satu renderer Telegram terus berjalan selama aplikasi terbuka. Pengukuran 21 Okt (6 layanan aktif, Telegram diparkir): 16 proses, working set ±1,16 GB, private ±2,0 GB, CPU idle ±2,2% satu core.
- Hibernate melepas renderer, tetapi proses browser bersama tetap ada.
- Pengukuran runtime terbatas tiga skenario tersedia di [laporan benchmark 20 Okt 2026](benchmarks/2026-10-20-runtime-report.md); bukan rata-rata atau janji performa. Matriks lengkap S0–S5 dengan akun login dan panggilan belum selesai.
- Beberapa situs (video/animasi) dapat memakai CPU tinggi meski layanan tidak aktif; hibernate adalah solusi.

## 7. Keterbatasan UI/UX

- Urutan sidebar hanya bisa diubah lewat `settings.json` (tidak ada drag-and-drop).
- Tidak ada indikator progres unduhan file, pencarian layanan, atau pengaturan per-layanan selain JSON.
- Baris Settings menampilkan `state ... · id ...`; id diperlukan untuk nama file logo manual. Ini teknis, bukan untuk semua pengguna.
- Tombol **Reset logo** di menu klik kanan nonaktif bila tidak ada logo kustom; di Settings tombolnya disembunyikan.
- Close-to-tray default dapat membingungkan; tidak ada dialog “first close” yang menjelaskan.
- Tema mengikuti sistem saat mode System. Halaman web juga diberi preferensi skema warna tema United, tapi layanan dengan pengaturan tema eksplisit di akunnya dapat menimpanya.
- Dialog file memakai dialog Windows; tidak ada drag-and-drop logo ke jendela Settings.
- **All messages** menyimpan event `CoreWebView2.NotificationReceived` (bukan riwayat chat) ditambah observer preview daftar chat untuk WhatsApp, Telegram, Discord, Facebook, Instagram, dan Threads. Observer hanya melaporkan **perubahan setelah snapshot awal**: tidak mengimpor riwayat, tidak membaca token, dan tidak mengirim pesan.
  - Telegram Web K hanya membangun daftar chat saat surface-nya ter-render, jadi slot Telegram diparkir di luar pane (bukan di-collapse) ketika tab lain aktif. Ini memakai sedikit CPU/GPU tambahan.
  - Instagram dibuka langsung ke `/direct/inbox/`; baris Notes (tanpa stempel waktu) dan status kehadiran seperti “Active 7m ago” dikecualikan.
  - Threads dibuka langsung ke `/messages/` (DM web); baris percakapan adalah anchor `/messages/t/<id>`. Preview milik sendiri (“You sent …”, “Sent a post”) diabaikan.
  - WhatsApp memakai `#pane-side [data-testid="cell-frame-container"]`; judul diambil dari span ber-`title` dan badge per baris dari `icon-unread-count`. WhatsApp Web tidak memberi href/ID baris yang berguna, jadi identitas dan reveal memakai judul percakapan.
  - Pesan yang tidak mengubah preview (mis. balasan di thread yang preview-nya sudah sama), chat yang di-mute, halaman hibernasi, atau pesan saat offline bisa terlewat. Perubahan markup situs dapat mematahkan selector. Badge Telegram/Facebook/WhatsApp dihitung per baris percakapan; Discord memakai angka di ikon Direct Messages (judul `(n)` adalah mention server, bukan DM); Instagram/Threads memakai angka dari `document.title`.
  - Identitas baris memakai href target percakapan bila tersedia (mis. `#chat-id`, `/channels/@me/<id>`), bukan judul yang ikut berubah saat preview baru masuk. WhatsApp adalah pengecualian karena memakai judul yang sudah dibersihkan. Data lama dengan target sama digabung saat load; judul yang berubah tidak membuat baris baru.
  - Sapuan interval 10 detik tidak menimpa snapshot lama; ia hanya mengisi baris baru. Ini mencegah race yang sempat membuat event Threads hilang.
  - Auto-buka percakapan memakai href baris yang tersimpan (`/channels/@me/<id>`, `/messages/t/<id>`) untuk klik exact; WhatsApp dan baris tanpa href memakai pencocokan judul. Jika baris tidak ter-render, fallback ke navigasi path atau pencocokan judul. Selector sidebar bisa usang saat situs mengubah DOM (lihat `InboxFocusCss` di `ServiceSlot.cs`).

## 8. Keterbatasan distribusi

- Belum ada installer Inno Setup, uninstaller, atau auto-update.
- Build framework-dependent membutuhkan **.NET 10 Desktop Runtime**; self-contained jauh lebih besar.
- WebView2 Runtime harus tersedia (biasanya sudah).
- Data **tidak portable** bersama folder aplikasi; tetap di `%LOCALAPPDATA%`.
- Build x64 saja (`win-x64`).
- Belum ada code signing; SmartScreen dapat memperingatkan saat exe baru diunduh/dijalankan.

## 9. Hal yang secara eksplisit belum diuji

- Login & sesi nyata: WhatsApp, Facebook, Instagram, Telegram, Discord, Threads sudah dipakai dengan akun nyata di mesin uji; X, Messenger, Reddit, LinkedIn, Bluesky, YouTube belum.
- Notifikasi nyata dari tiap layanan: Telegram dan WhatsApp terbukti dengan pesan nyata; Discord/Facebook/Instagram/Threads baru diuji lewat perubahan DOM terkontrol. Klik toast belum diuji.
- Badge unread Bluesky dan format judul yang berubah. Badge Instagram belum punya angka (inbox Instagram tidak menampilkan jumlah), jadi badge Instagram tetap 0/titik; badge Threads sudah terbukti dari judul `(n) Messages • Threads`.
- Panggilan audio/video, lampiran file besar, unduhan. Popup OAuth sudah dibuka di dalam aplikasi (host login Google/Apple/Microsoft/Steam), tetapi penyelesaian login penuh belum diuji untuk tiap penyedia.
- Perilaku setelah berhari-hari (memory leak/soak).
- Autostart setelah reboot, tray menu klik, dan skenario DPI multi-monitor.
- Uninstall bersih di mesin tanpa .NET/WebView2.

Jika Anda menemukan bug pada area di atas, sertakan log `%LOCALAPPDATA%\United\logs\united-YYYYMMDD.log` dan langkah reproduksi.
