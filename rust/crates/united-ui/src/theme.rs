//! Apple-flavoured visual system: SF-like type scale, system colours, hairlines,
//! rounded surfaces. Service identity comes from the official brand logos, not
//! from per-service chrome, so every inbox looks like the same Messages window.

use egui::{Color32, FontFamily, FontId};
use united_core::Service;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Light,
    Dark,
}

#[derive(Clone, Copy)]
pub struct Palette {
    /// Sidebar strip holding service logos.
    pub rail: Color32,
    /// Chat list surface.
    pub panel: Color32,
    /// Bars (headers, composer).
    pub header: Color32,
    /// Conversation canvas.
    pub chat_bg: Color32,
    pub bubble_in: Color32,
    pub bubble_out: Color32,
    pub text: Color32,
    pub text_muted: Color32,
    pub accent: Color32,
    pub hover: Color32,
    pub selected: Color32,
    pub divider: Color32,
    pub input_bg: Color32,
    /// Text drawn on an outgoing bubble; light bubbles need dark text.
    pub on_bubble_out: Color32,
}

const fn rgb(hex: u32) -> Color32 {
    Color32::from_rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

/// Hairline separators: one pixel at 0.5 physical scale looks crisp on every
/// monitor without a second colour per theme.
pub const HAIRLINE: f32 = 1.0;

/// Brand colour, used for logos, avatars and service dots.
pub fn brand(service: Service) -> Color32 {
    match service {
        Service::WhatsApp => rgb(0x25d366),
        Service::Telegram => rgb(0x2aabee),
        Service::Discord => rgb(0x5865f2),
    }
}

pub fn palette(mode: Mode) -> Palette {
    match mode {
        Mode::Light => Palette {
            rail: rgb(0xf2f2f7),
            panel: rgb(0xffffff),
            header: rgb(0xffffff),
            chat_bg: rgb(0xffffff),
            bubble_in: rgb(0xe9e9eb),
            bubble_out: rgb(0x007aff),
            text: rgb(0x000000),
            text_muted: rgb(0x8a8a8e),
            accent: rgb(0x007aff),
            hover: rgb(0xf5f5f7),
            selected: rgb(0xe5e5ea),
            divider: rgb(0xd8d8dc),
            input_bg: rgb(0xf2f2f7),
            on_bubble_out: Color32::WHITE,
        },
        Mode::Dark => Palette {
            rail: rgb(0x1c1c1e),
            panel: rgb(0x1c1c1e),
            header: rgb(0x1c1c1e),
            chat_bg: rgb(0x000000),
            bubble_in: rgb(0x3a3a3c),
            bubble_out: rgb(0x0a84ff),
            text: rgb(0xffffff),
            text_muted: rgb(0x98989f),
            accent: rgb(0x0a84ff),
            hover: rgb(0x2c2c2e),
            selected: rgb(0x3a3a3c),
            divider: rgb(0x38383a),
            input_bg: rgb(0x2c2c2e),
            on_bubble_out: Color32::WHITE,
        },
    }
}

/// Outgoing bubbles are always the system blue fill, so text is always white.
pub fn on_outgoing() -> Color32 {
    Color32::WHITE
}

/// The conversation pane wears each service's own web palette while the inbox
/// stays in one unified shell. Values follow the official web clients.
pub fn service_palette(mode: Mode, service: Service) -> Palette {
    let mut p = palette(mode);
    match (mode, service) {
        (Mode::Dark, Service::WhatsApp) => {
            p.chat_bg = rgb(0x0b141a);
            p.panel = rgb(0x111b21);
            p.header = rgb(0x202c33);
            p.hover = rgb(0x202c33);
            p.selected = rgb(0x2a3942);
            p.bubble_in = rgb(0x202c33);
            p.bubble_out = rgb(0x005c4b);
            p.input_bg = rgb(0x2a3942);
            p.accent = rgb(0x00a884);
            p.divider = rgb(0x2f3b43);
        }
        (Mode::Light, Service::WhatsApp) => {
            // WhatsApp Web's light chat is beige, not white.
            p.chat_bg = rgb(0xefeae2);
            p.panel = rgb(0xffffff);
            p.header = rgb(0xf0f2f5);
            p.hover = rgb(0xf5f6f6);
            p.selected = rgb(0xf0f2f5);
            p.bubble_in = rgb(0xffffff);
            p.bubble_out = rgb(0xd9fdd3);
            p.input_bg = rgb(0xf0f2f5);
            p.accent = rgb(0x00a884);
            p.divider = rgb(0xd1d7db);
            p.on_bubble_out = rgb(0x111b21);
        }
        // Telegram Web A: one #212121 card on a black doodle wallpaper, not
        // the older desktop client's blue-grey columns.
        (Mode::Dark, Service::Telegram) => {
            p.chat_bg = rgb(0x0a0a0a);
            p.panel = rgb(0x212121);
            p.header = rgb(0x212121);
            p.hover = rgb(0x2b2b2b);
            p.selected = rgb(0x2b2b2b);
            p.bubble_in = rgb(0x212121);
            p.bubble_out = rgb(0x2b2b2b);
            p.input_bg = rgb(0x2c2c2c);
            p.accent = rgb(0x3390ec);
            p.divider = rgb(0x2f2f2f);
            p.text = rgb(0xffffff);
            p.text_muted = rgb(0xaaaaaa);
        }
        (Mode::Light, Service::Telegram) => {
            p.chat_bg = rgb(0xdfe4e8);
            p.panel = rgb(0xffffff);
            p.header = rgb(0xffffff);
            p.hover = rgb(0xf1f1f1);
            p.selected = rgb(0xe7f0f7);
            p.bubble_in = rgb(0xffffff);
            p.bubble_out = rgb(0xeeffde);
            p.input_bg = rgb(0xf4f4f5);
            p.accent = rgb(0x3390ec);
            p.divider = rgb(0xe4e4e5);
            p.on_bubble_out = rgb(0x111b21);
        }
        // Discord's darker theme: a near-black rail and sidebar (#121214), the
        // message column a shade lighter (#1a1a1e), and the user bar raised.
        (Mode::Dark, Service::Discord) => {
            p.chat_bg = rgb(0x1a1a1e);
            p.panel = rgb(0x121214);
            p.header = rgb(0x1a1a1e);
            p.hover = rgb(0x202024);
            p.selected = rgb(0x2c2c30);
            p.input_bg = rgb(0x222225);
            p.bubble_in = rgb(0x2b2d31);
            p.bubble_out = rgb(0x2b2d31);
            p.accent = rgb(0x5865f2);
            p.divider = rgb(0x26262b);
            p.text = rgb(0xf2f3f5);
            p.text_muted = rgb(0x94949c);
        }
        (Mode::Light, Service::Discord) => {
            p.chat_bg = rgb(0xffffff);
            p.panel = rgb(0xf2f3f5);
            p.header = rgb(0xffffff);
            p.hover = rgb(0xe3e5e8);
            p.selected = rgb(0xebedef);
            p.input_bg = rgb(0xebedef);
            p.bubble_in = rgb(0xf2f3f5);
            p.bubble_out = rgb(0xf2f3f5);
            p.accent = rgb(0x5865f2);
            p.divider = rgb(0xe3e5e8);
        }
    }
    p
}

// ------------------------------------------------------------------ typography

/// Inter stands in for SF Pro: same humanist skeleton, OFL licensed, embedded so
/// the binary does not depend on the distro's font package.
pub fn install(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    for (key, bytes) in [
        (
            "inter",
            include_bytes!("../assets/Inter-Regular.ttf").as_slice(),
        ),
        (
            "inter-medium",
            include_bytes!("../assets/Inter-Medium.ttf").as_slice(),
        ),
        (
            "inter-semibold",
            include_bytes!("../assets/Inter-SemiBold.ttf").as_slice(),
        ),
    ] {
        fonts
            .font_data
            .insert(key.to_owned(), egui::FontData::from_static(bytes).into());
    }
    // Arabic: Inter has no Arabic block, so verses and greetings used to
    // render as tofu. DejaVu Sans ships with the big distros and covers the
    // Arabic letters plus their harakat. egui has no shaping yet, so the
    // letters stay isolated, but they are at least readable.
    for candidate in [
        "/usr/share/fonts/truetype/noto/NotoNaskhArabic-Regular.ttf",
        "/usr/share/fonts/truetype/noto/NotoSansArabic-Regular.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
        "/usr/share/fonts/TTF/DejaVuSans.ttf",
    ] {
        if let Ok(bytes) = std::fs::read(candidate) {
            fonts.font_data.insert(
                "arabic".to_owned(),
                egui::FontData::from_owned(bytes).into(),
            );
            break;
        }
    }
    let has_arabic = fonts.font_data.contains_key("arabic");
    // Emoji: egui cannot draw colour (CBDT) fonts, so use a monochrome emoji
    // face when the distro ships one. Symbola covers WhatsApp/Telegram emoji.
    for candidate in [
        "/usr/share/fonts/truetype/ancient-scripts/Symbola_hint.ttf",
        "/usr/share/fonts/truetype/symbola/Symbola.ttf",
        "/usr/share/fonts/truetype/noto/NotoEmoji-Regular.ttf",
        "/usr/share/fonts/truetype/noto/NotoEmoji-Regular.ttf.ttf",
    ] {
        if let Ok(bytes) = std::fs::read(candidate) {
            fonts
                .font_data
                .insert("emoji".to_owned(), egui::FontData::from_owned(bytes).into());
            break;
        }
    }
    let has_emoji = fonts.font_data.contains_key("emoji");
    let proportional = fonts.families.entry(FontFamily::Proportional).or_default();
    proportional.insert(0, "inter".to_owned());
    if has_emoji {
        proportional.push("emoji".to_owned());
    }
    if has_arabic {
        proportional.push("arabic".to_owned());
    }

    for (family, mut keys) in [
        ("medium", vec!["inter-medium", "inter"]),
        ("semibold", vec!["inter-semibold", "inter"]),
    ] {
        if has_emoji {
            keys.push("emoji");
        }
        if has_arabic {
            keys.push("arabic");
        }
        fonts.families.insert(
            FontFamily::Name(family.into()),
            keys.into_iter().map(str::to_owned).collect(),
        );
    }
    ctx.set_fonts(fonts);
}

pub fn regular(size: f32) -> FontId {
    FontId::proportional(size)
}

pub fn medium(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("medium".into()))
}

pub fn semibold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("semibold".into()))
}

/// Inline code in messages. egui ships a monospace family with every build,
/// so no extra font file is needed.
pub fn mono(size: f32) -> FontId {
    FontId::monospace(size)
}

/// Window chrome colours for the whole app (not per service).
pub fn apply_mode(ctx: &egui::Context, mode: Mode) {
    let p = palette(mode);
    let mut visuals = match mode {
        Mode::Dark => egui::Visuals::dark(),
        Mode::Light => egui::Visuals::light(),
    };
    visuals.override_text_color = Some(p.text);
    visuals.panel_fill = p.panel;
    visuals.window_fill = p.panel;
    visuals.faint_bg_color = p.input_bg;
    visuals.extreme_bg_color = p.input_bg;
    visuals.hyperlink_color = p.accent;
    visuals.selection.bg_fill = p.accent.gamma_multiply(0.35);
    visuals.selection.stroke = egui::Stroke::new(1.0, p.accent);
    visuals.window_stroke = egui::Stroke::new(HAIRLINE, p.divider);
    visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(HAIRLINE, p.divider);
    visuals.widgets.inactive.bg_fill = p.input_bg;
    visuals.widgets.hovered.bg_fill = p.hover;
    visuals.widgets.active.bg_fill = p.selected;
    // Window title bars and buttons inherit the surface, so sheets look like
    // macOS panels instead of grey egui defaults.
    for widget in [
        &mut visuals.widgets.noninteractive,
        &mut visuals.widgets.inactive,
        &mut visuals.widgets.hovered,
        &mut visuals.widgets.active,
        &mut visuals.widgets.open,
    ] {
        widget.bg_stroke = egui::Stroke::NONE;
    }
    visuals.widgets.noninteractive.bg_fill = p.panel;
    visuals.widgets.open.weak_bg_fill = p.panel;
    visuals.widgets.open.bg_fill = p.panel;
    visuals.widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
    visuals.widgets.hovered.weak_bg_fill = p.hover;
    visuals.widgets.active.weak_bg_fill = p.selected;
    ctx.set_visuals(visuals);

    let theme = match mode {
        Mode::Dark => egui::Theme::Dark,
        Mode::Light => egui::Theme::Light,
    };
    let mut style = (*ctx.style_of(theme)).clone();
    style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    style.spacing.button_padding = egui::vec2(10.0, 5.0);
    style
        .text_styles
        .insert(egui::TextStyle::Body, regular(13.5));
    style
        .text_styles
        .insert(egui::TextStyle::Button, medium(13.0));
    style
        .text_styles
        .insert(egui::TextStyle::Small, regular(11.5));
    ctx.set_style_of(theme, style);
}
