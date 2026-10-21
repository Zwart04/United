# Referensi Logo United

Dokumen ini menjawab tiga hal: **di mana** mengganti logo, **format/ukuran** yang dipakai, dan bagaimana logo brand bawaan bekerja.

> Untuk tutorial langkah demi langkah (tambah layanan, notifikasi, hibernasi), lihat [TUTORIAL.md](TUTORIAL.md).

## Jawaban singkat

| Pertanyaan | Jawaban |
|---|---|
| Ganti logo di mana? | **Settings → Services → Change logo...** pada baris layanan, atau klik kanan ikon di sidebar → **Change logo...** |
| Format apa? | **PNG, JPG/JPEG, ICO, BMP, GIF** (SVG dan WebP **tidak** didukung) |
| Ukuran berapa? | **Persegi 128–256 px**; aplikasi menyimpan PNG maksimal **256 px** (hanya mengecilkan, tidak memperbesar) |
| Latar? | **Transparan** (PNG) paling aman untuk sidebar gelap/terang |
| Disimpan di mana? | `%LOCALAPPDATA%\United\icons\<service-id>.png` |
| Kembali ke logo bawaan? | **Reset logo** (Settings atau klik kanan sidebar) |

## Langkah mengganti logo

### Lewat Settings (disarankan)

1. Klik ikon **gear** di kiri bawah → **Services**.
2. Temukan layanan. Setiap baris menampilkan logo saat ini, nama, status, dan `id` layanan.
3. Klik **Change logo...**.
4. Pilih file gambar. Pratinjau muncul di baris dan sidebar langsung berubah.
5. Jika ingin membatalkan, klik **Reset logo** (tombol ini hanya muncul jika layanan memakai logo kustom).

### Lewat menu klik kanan sidebar

1. Klik kanan ikon layanan di sidebar.
2. Pilih **Change logo...**.
3. Pilih gambar. Sidebar dan halaman Settings (jika terbuka) ikut diperbarui.
4. **Reset logo** tersedia di menu yang sama; item ini nonaktif jika layanan belum punya logo kustom.

### Saat menambah layanan baru

Dialog **Add custom service...** punya tombol **Choose image...** dan **Clear**. Logo yang dipilih disimpan otomatis sebagai `icons\<id-baru>.png` saat layanan ditambahkan. Jika tidak memilih gambar, dipakai glyph globe netral.

## Spesifikasi file

| Properti | Nilai |
|---|---|
| Format | PNG (disarankan), JPG/JPEG, ICO, BMP, GIF |
| Tidak didukung | SVG, WebP, AVIF, animasi GIF/APNG (hanya frame pertama) |
| Ukuran maksimal | 256 × 256 px (sisi terpanjang; gambar besar diperkecil proporsional) |
| Ukuran disarankan | 256 × 256 px persegi, atau 128 × 128 px |
| Rasio | 1:1 (persegi). Non-persegi tetap diterima, tetapi akan terlihat kecil/gepeng di slot persegi |
| Transparansi | Disarankan (PNG-24/32 dengan alpha) |
| Warna | Kontras terhadap sidebar gelap **dan** terang |
| Safe area | Sisakan ~8–12% margin di sekeliling logo |
| Ukuran file | Tidak ada batas keras; 256 px PNG biasanya < 50 KB |

**Catatan ICO:** aplikasi memilih frame terbesar (paling banyak piksel), lalu menyimpannya sebagai PNG.
**Catatan GIF:** hanya frame pertama yang dipakai; animasi hilang.
**Catatan upscale:** gambar 32×32 tidak diperbesar. Di layar 200% DPI hasilnya bisa buram; gunakan minimal 128 px.

## Lokasi & penamaan file

```
%LOCALAPPDATA%\United\icons\
    whatsapp.png
    facebook.png
    telegram.png
    test-chat.png
    ...
```

- Nama file = **id layanan** (huruf kecil, tanpa spasi) + ekstensi.
- Id terlihat di baris Settings setelah status, misalnya `state ready · id whatsapp`.
- Untuk layanan kustom, id dibuat otomatis dari nama (slug). Contoh: nama **Work Chat** → id `work-chat`. Jika bentrok, ditambah `-2`, `-3`, dan seterusnya.
- Saat menyimpan lewat UI, file lama dengan id sama dalam format lain (mis. `whatsapp.jpg`) dihapus agar hanya ada satu logo kanonik (`whatsapp.png`).
- `Reset logo` menghapus **semua** varian ekstensi untuk id tersebut.

### Id layanan bawaan

| Layanan | Id | Layanan | Id |
|---|---|---|---|
| WhatsApp | `whatsapp` | X | `x` |
| Instagram | `instagram` | Messenger | `messenger` |
| Facebook | `facebook` | Reddit | `reddit` |
| Telegram | `telegram` | LinkedIn | `linkedin` |
| Discord | `discord` | Bluesky | `bluesky` |
| Threads | `threads` | YouTube | `youtube` |

## Mengganti file secara manual

1. Tutup United sepenuhnya (tray → **Quit**).
2. Letakkan gambar di `%LOCALAPPDATA%\United\icons\` dengan nama `<id>.<ext>`.
3. Jalankan United lagi.

Jika aplikasi sedang berjalan, file manual belum tentu langsung terlihat di **sidebar**; membuka ulang Settings → Services memperbarui daftar Settings, tetapi sidebar biasanya perlu restart. Cara paling aman: gunakan tombol **Change logo...** (langsung live) atau restart aplikasi.

> Jangan menaruh logo di folder instalasi/repo. Folder `%LOCALAPPDATA%\United\icons` adalah satu-satunya lokasi override yang dibaca aplikasi.

## Urutan prioritas logo

1. File kustom `<id>.png` / `.jpg` / `.jpeg` / `.ico` / `.bmp` / `.gif` di folder icons.
2. Logo brand bawaan (`Brand.<Nama>` di `BrandIcons.xaml`).
3. Glyph globe netral (jika tidak ada keduanya).

Logo kustom menang atas logo brand, termasuk untuk WhatsApp/Facebook dan layanan bawaan lain.

## Logo brand bawaan (level source / developer)

Logo bawaan adalah **vektor XAML**, bukan file PNG, sehingga tetap tajam di semua DPI dan tidak menambah aset raster. Pipeline:

```
tools/logos/<slug>.svg
        │  (path SVG resmi, viewBox 24×24)
        ▼
tools/convert-logos.ps1  ──►  src/United.App/Resources/BrandIcons.xaml
        │                         (key: Brand.Whatsapp, Brand.Facebook, ...)
        ▼
build → tampil di sidebar & Settings
```

### Mengganti logo brand yang sudah ada

1. Siapkan SVG resmi/berlisensi, idealnya **viewBox `0 0 24 24`** dan satu `<path>` utama.
2. Ganti `tools/logos/<slug>.svg` (mis. `facebook.svg`).
3. Jika perlu, sesuaikan baris spesifikasi di `tools/convert-logos.ps1`:
   ```powershell
   @{ Key = 'Facebook'; Slug = 'facebook'; Style = 'markOnWhite'; Brush = '#0866FF' }
   ```
   - `Key` harus PascalCase; key resource menjadi `Brand.<Key>`.
   - `Style`:
     - `markOnWhite` — path sudah berbentuk badge; digambar di atas lingkaran putih, diisi warna `Brush` (cocok untuk Facebook, Telegram, Reddit, YouTube).
     - `badge` — kotak membulat berwarna brand, glyph putih di atasnya, dengan `Scale`/`Tx` untuk memusatkan (Instagram, Discord, Threads, X, LinkedIn, Bluesky).
     - `wa` — kasus khusus WhatsApp (gradien hijau + gagang telepon putih dari SVG resmi `whatsapp-official.svg`).
4. Jalankan generator:
   ```powershell
   powershell -ExecutionPolicy Bypass -File tools\convert-logos.ps1
   ```
5. Build ulang dan jalankan. Tidak perlu menyentuh C#.

### Menambah brand baru

Tambahkan satu baris di array `$spec`:

```powershell
@{ Key = 'Mastodon'; Slug = 'mastodon'; Style = 'badge'; Shape = 'rect'; Scale = '0.72'; Tx = '3.36'; Brush = '#6364FF' }
```

Letakkan `tools/logos/mastodon.svg`, jalankan converter, lalu gunakan `"icon": "mastodon"` pada definisi layanan (atau id yang sama dengan file ikon). Untuk layanan kustom, cara termudah tetap logo PNG lewat UI.

### Catatan format SVG untuk converter

- Converter mengambil atribut `d` dari `<path>` pertama (atau path dengan fill tertentu untuk WhatsApp). Hindari SVG dengan banyak path terpisah, `<style>`, `<image>`, atau referensi eksternal.
- Koordinat diasumsikan dalam kanvas 24×24. Untuk viewBox lain, penyesuaian `Scale`/`Tx`/matrix mungkin diperlukan.
- Warna diatur lewat `Brush` pada spesifikasi, bukan dari atribut fill SVG (kecuali kasus `wa`).
- Lisensi: gunakan SVG resmi atau [simple-icons](https://simpleicons.org) (CC0). Logo adalah merek dagang pemiliknya; jangan jadikan logo pihak lain sebagai identitas produk Anda.

## Ikon aplikasi & tray

| Elemen | Lokasi | Cara mengganti |
|---|---|---|
| Ikon **exe**, taskbar, title bar, shortcut | `src/United.App/Assets/united.ico` | Ganti file ICO (16–256 px, multi-size), atau edit `tools/gen-icon.ps1` lalu jalankan untuk membuat ulang; build ulang |
| Ikon **tray** | Digambar saat runtime di `TrayIconService.RenderIcon` (huruf **U** 32×32 + badge) | Edit kode `RenderIcon`; ini **bukan** file `united.ico` |
| Logo **layanan** | `%LOCALAPPDATA%\United\icons\` atau `BrandIcons.xaml` | Seperti dijelaskan di atas |

Mengganti `united.ico` **tidak** otomatis mengganti huruf U di tray, dan sebaliknya.

## Tips membuat logo yang bagus

- Gunakan kanvas persegi 256×256, latar transparan.
- Simpan glyph di tengah dengan margin aman 10%.
- Hindari detail tipis (garis < 2 px pada 256) karena sidebar menampilkannya sekitar 24–28 px.
- Uji di tema **gelap** dan **terang**.
- Untuk logo yang memang punya latar putih (mis. X), sertakan bentuk lingkaran/kotak putih di dalam PNG — jangan andalkan warna sidebar.
- Jangan memakai screenshot; gunakan SVG/PNG resmi beresolusi tinggi.

## Troubleshooting logo

| Gejala | Penyebab & solusi |
|---|---|
| Logo tidak berubah setelah memilih file | Pastikan file tidak rusak dan format didukung. Coba **Reset logo**, lalu **Change logo...** lagi. Cek log `%LOCALAPPDATA%\United\logs`. |
| Logo terlihat buram | Sumber < 128 px atau non-persegi. Gunakan PNG 256×256. |
| Logo putih/kotak aneh di sidebar gelap | Gambar punya latar putih (JPG tidak punya alpha). Gunakan PNG transparan. |
| Logo manual tidak muncul | Nama file harus `<id>.<ext>` dan id harus sama persis. Restart aplikasi. |
| **Reset logo** tidak ada / nonaktif | Tidak ada file kustom untuk id itu — artinya logo yang tampil sudah bawaan. |
| Setelah reset masih logo lama | Hapus cache/restart aplikasi. File mungkin masih terbuka; tutup lalu ulangi. |
| Ingin logo berbeda per tema | Belum didukung; satu logo dipakai untuk kedua tema. Pilih yang kontras di keduanya. |
