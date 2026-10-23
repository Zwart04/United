# Opsi native / lebih ringan (riset Oktober 2026)

Dokumen ini merangkum pencarian klien atau library native untuk enam layanan bawaan
United. Tujuannya bukan mengganti WebView2 tanpa alasan, tetapi menilai apakah ada
proyek yang bisa membuat United lebih ringan tanpa mengorbankan keamanan akun.

## Ringkasan

| Layanan | Kandidat | Status | Cocok untuk United? |
|---|---|---|---|
| WhatsApp | [`whatsmeow`](https://github.com/tulir/whatsmeow) (sudah dipakai), [`whatsapp-rust`](https://github.com/oxidezap/whatsapp-rust), [`oxidezap/client`](https://github.com/oxidezap/client) | Library protokol tidak resmi; aktif | `whatsmeow` sudah menjadi bridge background. UI native penuh berarti menulis ulang klien chat. |
| Telegram | [TDLib](https://github.com/tdlib/td) + [binding .NET](https://www.nuget.org/packages/TDLib) | Library resmi Telegram | Layak untuk bridge background, tetapi butuh `api_id`/`api_hash`, login nomor, dan UI chat sendiri. |
| Discord | [Litecord](https://github.com/ak4ai/Litecord) | Klien native Rust + Slint, installer Windows, MIT | Menarik untuk aplikasi terpisah; sulit di-embed ke United dan tetap tidak resmi. |
| Discord | Dissent, LemonCord, Paicord, QTCord | Linux/macOS atau wrapper web | Tidak memberi keuntungan native di Windows. |
| Facebook Messenger | [`mautrix/meta`](https://github.com/mautrix/meta) | Bridge Matrix untuk Messenger + Instagram DM | Berjalan di server Matrix + bridge; total konsumsi lebih berat dan melanggar ToS bila dipakai untuk akun pribadi. |
| Instagram DM | `mautrix/meta` (bridge Instagram) | Sama seperti di atas | Tidak ada SDK resmi untuk DM akun pribadi. |
| Threads | Threads API resmi | API hanya untuk publikasi, balasan, dan insight | **Tidak ada endpoint DM**; klien native DM tidak mungkin tanpa reverse engineering. |

## Temuan per layanan

### WhatsApp

- `whatsmeow` (Go) sudah dipakai United untuk menerima preview saat WebView tidur.
  Ini adalah jalur native yang paling matang dan sudah teruji di aplikasi ini.
- `whatsapp-rust` adalah implementasi protokol WhatsApp Web multi-device di Rust.
  `oxidezap/client` di atasnya menyediakan klien desktop. Klaim komunitas: jauh lebih
  ringan daripada WhatsApp Web/Tauri, tetapi tetap memuat konten web untuk sebagian
  tampilan.
- Untuk mengganti WebView WhatsApp sepenuhnya, United harus membangun daftar chat,
  composer, media, reaksi, panggilan, dan penanganan sesi sendiri. Itu proyek baru,
  bukan sekadar mengganti dependency.
- Risiko: library tidak resmi. WhatsApp dapat membatasi atau memblokir akun yang
  memakai companion tidak resmi. Karena itu mode native harus tetap opsional.

### Telegram

- TDLib adalah library resmi Telegram (C++, dengan binding C#/.NET). Ini satu-satunya
  jalur native yang relatif "resmi" untuk Telegram.
- Kendala: aplikasi harus mendaftarkan `api_id` dan `api_hash`, lalu pengguna login
  dengan nomor telepon/kode. United belum punya alur itu.
- TDLib bisa dipakai sebagai **bridge background** (notifikasi dan preview) sambil
  mempertahankan WebView Telegram untuk membaca/membalas. Ini menurunkan kebutuhan
  menjalankan WebView di latar, tetapi menambah satu proses native dan kompleksitas
  penyimpanan sesi.
- UI Telegram native penuh (chat list, media, folder, secret chat) adalah proyek besar.

### Discord

- **Litecord** (Rust + Slint) adalah klien Discord native dengan installer Windows,
  lisensi MIT. Klaim proyek: konsumsi RAM aktif sekitar 16–25 MB, jauh di bawah
  Electron/WebView.
- Namun Litecord adalah **aplikasi terpisah**, bukan library UI. Meng-embed jendelanya
  ke WPF (`HwndHost`/reparenting) rapuh, tidak mendukung notifikasi terpadu dengan
  mudah, dan tetap memakai protokol pengguna tidak resmi.
- Dissent (GTK4), LemonCord, Paicord (SwiftUI/macOS), dan QTCord adalah klien untuk
  platform lain atau wrapper web. Tidak ada yang bisa dipakai langsung sebagai kontrol
  Windows di United.
- Rekomendasi: tawarkan Litecord sebagai tautan "native Discord" eksternal di masa
  depan, bukan menggantikan slot Discord di United.

### Facebook Messenger, Instagram DM, dan Threads

- Tidak ada API resmi untuk DM akun pribadi di ketiga layanan ini. API Messenger/
  Instagram yang resmi ditujukan untuk akun profesional/bisnis dan halaman, bukan inbox
  pribadi.
- `mautrix/meta` adalah implementasi protokol tidak resmi yang paling aktif. Arsitekturnya
  adalah bridge Matrix: pengguna menjalankan homeserver Matrix + bridge, lalu memakai
  klien Matrix. Total sumber daya (Matrix + bridge + klien) biasanya **lebih berat**
  daripada satu WebView2, dan risiko ToS tetap ada.
- Threads API resmi (2026) mendukung publikasi, balasan, dan insight; **tidak ada DM**.
  Threads DM hanya tersedia di aplikasi/web resmi. Klien native DM berarti reverse
  engineering penuh.
- Karena itu WebView2 untuk Meta tetap pilihan paling aman dan paling murah untuk
  dirawat.

## Mengapa WebView2 masih masuk akal

- United memakai **satu proses browser Chromium bersama** untuk semua layanan, bukan
  satu Electron per layanan.
- Layanan latar memakai mode low-memory; layanan yang tidak dipakai bisa dihibernate
  total. Ini sudah menghasilkan penghematan besar tanpa menulis ulang protokol chat.
- Halaman resmi menangani login, enkripsi, panggilan, media, dan perubahan protokol.
  Klien native harus mengejar semua itu sendiri.
- Setiap klien tidak resmi menambah risiko pembatasan akun dan beban maintenance.

## Rekomendasi

1. **Pertahankan arsitektur hybrid sekarang**: WebView2 untuk UI, `whatsmeow` untuk
   background WhatsApp.
2. **Jangan mengejar native untuk Meta/Threads**: tidak ada API DM resmi; bridge Matrix
   lebih berat dan berisiko.
3. **Eksperimen berikutnya yang paling masuk akal**: bridge background TDLib untuk
   Telegram (notifikasi/preview) sebagai opsional, dengan fallback WebView.
4. **Discord native**: dokumentasikan Litecord sebagai alternatif eksternal; jangan
   embed ke United sampai ada library UI yang stabil.
5. Jika tujuan utama adalah angka RAM, optimasi yang lebih aman adalah:
   - hibernate lebih agresif untuk layanan yang tidak terlihat,
   - satu proses browser bersama (sudah),
   - batasi layanan yang aktif saat startup,
   - hindari memuat ulang WebView yang sudah tidur.

## Catatan keamanan

Semua proyek tidak resmi di atas menggunakan protokol klien, bukan API resmi. Memakai
mereka dapat melanggar ToS layanan dan berisiko akun dibatasi. United tidak menyimpan
kata sandi; sesi tetap berada di profil WebView2 atau penyimpanan lokal helper. Setiap
integrasi native baru harus bersifat opt-in, memiliki fallback WebView, dan dijelaskan
risikonya sebelum dipakai.
