use egui::{Color32, Context, TextureHandle};
use qrcode::EcLevel;
use qrcode::QrCode;
use qrcode::types::Color as QrColor;

/// Build a QR texture once, reused for the lifetime of the prompt.
pub fn qr_texture(ctx: &Context, payload: &str, name: &str) -> Option<TextureHandle> {
    // Low EC keeps the module count (and therefore the module size) large
    // enough to scan from a phone at arm's length, which is what WhatsApp's own
    // pairing code uses too.
    let code = QrCode::with_error_correction_level(payload.as_bytes(), EcLevel::L).ok()?;
    let colors = code.to_colors();
    let width = code.width();
    // Pick an integer module size that lands near 232 px, with the 4-module
    // quiet zone the spec wants. Native-size drawing keeps the edges crisp;
    // downscaling a big texture turns the modules grey and confuses scanners.
    let quiet = 4usize;
    let modules = width + quiet * 2;
    let scale = (264 / modules).clamp(2, 12);
    let size = modules * scale;

    let mut pixels = vec![Color32::WHITE; size * size];
    for y in 0..size {
        for x in 0..size {
            let mx = x / scale;
            let my = y / scale;
            if mx < quiet || my < quiet || mx >= width + quiet || my >= width + quiet {
                continue;
            }
            let module = colors[(my - quiet) * width + (mx - quiet)];
            if module == QrColor::Dark {
                pixels[y * size + x] = Color32::BLACK;
            }
        }
    }

    let image = egui::ColorImage::new([size, size], pixels);
    Some(ctx.load_texture(name, image, egui::TextureOptions::NEAREST))
}
