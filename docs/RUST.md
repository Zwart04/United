# United (Rust) — native rewrite

United versi baru ditulis ulang dari nol dalam **Rust murni**: tidak ada Chromium,
tidak ada WebView2/WebKit, tidak ada browser engine di dalam proses. UI digambar
dengan `egui`/`eframe`, dan setiap layanan berbicara protokolnya sendiri lewat
socket dan HTTP langsung.

Kode lama (.NET 10 + WebView2, `src/`) tetap ada sebagai rilis 0.2.3; pohon Rust
berada di `rust/`.

## Arsitektur

```
rust/
  crates/united-core      domain model, SQLite store, event bus, Adapter trait
  crates/united-ui        shell egui: rail layanan, daftar chat, conversation, inbox, settings
  crates/united-whatsapp  WhatsApp multi-device (whatsapp-rust)
  crates/united-telegram  Telegram MTProto (grammers)
  crates/united-discord   Discord REST v9 + Gateway WebSocket
  crates/united-app       binary `united`: runtime tokio + window
  packaging/              .desktop, install.sh, PKGBUILD
```

Alur data:

1. Adapter membuka sesi tersimpan, lalu mem-publish `CoreEvent` (status, prompt
   login, chat, pesan) ke `tokio::sync::broadcast` bus.
2. UI thread `try_recv` event tiap frame, menulis ke SQLite (satu penulis: UI),
   dan menggambar. Bus menyimpan ring 4096 event terakhir: adapter mulai
   sebelum window ada, jadi UI me-replay event awal (riwayat, daftar chat,
   prompt login) saat dibuat.
3. Riwayat lama: WhatsApp mengirim `HistorySync` (didekode per conversation,
   maksimal 200 pesan terakhir per chat agar satu chunk tidak membanjiri UI).
   Karena WhatsApp hanya mengirim snapshot sekali per link, chat yang dibuka
   meminta halaman lebih tua lewat PDO (`fetch_message_history`), dan
   menggulir ke atas meminta halaman berikutnya. Telegram (`offset_id`) dan
   Discord (`before`) memakai anchor pesan tertua yang tersimpan dengan cara
   yang sama.
4. Kirim pesan: UI memanggil `Adapter::send_text`, adapter mengirim ke protokol,
   hasilnya kembali sebagai `CoreEvent::Message`.

Inbox gabungan karena itu berisi **pesan protokol asli** — id, pengirim,
timestamp, status — bukan hasil scraping judul halaman. Tidak ada render DOM,
tidak ada halaman yang perlu "dibangunkan" untuk membaca pesan.

## Tampilan

UI mengikuti bahasa desain WhatsApp Web: palet web resmi per layanan, hairline
1 px, bubble radius 8, baris daftar dengan avatar 49 px dan judul 15.5, panel
daftar 340 px, composer pill, switch iOS, dan segmented control. Identitas
layanan hanya muncul lewat logo resmi + titik brand kecil di baris chat, jadi
inbox gabungan tetap terbaca sebagai satu aplikasi.

- Font: **Inter** (Regular/Medium/SemiBold, lisensi OFL) ditanam di binary sebagai
  pengganti SF Pro — mesin tidak bergantung font distro. `theme.rs` mendaftarkan
  family `medium`/`semibold` karena egui tidak punya bobot font.
- Ikon rail: SVG resmi WhatsApp, Telegram, dan Discord di
  `crates/united-ui/assets/`.
- Ikon chrome (Bubbles, Compose, Search, Gear, ArrowUp, Ellipsis, Plus, Check,
  Phone, Mute) digambar sebagai vektor di `icons.rs`, bukan font ikon. Gear
  adalah cog, Mute speaker dicoret, Phone handset terisi, dan ArrowUp paper
  plane seperti WhatsApp Web.
- Emoji berwarna: egui hanya merasterisasi outline, sedangkan Noto Color Emoji
  cuma punya strike bitmap CBDT. `emoji.rs` membaca strike itu lewat `skrifa`
  (sudah ikut sebagai dependensi epaint), meng-cache tiap emoji sebagai tekstur,
  lalu menimpanya tepat di atas glyph monokrom yang sudah diletakkan layout.
  Cluster gabungan (ZWJ, skin tone, bendera) tetap monokrom.
- Title bar digambar sendiri (minimise, maximise/restore, close X) karena
  compositor Wayland tidak konsisten soal server-side decoration; tepi jendela
  tetap bisa di-resize lewat hot zone 5 px.
- Panel percakapan memakai palet web resmi tiap layanan: WhatsApp (`#0b141a` /
  `#005c4b`, light `#efeae2` / `#d9fdd3`), Telegram (`#17212b` / `#2b5278`,
  light `#e6ebee` / `#eeffde`), Discord (`#313338` datar, blurple). Daftar chat
  pada tampilan layanan ikut palet yang sama (panel, hover, baris terpilih),
  jadi list + percakapan terbaca sebagai satu jendela web; inbox bersama dan
  Settings tetap satu bahasa desain.
- WhatsApp Web: rail kedua 64 px di dalam view layanan (Calls, Status, Channels,
  Communities, Business, Profile, Settings) di atas rail global 76 px. Ke-66
  glyph di-capture dari klien web sebagai mask putih di `assets/wa/`, lalu
  diwarnai saat paint. Sheet Media (tab Media/Dokumen/Tautan, pencarian) dan
  dialog Advertise memakai backdrop gelap + klik-luar untuk menutup.
- Telegram Web A: kolom kiri 406 px berisi kartu mengambang 374 px (inset 16 px,
  `#212121`) di atas wallpaper hitam `#0a0a0a`. Drawer 216 px dan submenu
  **More** adalah kartu terpisah di kanan drawer (Night Mode, UI Features,
  Telegram Features, Report a Bug, Switch to K Version, Install App + footer
  versi), bukan baris yang memanjangkan drawer. Ke-76 glyph berasal dari font
  ikon web resmi (`icons-BkPSW-Eu.woff2`) sebagai mask di `assets/tg/`.
- Discord Web A: rail guild 72 px + sidebar DM 300 px + halaman Friends/DM.
  Palet `theme-darker`: rail/sidebar `#121214`, kolom pesan `#1a1a1e`, search
  `#222225`, baris terpilih `#2c2c30`, user bar `#202024`, blurple `#5865f2`.
  Ke-14 glyph di-capture dari DOM (`assets/dc/`), mic/headphones digambar sebagai
  vektor. Add Server/Discover/Nitro/Shop/Quests/Add Friend membuka kartu
  penjelasan jujur karena kanal guild & fitur akun di luar cakupan adapter DM.
- Foto profil chat diambil lewat `Adapter::load_avatar` untuk chat yang tampil
  di daftar dan untuk chat yang dibuka, sekali per sesi. Sebelumnya hanya
  pesan live dan 12 pengirim riwayat yang memicu permintaan, jadi kontak yang
  tidak sedang mengirim pesan selalu tampil sebagai inisial. IQ
  `w:profile:picture` menyertakan `tc_token` (`client.tc_token().get(<user>)`):
  tanpa token itu server menjawab 401 not-authorized untuk kontak yang belum
  menyimpan nomor kita; `404 item-not-found` berarti memang tidak ada foto
  (atau disembunyikan privasi) dan tidak dicoba ulang. Chat LID ditanyakan
  dengan LID-nya dulu (token tersimpan di situ), baru nomor telepon sebagai
  cadangan — menerjemahkan LID→PN lebih dulu membuang token dan berujung 401.
  Prefetch dibatasi 15 chat teratas: meminta 800+ sekaligus memicu rate-limit
  dan justru membuat semua foto gagal.
- Bubble masuk di grup menampilkan foto profil pengirim (28 px) di kiri.
  Foto disimpan per JID di tabel `avatars` dan diambil sekali per pengirim:
  dari pesan live, atau dari 12 pengirim terbaru saat riwayat dibuka.
  Suffix device (`:12@s.whatsapp.net`) dibuang supaya satu orang tidak menjadi
  dua baris/dua warna.
- Composer adalah bottom panel yang mengikuti tinggi isinya: draf yang
  terbungkus menambah tinggi bar, bukan terpotong. Tombol `+` di dalam field
  membuka menu *Photo or video…* / *Document…* lewat portal file XDG (`rfd`),
  lalu file diunggah adapter dan draf dipakai sebagai caption. Tombol kirim
  bulat ikut di dalam pill sehingga tidak pernah terpotong tepi kanan.
- Media: `Message.media` menyimpan JSON `MediaRef` (kind, nama, MIME, URL CDN,
  kunci/sha WhatsApp, panjang, `thumb`, plus `path` lokal setelah diunduh).
  `thumb` menyimpan pratinjau kecil: base64 JPEG/PNG dari protokol WhatsApp
  (`jpegThumbnail`, `pngThumbnail` sticker) atau path thumbnail hasil unduhan
  Telegram. Bubble memakai `thumb` untuk video (bingkai + tombol play) dan
  sticker sebelum file penuh ada; `path` yang sudah terunduh menang. Sticker
  WebP/GIF digambar tanpa latar bubble, jadi transparansinya sama seperti web.
  Foto/ dokumen/audio memakai tekstur lokal. Bubble gambar membuka viewer
  layar penuh di dalam aplikasi (scroll untuk zoom 1–8×, seret untuk geser,
  Esc/klik latar menutup, tombol Salin/Simpan/Tutup); video dan dokumen
  membuka aplikasi sistem.
  Unduhan berjalan malas: maksimal 8 lampiran terbaru per chat
  saat dibuka plus setiap lampiran yang benar-benar terlihat di viewport
  (`ui.is_rect_visible`, backoff 15 detik per pesan), hasilnya dikirim balik
  sebagai `CoreEvent::MediaReady` dan disimpan ke SQLite.
  Unduhan memakai **URL bertanda tangan dari pesan itu sendiri** (`static_url`
  lewat `Downloadable`), bukan `direct_path` saja: path tanpa token CDN
  dijawab 403. Kalau tautan sudah kedaluwarsa, `MediaReuploadRequest` meminta HP
  menerbitkan ulang (`MediaRetryResult::Success { direct_path }`), URL lama
  dibuang, lalu satu percobaan ulang; jawaban `NotFound` dianggap final supaya
  slot unduhan tidak tersandera. Konkurensi dibatasi 4 (`Semaphore`), dan
  file yang sudah ada di disk diadopsi saat start
  (`Store::link_local_media`) sehingga media lama tidak diunduh ulang.
- Emoji: fallback monokrom dari sistem tetap dipasang supaya layout punya glyph,
  lalu `emoji.rs` menimpa bitmap CBDT berwarna di atasnya (lihat bagian desain).
  Bitmap menjaga rasio asli, diskalakan setinggi satu em, dan modifier skin tone
  dilewati agar tidak muncul kotak warna.
- Arab: egui tidak punya shaping/bidi, jadi `widgets::display_text` me-reshape
  teks Arab ke presentation form (`arabic_reshaper`) lalu menjalankan algoritma
  bidi per paragraf (`unicode-bidi`) sebelum layout. Font fallback memakai Noto
  Naskh/Sans Arabic bila ada, kalau tidak DejaVu Sans. Ligatur "الله" (U+FDF2)
  dipecah kembali ke presentation form karena DejaVu tidak punya glyph-nya.
  Harakat dibuang oleh shaper: egui menempelkan mark di posisi pena, jadi
  mempertahankannya justru membuat harakat melenceng. Teks Latin tidak disentuh
  supaya kerning tetap.
- Tautan di dalam bubble dideteksi (`link_spans`), digambar hijau + garis bawah
  saat hover, dan area kliknya dihitung dari posisi glyph (`pos_from_cursor`);
  klik membuka browser lewat `widgets::open_url`. Autolink `www.` diberi skema
  https. Mention `@nama` memakai warna hijau yang sama.
- Panel info: menu `...` membuka panel kanan (foto, nama, jumlah anggota, daftar
  anggota, thumbnail media terakhir, mute). Nama anggota dipetakan lewat
  `contact_names` seperti picker mention, jadi nomor telepon tampil sebagai nama
  kontak. Ikon kaca pembesar di field pencarian dan di header sama-sama
  memfokuskan field cari.
- Mention: mengetik `@` di grup membuka picker anggota di atas composer.
  Daftar diambil dari roster protokol (`Adapter::members`: metadata grup
  WhatsApp + resolusi nomor telepon untuk grup LID, `iter_participants`
  Telegram); kalau roster belum ada (offline/Discord), picker memakai pengirim
  yang sudah muncul di chat, dan fetch dicoba ulang tiap 5 detik. Filter
  mengetik nama/nomor, navigasi ↑/↓, Enter/Tab memilih, Esc menutup.
  WhatsApp menyisipkan `@<nomor>` — bentuk yang dikenali server — lalu
  mengirim `contextInfo.mentionedJid` berisi JID peserta, jadi tag tampil
  sebagai nama di HP penerima. Bubble, preview chat, dan isi picker mengganti
  `@<nomor>` menjadi `@<nama>` memakai nama kontak, roster, lalu pengirim chat.
  Discord menyisipkan `<@id>` (ping asli) dan bubble menampilkannya sebagai
  `@Nama`. Telegram memakai teks `@Nama` biasa.
- Nama kontak: WhatsApp mengirim nama lewat app-state (`ContactAction`), yang
  tersimpan di tabel `contacts` (id PN dan LID sekaligus). Snapshot
  `critical_unblock_low` + `regular` diputar ulang sekali tiap launch supaya
  nama yang berubah di HP ikut tersegarkan, lalu delta menjaga sisanya. Nama
  buku telepon menang atas push name dan placeholder masking; `backfill_names`
  mengisi sisa placeholder dari kontak, nama pengirim yang pernah terlihat,
  penulis terakhir chat langsung, lalu `+<nomor>`. Chat langsung tanpa pesan
  dan tanpa nama tidak ditampilkan di inbox.
- Riwayat grup: author pesan diambil dari `key.participant` atau
  `WebMessageInfo.participant`, bukan dari id grup.
- Daftar pesan tidak lagi memperkirakan tinggi baris: semua bubble yang sudah
  dimuat (200 terbaru per chat) digambar apa adanya, jadi chat tidak berkedip
  saat dibuka dan scrollbar tidak melompat.
- Paste gambar: `Ctrl/Cmd+V` di mana saja dalam percakapan yang terbuka
  membaca clipboard lewat `arboard` (fitur `wayland-data-control`),
  menyimpannya ke PNG sementara, lalu mengunggahnya sebagai lampiran dengan
  draf sebagai caption. GNOME menolak protokol data-control, jadi bila arboard
  tidak melihat gambar jalur cadangan `xclip -t image/png -o` (XWayland)
  dipakai; ini yang menyelamatkan screenshot dari aplikasi native.
- Emoji: tombol senyum di composer membuka grid 48 emoji umum
  (`EMOJI` di `app.rs`); pilihan disisipkan ke draf. Emoji berwarna tetap
  digambar oleh `emoji.rs` di atas glyph monokrom.
- Foto: klik bubble gambar membuka viewer layar penuh (latar hitam, gambar
  diskalakan ke viewport); scroll memperbesar sampai 8×, seret menggeser saat
  sudah zoom, Esc/klik latar menutup. Toolbar viewer menyediakan Salin
  (piksel ke clipboard: `arboard`, cadangan `xclip` PNG untuk XWayland) dan
  Simpan sebagai… (dialog portal). Menu klik-kanan bubble gambar juga punya
  Salin gambar dan Simpan sebagai…, plus Balas, Salin teks, Pilih pesan, dan
  Hapus untuk saya. Video dan dokumen tetap membuka aplikasi eksternal.
- Mode "Pilih pesan" (dari menu klik-kanan bubble): header chat diganti bar
  Pilih dengan hitungan, Salin, Hapus, dan ✕; klik bubble menandai centang;
  Esc atau ✕ keluar. Salin menggabungkan teks pesan terpilih, Hapus menghapus
  baris itu dari SQLite dan layar.
- Draf dan chat terakhir: `AppSettings.last_chat` menyimpan
  `[service, account, chat_id]`, dipulihkan saat start. Teks composer yang
  belum terkirim disimpan per `service|account|chat` di `AppSettings.drafts`
  dan dipulihkan ke `self.drafts`; menghapus teks atau mengirim pesan
  membersihkan entri itu. `UNITED_OPEN_CHAT` tetap menang atas chat terakhir.
- Setelah scan QR, riwayat masuk sebagai burst: pesan ditulis satu transaksi
  (`Store::put_messages`), preview chat diperbarui sekali per percakapan, dan
  frame dibatasi ~1200 event supaya jendela tetap repaint. Pill
  "Syncing … messages…" tampil selama burst berjalan, bukan modal.
- Putus akun: klik kanan ikon layanan di rail → **Unlink account**, atau
  **Disconnect** di Settings › Services saat layanan Online/Connecting.
  `Adapter::logout` mencabut sesi (WhatsApp `logout`, Telegram `sign_out`,
  Discord stop gateway + hapus token), menghapus kredensial lokal, lalu store
  membersihkan baris layanan itu (`Store::clear_service`).
- Scroll: semua bubble yang sudah dimuat (200 terbaru per chat) di-layout apa
  adanya. State scroll di-`id_salt` per percakapan; satu id bersama membuat
  chat baru dibuka pada offset chat sebelumnya. Viewport menempel ke pesan
  terbaru (`pinned_bottom`): tinggi akhir (`content - viewport`) dicatat per
  percakapan di `chat_bottom` dan frame berikutnya diumpankan lewat
  `vertical_scroll_offset`, yang di-clamp sendiri oleh scroll area. Request
  `scroll_to_cursor` tidak dipakai lagi: target itu diambil area lain dan
  nilainya hilang, sehingga chat panjang selalu terbuka di atas. Offset hanya
  diminta saat `at_bottom` false, jadi foto yang selesai decode atau pesan baru
  tidak memicu lompatan tiap frame (sumber kedipan). Lepas saat roda digulir
  ke atas; tombol panah bawah memasangnya kembali. `MediaRef` di-parse sekali
  per pesan (`media_parsed`) dan path lokal
  di-cache (`media_locals`) supaya tidak ada `stat`/JSON per frame. Decoding
  gambar besar terjadi di satu worker thread; thumbnail (beberapa KB) di-decode
  inline supaya bubble tidak sempat tampil setinggi placeholder 62 px lalu
  melompat — itu yang terbaca sebagai kedipan saat chat dibuka. Cache tekstur
  dibatasi (media/thumb 240, avatar 320), dan transkrip di memori hanya
  menyimpan 24 percakapan terakhir; chat lain dibaca ulang dari SQLite saat
  dibuka.
- Lampiran di disk dibatasi `paths::MEDIA_BUDGET` (500 MB per layanan) dan
  dipangkas saat start + setelah tiap unduhan; file tertua dihapus lebih dulu
  dan diunduh lagi saat dibuka. Tanpa ini, cache video setahun bisa mencapai
  beberapa GB.
- Preview daftar chat hanya ditimpa bila `last_at` yang datang tidak lebih
  lama dari yang tersimpan (upsert tunggal dan batch). Tanpa guard ini, chunk
  history on-demand yang dibuka saat chat dibuka membawa preview lama dan
  menimpanya sementara `last_at` tetap yang terbaru — daftar menampilkan pesan
  usang dengan jam baru. `Chat.preview_sender` menyimpan penulis untuk grup,
  jadi baris grup langsung tampil `Nama: teks` tanpa query per frame. UPSERT
  juga mempertahankan `unread` dan `muted` saat adapter mengirim ulang baris
  dengan nilai nol. `Store::repair_previews` berjalan tiap start dan setelah
  hapus lokal: preview, `preview_sender`, dan `last_at` diturunkan ulang dari
  pesan terbaru di tabel `messages` bila berbeda, sehingga baris lama ikut
  sembuh tanpa menunggu pesan baru.
- Notifikasi desktop berjalan di task latar yang subscribe `EventBus` langsung,
  bukan dari frame UI: window minimized di Wayland bisa berhenti paint, dan
  banner tidak boleh menunggu frame berikutnya. Hanya pesan `live` yang bukan
  kiriman sendiri, chat tidak muted, dan bukan chat yang sedang fokus+terbuka.
  Panggilan masuk memakai jalur yang sama. Banner memakai appname `United` +
  ikon `united`; handle notifikasi ditahan 8 detik karena melepas koneksi
  D-Bus segera setelah `Notify` membuat GNOME menutup banner dalam ~40 ms
  (`NotificationClosed` reason 2).
- Centang pesan keluar mengikuti status: satu centang setelah server menerima,
  dua centang setelah sampai di HP, dua centang biru setelah dibaca
  (`CoreEvent::Receipt` → `Store::update_status` + patch daftar di memori).
  Riwayat dari HP memakai `WebMessageInfo.status` (DELIVERY_ACK/READ/PLAYED)
  jadi centang benar sejak paint pertama.
- Membuka chat mengirim tanda baca (`Adapter::mark_read`; WhatsApp
  `mark_as_read` per pengirim), berlangganan presence (`watch_presence`), dan
  composer mengirim "typing" maksimal tiap 4 detik plus satu "paused" saat
  kosong atau setelah kirim. Header menampilkan "typing…"/"recording audio…",
  lalu Online / Last seen, baru status koneksi.
- Daftar chat: jumlah unread tampil sebagai pil angka (unread tunggal tetap
  titik), dan draf yang belum terkirim muncul sebagai `Draft: …`. Saat chat
  dengan unread dibuka, snapshot pesan pertama yang belum dibaca dipakai untuk
  menggambar pembatas "N pesan belum dibaca"; penghitung langsung dinolkan
  tetapi pembatas tetap terlihat sampai chat ditutup.
- Klik-kanan bubble membuka menu **Balas** / **Salin** / **Hapus untuk saya**.
  Hapus hanya menghapus baris lokal lewat `Store::delete_message` (tidak
  mencabut pesan di perangkat lain), seperti "Delete for me" di web.
- Menu yang sama punya baris reaksi cepat (👍 ❤️ 😂 😮 😢 🙏), **Edit** untuk
  pesan keluar berteks, dan **Hapus untuk semua orang**. Reaksi disimpan di
  tabel `reactions` (satu baris per pengirim per pesan, upsert/delete) dan
  tampil sebagai pil emoji + jumlah di bawah bubble. Edit memakai
  `edit_message_raw` lalu mengganti body baris yang sama; revoke memakai
  `revoke_message_raw` dan menyisakan placeholder "Pesan ini telah dihapus"
  supaya posisi percakapan tidak bergeser. Keduanya masuk lewat
  `CoreEvent::MessageEdit`/`MessageRevoke`, termasuk untuk pesan dari
  perangkat lain (`protocol_message` REVOKE/MESSAGE_EDIT).
- Pesan live tidak lagi membaca ulang dua daftar chat 5000 baris:
  `patch_chat` mengganti satu baris di `self.chats`/`self.unified`, dan
  `CoreEvent::Read` menolkan unread di tempat. Muat ulang penuh hanya saat
  chat baru muncul (history sync) atau status layanan berubah.
- Bubble mendukung format WhatsApp `*tebal*`, `_miring_`, `~coret~`,
  `` `kode` `` lewat `LayoutJob`; tanda format yang tidak berpasangan tetap
  teks biasa. Perataan area klik tautan menyesuaikan byte yang dibuang.
- Klik-kanan bubble membuka menu **Balas** / **Salin** seperti WhatsApp Web.
  Balasan tampil sebagai strip di atas composer (nama, kutipan, tombol tutup)
  dan sebagai kutipan beraksen di dalam bubble. Pesan masuk membaca
  `contextInfo.stanzaId`; pesan keluar mengirim `build_quote_context`. Kalau
  pesan yang dikutip sudah lewat jendela 200 baris, isinya diambil satu baris
  dari SQLite supaya kutipan tetap tampil setelah restart.
- Ukuran piksel dari protokol (`image/video/sticker` `width`/`height`)
  disimpan di `MediaRef`; bubble memesan rasio akhir sejak frame pertama, jadi
  preview tidak lagi melompat dari kotak 62 px ke tinggi penuh saat unduhan
  selesai (efek kedap-kedip).
- `Store::repair_contact_names` mengganti judul chat dan nama pengirim yang
  masih berupa nomor dengan nama buku alamat, dijalankan saat start dan setiap
  batch `CoreEvent::Contacts`. Nama push/group subject yang asli tidak pernah
  ditimpa.
- Saat di-scroll ke atas, tombol panah melayang mengembalikan ke pesan terbaru,
  dan lampiran lama baru diunduh ketika bubble-nya benar-benar terlihat.
- Seleksi teks di bubble: drag menandai rentang karakter, `Ctrl/Cmd+C` menyalin
  bila tidak ada `TextEdit` yang fokus. Wash biru digambar per baris galley
  (egui hanya menyediakan seleksi di dalam `TextEdit`), dan permukaan drag
  dikembalikan sebagai response bubble supaya menu klik-kanan tetap jalan.
- Tampilan: wallpaper titik samar di belakang transkrip, highlight baris chat
  membulat dengan inset, bubble radius 12 px, cincin fokus halus di composer,
  lingkaran hover pada tombol lampiran, dan titik aksen di samping judul
  jendela.
- Repaint tidak lagi dipolling tiap 400 ms: thread kecil membangunkan jendela
  saat adapter mempublish event, dan timer hanya aktif selama ada animasi
  (typing, toast, syncing, decode). `settings.save()` hanya menulis kalau ada
  perubahan.
- Chat list WhatsApp/Telegram punya pil **All / Unread** seperti tab web; Discord
  tetap memakai daftar channel.
- Sheet sign-in Discord memuat tutorial 4 langkah membuat bot dan dua tautan
  resmi (Developer Portal + bot authorization flow), bukan sekadar kolom token.
- Link panduan memakai `widgets::open_url` (BROWSER → `xdg-open` → `gio open` →
  `sensible-browser` → `x-www-browser`) karena jalur `webbrowser` egui tidak
  selalu ikut di paket distro.
- Settings memakai kartu bergrup (Appearance, Window, Services, About) dengan
  switch dan segmented control, di atas latar `input_bg` seperti System Settings.

Hook debug (hanya untuk review visual, tidak memengaruhi build rilis):

| Variabel | Efek |
| --- | --- |
| `UNITED_DATA_DIR` | pakai data dir lain (mis. `/tmp/opencode/uidata`) |
| `UNITED_SCREENSHOT=/path.png` | tulis satu frame ke PNG lalu keluar |
| `UNITED_OPEN_CHAT="account\|chat"` | buka percakapan saat start |
| `UNITED_VIEW=settings` | langsung ke panel Settings |
| `UNITED_PROMPT=whatsapp` | tampilkan sheet pairing dengan QR contoh |
| `UNITED_PROMPT=discord` | tampilkan sheet tutorial bot token Discord |
| `UNITED_THEME=light\|dark` | paksa mode saat tema `system` |
| `UNITED_SCROLL=700` | buka halaman Settings pada offset scroll tertentu (uji layout) |
| `UNITED_DRAFT="teks"` | isi draf composer untuk uji tinggi/wrapping |
| `UNITED_SHOT_DELAY=18` | tunda screenshot N detik (untuk QR/animasi) |
| `UNITED_STATUS=online` | paksa semua status jadi Connected (uji tombol Disconnect) |
| `UNITED_NO_PROMPT=1` | sembunyikan sheet sign-in saat screenshot |
| `UNITED_CONTEXT=whatsapp` | buka menu klik-kanan ikon rail untuk screenshot |
| `UNITED_REPLY=1` | isi strip balasan dengan pesan teks terbaru |
| `UNITED_FAKE_REPLY=1` | paksa bubble terbaru mengutip pesan sebelumnya (uji kutipan tanpa mengubah DB) |
| `UNITED_SCROLL_ANIM=1` | geser percakapan otomatis; melepas tempelan ke bawah supaya benar-benar bergerak (uji kelancaran scroll + unduh media malas) |
| `UNITED_SELECT=1` | tandai 6 karakter pertama bubble terbaru (uji wash seleksi) |
| `UNITED_EMOJI=1` | buka picker emoji composer saat start (uji grid + emoji berwarna) |
| `UNITED_TEST_NOTIFY=1` | tembakkan satu notifikasi desktop saat frame pertama (uji D-Bus) |
| `UNITED_CALL_TEST=1` | tampilkan layar panggilan masuk contoh (uji layout, tanpa panggilan nyata) |
| `UNITED_FPS=1` | log fps + waktu tiap bagian UI per 2 detik |
| `UNITED_PROF=1` | sertakan rincian chrome/rail/main di log fps |
| `UNITED_FLOOD=12000` | replay N baris riwayat lewat jalur batch (uji ANR) |
| `UNITED_PASTE_TEST=1` | baca gambar clipboard sekali dan catat path hasilnya di log |
| `UNITED_WA_TAB=media\|advertise\|calls\|status\|...` | buka tab/sheet WhatsApp untuk screenshot |
| `UNITED_TG=drawer\|more\|contacts\|settings\|profile\|info` | buka pane Telegram Web A untuk screenshot |
| `UNITED_DC=friends\|dm\|nitro\|shop\|quests\|addfriend\|addserver\|discover` | buka pane Discord + 5 DM contoh bila belum ada akun |

## Cakupan Rust

Tidak ada kode .NET, WebView2, WebKit, atau Chromium yang ikut jalan. Semua yang
dipakai aplikasi ini adalah Rust:

| Bagian | Implementasi |
| --- | --- |
| Model, chat, pesan, unread | `united-core` (Rust, `rusqlite` bundled) |
| Event bus antar-adapter | `tokio::sync::broadcast` |
| UI, layout, animasi | `egui`/`eframe` + GLOW (OpenGL) |
| Ikon, QR, avatar, bubble | digambar di `united-ui` (`icons.rs`, `qr.rs`) |
| WhatsApp | `whatsapp-rust` (protokol multi-device langsung, SQLite store) |
| Riwayat WhatsApp | `HistorySync` event didekode per conversation (`HistorySyncStream`) |
| Telegram | `grammers` (MTProto, bukan web/tdlib) |
| Discord | REST v9 + Gateway WebSocket (tanpa gateway web client) |
| Notifikasi | `notify-rust` (D-Bus di Linux, toast di Windows) |
| Build/instal | `cargo` + `packaging/install.sh`, `PKGBUILD` |

Yang **bukan** Rust hanya dependensi sistem yang di-link: driver OpenGL (mesa),
`libxkbcommon`/X11/Wayland, dan pustaka C untuk SQLite (di-bundle lalu di-link).

## Benchmark

`rust/tools/bench.sh` mengukur langsung dari kernel (`/proc/<pid>/status`), bukan
dari alokator: cold start (sampai CPU berhenti naik), RSS median, puncak RSS,
jumlah thread, dan file descriptor. Jalankan:

```bash
cd rust && cargo build --release -p united-app
./tools/bench.sh target/release/united /tmp/bench-10s 10
```

Hasil di mesin ini (Arch, Wayland, Ryzen, binary 27 MB):

| Skenario | Cold start | RSS median | Puncak RSS | Thread | FD |
| --- | --- | --- | --- | --- | --- |
| UI saja (tidak ada layanan aktif) | 1,3–2,2 s | 98,7 MiB | 98,7 MiB | 18 | 30 |
| Semua adapter aktif, belum login | 2,0–2,7 s | 109,2 MiB | 109,2 MiB | 23 | 36 |
| WhatsApp + 1 chat (terukur sebelumnya) | ~1,4 s | ~107 MiB | ~110 MiB | 20 | 35 |

Cold start bervariasi 1–3 detik tergantung cache disk dan beban mesin; RSS tidak.

Pembanding: build .NET 10 + WebView2 lama menjalankan satu proses renderer
tambahan; RSS-nya 2–4× lipat karena Blink + compositor. Angka di atas stabil
(sampling 250 ms selama 10 detik) dan tidak naik setelah idle.

Kalau RSS perlu lebih kecil: font emoji fallback egui adalah bagian terbesar yang
bisa dipangkas (~10–15 MiB) dengan mengorbankan emoji di isi pesan.

## Status protokol

| Layanan | Protokol | Status |
|---|---|---|
| WhatsApp | multi-device (Noise + Signal), `whatsapp-rust` rev `e634d24` | QR pairing, pesan masuk/keluar, media placeholder, history via sync perangkat |
| Telegram | MTProto, `grammers-client 0.12` | login nomor → kode → password 2FA, dialog sync, pesan masuk/keluar |
| Discord | REST v9 + Gateway v10 | token dari web client, READY, MESSAGE_CREATE, history warm-up, kirim pesan |

### Kenapa Messenger, Instagram, dan Threads tidak ada

Ketiganya tidak punya API pesan untuk akun pribadi. Klien web memakai GraphQL
privat + MQTT (dokumentasi tidak resmi), dan Threads belum punya DM sama sekali.
Implementasi native berarti mem-port ulang protokol privat itu (seperti
`mautrix/meta` di Go) dan akan rusak setiap kali Meta mengubah skema. Karena itu
ketiganya tidak masuk daftar layanan; hanya WhatsApp, Telegram, dan Discord yang
punya adapter native.

## Build & install

Butuh Rust stable (1.98+), lalu:

```bash
cd rust
cargo build --release -p united-app     # binary: target/release/united
./packaging/install.sh                  # ~/.local (PREFIX=/usr untuk sistem)
```

`install.sh` hanya menyalin binary, `united.desktop`, dan ikon PNG
(`packaging/united-256.png` / `united-512.png` ke
`hicolor/{256x256,512x512}/apps/united.png`; SVG lama dihapus karena menang
lookup tema). Ikon yang sama ditanam di binary untuk judul jendela dan taskbar
(`include_bytes!`), jadi berlaku untuk distro apa pun (Arch, Debian/Ubuntu,
Fedora, openSUSE, Alpine + glibc…).
`packaging/PKGBUILD` tersedia untuk Arch.

Dependensi runtime (X11 atau Wayland): `libxkbcommon`, `libx11`/`wayland`,
`libxcursor`, `libxi`, `libxrandr`, driver GL (`mesa`). Binary memakai GLOW
(OpenGL) — tidak ada Vulkan wajib.

Ukuran rilis: binary 41 MB (LTO + strip, termasuk stack VoIP native), RSS
~107 MiB saat idle dengan sesi
WhatsApp terhubung. Tidak ada proses anak, tidak ada GPU process terpisah.
Saat panggilan berlangsung, `pw-record` dan `pw-cat` hidup sebagai proses audio;
keduanya dimatikan begitu telepon selesai.

Panggilan suara 1:1 WhatsApp memakai stack VoIP bawaan `whatsapp-rust`
(fitur `voip-mlow` + `voip-relay-native`, codec MLOW murni Rust, tanpa
libopus/CPAL). Tombol telepon di header chat langsung memanggil; panggilan
masuk menampilkan layar penuh dengan Terima/Tolak, panggilan aktif menghitung
durasi dan punya tombol Akhiri. Audio 16 kHz mono dijembatani ke PipeWire lewat
`pw-record` (mikrofon) dan `pw-cat --playback` (speaker); tidak ada audio
device yang dibuka saat idle. Panggilan grup dan video belum didukung.

Notifikasi memakai `org.freedesktop.Notifications` (D-Bus), jadi berjalan di
GNOME/KDE/XFCE tanpa dependensi khusus. Banner muncul untuk pesan masuk live
saat chat tidak terbuka, atau saat jendela tidak fokus; chat terbuka yang
sedang fokus dianggap sudah dibaca, dan chat mute tidak pernah memunculkan
banner. Panggilan D-Bus berjalan di thread terpisah supaya daemon yang lambat
tidak membekukan frame. "Launch at login" menulis
`~/.config/autostart/united.desktop`. Satu instance per data dir dijaga dengan
`flock` supaya dua proses tidak berebut device store WhatsApp.

## Windows

Kode sumber sudah dibuat portabel, tetapi **belum diuji di Windows** — mesin
pengembangan ini hanya punya toolchain Linux, jadi tidak ada cross-check
`cargo check --target x86_64-pc-windows-*` yang bisa dijalankan.

Yang sudah dipindahkan ke jalur portabel:

- Single-instance lock: `flock` di Unix, file `create_new` (dihapus saat keluar)
  di Windows, lihat `lock_data_dir()` di `united-app/src/main.rs`.
- Format waktu tidak lagi memakai `libc::localtime_r`/`uname`; `chrono` dipakai
  supaya zona waktu lokal bekerja di CRT Windows juga.
- Permission 0600 untuk kredensial hanya di-set di Unix (`#[cfg(unix)]`).
- Notifikasi memakai backend Windows milik `notify-rust`.
- `libc` sekarang hanya dependensi `cfg(unix)`.

Yang masih khusus Unix/desktop Linux dan perlu pekerjaan kalau Windows jadi
target resmi: `packaging/install.sh`, `PKGBUILD`, entri `.desktop`, dan
"Launch at login" yang menulis `~/.config/autostart/united.desktop`. Di Windows
opsi itu tidak berpengaruh; gantinya perlu entri registry `Run` atau shortcut di
folder Startup.

## Data & sesi

- `$UNITED_DATA_DIR` atau `~/.local/share/united`
- `united.db` — chat, pesan, status baca (SQLite WAL)
- `settings.json` — tema, layanan aktif, perilaku tutup jendela
- `sessions/<service>/` — device store WhatsApp, sesi Telegram, token Discord (0600)

## Kredensial layanan

- **WhatsApp**: jalan **sepenuhnya lokal**: protokol multi-device ditangani
  `whatsapp-rust` di proses ini, tidak ada browser/WebView yang ikut jalan dan
  tidak ada data chat yang dikirim ke server selain ke WhatsApp sendiri.
  Scan QR di modal login (Linked devices). Kode hanya hidup ~2
  menit (WhatsApp merotasi enam ref lalu menutup socket); kalau habis, sheet
  berubah jadi "This code expired" dengan tombol **Show a new code** yang
  membuka sesi pairing baru tanpa restart aplikasi.
- **Telegram**: login butuh `api_id`/`api_hash` dari
  <https://my.telegram.org/apps>. Kalau belum ada, aplikasi menampilkan form
  kredensial saat pertama dibuka dan menyimpannya ke
  `sessions/telegram/credentials.json` (0600) sebelum lanjut ke langkah nomor
  telepon. Tanpa kredensial ini Telegram memang menolak semua login.
  Alternatif tanpa form: `UNITED_TELEGRAM_API_ID` +
  `UNITED_TELEGRAM_API_HASH`.
- **Discord**: token **bot** dari <https://discord.com/developers/applications>
  (Bot → Reset Token), disimpan mode 0600. Discord tidak punya API resmi untuk
  akun pribadi; token user melanggar ToS dan bisa membuat akun dibanned.

### WhatsApp: chat lama belum muncul?

WhatsApp mengirim **snapshot riwayat sekali per link** (yang terbaru saja, bukan
seluruh arsip), persis seperti WhatsApp Web/ZapFast. Jadi:

1. Chat yang belum pernah dikirim ke perangkat ini tidak bisa didaftar dari
   aplikasi; buka chat yang sudah ada lalu scroll ke atas untuk menarik halaman
   lebih tua lewat PDO (HP harus online).
2. Untuk memicu snapshot baru: HP → WhatsApp → **Linked devices** → pilih
   perangkat ini → **Log out**, lalu buka aplikasi → **Sign In** → **Show a new
   code** dan scan. Build ini menghapus session lama saat logout sehingga
   pairing berikutnya dianggap perangkat baru dan riwayat terbaru dikirim ulang.
3. Chat "Message yourself" (nomor sendiri) sengaja tidak ditampilkan; pesan
   keluar tetap tersimpan di chat lawan bicara yang benar.

Semua langkah di atas juga ada di aplikasi: **Settings → Sign-in guide**,
lengkap dengan tautan resmi tiap layanan.
