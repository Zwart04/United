//! Apple-Messages-style list rows, bubbles and composer.

use egui::{
    Align2, Color32, CornerRadius, Frame, Margin, Pos2, Rect, Response, RichText, Sense, Stroke,
    Ui, Vec2,
};
use united_core::{Chat, Message, Service};

use crate::format;
use crate::icons::{self, Icon};
use crate::theme::{self, HAIRLINE, Palette};

/// Circular avatar: the downloaded profile picture when there is one, otherwise
/// the initials macOS Messages would generate.
pub fn avatar(
    ui: &mut Ui,
    name: &str,
    size: f32,
    seed: &str,
    picture: Option<&egui::TextureHandle>,
) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    if let Some(texture) = picture {
        egui::Image::new((texture.id(), Vec2::splat(size)))
            .corner_radius((size / 2.0) as u8)
            .paint_at(ui, rect);
        return resp;
    }
    let painter = ui.painter();
    painter.circle_filled(rect.center(), size / 2.0, format::avatar_color(seed));
    if name.chars().any(|c| c.is_alphabetic()) {
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            format::initials(name),
            theme::medium(size * 0.4),
            Color32::WHITE,
        );
    } else {
        // Phone numbers have no initials; a person mark reads better than "+8".
        crate::icons::paint(
            painter,
            Rect::from_center_size(rect.center(), Vec2::splat(size * 0.62)),
            crate::icons::Icon::Person,
            Color32::WHITE,
            2.0,
        );
    }
    resp
}

/// Sidebar logo tile: official artwork, rounded selection fill behind it.
pub fn service_chip(
    ui: &mut Ui,
    service: Service,
    size: f32,
    active: bool,
    badge: u32,
    p: &Palette,
) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size + 12.0), Sense::click());
    let center = rect.center();
    let tile = Rect::from_center_size(center, Vec2::splat(size + 4.0));

    if active {
        // Selected service: tinted tile in its own brand plus a rail marker.
        let brand = theme::brand(service);
        ui.painter().rect_filled(
            tile,
            CornerRadius::same(11),
            Color32::from_rgba_unmultiplied(brand.r(), brand.g(), brand.b(), 36),
        );
        let bar = Rect::from_min_size(
            egui::pos2(tile.left() - 5.0, tile.center().y - 10.0),
            Vec2::new(3.0, 20.0),
        );
        ui.painter().rect_filled(bar, CornerRadius::same(2), brand);
    } else if resp.hovered() {
        ui.painter()
            .rect_filled(tile, CornerRadius::same(11), p.hover);
    }

    let logo = Rect::from_center_size(center, Vec2::splat(size - 8.0));
    match brand_icon(service) {
        Some(src) => {
            egui::Image::new(src)
                .fit_to_exact_size(logo.size())
                .paint_at(ui, logo);
        }
        None => {
            ui.painter()
                .circle_filled(logo.center(), logo.width() / 2.0, theme::brand(service));
        }
    }

    if badge > 0 {
        let pos = center + Vec2::new(size * 0.38, -size * 0.38);
        let label = if badge > 9 {
            "9+".to_string()
        } else {
            badge.to_string()
        };
        let galley = ui
            .painter()
            .layout_no_wrap(label, theme::semibold(9.5), Color32::WHITE);
        let w = (galley.size().x + 8.0).max(16.0);
        let pill = Rect::from_center_size(pos, Vec2::new(w, 16.0));
        ui.painter().rect_filled(
            pill,
            CornerRadius::same(8),
            Color32::from_rgb(0xff, 0x3b, 0x30),
        );
        ui.painter()
            .galley(pill.center() - galley.size() / 2.0, galley, Color32::WHITE);
    }
    resp
}

pub fn brand_icon(service: Service) -> Option<egui::ImageSource<'static>> {
    Some(match service {
        Service::WhatsApp => egui::include_image!("../assets/whatsapp.svg"),
        Service::Telegram => egui::include_image!("../assets/telegram.svg"),
        Service::Discord => egui::include_image!("../assets/discord.svg"),
    })
}

pub fn unread_pill(ui: &mut Ui, count: u32, accent: Color32) {
    if count == 0 {
        return;
    }
    let text = if count > 99 {
        "99+".to_string()
    } else {
        count.to_string()
    };
    let galley = ui
        .painter()
        .layout_no_wrap(text, theme::semibold(11.0), accent);
    ui.allocate_exact_size(galley.size(), Sense::hover());
}

/// One conversation row. Returns true when clicked.
pub fn chat_row(
    ui: &mut Ui,
    chat: &Chat,
    p: &Palette,
    selected: bool,
    unified: bool,
    time: &str,
    picture: Option<&egui::TextureHandle>,
    pinned: bool,
) -> bool {
    let width = ui.available_width();
    let height = 72.0;
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, height), Sense::click());
    let highlight = rect;
    if selected {
        ui.painter()
            .rect_filled(highlight, CornerRadius::ZERO, p.selected);
    } else if resp.hovered() {
        ui.painter()
            .rect_filled(highlight, CornerRadius::ZERO, p.hover);
    }

    let center_y = rect.min.y + height / 2.0;
    let avatar_rect =
        Rect::from_center_size(egui::pos2(rect.min.x + 36.0, center_y), Vec2::splat(49.0));
    if let Some(texture) = picture {
        egui::Image::new((texture.id(), Vec2::splat(49.0)))
            .corner_radius(25)
            .paint_at(ui, avatar_rect);
    } else {
        ui.painter().circle_filled(
            avatar_rect.center(),
            24.5,
            format::avatar_color(&chat.title),
        );
        if chat.title.chars().any(|c| c.is_alphabetic()) {
            ui.painter().text(
                avatar_rect.center(),
                Align2::CENTER_CENTER,
                format::initials(&chat.title),
                theme::medium(17.0),
                Color32::WHITE,
            );
        } else {
            crate::icons::paint(
                ui.painter(),
                Rect::from_center_size(avatar_rect.center(), Vec2::splat(25.0)),
                crate::icons::Icon::Person,
                Color32::WHITE,
                2.0,
            );
        }
    }

    let text_x = rect.min.x + 78.0;
    let right = rect.max.x - 16.0;

    ui.painter().text(
        egui::pos2(text_x, rect.min.y + 26.0),
        Align2::LEFT_CENTER,
        elide(&chat.title, right - text_x - 76.0, 15.5),
        theme::semibold(15.5),
        p.text,
    );
    let time_galley =
        ui.painter()
            .layout_no_wrap(time.to_string(), theme::regular(11.5), p.text_muted);
    let time_w = time_galley.size().x;
    ui.painter().galley(
        egui::pos2(
            right - time_w,
            rect.min.y + 26.0 - time_galley.size().y / 2.0,
        ),
        time_galley,
        p.text_muted,
    );
    // Pinned and muted state sit between the name and the clock, the way both
    // web clients signal them without opening the conversation.
    let mut mark_x = right - time_w - 15.0;
    if chat.muted {
        crate::icons::paint(
            ui.painter(),
            Rect::from_center_size(egui::pos2(mark_x, rect.min.y + 26.0), Vec2::splat(13.0)),
            crate::icons::Icon::Mute,
            p.text_muted,
            1.3,
        );
        mark_x -= 15.0;
    }
    if pinned {
        crate::icons::paint(
            ui.painter(),
            Rect::from_center_size(egui::pos2(mark_x, rect.min.y + 26.0), Vec2::splat(13.0)),
            crate::icons::Icon::Pin,
            p.text_muted,
            1.3,
        );
    }

    let mut preview_end = right;
    if chat.unread > 0 {
        // A count pill, exactly like WhatsApp Web: "1" is a number too, not a
        // dot, so the badge always tells how much is waiting.
        let pill = egui::pos2(right - 3.0, rect.min.y + 52.0);
        let text = if chat.unread > 99 {
            "99+".to_string()
        } else {
            chat.unread.to_string()
        };
        let galley = ui
            .painter()
            .layout_no_wrap(text, theme::semibold(10.5), Color32::WHITE);
        let size = Vec2::new(galley.size().x + 10.0, 17.0);
        let rect_pill = Rect::from_center_size(egui::pos2(pill.x - size.x / 2.0, pill.y), size);
        ui.painter()
            .rect_filled(rect_pill, CornerRadius::same(9), p.accent);
        ui.painter().galley(
            egui::pos2(
                rect_pill.center().x - galley.size().x / 2.0,
                rect_pill.center().y - galley.size().y / 2.0,
            ),
            galley,
            Color32::WHITE,
        );
        preview_end = rect_pill.min.x - 6.0;
    } else if unified {
        // Service identity as a small brand dot, keeping the merged list scannable.
        let dot = egui::pos2(right - 3.0, rect.min.y + 52.0);
        ui.painter()
            .circle_filled(dot, 3.5, theme::brand(chat.service));
        preview_end = right - 14.0;
    }

    let preview = if chat.preview.is_empty() {
        "No messages yet".to_string()
    } else {
        // Chat lists show plain text: marker pairs are dropped, exactly like
        // the bubbles that carry them.
        let (runs, _) = markup_runs(&chat.preview);
        let plain = runs
            .iter()
            .map(|(range, _)| &chat.preview[range.clone()])
            .collect::<String>()
            .replace('\n', " ");
        display_text(&plain).into_owned()
    };
    let sender = if chat.preview_sender.is_empty() {
        String::new()
    } else {
        format!("{}: ", chat.preview_sender)
    };
    let mut body_x = text_x;
    if !sender.is_empty() {
        let galley = ui
            .painter()
            .layout_no_wrap(sender, theme::regular(13.5), p.text);
        body_x += galley.size().x;
        ui.painter().galley(
            egui::pos2(text_x, rect.min.y + 49.0 - galley.size().y / 2.0),
            galley,
            p.text,
        );
    }
    ui.painter().text(
        egui::pos2(body_x, rect.min.y + 49.0),
        Align2::LEFT_CENTER,
        elide(&preview, preview_end - body_x, 13.5),
        theme::regular(13.5),
        p.text_muted,
    );

    ui.painter().hline(
        egui::Rangef::new(text_x, rect.max.x),
        rect.max.y - HAIRLINE,
        Stroke::new(HAIRLINE, p.divider),
    );
    resp.clicked()
}

fn elide(text: &str, width: f32, font_size: f32) -> String {
    if width <= 0.0 {
        return String::new();
    }
    let max_chars = (width / (font_size * 0.52)) as usize;
    let count = text.chars().count();
    if count <= max_chars {
        return text.to_string();
    }
    let keep = max_chars.saturating_sub(1);
    let mut out: String = text.chars().take(keep).collect();
    out.push('…');
    out
}

/// An attachment drawn inside a bubble. The texture is `None` for documents,
/// videos and audio, which render as a labelled tile instead.
#[derive(Clone)]
pub struct BubbleMedia<'a> {
    /// Full-resolution photo or sticker, once downloaded.
    pub texture: Option<&'a egui::TextureHandle>,
    /// Low-resolution protocol preview (video frame, sticker still).
    pub thumb: Option<&'a egui::TextureHandle>,
    pub path: String,
    pub label: String,
    pub caption: Option<String>,
    pub sticker: bool,
    /// Video tile: draw a play badge over the thumbnail.
    pub play: bool,
    /// Protocol pixel size, so the placeholder keeps the final aspect ratio.
    pub aspect: Option<(u32, u32)>,
    /// Image, video or sticker (as opposed to a document/audio tile).
    pub picture: bool,
}

/// Quoted message drawn above a reply, like WhatsApp Web's reply preview.
#[derive(Clone, Copy)]
pub struct Quoted<'a> {
    pub name: &'a str,
    pub body: &'a str,
}

fn paint_quote(
    ui: &Ui,
    origin: Pos2,
    width: f32,
    quote: &Quoted<'_>,
    text_color: Color32,
    accent: Color32,
) {
    const HEIGHT: f32 = 44.0;
    let rect = Rect::from_min_size(origin, Vec2::new(width, HEIGHT));
    ui.painter()
        .rect_filled(rect, 6, Color32::from_black_alpha(30));
    ui.painter().rect_filled(
        Rect::from_min_size(origin, Vec2::new(3.0, HEIGHT)),
        2,
        accent,
    );
    let name = ui.painter().layout_no_wrap(
        display_text(quote.name).into_owned(),
        theme::semibold(12.0),
        accent,
    );
    ui.painter()
        .galley(origin + Vec2::new(10.0, 5.0), name, accent);
    let body = display_text(quote.body).replace('\n', " ");
    let body = elide(&body, (width - 20.0).max(40.0), 12.5);
    let galley = ui
        .painter()
        .layout(body, theme::regular(12.5), text_color, width - 20.0);
    ui.painter()
        .galley(origin + Vec2::new(10.0, 21.0), galley, text_color);
}

/// Highlight a character range of a laid-out galley. egui only offers
/// selection inside `TextEdit`; bubbles paint their own galleys, so the same
/// blue wash is reconstructed row by row.
fn paint_text_selection(ui: &Ui, galley: &egui::Galley, origin: Pos2, a: usize, b: usize) {
    let (lo, hi) = (a.min(b), a.max(b));
    if lo == hi {
        return;
    }
    let color = Color32::from_rgba_unmultiplied(0x4c, 0x9a, 0xff, 96);
    let mut base = 0usize;
    for row in &galley.rows {
        let count = row.char_count_excluding_newline().0;
        let start = lo.max(base);
        let end = hi.min(base + count);
        if start < end {
            let x0 = row.pos.x + row.x_offset(egui::text::CharIndex(start - base));
            let x1 = row.pos.x + row.x_offset(egui::text::CharIndex(end - base));
            ui.painter().rect_filled(
                Rect::from_min_max(
                    origin + Vec2::new(x0, row.pos.y),
                    origin + Vec2::new(x1, row.pos.y + row.size.y),
                ),
                1,
                color,
            );
        }
        base += count + usize::from(row.ends_with_newline);
        if base > hi {
            break;
        }
    }
}

/// Faint dotted wallpaper behind the transcript. WhatsApp ships a hand-drawn
/// doodle; a low-alpha dot grid gives the same depth without another asset.
pub fn wallpaper(ui: &Ui, p: &Palette) {
    let rect = ui.max_rect();
    let alpha = if p.chat_bg.r() > 128 { 18 } else { 10 };
    let dot = Color32::from_rgba_unmultiplied(
        p.text_muted.r(),
        p.text_muted.g(),
        p.text_muted.b(),
        alpha,
    );
    let painter = ui.painter();
    let step = 28.0;
    let mut y = rect.top();
    while y < rect.bottom() {
        let mut x = rect.left();
        while x < rect.right() {
            painter.circle_filled(egui::pos2(x, y), 1.0, dot);
            x += step;
        }
        y += step;
    }
}

/// Byte ranges of bare URLs in one message body. Punctuation that hugs a link
/// (".", ")", ",") is trimmed so the clickable area stays exact.
/// Split a message body into styled runs. WhatsApp formatting is a toggle
/// syntax: `*bold*`, `_italic_`, `~strike~`, `` `mono` ``. Unpaired markers
/// stay literal, exactly like the web clients. Flags: 1 bold, 2 italic,
/// 4 strike, 8 mono; markers themselves are dropped from the output.
///
/// The second return maps a byte offset in the body to its offset in the
/// rendered text, so link rectangles still line up after markers vanish.
fn markup_runs(text: &str) -> (Vec<(std::ops::Range<usize>, u8)>, Vec<usize>) {
    let bytes = text.as_bytes();
    let mut flags = vec![0u8; bytes.len()];
    let mut drop = vec![false; bytes.len()];
    let mut i = 0;
    while i < bytes.len() {
        let (bit, close) = match bytes[i] {
            b'*' => (1u8, b'*'),
            b'_' => (2, b'_'),
            b'~' => (4, b'~'),
            b'`' => (8, b'`'),
            _ => {
                i += 1;
                continue;
            }
        };
        // The closing marker must be later and the pair non-empty.
        if let Some(off) = bytes[i + 1..].iter().position(|&c| c == close) {
            let end = i + 1 + off;
            if end > i + 1 {
                for flag in flags.iter_mut().take(end).skip(i + 1) {
                    *flag |= bit;
                }
                drop[i] = true;
                drop[end] = true;
            }
        }
        i += 1;
    }

    let mut runs: Vec<(std::ops::Range<usize>, u8)> = Vec::new();
    let mut shift = vec![0usize; bytes.len() + 1];
    let mut dropped = 0;
    for (index, &byte) in bytes.iter().enumerate() {
        shift[index] = dropped;
        let _ = byte;
        if drop[index] {
            dropped += 1;
            continue;
        }
        match runs.last_mut() {
            Some((range, flag)) if *flag == flags[index] && range.end == index => {
                range.end = index + 1;
            }
            _ => runs.push((index..index + 1, flags[index])),
        }
    }
    shift[bytes.len()] = dropped;
    (runs, shift)
}

fn link_spans(text: &str) -> Vec<(std::ops::Range<usize>, &str)> {
    let mut spans = Vec::new();
    let mut search = 0;
    for token in text.split_whitespace() {
        let Some(found) = text[search..].find(token) else {
            break;
        };
        let offset = search + found;
        let trimmed = token.trim_end_matches(['.', ',', ';', ':', '!', '?', ')', ']', '}']);
        if trimmed.starts_with("http://")
            || trimmed.starts_with("https://")
            || trimmed.starts_with("www.")
        {
            spans.push((offset..offset + trimmed.len(), trimmed));
        }
        search = offset + token.len();
    }
    spans
}

/// Ticks beside the clock of an outgoing bubble: one check once the server
/// has it, two when it reached the phone, blue when it was read. Painted as
/// strokes so no font has to carry a glyph.
fn paint_ticks(ui: &Ui, msg: &Message, ink: Color32, tick_x: f32, tick_y: f32) {
    if !msg.outgoing || !matches!(msg.service, Service::WhatsApp | Service::Telegram) {
        return;
    }
    let stroke = Stroke::new(1.2, ink);
    let wide = msg.service == Service::Telegram;
    let double = wide
        || matches!(
            msg.status,
            united_core::model::MessageStatus::Delivered | united_core::model::MessageStatus::Read
        );
    ui.painter().line_segment(
        [
            egui::pos2(tick_x, tick_y),
            egui::pos2(tick_x + 3.0, tick_y + 3.0),
        ],
        stroke,
    );
    ui.painter().line_segment(
        [
            egui::pos2(tick_x + 3.0, tick_y + 3.0),
            egui::pos2(tick_x + 9.0, tick_y - 4.0),
        ],
        stroke,
    );
    if double {
        ui.painter().line_segment(
            [
                egui::pos2(tick_x + 5.0, tick_y + 3.0),
                egui::pos2(tick_x + 14.0, tick_y - 4.0),
            ],
            stroke,
        );
    }
}

/// Tick ink that reads on both the bubble fill and a photo scrim.
fn tick_ink(p: &Palette, msg: &Message) -> Color32 {
    if msg.status == united_core::model::MessageStatus::Read {
        Color32::from_rgb(0x53, 0xbd, 0xeb)
    } else if p.on_bubble_out == Color32::WHITE {
        Color32::from_white_alpha(200)
    } else {
        p.text_muted
    }
}

/// `widgets::open_url` needs a scheme; bare "www." links get https.
fn http_url(url: &str) -> String {
    if url.starts_with("http://") || url.starts_with("https://") {
        url.to_string()
    } else {
        format!("https://{url}")
    }
}

/// iMessage-style bubble: 18 pt radius, tuning-fork tail omitted for calm
/// rendering, timestamp tucked into the bottom-right corner.
/// egui lays text out code point by code point: no shaping and no bidi. Arabic
/// therefore arrives as isolated letters in left-to-right order. Reshape to
/// presentation forms and run the Unicode bidi algorithm per paragraph before
/// layout. Latin-only text is returned untouched so kerning stays intact.
pub(crate) fn display_text(text: &str) -> std::borrow::Cow<'_, str> {
    let is_arabic = |ch: char| {
        matches!(ch as u32,
            0x0600..=0x06FF | 0x0750..=0x077F | 0x08A0..=0x08FF
            | 0xFB50..=0xFDFF | 0xFE70..=0xFEFF)
    };
    if !text.chars().any(is_arabic) {
        return text.into();
    }
    // Shaping and bidi on every frame for every Arabic bubble was measurable;
    // cache the result per body and only recompute on a new message.
    static CACHE: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<String, String>>> =
        std::sync::OnceLock::new();
    let cache = CACHE.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()));
    if let Some(hit) = cache.lock().unwrap().get(text) {
        return hit.clone().into();
    }
    // Building the shaper compiles its ligature patterns; reuse one instance.
    static SHAPER: std::sync::OnceLock<arabic_reshaper::ArabicReshaper<'static>> =
        std::sync::OnceLock::new();
    let shaped = SHAPER
        .get_or_init(arabic_reshaper::ArabicReshaper::new)
        .reshape(text);
    // The shaper folds "الله" into U+FDF2, and DejaVu (the broadest Arabic
    // fallback most distros ship) has no glyph for it: expand the ligature
    // back into the presentation forms the font can draw.
    let shaped = shaped.replace('\u{FDF2}', "\u{FE8D}\u{FEDF}\u{FEE0}\u{FEEA}");
    let bidi = unicode_bidi::BidiInfo::new(&shaped, None);
    let mut out = String::with_capacity(shaped.len());
    for para in &bidi.paragraphs {
        out.push_str(&bidi.reorder_line(para, para.range.clone()));
    }
    let mut cache = cache.lock().unwrap();
    if cache.len() > 512 {
        cache.clear();
    }
    cache.insert(text.to_string(), out.clone());
    out.into()
}

pub fn bubble(
    ui: &mut Ui,
    msg: &Message,
    body: &str,
    p: &Palette,
    show_author: bool,
    media: Option<BubbleMedia<'_>>,
    mentions: &[std::ops::Range<usize>],
    quoted: Option<Quoted<'_>>,
) -> Response {
    let max_w = (ui.available_width() * 0.72).clamp(150.0, 560.0);
    let text_color = if msg.outgoing {
        p.on_bubble_out
    } else {
        p.text
    };
    let bg = if msg.outgoing {
        p.bubble_out
    } else {
        p.bubble_in
    };
    let meta = format::clock(msg.at);
    let meta_color = if msg.outgoing {
        if p.on_bubble_out == Color32::WHITE {
            Color32::from_white_alpha(190)
        } else {
            p.text_muted
        }
    } else {
        p.text_muted
    };
    let meta_galley = ui
        .painter()
        .layout_no_wrap(meta.clone(), theme::regular(10.5), meta_color);
    let meta_w = meta_galley.size().x + 20.0;
    let pad = Vec2::new(13.0, 8.0);

    if media.is_none() {
        // Text message: the clock sits on the last line, bottom right. Bare
        // URLs are painted blue and clickable instead of dead text.
        // A reshaped body no longer matches the stored byte offsets, so the
        // link and mention decoration only runs on the original text.
        let shaped = display_text(body);
        let plain_rtl = shaped != body;
        let body = if plain_rtl { shaped.as_ref() } else { body };
        let links = if plain_rtl {
            Vec::new()
        } else {
            link_spans(body)
        };
        let mentions = if plain_rtl { &[][..] } else { mentions };
        // WhatsApp paints links and @mentions green. On the dark outgoing
        // bubble a mint green keeps contrast; elsewhere the accent green does.
        let link_color = if msg.outgoing {
            if p.on_bubble_out == Color32::WHITE {
                Color32::from_rgb(0xa7, 0xf3, 0xd0)
            } else {
                Color32::from_rgb(0x02, 0x7a, 0x48)
            }
        } else {
            Color32::from_rgb(0x00, 0xa8, 0x84)
        };
        let font = theme::regular(14.5);
        let plain = egui::TextFormat {
            font_id: font.clone(),
            color: text_color,
            ..Default::default()
        };
        let link_format = egui::TextFormat {
            font_id: font.clone(),
            color: link_color,
            underline: Stroke::new(1.0, link_color),
            ..Default::default()
        };
        let mention_format = egui::TextFormat {
            font_id: font,
            color: link_color,
            ..Default::default()
        };
        let mut job = egui::text::LayoutJob::default();
        // Leave room for the clock and the ticks that sit left of it, or the
        // last word runs under them.
        job.wrap.max_width = (max_w - 26.0 - meta_w - 15.0).max(80.0);
        // Links and inline markup are one pass: a URL inside `*bold*` keeps
        // both its colour and its weight.
        let (runs, shift) = if plain_rtl {
            (vec![(0..body.len(), 0u8)], vec![0usize; body.len() + 1])
        } else {
            markup_runs(body)
        };
        // A markup run spans a whole unformatted paragraph, so it usually
        // contains a URL without *being* the URL. Cut every run at link and
        // mention edges before choosing a colour, or the link stays plain.
        for (range, flags) in &runs {
            let mut edges = vec![range.start, range.end];
            for (link, _) in &links {
                edges.push(link.start.clamp(range.start, range.end));
                edges.push(link.end.clamp(range.start, range.end));
            }
            for tag in mentions {
                edges.push(tag.start.clamp(range.start, range.end));
                edges.push(tag.end.clamp(range.start, range.end));
            }
            edges.sort_unstable();
            edges.dedup();
            for pair in edges.windows(2) {
                let (start, end) = (pair[0], pair[1]);
                if start >= end {
                    continue;
                }
                let mut format = if links
                    .iter()
                    .any(|(link, _)| link.start <= start && end <= link.end)
                {
                    link_format.clone()
                } else if mentions
                    .iter()
                    .any(|tag| tag.start <= start && end <= tag.end)
                {
                    mention_format.clone()
                } else {
                    plain.clone()
                };
                if flags & 2 != 0 {
                    format.italics = true;
                }
                if flags & 1 != 0 {
                    format.font_id = theme::semibold(14.5);
                }
                if flags & 8 != 0 {
                    format.font_id = theme::mono(13.5);
                }
                if flags & 4 != 0 {
                    format.strikethrough = Stroke::new(1.0, format.color);
                }
                job.append(&body[start..end], 0.0, format);
            }
        }
        let galley = ui.painter().layout_job(job);
        let text_w = (galley.size().x + meta_w).min(max_w - pad.x * 2.0);
        let quote_w = quoted.is_some().then(|| (max_w - pad.x * 2.0).min(260.0));
        let mut size = Vec2::new(
            text_w.max(64.0).max(quote_w.unwrap_or(0.0)) + pad.x * 2.0,
            galley.size().y + pad.y * 2.0 + if quoted.is_some() { 48.0 } else { 0.0 },
        );
        if show_author {
            size.y += 17.0;
        }
        let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
        ui.painter().rect_filled(rect, CornerRadius::same(8), bg);
        let mut cursor = rect.min + pad;
        if show_author {
            ui.painter().text(
                cursor + Vec2::new(0.0, 6.0),
                Align2::LEFT_CENTER,
                &msg.sender_name,
                theme::semibold(12.5),
                sender_color(&msg.sender_id),
            );
            cursor.y += 17.0;
        }
        if let Some(quote) = &quoted {
            paint_quote(
                ui,
                cursor,
                quote_w.unwrap_or(size.x - pad.x * 2.0),
                quote,
                text_color,
                p.accent,
            );
            cursor.y += 48.0;
        }
        let text_top = cursor.y;
        let text_h = galley.size().y;
        // Drag selects a character range; Ctrl/Cmd+C copies it. The range is
        // kept in egui's data store so it survives frames.
        let sel_id = egui::Id::new(("bubble-text-sel", msg.id.as_str()));
        let active_key = egui::Id::new("bubble-text-sel-active");
        let (mut sel_lo, mut sel_hi) = ui
            .data(|d| d.get_temp::<(usize, usize)>(sel_id))
            .unwrap_or((0, 0));
        let mut sel_active =
            ui.data(|d| d.get_temp::<String>(active_key)).as_deref() == Some(msg.id.as_str());
        // "Select messages" mode owns the click; leave the text alone so the
        // bubble below can toggle its checkbox.
        let select_mode = ui
            .data(|d| d.get_temp::<bool>(egui::Id::new("select-mode-active")))
            .unwrap_or(false);
        let drag = ui.interact(
            Rect::from_min_size(cursor, galley.size()),
            ui.id().with(("bubble-text-drag", msg.id.as_str())),
            if select_mode {
                Sense::hover()
            } else {
                Sense::click_and_drag()
            },
        );
        if drag.hovered() && !select_mode {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Text);
        }
        if drag.drag_started() {
            if let Some(pos) = drag.interact_pointer_pos() {
                let index = galley.cursor_from_pos(pos - cursor).index.0;
                sel_lo = index;
                sel_hi = index;
                sel_active = true;
                ui.data_mut(|d| {
                    d.insert_temp(active_key, msg.id.clone());
                    d.insert_temp(sel_id, (sel_lo, sel_hi));
                });
            }
        } else if drag.dragged() {
            if let Some(pos) = drag.interact_pointer_pos() {
                sel_hi = galley.cursor_from_pos(pos - cursor).index.0;
                ui.data_mut(|d| d.insert_temp(sel_id, (sel_lo, sel_hi)));
            }
        }
        if !select_mode && sel_active && sel_lo != sel_hi {
            paint_text_selection(ui, &galley, cursor, sel_lo, sel_hi);
            if ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::C))
                && ui.memory(|m| m.focused().is_none())
            {
                let (lo, hi) = (sel_lo.min(sel_hi), sel_lo.max(sel_hi));
                let text: String = galley.chars().skip(lo).take(hi - lo).collect();
                ui.ctx().copy_text(text);
            }
        }
        // Links and mentions share one hover treatment: a hand cursor and a
        // soft green wash behind the words. Interact before the galley paints
        // so the wash stays behind the glyphs instead of dimming them.
        let span_rect = |range: &std::ops::Range<usize>| {
            let start_at = range.start - shift[range.start];
            let end_at = range.end - shift[range.end];
            let start = galley.pos_from_cursor(egui::text::CCursor::new(start_at));
            let end = galley.pos_from_cursor(egui::text::CCursor::new(end_at));
            Rect::from_min_max(start.min, end.max).translate(cursor.to_vec2())
        };
        let mut hovered: Vec<Rect> = Vec::new();
        let mut clicked: Option<String> = None;
        for (index, (range, url)) in links.iter().enumerate() {
            let area = span_rect(range);
            let hit = ui.interact(
                area,
                ui.id().with(("bubble-link", msg.id.as_str(), index)),
                Sense::click_and_drag(),
            );
            if hit.hovered() {
                hovered.push(area);
            }
            if hit.clicked() && !select_mode {
                clicked = Some(http_url(url));
            }
        }
        for (index, range) in mentions.iter().enumerate() {
            let area = span_rect(range);
            let hit = ui.interact(
                area,
                ui.id().with(("bubble-mention", msg.id.as_str(), index)),
                Sense::click_and_drag(),
            );
            if hit.hovered() {
                hovered.push(area);
            }
        }
        for area in &hovered {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            // Underline per row so a wrapped URL does not get one giant
            // rectangle across the paragraph.
            let local = area.translate(-cursor.to_vec2());
            for row in &galley.rows {
                let r = row.rect();
                let (x0, x1) = (r.left().max(local.left()), r.right().min(local.right()));
                if x1 > x0 && r.top() < local.bottom() && r.bottom() > local.top() {
                    let y = cursor.y + r.bottom() - 2.0;
                    ui.painter().line_segment(
                        [egui::pos2(cursor.x + x0, y), egui::pos2(cursor.x + x1, y)],
                        Stroke::new(1.4, link_color),
                    );
                }
            }
        }
        ui.painter().galley(cursor, galley.clone(), text_color);
        // Color emoji on top of the monochrome fallback egui just drew.
        crate::emoji::paint(ui.painter(), cursor, &galley, Some(bg));
        ui.painter().galley(
            egui::pos2(
                rect.max.x - pad.x - meta_galley.size().x,
                text_top + text_h - meta_galley.size().y,
            ),
            meta_galley.clone(),
            meta_color,
        );
        paint_ticks(
            ui,
            msg,
            tick_ink(p, msg),
            rect.max.x - pad.x - meta_galley.size().x - 15.0,
            text_top + text_h - 3.0,
        );
        if let Some(url) = clicked {
            open_url(&url);
        }
        // Selection covers the body text; the union keeps the right-click
        // menu working over the words as well as the padding.
        return drag.union(resp);
    }

    let media = media.unwrap();
    let visual = media.texture.or(media.thumb);
    let has_visual = visual.is_some();
    let picture = media.picture || has_visual;
    let overlay_meta = has_visual && !media.sticker;
    let media_w = if media.sticker {
        max_w.min(180.0)
    } else {
        max_w.min(330.0)
    };
    // Reserve the protocol's aspect ratio even before the file is decoded:
    // otherwise the bubble starts as a 62 px tile and grows to 330x380 when
    // the download lands, which reads as the whole transcript flickering.
    let (image_w, image_h) = match media
        .aspect
        .filter(|_| media.picture)
        .filter(|(w, h)| *w > 0 && *h > 0)
    {
        Some((w, h)) => {
            let cap = if media.sticker { 180.0 } else { 380.0 };
            (media_w, (media_w * h as f32 / w as f32).min(cap))
        }
        None => match visual {
            Some(texture) => {
                let size = texture.size_vec2();
                let width = if media.sticker {
                    size.x.min(media_w).max(120.0)
                } else {
                    media_w
                };
                let height = if size.x > 0.0 {
                    (width * size.y / size.x).min(380.0)
                } else {
                    180.0
                };
                (width, height)
            }
            None if media.picture => {
                // Rows synced before dimensions were stored have neither a
                // protocol size nor a thumbnail yet. Reserve a landscape box
                // instead of a 62 px strip; the final photo then grows once
                // instead of making the transcript jump.
                (media_w, (media_w * 0.75).min(300.0))
            }
            None => (media_w.min(280.0), 62.0),
        },
    };
    // Pictures sit flush inside the bubble like WhatsApp Web; document tiles
    // keep a padded row so the icon and name have room.
    let inset = if picture { 4.0 } else { 10.0 };
    let caption = media.caption.and_then(|text| {
        if text.trim().is_empty() {
            None
        } else {
            let shaped = display_text(&text);
            Some(ui.painter().layout(
                shaped.into_owned(),
                theme::regular(14.0),
                text_color,
                image_w.max(120.0),
            ))
        }
    });
    let caption_h = caption.as_ref().map(|g| g.size().y + 4.0).unwrap_or(0.0);
    let mut size = Vec2::new(
        (image_w + inset * 2.0).max(96.0),
        image_h + inset * 2.0 + caption_h + if quoted.is_some() { 48.0 } else { 0.0 },
    );
    if show_author {
        size.y += 17.0;
    }
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    // Stickers drop the bubble fill so transparent WebP looks like WhatsApp.
    if !media.sticker {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(if picture { 12 } else { 16 }), bg);
    }

    let mut cursor = rect.min + Vec2::new(inset, inset);
    if show_author {
        ui.painter().text(
            cursor + Vec2::new(0.0, 6.0),
            Align2::LEFT_CENTER,
            &msg.sender_name,
            theme::semibold(12.5),
            sender_color(&msg.sender_id),
        );
        cursor.y += 17.0;
    }
    if let Some(quote) = &quoted {
        paint_quote(ui, cursor, image_w, quote, text_color, p.accent);
        cursor.y += 48.0;
    }
    let image_rect = Rect::from_min_size(cursor, Vec2::new(image_w, image_h));
    match visual {
        Some(texture) => {
            egui::Image::new((texture.id(), Vec2::new(image_w, image_h)))
                .corner_radius(if media.sticker { 0 } else { 9 })
                .paint_at(ui, image_rect);
            if media.play {
                // Dim the frame and drop a play badge in the middle.
                ui.painter().rect_filled(
                    image_rect,
                    CornerRadius::same(9),
                    Color32::from_black_alpha(40),
                );
                ui.painter().circle_filled(
                    image_rect.center(),
                    24.0,
                    Color32::from_black_alpha(130),
                );
                ui.painter().text(
                    image_rect.center() + Vec2::new(1.0, 1.0),
                    Align2::CENTER_CENTER,
                    "\u{25B6}",
                    theme::regular(20.0),
                    Color32::from_white_alpha(235),
                );
            }
        }
        None => {
            // Video, audio and documents: a readable tile with a play/doc glyph.
            let tile = bg.gamma_multiply(if msg.outgoing { 1.18 } else { 1.1 });
            ui.painter()
                .rect_filled(image_rect, CornerRadius::same(10), tile);
            let glyph = if media.label.contains("Video") {
                "\u{25B6}"
            } else if media.label.contains("Voice") || media.label.contains("Audio") {
                "\u{266A}"
            } else {
                "\u{1F4C4}"
            };
            let icon_rect = Rect::from_min_size(
                image_rect.min + Vec2::new(10.0, image_h / 2.0 - 10.0),
                Vec2::splat(20.0),
            );
            ui.painter().text(
                icon_rect.center(),
                Align2::CENTER_CENTER,
                glyph,
                theme::regular(14.0),
                text_color,
            );
            // Drop the emoji prefix: the bundled fonts have no glyph for it.
            let cleaned: String = media
                .label
                .trim_start_matches(|c: char| !c.is_ascii())
                .trim()
                .to_string();
            let label = if cleaned.is_empty() {
                media.label.clone()
            } else {
                cleaned
            };
            let label_galley = ui.painter().layout(
                label,
                theme::medium(12.5),
                text_color,
                (image_w - 48.0).max(60.0),
            );
            ui.painter().galley(
                egui::pos2(
                    image_rect.min.x + 40.0,
                    image_rect.center().y - label_galley.size().y / 2.0,
                ),
                label_galley,
                text_color,
            );
        }
    }
    cursor.y += image_h;
    if let Some(galley) = caption {
        // Pictures have a 4 px inset; the caption text still needs breathing room.
        cursor.x = rect.min.x + if picture { 9.0 } else { inset };
        cursor.y += 4.0;
        ui.painter().galley(cursor, galley.clone(), text_color);
        crate::emoji::paint(ui.painter(), cursor, &galley, Some(bg));
    }

    // The clock and ticks live inside the picture, on a soft scrim, like the
    // web clients. Tiles keep them in the bubble corner.
    let meta_size = meta_galley.size();
    let (meta_pos, meta_draw) = if overlay_meta {
        let pos = egui::pos2(
            image_rect.max.x - meta_size.x - 12.0,
            image_rect.max.y - meta_size.y - 5.0,
        );
        ui.painter().rect_filled(
            Rect::from_min_size(pos, meta_size).expand2(Vec2::new(6.0, 2.0)),
            CornerRadius::same(8),
            Color32::from_black_alpha(90),
        );
        (pos, Color32::from_white_alpha(235))
    } else {
        (
            egui::pos2(
                rect.max.x - inset - meta_size.x,
                rect.max.y - inset - meta_size.y + 1.0,
            ),
            meta_color,
        )
    };
    let tick_base = if overlay_meta {
        egui::pos2(meta_pos.x - 15.0, meta_pos.y + meta_size.y - 3.0)
    } else {
        egui::pos2(meta_pos.x - 15.0, rect.max.y - inset - 5.0)
    };
    ui.painter().galley(meta_pos, meta_galley, meta_draw);
    // Ticks on outgoing bubbles, like the web clients: one check once the
    // server has it, two when it reached the phone, blue when it was read.
    let ink = if overlay_meta {
        Color32::from_white_alpha(220)
    } else {
        tick_ink(p, msg)
    };
    paint_ticks(ui, msg, ink, tick_base.x, tick_base.y);
    resp
}

/// Flat message row for services whose own clients use dense lists (Discord).
pub fn flat_message(
    ui: &mut Ui,
    msg: &Message,
    body: &str,
    p: &Palette,
    show_header: bool,
) -> Response {
    let indent = 60.0;
    let width = ui.available_width();
    let galley = ui.painter().layout(
        body.to_string(),
        theme::regular(14.5),
        p.text,
        (width - indent - 24.0).max(120.0),
    );
    let body_h = galley.size().y;
    let row_h = if show_header {
        body_h + 34.0
    } else {
        body_h + 8.0
    };
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, row_h), Sense::click());
    let hover_fill = p.hover.gamma_multiply(0.6);
    let row_bg = if resp.hovered() {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(6), hover_fill);
        hover_fill
    } else {
        p.chat_bg
    };
    if show_header {
        let av = Rect::from_min_size(rect.min + Vec2::new(16.0, 4.0), Vec2::splat(36.0));
        ui.painter()
            .circle_filled(av.center(), 18.0, format::avatar_color(&msg.sender_name));
        ui.painter().text(
            av.center(),
            Align2::CENTER_CENTER,
            format::initials(&msg.sender_name),
            theme::medium(13.0),
            Color32::WHITE,
        );
        ui.painter().text(
            egui::pos2(rect.min.x + indent, rect.min.y + 12.0),
            Align2::LEFT_CENTER,
            &msg.sender_name,
            theme::semibold(14.0),
            sender_color(&msg.sender_id),
        );
        let name_w = ui
            .painter()
            .layout_no_wrap(msg.sender_name.clone(), theme::semibold(14.0), p.text)
            .size()
            .x;
        ui.painter().text(
            egui::pos2(rect.min.x + indent + name_w + 10.0, rect.min.y + 13.0),
            Align2::LEFT_CENTER,
            format::clock(msg.at),
            theme::regular(10.5),
            p.text_muted,
        );
        let pos = egui::pos2(rect.min.x + indent, rect.min.y + 28.0);
        ui.painter().galley(pos, galley.clone(), p.text);
        crate::emoji::paint(ui.painter(), pos, &galley, Some(row_bg));
    } else {
        let pos = egui::pos2(rect.min.x + indent, rect.min.y + 4.0);
        ui.painter().galley(pos, galley.clone(), p.text);
        crate::emoji::paint(ui.painter(), pos, &galley, Some(row_bg));
    }
    resp
}

fn sender_color(seed: &str) -> Color32 {
    let mut hash: u32 = 0x811c9dc5;
    for b in seed.bytes() {
        hash ^= b as u32;
        hash = hash.wrapping_mul(16777619);
    }
    // Apple system palette: red, orange, green, blue, purple, pink.
    let colors: [Color32; 6] = [
        Color32::from_rgb(0xff, 0x3b, 0x30),
        Color32::from_rgb(0xff, 0x95, 0x00),
        Color32::from_rgb(0x34, 0xc7, 0x59),
        Color32::from_rgb(0x00, 0x7a, 0xff),
        Color32::from_rgb(0xaf, 0x52, 0xde),
        Color32::from_rgb(0xff, 0x2d, 0x55),
    ];
    colors[(hash as usize) % colors.len()]
}

/// Launch the system browser. eframe's webbrowser crate is not built into every
/// distro package, so the usual desktop openers are tried directly.
pub fn open_url(url: &str) {
    use std::process::{Command, Stdio};
    let mut candidates: Vec<(String, Vec<String>)> = Vec::new();
    if let Ok(browser) = std::env::var("BROWSER") {
        if let Some((program, rest)) = browser.split_once(char::is_whitespace) {
            let mut args: Vec<String> = rest.split_whitespace().map(str::to_string).collect();
            args.push(url.to_string());
            candidates.push((program.to_string(), args));
        } else if !browser.trim().is_empty() {
            candidates.push((browser, vec![url.to_string()]));
        }
    }
    candidates.push(("xdg-open".into(), vec![url.to_string()]));
    candidates.push(("gio".into(), vec!["open".into(), url.to_string()]));
    candidates.push(("sensible-browser".into(), vec![url.to_string()]));
    candidates.push(("x-www-browser".into(), vec![url.to_string()]));
    for (program, args) in candidates {
        let spawned = Command::new(&program)
            .args(&args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
        if spawned.is_ok() {
            return;
        }
    }
}

/// Text link that hands the URL to [`open_url`] instead of egui's viewport
/// command, which is a no-op when no browser helper is bundled.
pub fn link(ui: &mut Ui, label: &str, url: &str, p: &Palette) -> Response {
    let resp = ui.add(
        egui::Label::new(
            RichText::new(label)
                .font(theme::medium(12.0))
                .color(p.accent),
        )
        .sense(Sense::click()),
    );
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    if resp.clicked() {
        open_url(url);
    }
    resp
}

/// What the composer wants the shell to do after this frame.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ComposerAction {
    Send,
    /// Photo or video chosen from the attach menu.
    AttachMedia,
    /// Any other file.
    AttachFile,
    /// Toggle the emoji picker above the composer.
    Emoji,
}

/// Composer bar: attach menu, growing text field and send button inside one
/// rounded pill, matching the WhatsApp/Telegram/Discord web composers. The send
/// button lives inside the pill so it can never be clipped at the right edge.
pub struct ComposerOutput {
    pub action: Option<ComposerAction>,
    /// Keyboard focus is on the text field, so paste shortcuts belong to it.
    pub focused: bool,
    /// Outer pill rect, used to anchor popups above the field.
    pub rect: Rect,
}

pub fn composer(ui: &mut Ui, draft: &mut String, p: &Palette, placeholder: &str) -> ComposerOutput {
    let mut action = None;
    let mut focused = false;
    let mut rect = Rect::NOTHING;
    ui.horizontal(|ui| {
        let width = ui.available_width().max(160.0);
        // WhatsApp paints a white field on its grey footer; Telegram and
        // Discord already give the field its own tint.
        let fill = if p.input_bg == p.header {
            p.panel
        } else {
            p.input_bg
        };
        let pill = egui::Frame::new()
            .fill(fill)
            .stroke(Stroke::new(HAIRLINE, p.divider))
            .corner_radius(20)
            .inner_margin(egui::Margin::symmetric(8, 5))
            .show(ui, |ui| {
                ui.set_width((width - 16.0).max(120.0));
                ui.horizontal(|ui| {
                    let (slot, resp) = ui.allocate_exact_size(Vec2::splat(28.0), Sense::click());
                    if resp.hovered() {
                        ui.painter().circle_filled(slot.center(), 14.0, p.hover);
                    }
                    let ink = if resp.hovered() { p.text } else { p.text_muted };
                    icons::paint(
                        ui.painter(),
                        Rect::from_center_size(slot.center(), Vec2::splat(17.0)),
                        Icon::Plus,
                        ink,
                        1.7,
                    );
                    resp.clone().on_hover_text("Attach");
                    egui::Popup::menu(&resp).show(|ui| {
                        ui.set_min_width(190.0);
                        if ui.button("Photo or video…").clicked() {
                            action = Some(ComposerAction::AttachMedia);
                            ui.close();
                        }
                        if ui.button("Document…").clicked() {
                            action = Some(ComposerAction::AttachFile);
                            ui.close();
                        }
                    });
                    ui.add_space(2.0);
                    let (emoji_slot, emoji_resp) =
                        ui.allocate_exact_size(Vec2::splat(28.0), Sense::click());
                    if emoji_resp.hovered() {
                        ui.painter()
                            .circle_filled(emoji_slot.center(), 14.0, p.hover);
                    }
                    let emoji_ink = if emoji_resp.hovered() {
                        p.text
                    } else {
                        p.text_muted
                    };
                    icons::paint(
                        ui.painter(),
                        Rect::from_center_size(emoji_slot.center(), Vec2::splat(17.0)),
                        Icon::Smile,
                        emoji_ink,
                        1.6,
                    );
                    emoji_resp.clone().on_hover_text("Emoji");
                    if emoji_resp.clicked() {
                        action = Some(ComposerAction::Emoji);
                    }
                    ui.add_space(2.0);
                    let send_w = 32.0;
                    let edit = egui::TextEdit::multiline(draft)
                        .hint_text(RichText::new(placeholder).color(p.text_muted))
                        .desired_rows(1)
                        .desired_width((ui.available_width() - send_w - 6.0).max(60.0))
                        .frame(egui::Frame::NONE)
                        .font(theme::regular(14.0));
                    let resp = ui.add(edit);
                    focused = resp.has_focus();
                    if focused
                        && ui.input(|i| i.key_pressed(egui::Key::Enter) && !i.modifiers.shift)
                    {
                        action = Some(ComposerAction::Send);
                    }
                    let enabled = !draft.trim().is_empty();
                    let (rect, send) = ui.allocate_exact_size(Vec2::splat(send_w), Sense::click());
                    let fill = if enabled { p.accent } else { p.divider };
                    let fill = if send.hovered() && enabled {
                        p.accent.gamma_multiply(0.85)
                    } else {
                        fill
                    };
                    ui.painter()
                        .circle_filled(rect.center(), send_w / 2.0, fill);
                    icons::paint(
                        ui.painter(),
                        Rect::from_center_size(rect.center(), Vec2::splat(15.0)),
                        Icon::ArrowUp,
                        if enabled {
                            Color32::WHITE
                        } else {
                            p.text_muted
                        },
                        1.9,
                    );
                    send.clone().on_hover_text("Send");
                    if send.clicked() && enabled {
                        action = Some(ComposerAction::Send);
                    }
                });
            });
        rect = pill.response.rect;
        if focused {
            // Soft focus ring, like the web clients' green composer outline.
            ui.painter().rect_stroke(
                rect,
                20,
                Stroke::new(1.0, p.accent.gamma_multiply(0.55)),
                egui::StrokeKind::Inside,
            );
        }
    });

    if action == Some(ComposerAction::Send) {
        while draft.ends_with('\n') {
            draft.pop();
        }
    }
    ComposerOutput {
        action,
        focused,
        rect,
    }
}

/// iOS-style switch (40×24) drawn without extra textures.
pub fn switch(ui: &mut Ui, on: &mut bool, p: &Palette) -> Response {
    let size = Vec2::new(40.0, 24.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    if resp.clicked() {
        *on = !*on;
    }
    let track = if *on { p.accent } else { p.divider };
    ui.painter()
        .rect_filled(rect, CornerRadius::same(12), track);
    let knob_x = if *on {
        rect.max.x - 12.0
    } else {
        rect.min.x + 12.0
    };
    ui.painter()
        .circle_filled(egui::pos2(knob_x, rect.center().y), 10.0, Color32::WHITE);
    resp
}

/// Section caption above an inset grouped card.
pub fn section(ui: &mut Ui, title: &str, p: &Palette) {
    ui.add_space(18.0);
    ui.horizontal(|ui| {
        ui.add_space(16.0);
        ui.label(
            RichText::new(title.to_uppercase())
                .font(theme::medium(11.0))
                .color(p.text_muted),
        );
    });
    ui.add_space(6.0);
}

/// White rounded card that groups settings rows.
pub fn card<R>(ui: &mut Ui, p: &Palette, add: impl FnOnce(&mut Ui) -> R) -> R {
    Frame::new()
        .fill(p.panel)
        .corner_radius(12)
        .inner_margin(Margin::symmetric(0, 2))
        .show(ui, |ui| add(ui))
        .inner
}

/// One row inside a card: label, optional trailing widget, hairline separator.
pub fn row(ui: &mut Ui, p: &Palette, label: &str, detail: Option<&str>, add: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        ui.add_space(16.0);
        ui.vertical(|ui| {
            ui.add_space(9.0);
            ui.label(
                RichText::new(label)
                    .font(theme::regular(13.5))
                    .color(p.text),
            );
            if let Some(detail) = detail {
                ui.label(
                    RichText::new(detail)
                        .font(theme::regular(11.5))
                        .color(p.text_muted),
                );
            }
            ui.add_space(9.0);
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(16.0);
            add(ui);
        });
    });
    ui.horizontal(|ui| {
        ui.add_space(16.0);
        let width = ui.available_width();
        ui.painter().hline(
            egui::Rangef::new(ui.cursor().min.x, ui.cursor().min.x + width),
            ui.cursor().min.y,
            Stroke::new(HAIRLINE, p.divider),
        );
        ui.add_space(1.0);
    });
}

/// Segmented control, macOS style.
pub fn segmented(ui: &mut Ui, p: &Palette, current: usize, labels: &[&str]) -> Option<usize> {
    let width = 240.0;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 26.0), Sense::hover());
    ui.painter()
        .rect_filled(rect, CornerRadius::same(8), p.input_bg);
    let seg_w = (width - 4.0) / labels.len() as f32;
    let mut picked = None;
    for (index, label) in labels.iter().enumerate() {
        let seg = Rect::from_min_size(
            egui::pos2(rect.min.x + 2.0 + seg_w * index as f32, rect.min.y + 2.0),
            Vec2::new(seg_w, rect.height() - 4.0),
        );
        let resp = ui.interact(seg, ui.id().with(("seg", index)), Sense::click());
        if current == index {
            ui.painter()
                .rect_filled(seg, CornerRadius::same(6), p.panel);
            ui.painter().rect_stroke(
                seg,
                CornerRadius::same(6),
                Stroke::new(HAIRLINE, p.divider),
                egui::StrokeKind::Inside,
            );
        }
        ui.painter().text(
            seg.center(),
            Align2::CENTER_CENTER,
            label,
            if current == index {
                theme::medium(12.5)
            } else {
                theme::regular(12.5)
            },
            p.text,
        );
        if resp.clicked() {
            picked = Some(index);
        }
    }
    picked
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_link_inside_a_paragraph_is_its_own_run() {
        let body = "izin kirimkan link\n\nBerikut akses ke : *Workshop*\n\nhttps://us02web.zoom.us/j/8625761382?pwd=QS91QlVabURjU2V0ZjZ6QU1CbHFoUT09&omn=83758270654";
        let spans = link_spans(body);
        assert_eq!(spans.len(), 1);
        assert!(spans[0].1.starts_with("https://us02web.zoom.us/"));
    }

    #[test]
    fn links_are_found_and_trimmed() {
        let body = "cek https://discord.com/developers/applications, atau www.telegram.org ya";
        let spans = link_spans(body);
        assert_eq!(spans.len(), 2);
        assert_eq!(
            &body[spans[0].0.clone()],
            "https://discord.com/developers/applications"
        );
        assert_eq!(&body[spans[1].0.clone()], "www.telegram.org");
        assert!(link_spans("tidak ada tautan di sini").is_empty());
    }

    #[test]
    fn arabic_is_shaped_and_reordered() {
        let out = display_text("بِسْمِ اللهِ الرَّحْمَنِ الرَّحِيْمِ");
        // No tofu ligature: the Allah word is expanded to real glyphs.
        assert!(!out.contains('\u{FDF2}'));
        assert!(out.contains('\u{FE8D}'));
        // Visual order: the last word comes first in the laid-out string.
        assert!(out.starts_with('\u{FEE2}'));
        assert!(out.ends_with('\u{FE91}'));
    }

    #[test]
    fn bare_www_gets_scheme() {
        assert_eq!(http_url("www.telegram.org"), "https://www.telegram.org");
        assert_eq!(http_url("http://a.b"), "http://a.b");
    }

    #[test]
    fn markup_marks_runs_and_drops_markers() {
        let (runs, shift) = markup_runs("*tebal* dan _miring_ ~coret~ `kode`");
        // Markers are skipped, so the rendered text is shorter by 8 bytes.
        assert_eq!(shift["*tebal*".len()], 2);
        let bold: String = runs
            .iter()
            .filter(|(_, f)| f & 1 != 0)
            .map(|(r, _)| &"*tebal* dan _miring_ ~coret~ `kode`"[r.clone()])
            .collect();
        assert_eq!(bold, "tebal");
        let italic: String = runs
            .iter()
            .filter(|(_, f)| f & 2 != 0)
            .map(|(r, _)| &"*tebal* dan _miring_ ~coret~ `kode`"[r.clone()])
            .collect();
        assert_eq!(italic, "miring");
        // Unpaired markers stay literal text.
        let (runs, shift) = markup_runs("*tidak ditutup");
        assert!(runs.iter().all(|(_, f)| *f == 0));
        assert_eq!(shift[0], 0);
    }
}
