//! Color emoji.
//!
//! egui rasterizes font outlines only, so a color emoji face (Noto Color
//! Emoji ships a CBDT bitmap strike, no outlines) comes out as the monochrome
//! fallback. The layout already placed that fallback glyph, so the color
//! bitmap is painted on top of it, one bitmap per grapheme cluster.
//!
//! Clusters matter: egui logs one glyph per char and reports the cluster's
//! first char for every glyph in it, so painting per glyph drew a ZWJ family
//! five times and a variation-selector heart twice. Shaping the cluster with
//! the color face also picks the flag/keycap/family ligature instead of its
//! first letter.

use std::collections::HashMap;
use std::ops::Range;
use std::sync::{Mutex, OnceLock};

use egui::{Color32, Painter, Pos2, Rect, TextureHandle};
use unicode_segmentation::UnicodeSegmentation;

/// Common install paths for the bitmap emoji face, in package order.
const FACES: &[&str] = &[
    "/usr/share/fonts/truetype/noto/NotoColorEmoji.ttf",
    "/usr/share/fonts/noto/NotoColorEmoji.ttf",
    "/usr/share/fonts/google-noto-emoji/NotoColorEmoji.ttf",
    "/usr/share/fonts/noto-emoji/NotoColorEmoji.ttf",
    "/usr/share/fonts/TTF/NotoColorEmoji.ttf",
];

/// Whether `ch` is a pictograph the color face would draw. Deliberately
/// narrow: the ranges are the emoji planes plus the older symbol blocks.
fn is_emoji(ch: char) -> bool {
    matches!(ch as u32,
        0x2190..=0x21FF      // arrows
        | 0x2300..=0x23FF    // misc technical
        | 0x2460..=0x24FF    // enclosed alphanumerics
        | 0x25A0..=0x27BF    // geometric shapes + dingbats
        | 0x2900..=0x297F    // arrows B
        | 0x2B00..=0x2BFF    // misc symbols and arrows
        | 0x3030 | 0x303D | 0x3297 | 0x3299
        | 0x1F000..=0x1FAFF) // emoji planes, flags included
}

/// Skin-tone swatches are modifiers, not emoji of their own.
fn is_skin_tone(ch: char) -> bool {
    (0x1F3FB..=0x1F3FF).contains(&(ch as u32))
}

/// A cluster is worth a bitmap when it starts with a pictograph or is a
/// keycap sequence (`1` + VS16 + enclosing keycap).
fn is_color_cluster(cluster: &str) -> bool {
    match cluster.chars().next() {
        Some(ch) => !is_skin_tone(ch) && (is_emoji(ch) || cluster.contains('\u{20e3}')),
        None => false,
    }
}

fn face() -> Option<&'static [u8]> {
    static BYTES: OnceLock<Option<Vec<u8>>> = OnceLock::new();
    BYTES
        .get_or_init(|| FACES.iter().find_map(|path| std::fs::read(path).ok()))
        .as_deref()
}

fn texture(ctx: &egui::Context, cluster: &str) -> Option<TextureHandle> {
    static CACHE: OnceLock<Mutex<HashMap<String, Option<TextureHandle>>>> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    let mut cache = cache.lock().unwrap();
    if let Some(hit) = cache.get(cluster) {
        return hit.clone();
    }
    let made = decode(ctx, cluster);
    cache.insert(cluster.to_string(), made.clone());
    made
}

fn decode(ctx: &egui::Context, cluster: &str) -> Option<TextureHandle> {
    let font = skrifa::FontRef::new(face()?).ok()?;
    let id = glyph_id(&font, cluster)?;
    use skrifa::MetadataProvider as _;
    let strike = font
        .bitmap_strikes()
        .glyph_for_size(skrifa::instance::Size::unscaled(), id)?;
    let skrifa::bitmap::BitmapData::Png(png) = strike.data else {
        return None;
    };
    let picture = image::load_from_memory(png).ok()?.into_rgba8();
    let [w, h] = [picture.width() as usize, picture.height() as usize];
    let pixels = egui::ColorImage::from_rgba_unmultiplied([w, h], picture.as_raw());
    Some(ctx.load_texture(
        format!("emoji-{}", id.to_u32()),
        pixels,
        egui::TextureOptions::LINEAR,
    ))
}

/// Glyph the color face would use for a whole cluster. A single scalar goes
/// through the cmap; a sequence is shaped so the face's own GSUB ligature
/// (flags, keycaps, ZWJ families, skin tones) is selected.
fn glyph_id(font: &skrifa::FontRef<'_>, cluster: &str) -> Option<skrifa::GlyphId> {
    use skrifa::MetadataProvider as _;
    let mut chars = cluster.chars();
    let first = chars.next()?;
    if chars.next().is_none() {
        return font.charmap().map(first);
    }
    let data = harfrust::ShaperData::new(font);
    let shaper = data.shaper(font).build();
    let mut buffer = harfrust::UnicodeBuffer::new();
    buffer.push_str(cluster);
    buffer.guess_segment_properties();
    let glyphs = shaper.shape(buffer, harfrust::ShapeOptions::new());
    let id = glyphs.glyph_infos().first()?.glyph_id;
    (id != 0).then(|| skrifa::GlyphId::new(id))
}

/// Paint the color bitmaps for every emoji cluster in `galley`, which was
/// painted at `origin` by `painter.galley(origin, ..)`. `backdrop` is the
/// surface behind the text: the monochrome fallback is drawn under the
/// bitmap, so its outline would halo the color face. Painting the fill under
/// the cluster hides it without touching the text next to it.
pub fn paint(painter: &Painter, origin: Pos2, galley: &egui::Galley, backdrop: Option<Color32>) {
    // epaint keeps one glyph per char (newlines excluded). When that holds
    // the original text can be zipped to the glyphs, which is the only place
    // the ZWJ/modifier scalars are still available.
    let chars: Vec<char> = galley.text().chars().filter(|&ch| ch != '\n').collect();
    let glyphs: usize = galley.rows.iter().map(|row| row.glyphs.len()).sum();
    if chars.len() == glyphs {
        paint_clusters(painter, origin, galley, backdrop, &chars);
    } else {
        // Defensive path for a layout epaint changed under us: paint the
        // base glyph of each run once, at its advance box.
        paint_fallback(painter, origin, galley, backdrop);
    }
}

fn paint_clusters(
    painter: &Painter,
    origin: Pos2,
    galley: &egui::Galley,
    backdrop: Option<Color32>,
    chars: &[char],
) {
    let mut start = 0usize;
    for row in &galley.rows {
        let end = start + row.glyphs.len();
        let Some(row_chars) = chars.get(start..end) else {
            return;
        };
        start = end;
        let text: String = row_chars.iter().collect();
        let mut index = 0usize;
        for cluster in text.graphemes(true) {
            let len = cluster.chars().count();
            if is_color_cluster(cluster) {
                paint_cluster(painter, origin, row, index..index + len, cluster, backdrop);
            }
            index += len;
        }
    }
}

/// Advance box of a cluster and the vertical center its bitmap should use.
///
/// The box is the sum of the cluster's glyph advances, so two neighbouring
/// clusters can never claim the same pixels; the center follows the monochrome
/// fallback's ink so the bitmap sits on the same baseline.
fn cluster_frame(
    origin: Pos2,
    row: &egui::epaint::text::PlacedRow,
    glyphs: &[egui::epaint::text::Glyph],
) -> Option<(Rect, Pos2)> {
    let first = glyphs.first()?;
    let width: f32 = glyphs.iter().map(|glyph| glyph.advance_width).sum();
    if width <= 0.0 {
        return None;
    }
    // Union of the fallback ink boxes: it puts the bitmap where the mono
    // glyph sat without trusting the cluster's first-char position for the
    // rest of a ZWJ sequence.
    let mut ink: Option<Rect> = None;
    for glyph in glyphs {
        if glyph.uv_rect.size.x > 0.5 && glyph.uv_rect.size.y > 0.5 {
            let rect = Rect::from_min_size(glyph.pos + glyph.uv_rect.offset, glyph.uv_rect.size);
            ink = Some(ink.map_or(rect, |union: Rect| union.union(rect)));
        }
    }
    let height = first.line_height.max(1.0);
    let cell = Rect::from_min_size(
        origin + row.pos.to_vec2() + egui::vec2(first.pos.x, 0.0),
        egui::vec2(width, height),
    );
    let center = ink.map_or_else(
        || cell.center(),
        |ink| Pos2::new(cell.center().x, origin.y + row.pos.y + ink.center().y),
    );
    Some((cell, center))
}

fn paint_cluster(
    painter: &Painter,
    origin: Pos2,
    row: &egui::epaint::text::PlacedRow,
    range: Range<usize>,
    cluster: &str,
    backdrop: Option<Color32>,
) {
    let Some((cell, center)) = cluster_frame(origin, row, &row.glyphs[range]) else {
        return;
    };
    let Some(texture) = texture(painter.ctx(), cluster) else {
        return;
    };
    // Stay inside the cluster's advance box so adjacent emoji never overlap.
    let size = texture.size_vec2();
    let scale = (cell.width() / size.x).min(cell.height() / size.y);
    let rect = Rect::from_center_size(center, size * scale);
    if let Some(fill) = backdrop {
        painter.rect_filled(cell, 0, fill);
    }
    painter.image(
        texture.id(),
        rect,
        Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
        Color32::WHITE,
    );
}

fn paint_fallback(
    painter: &Painter,
    origin: Pos2,
    galley: &egui::Galley,
    backdrop: Option<Color32>,
) {
    for row in &galley.rows {
        let mut previous = None;
        for glyph in &row.glyphs {
            let ch = glyph.chr;
            let duplicate = previous == Some(ch);
            previous = Some(ch);
            if duplicate || glyph.advance_width <= 0.0 || !is_emoji(ch) || is_skin_tone(ch) {
                continue;
            }
            let Some(texture) = texture(painter.ctx(), &ch.to_string()) else {
                continue;
            };
            let cell = Rect::from_min_size(
                origin + row.pos.to_vec2() + glyph.pos.to_vec2(),
                egui::vec2(glyph.advance_width, glyph.line_height),
            );
            let size = texture.size_vec2();
            let scale = (cell.width() / size.x).min(cell.height() / size.y);
            let rect = Rect::from_center_size(cell.center(), size * scale);
            if let Some(fill) = backdrop {
                painter.rect_filled(cell, 0, fill);
            }
            painter.image(
                texture.id(),
                rect,
                Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                Color32::WHITE,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modifiers_stay_in_one_cluster() {
        let clusters: Vec<&str> = "❤️👍🏽👨\u{200d}👩\u{200d}👧🇮🇩1️⃣😀😀"
            .graphemes(true)
            .collect();
        assert_eq!(
            clusters,
            ["❤️", "👍🏽", "👨\u{200d}👩\u{200d}👧", "🇮🇩", "1️⃣", "😀", "😀"]
        );
    }

    /// The colour bitmap is drawn once per grapheme, inside that grapheme's
    /// advance box. Multi-line text must keep each line's boxes on its own
    /// row, and neighbouring boxes on one row must not overlap, or the
    /// emoji come out doubled or clipped.
    #[test]
    fn multiline_emoji_boxes_stay_in_line() {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        // Fonts only exist after one pass; the test does not paint that pass.
        let mut output = ctx.run_ui(Default::default(), |_| {});
        output.textures_delta.clear();
        let body = "Line one 😀😀 ❤️ 👨\u{200d}👩\u{200d}👧 🇮🇩\nSecond line 👍🏽🎉 1️⃣ ✨";
        let job = egui::text::LayoutJob::simple(
            body.to_owned(),
            crate::theme::regular(14.5),
            Color32::WHITE,
            320.0,
        );
        let galley = ctx.fonts_mut(|fonts| fonts.layout_job(job));
        let chars: Vec<char> = galley.text().chars().filter(|&ch| ch != '\n').collect();
        let glyphs: usize = galley.rows.iter().map(|row| row.glyphs.len()).sum();
        assert_eq!(
            chars.len(),
            glyphs,
            "one glyph per char keeps clusters aligned"
        );
        assert_eq!(galley.rows.len(), 2, "the newline starts a second row");

        let mut start = 0usize;
        let mut lines: Vec<Vec<Rect>> = Vec::new();
        for row in &galley.rows {
            let row_chars = &chars[start..start + row.glyphs.len()];
            start += row.glyphs.len();
            let text: String = row_chars.iter().collect();
            let mut index = 0usize;
            let mut cells = Vec::new();
            for cluster in text.graphemes(true) {
                let len = cluster.chars().count();
                if is_color_cluster(cluster) {
                    let (cell, center) =
                        cluster_frame(Pos2::ZERO, row, &row.glyphs[index..index + len])
                            .expect("emoji cluster has an advance");
                    assert!(
                        cell.contains(center),
                        "{cluster:?} bitmap center leaves its box"
                    );
                    cells.push(cell);
                }
                index += len;
            }
            assert!(!cells.is_empty(), "row has emoji");
            for pair in cells.windows(2) {
                assert!(
                    pair[0].right() <= pair[1].left() + 0.01,
                    "emoji boxes overlap: {:?} then {:?}",
                    pair[0],
                    pair[1]
                );
            }
            lines.push(cells);
        }
        let top_bottom = lines[0]
            .iter()
            .map(|cell| cell.bottom())
            .fold(0.0, f32::max);
        let second_top = lines[1]
            .iter()
            .map(|cell| cell.top())
            .fold(f32::MAX, f32::min);
        assert!(
            second_top >= top_bottom,
            "second line collides with the first"
        );
    }
}
