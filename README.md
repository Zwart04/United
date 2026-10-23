# United Messenger

[![Support United on Tako](https://img.shields.io/badge/Support%20United-Tako-ff5e5b?style=flat-square)](https://tako.id/Zwart04/gift)
[![Latest release](https://img.shields.io/github/v/release/Zwart04/United?style=flat-square)](https://github.com/Zwart04/United/releases/latest)

Satu aplikasi Windows yang ringan untuk **WhatsApp, Facebook, Instagram, Telegram, Discord, Threads** — plus layanan lain yang Anda tambahkan sendiri (X, Messenger, Reddit, LinkedIn, Bluesky, YouTube, atau situs apa pun).

United adalah shell desktop berbasis **.NET 10 + WebView2** yang membuka situs resmi setiap layanan di profil terisolasi. Sesi login tersimpan, notifikasi memakai toast Windows asli, sidebar dibuat minimal, dan ada mode hemat memori. Tidak ada backend cloud dan tidak ada data yang dikirim ke pihak ketiga selain situs layanan itu sendiri (plus pengecekan rilis GitHub bila auto-update aktif). Sejak 0.2.1, WhatsApp punya **mode native opsional** (hybrid): helper `whatsmeow` mengumpulkan preview saat WebView WhatsApp tidur, lalu WebView bangun hanya ketika Anda membuka chat untuk membaca/membalas.

> Status: **0.2.2** · Windows 10 2004+ / Windows 11 · x64
> UI aplikasi berbahasa Inggris; dokumentasi ini berbahasa Indonesia.
>
> **Unduh installer:** [Releases](https://github.com/Zwart04/United/releases/latest) · build portable juga tersedia · butuh .NET 10 Desktop Runtime dan WebView2 Runtime.

![Halaman Services](docs/images/services-top.png)

## Fitur

- **6 layanan bawaan**: WhatsApp, Facebook, Instagram, Telegram, Discord, Threads.
- **6 layanan opsional sekali klik**: X, Messenger, Reddit, LinkedIn, Bluesky, YouTube.
- **WhatsApp hybrid (0.2.1)**: setelah scan QR sekali di Settings, WhatsApp WebView tetap tidur dan pesan masuk tetap tampil di All messages. Klik chat untuk membangunkan WebView resmi dan membalas. Ini menghemat sekitar **900 MB private / 985 MB working set** pada mesin uji (lihat [docs/VERIFICATION.md](docs/VERIFICATION.md)).
- **Update otomatis (0.2.2)**: cek GitHub Releases saat startup (bisa dimatikan), unduh installer, verifikasi SHA-256, pasang secara silent, lalu buka ulang United. Build portable diarahkan ke halaman Releases.
- **Dukung United**: tombol Support di Settings → About dan menu tray, plus tombol Sponsor di halaman GitHub repo.
- **Tambah situs apa pun** lewat dialog (nama, URL, logo, mode notifikasi) atau `services.json`.
- **Logo bisa diganti sendiri** per layanan — PNG/JPG/ICO/BMP/GIF, dinormalisasi ke PNG maksimal 256 px. Lihat [docs/LOGOS.md](docs/LOGOS.md).
- **Sesi terisolasi** per layanan; login sekali, tetap tersimpan setelah restart.
- **Notifikasi Windows asli** dengan tiga mode: Host (toast aplikasi), Webview (notifikasi situs), Off.
- **Badge unread** di sidebar dan tray (jika judul halaman menyediakan angka).
- **All messages** (`Ctrl+0`): inbox terpadu berisi aktivitas/notifikasi dari semua layanan; klik satu baris untuk langsung membuka percakapan tersebut di webview aslinya dan membalas. Daftar chat layanan tidak dobel, dan mode ini otomatis menyembunyikan sidebar kontak layanan saat percakapan terbuka.
- **Observer daftar chat untuk 6 layanan bawaan** (WhatsApp, Telegram, Discord, Facebook, Instagram, Threads): hanya perubahan preview setelah snapshot awal yang dilaporkan; riwayat lama tidak diimpor, token tidak dibaca, dan tidak ada pesan yang dikirim.
- **Login OAuth di dalam aplikasi**: host login umum (Google, Apple, Microsoft, Steam, YouTube) dibuka sebagai popup United dengan profil sesi yang sama, jadi Anda tidak keluar ke browser.
- **Hemat memori**: satu proses Chromium bersama, mode low-memory untuk layanan latar, dan hibernate total untuk melepas RAM.
- **Tray + autostart**, close-to-tray, tema terang/gelap/mengikuti sistem (halaman web ikut tema United), deep link `united://` untuk klik toast.
- **Installer Inno Setup + build portable**: installer per-user dengan auto-update dan uninstaller; ZIP portable untuk tanpa instalasi. Data pengguna tetap di `%LOCALAPPDATA%\United`.

## Mulai cepat

### Prasyarat

| Kebutuhan | Keterangan |
|---|---|
| Windows | 10 versi 2004 (build 19041) atau lebih baru, 64-bit |
| .NET | **.NET 10 Desktop Runtime** (untuk build framework-dependent) |
| WebView2 | Microsoft Edge WebView2 Runtime (sudah ada di Windows 11; Windows 10 biasanya sudah terpasang via Edge) |

### Menjalankan

```powershell
# Dari repo
$env:Path = "$env:LOCALAPPDATA\Microsoft\dotnet;$env:Path"   # mesin pengembangan ini
dotnet build src\United.slnx
src\United.App\bin\Debug\net10.0-windows10.0.19041.0\United.exe
```

Atau pakai folder portable hasil publish:

```
build\United-portable-win-x64\United.exe
```

Saat pertama dibuka, sidebar berisi 6 layanan. Klik ikon layanan, lalu login seperti di browser biasa. Untuk WhatsApp, pindai QR dari ponsel. Setelah login, sesi tersimpan di `%LOCALAPPDATA%\United\WebData`.

### Mengaktifkan mode native WhatsApp (opsional)

1. Buka **Settings** (gear kiri bawah) > **General** > **Native WhatsApp**.
2. Klik **Pair native WhatsApp...**, lalu pindai QR di ponsel: **WhatsApp > Linked devices > Link a device**.
3. Setelah status **Connected**, tutup dialog. WhatsApp WebView akan tidur saat tidak dipakai; pesan baru tetap masuk ke **All messages** dan toast Windows.
4. Klik baris WhatsApp di All messages (atau ikon WhatsApp) untuk membangunkan WebView resmi saat ingin membaca/membalas.
5. Untuk kembali sepenuhnya ke WebView: **Settings > General > Forget native pairing**.

Helper native memakai library tidak resmi `whatsmeow`; lihat peringatan di [docs/KNOWN-LIMITATIONS.md](docs/KNOWN-LIMITATIONS.md). Jika helper hilang, belum dipasangkan, atau gagal, United otomatis memakai WhatsApp WebView biasa.

## Update otomatis

Buka **Settings → About** untuk melihat status versi dan tombol **Check for updates**.

- **Check for updates automatically at startup** aktif secara default dan dibatasi maksimal sekali per 24 jam.
- Jika ada rilis baru, United mengunduh `United-Setup-<versi>.exe` dari GitHub Releases, memeriksa **SHA-256** yang diterbitkan bersama rilis, menjalankan installer Inno Setup secara silent, lalu membuka ulang aplikasi.
- Tombol **Download & install** meminta konfirmasi sebelum memasang. Tidak ada update yang dipasang tanpa persetujuan.
- Build **portable** tidak dapat memasang update ke dalam foldernya sendiri; tombol update akan membuka halaman Releases agar Anda bisa mengunduh ZIP terbaru.
- Auto-update hanya memeriksa `Zwart04/United` di GitHub. Tidak ada server update pihak ketiga.

## Dukung United

United gratis dan open source. Jika aplikasi ini bermanfaat, Anda bisa memberi dukungan lewat Tako:

**https://tako.id/Zwart04/gift**

Tombol **Support United** tersedia di **Settings → About** dan menu tray. Di halaman GitHub, tombol **Sponsor** muncul dari [`.github/FUNDING.yml`](.github/FUNDING.yml). Dukungan bersifat opsional dan tidak mengubah fitur aplikasi.

**Penting:** tutup jendela dengan tombol **X** akan menyembunyikan aplikasi ke tray (default), bukan keluar. Keluar sepenuhnya lewat menu tray → **Quit**, atau matikan opsi close-to-tray di Settings.

## Ganti logo layanan (ringkas)

1. Buka **Settings** (ikon gear di kiri bawah) → **Services**.
2. Pada baris layanan, klik **Change logo...**.
3. Pilih gambar. Format yang didukung: **PNG, JPG/JPEG, ICO, BMP, GIF** (bukan SVG/WebP).
4. Ukuran yang disarankan: **persegi, 128–256 px, latar transparan**. Aplikasi menyimpan salinan PNG maksimal 256 px.
5. Untuk kembali ke logo bawaan: klik **Reset logo** (muncul jika ada logo kustom), atau klik kanan ikon di sidebar → **Reset logo**.

Lokasi file logo kustom:

```
%LOCALAPPDATA%\United\icons\<service-id>.png
```

Contoh: `whatsapp.png`, `facebook.png`, `test-chat.png`. Nama file mengikuti **id** layanan yang tertera di baris Settings (`state ... · id ...`).

Panduan lengkap format, ukuran, dan cara membuat logo: **[docs/LOGOS.md](docs/LOGOS.md)**.

## Tambah layanan (ringkas)

### Layanan bawaan / opsional

Settings → Services → bagian **Add a service** → klik **Add** pada X, Messenger, Reddit, LinkedIn, Bluesky, atau YouTube.

### Situs sendiri

Settings → Services → **Add custom service...** → isi nama, alamat (`https://...`), logo (opsional), mode notifikasi → **Add service**. Layanan langsung muncul di sidebar dan definisinya disimpan di `%LOCALAPPDATA%\United\services.json`.

### Lewat JSON

Klik **Edit services.json**, tambahkan objek layanan, simpan, lalu buka kembali halaman Services dan klik **Add**. Contoh dan skema lengkap ada di [Tutorial](docs/TUTORIAL.md#8-menambah-layanan-lewat-servicesjson).

## Dokumentasi

| Dokumen | Isi |
|---|---|
| **[docs/TUTORIAL.md](docs/TUTORIAL.md)** | Tutorial lengkap: login, tambah layanan, ganti logo, notifikasi, hibernasi, troubleshooting |
| [docs/LOGOS.md](docs/LOGOS.md) | Referensi logo: format, ukuran, lokasi file, nama, reset, logo brand bawaan |
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | Arsitektur teknis dan alur data |
| [docs/KNOWN-LIMITATIONS.md](docs/KNOWN-LIMITATIONS.md) | Batasan dan hal yang belum diverifikasi dengan akun nyata |
| [docs/NATIVE-OPTIONS.md](docs/NATIVE-OPTIONS.md) | Riset klien native/lebih ringan untuk enam layanan dan rekomendasinya |
| [docs/VERIFICATION.md](docs/VERIFICATION.md) | Matriks verifikasi fitur + checklist uji manual |
| [docs/benchmarks/](docs/benchmarks/) | Metodologi dan hasil pengukuran RAM/CPU |
| [PLAN.md](PLAN.md) | Dokumen desain historis dan riset awal |

## Build & test

```powershell
$env:Path = "$env:LOCALAPPDATA\Microsoft\dotnet;$env:Path"
dotnet build src\United.slnx
dotnet test  src\United.slnx
```

Publish portable (framework-dependent, butuh .NET 10 Desktop Runtime):

> Hentikan United dulu — `Compress-Archive` gagal jika DLL sedang dipakai. Alternatif: salin folder publish ke staging, lalu compress staging.

```powershell
dotnet publish src\United.App\United.App.csproj -c Release -r win-x64 --self-contained false `
  -o build\United-portable-win-x64
Compress-Archive -Path build\United-portable-win-x64\* `
  -DestinationPath build\United-portable-win-x64.zip -Force
```

Installer (butuh Inno Setup 6; build native helper dulu jika belum):

```powershell
.\tools\build-native.ps1
.\tools\build-installer.ps1
# hasil: build\United-Setup-0.2.2.exe
```

Self-contained (lebih besar, tanpa prasyarat .NET):

```powershell
dotnet publish src\United.App\United.App.csproj -c Release -r win-x64 --self-contained true `
  -o build\United-selfcontained-win-x64
```

Native WhatsApp helper (butuh Go + C compiler; hasilnya ikut tercopy saat publish):

```powershell
.\tools\build-native.ps1
```

## Struktur repo

```
src/United.Core              model layanan, settings, notifikasi, logika murni + unit test
src/United.Platform.Windows  WebView2, toast, tray, autostart, deep link, metrics
src/United.App               WPF shell, sidebar, Settings, dialog, tema, ikon brand
native/whatsapp-bridge       helper Go/whatsmeow untuk preview WhatsApp native
tests/United.Core.Tests      xUnit (68 test)
tools/                       generator logo/ikon, build native, skrip verifikasi UI
docs/                        tutorial, referensi logo, arsitektur, benchmark
```

## Data & privasi

Semua data lokal di `%LOCALAPPDATA%\United`:

| Path | Isi |
|---|---|
| `settings.json` | tema, layanan aktif, mode notifikasi, status hibernate, geometri jendela |
| `services.json` | definisi layanan kustom |
| `icons\` | logo kustom per layanan |
| `WebData\` | profil WebView2 (cookie, sesi login, cache) |
| `WhatsAppNative\` | sesi companion WhatsApp native, preview JSONL, status, QR, log helper |
| `updates\` | installer hasil unduhan auto-update dan log helper update |
| `logs\` | log harian untuk troubleshooting |

Tidak ada telemetri, akun cloud, atau server perantara. Aplikasi hanya menghubungi situs layanan yang Anda buka dan, jika auto-update aktif, `api.github.com` untuk memeriksa rilis United. Izin web: notifikasi dan penyimpanan persisten diizinkan; kamera/mikrofon hanya dari origin layanan itu sendiri; clipboard-read dan izin lain ditolak. Detail dan implikasi privasi ada di tutorial.

## Merek dagang & lisensi

Nama dan logo WhatsApp, Facebook, Instagram, Messenger, Telegram, Discord, Threads, X, Reddit, LinkedIn, Bluesky, dan YouTube adalah merek dagang pemiliknya masing-masing. Logo brand di aplikasi berasal dari [simple-icons](https://simpleicons.org) (CC0) dan SVG resmi WhatsApp (Wikimedia Commons), digunakan hanya untuk menandai layanan yang Anda buka. Proyek ini tidak berafiliasi dengan layanan mana pun.

Kode aplikasi: MIT (lihat [LICENSE](LICENSE)). Paket pihak ketiga: .NET 10, WPF, Microsoft.Web.WebView2.

## Batasan penting

- Ini **bukan** klien API resmi. Lima layanan lain tetap memakai halaman web resmi; ToS tiap layanan tetap berlaku.
- Mode native WhatsApp memakai `whatsmeow`, library **tidak resmi**. Menautkan perangkat dengan cara ini berisiko pembatasan akun WhatsApp. Gunakan hanya jika Anda menerima risiko tersebut; WebView resmi tetap tersedia sebagai fallback.
- Preview native WhatsApp hanya berisi teks/ringkasan media, bukan riwayat lengkap. Pesan yang diterima saat helper mati, belum dipasangkan, atau sedang reconnect tidak dapat dipulihkan.
- Notifikasi dan badge bergantung pada dukungan situs serta markup yang dapat berubah. Badge Threads sudah terverifikasi dari judul `(n) Messages • Threads`; Bluesky belum. Instagram tidak menampilkan angka unread di inbox sehingga badge-nya berupa titik/0.
- Enam layanan bawaan sudah dipakai dengan akun nyata. Notifikasi nyata end-to-end baru terbukti untuk Telegram dan WhatsApp; empat layanan lain diverifikasi lewat observer DOM dengan perubahan DOM terkontrol, belum dengan pesan masuk dari akun lain.
- End-to-end toast click routing belum diuji dengan notifikasi nyata.
- SVG/WebP tidak didukung untuk logo kustom; GIF animasi tidak dipertahankan.
- Installer resmi tersedia (`United-Setup-0.2.2.exe`); data pengguna tetap di `%LOCALAPPDATA%\United` dan tidak dihapus saat uninstall.
- Auto-update di dalam aplikasi hanya untuk build installer. Build portable harus mengunduh ZIP baru secara manual dari Releases. Installer belum ditandatangani secara digital, jadi SmartScreen dapat menampilkan peringatan saat pertama kali dijalankan.

Daftar lengkap: [docs/KNOWN-LIMITATIONS.md](docs/KNOWN-LIMITATIONS.md).
