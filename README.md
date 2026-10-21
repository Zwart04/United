# United Messenger

Satu aplikasi Windows yang ringan untuk **WhatsApp, Facebook, Instagram, Telegram, Discord, Threads** — plus layanan lain yang Anda tambahkan sendiri (X, Messenger, Reddit, LinkedIn, Bluesky, YouTube, atau situs apa pun).

United adalah shell desktop berbasis **.NET 10 + WebView2** yang membuka situs resmi setiap layanan di profil terisolasi. Sesi login tersimpan, notifikasi memakai toast Windows asli, sidebar dibuat minimal, dan ada mode hemat memori. Tidak ada backend cloud, tidak ada API tidak resmi, dan tidak ada data yang dikirim ke pihak ketiga selain situs layanan itu sendiri.

> Status: **0.1.0** · Windows 10 2004+ / Windows 11 · x64
> UI aplikasi berbahasa Inggris; dokumentasi ini berbahasa Indonesia.
>
> **Unduh build portable:** [Releases](https://github.com/Zwart04/United/releases/latest) · butuh .NET 10 Desktop Runtime dan WebView2 Runtime.

![Halaman Services](docs/images/services-top.png)

## Fitur

- **6 layanan bawaan**: WhatsApp, Facebook, Instagram, Telegram, Discord, Threads.
- **6 layanan opsional sekali klik**: X, Messenger, Reddit, LinkedIn, Bluesky, YouTube.
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
- **Tanpa installer wajib**: jalankan folder portable; data disimpan di `%LOCALAPPDATA%\United`.

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

Self-contained (lebih besar, tanpa prasyarat .NET):

```powershell
dotnet publish src\United.App\United.App.csproj -c Release -r win-x64 --self-contained true `
  -o build\United-selfcontained-win-x64
```

## Struktur repo

```
src/United.Core              model layanan, settings, notifikasi, logika murni + unit test
src/United.Platform.Windows  WebView2, toast, tray, autostart, deep link, metrics
src/United.App               WPF shell, sidebar, Settings, dialog, tema, ikon brand
tests/United.Core.Tests      xUnit (54 test)
tools/                       generator logo/ikon, skrip verifikasi UI
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
| `logs\` | log harian untuk troubleshooting |

Tidak ada telemetri, akun cloud, atau server perantara. Aplikasi hanya menghubungi situs layanan yang Anda buka. Izin web: notifikasi dan penyimpanan persisten diizinkan; kamera/mikrofon hanya dari origin layanan itu sendiri; clipboard-read dan izin lain ditolak. Detail dan implikasi privasi ada di tutorial.

## Merek dagang & lisensi

Nama dan logo WhatsApp, Facebook, Instagram, Messenger, Telegram, Discord, Threads, X, Reddit, LinkedIn, Bluesky, dan YouTube adalah merek dagang pemiliknya masing-masing. Logo brand di aplikasi berasal dari [simple-icons](https://simpleicons.org) (CC0) dan SVG resmi WhatsApp (Wikimedia Commons), digunakan hanya untuk menandai layanan yang Anda buka. Proyek ini tidak berafiliasi dengan layanan mana pun.

Kode aplikasi: MIT (lihat [LICENSE](LICENSE)). Paket pihak ketiga: .NET 10, WPF, Microsoft.Web.WebView2.

## Batasan penting

- Ini **bukan** klien API resmi. Semua interaksi terjadi di halaman web resmi; ToS tiap layanan tetap berlaku.
- Notifikasi dan badge bergantung pada dukungan situs serta markup yang dapat berubah. Badge Threads sudah terverifikasi dari judul `(n) Messages • Threads`; Bluesky belum. Instagram tidak menampilkan angka unread di inbox sehingga badge-nya berupa titik/0.
- Enam layanan bawaan sudah dipakai dengan akun nyata. Notifikasi nyata end-to-end baru terbukti untuk Telegram dan WhatsApp; empat layanan lain diverifikasi lewat observer DOM dengan perubahan DOM terkontrol, belum dengan pesan masuk dari akun lain.
- End-to-end toast click routing belum diuji dengan notifikasi nyata.
- SVG/WebP tidak didukung untuk logo kustom; GIF animasi tidak dipertahankan.
- Belum ada installer/uninstaller resmi; gunakan folder portable + hapus data manual.

Daftar lengkap: [docs/KNOWN-LIMITATIONS.md](docs/KNOWN-LIMITATIONS.md).
