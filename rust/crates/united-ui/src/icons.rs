//! Small line icons drawn as vectors, in the spirit of SF Symbols: one stroke
//! weight, rounded caps, optical size independent of the display scale.

use egui::{Color32, CornerRadius, Pos2, Rect, Response, Sense, Shape, Stroke, Ui, Vec2};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Bubbles,
    Compose,
    Search,
    Gear,
    ArrowUp,
    Ellipsis,
    Plus,
    Check,
    /// Anonymous contact mark: a phone number has no initials to show.
    Person,
    Pin,
    Mute,
    Smile,
    /// Chats: one speech bubble with a bottom-left tail.
    Chat,
    /// Voice call: a diagonal handset with round earpieces.
    Phone,
    /// Status updates: a dashed ring, like WhatsApp Web.
    Status,
    /// Channels: a broadcast mark with two waves.
    Channel,
    /// Communities: three people.
    Community,
    /// Business tools: a tray with a downward arrow.
    Tray,
    /// Advertise: a megaphone.
    Megaphone,
    /// Media sheet: a photo frame.
    Image,
    /// Microphone: a capsule over an arc, Discord's voice bar.
    Mic,
    /// Headphones: an arc with two ear cups.
    Headphones,
}

/// Paints `icon` inside `rect`, keeping a 1-point margin so strokes do not clip.
pub fn paint(painter: &egui::Painter, rect: Rect, icon: Icon, color: Color32, weight: f32) {
    let c = rect.center();
    let r = rect.width().min(rect.height()) * 0.5;
    let s = Stroke::new(weight, color);
    let line = |a: Pos2, b: Pos2| painter.line_segment([a, b], s);
    match icon {
        Icon::Bubbles => {
            // Two overlapping speech bubbles, wide rather than round so the
            // mark still reads at 24 px.
            let radius = CornerRadius::same((r * 0.42) as u8);
            let back = Rect::from_center_size(
                c + Vec2::new(r * 0.32, -r * 0.30),
                Vec2::new(r * 1.20, r * 0.86),
            );
            let front = Rect::from_center_size(
                c + Vec2::new(-r * 0.22, r * 0.22),
                Vec2::new(r * 1.34, r * 0.94),
            );
            painter.rect_stroke(back, radius, s, egui::StrokeKind::Inside);
            painter.rect_filled(front, radius, color);
        }
        Icon::Compose => {
            let square = Rect::from_center_size(c, Vec2::splat(r * 1.35));
            painter.rect_stroke(
                square,
                CornerRadius::same((r * 0.28) as u8),
                s,
                egui::StrokeKind::Inside,
            );
            line(
                c + Vec2::new(r * 0.15, -r * 0.55),
                c + Vec2::new(r * 0.75, r * 0.05),
            );
        }
        Icon::Search => {
            let lens = c + Vec2::new(-r * 0.18, -r * 0.18);
            painter.circle_stroke(lens, r * 0.55, s);
            line(
                lens + Vec2::splat(r * 0.39),
                c + Vec2::new(r * 0.72, r * 0.72),
            );
        }
        Icon::Gear => {
            // Cog: a ringed wheel with eight teeth, like WhatsApp Web's
            // settings button rather than macOS sliders.
            painter.circle_stroke(c, r * 0.58, s);
            painter.circle_stroke(c, r * 0.22, s);
            for i in 0..8 {
                let angle = i as f32 * std::f32::consts::TAU / 8.0;
                let dir = Vec2::angled(angle);
                line(c + dir * (r * 0.62), c + dir * (r * 0.92));
            }
        }
        Icon::ArrowUp => {
            // Paper plane, the send mark on WhatsApp Web.
            painter.add(Shape::line(
                vec![
                    c + Vec2::new(-r * 0.72, -r * 0.58),
                    c + Vec2::new(r * 0.78, 0.0),
                    c + Vec2::new(-r * 0.72, r * 0.58),
                    c + Vec2::new(-r * 0.34, 0.0),
                    c + Vec2::new(-r * 0.72, -r * 0.58),
                ],
                s,
            ));
            line(c + Vec2::new(-r * 0.34, 0.0), c + Vec2::new(r * 0.78, 0.0));
        }
        Icon::Ellipsis => {
            for dx in [-0.5, 0.0, 0.5] {
                painter.circle_filled(c + Vec2::new(r * dx * 1.1, 0.0), weight * 0.85, color);
            }
        }
        Icon::Plus => {
            line(c + Vec2::new(0.0, -r * 0.7), c + Vec2::new(0.0, r * 0.7));
            line(c + Vec2::new(-r * 0.7, 0.0), c + Vec2::new(r * 0.7, 0.0));
        }
        Icon::Person => {
            painter.circle_filled(c + Vec2::new(0.0, -r * 0.34), r * 0.30, color);
            let shoulders = Rect::from_min_size(
                c + Vec2::new(-r * 0.62, r * 0.12),
                Vec2::new(r * 1.24, r * 0.68),
            );
            painter.rect_filled(
                shoulders,
                CornerRadius {
                    nw: (r * 0.62) as u8,
                    ne: (r * 0.62) as u8,
                    sw: 0,
                    se: 0,
                },
                color,
            );
        }
        Icon::Pin => {
            painter.circle_filled(c + Vec2::new(r * 0.22, -r * 0.22), r * 0.34, color);
            line(
                c + Vec2::new(r * 0.02, r * 0.02),
                c + Vec2::new(-r * 0.55, r * 0.6),
            );
        }
        Icon::Mute => {
            // Speaker with the sound cone and a slash: the muted-chat mark.
            painter.rect_filled(
                Rect::from_min_max(
                    c + Vec2::new(-r * 0.66, -r * 0.20),
                    c + Vec2::new(-r * 0.32, r * 0.20),
                ),
                0.0,
                color,
            );
            painter.add(Shape::convex_polygon(
                vec![
                    c + Vec2::new(-r * 0.32, -r * 0.20),
                    c + Vec2::new(r * 0.10, -r * 0.54),
                    c + Vec2::new(r * 0.10, r * 0.54),
                    c + Vec2::new(-r * 0.32, r * 0.20),
                ],
                color,
                Stroke::NONE,
            ));
            line(
                c + Vec2::new(-r * 0.74, -r * 0.60),
                c + Vec2::new(r * 0.70, r * 0.60),
            );
        }
        Icon::Smile => {
            painter.circle_stroke(c, r * 0.82, s);
            painter.circle_filled(c + Vec2::new(-r * 0.30, -r * 0.18), weight * 0.9, color);
            painter.circle_filled(c + Vec2::new(r * 0.30, -r * 0.18), weight * 0.9, color);
            painter.add(Shape::line(
                vec![
                    c + Vec2::new(-r * 0.36, r * 0.18),
                    c + Vec2::new(0.0, r * 0.44),
                    c + Vec2::new(r * 0.36, r * 0.18),
                ],
                Stroke::new(weight * 0.9, color),
            ));
        }
        Icon::Chat => {
            // One bubble with a bottom-left tail, like WhatsApp's Chats mark.
            let bubble = Rect::from_center_size(
                c + Vec2::new(0.0, -r * 0.12),
                Vec2::new(r * 1.52, r * 1.18),
            );
            painter.rect_stroke(
                bubble,
                CornerRadius::same((r * 0.34) as u8),
                s,
                egui::StrokeKind::Inside,
            );
            let tail = bubble.left_bottom();
            line(
                tail + Vec2::new(r * 0.16, 0.0),
                c + Vec2::new(-r * 0.52, r * 0.72),
            );
            line(
                c + Vec2::new(-r * 0.52, r * 0.72),
                tail + Vec2::new(r * 0.60, 0.0),
            );
        }
        Icon::Phone => {
            // Handset: a diagonal bar with perpendicular ear cups.
            let thick = weight * 1.7;
            painter.add(Shape::line(
                vec![
                    c + Vec2::new(-r * 0.34, -r * 0.34),
                    c + Vec2::new(r * 0.34, r * 0.34),
                ],
                Stroke::new(thick, color),
            ));
            painter.add(Shape::line(
                vec![
                    c + Vec2::new(-r * 0.66, -r * 0.28),
                    c + Vec2::new(-r * 0.28, -r * 0.66),
                ],
                Stroke::new(thick * 1.15, color),
            ));
            painter.add(Shape::line(
                vec![
                    c + Vec2::new(r * 0.28, r * 0.66),
                    c + Vec2::new(r * 0.66, r * 0.28),
                ],
                Stroke::new(thick * 1.15, color),
            ));
        }
        Icon::Check => {
            painter.add(Shape::line(
                vec![
                    c + Vec2::new(-r * 0.6, r * 0.0),
                    c + Vec2::new(-r * 0.15, r * 0.45),
                    c + Vec2::new(r * 0.65, -r * 0.45),
                ],
                s,
            ));
        }
        Icon::Status => {
            // Dashed ring: twelve short arcs, the way WhatsApp draws "Status".
            let radius = r * 0.76;
            for i in 0..12 {
                let start = i as f32 * std::f32::consts::TAU / 12.0 + 0.06;
                let end = start + std::f32::consts::TAU / 12.0 - 0.24;
                let pts: Vec<Pos2> = (0..4)
                    .map(|k| {
                        let a = start + (end - start) * k as f32 / 3.0;
                        c + Vec2::angled(a) * radius
                    })
                    .collect();
                painter.add(Shape::line(pts, Stroke::new(weight * 0.95, color)));
            }
        }
        Icon::Channel => {
            // Broadcast: a dot with two wide waves on each side.
            painter.circle_filled(c, weight * 0.85, color);
            for (radius, spread) in [(r * 0.44, 0.78f32), (r * 0.76, 0.62)] {
                for side in [-1.0f32, 1.0] {
                    let pts: Vec<Pos2> = (0..6)
                        .map(|k| {
                            let a = side * (std::f32::consts::PI - spread)
                                + side * 2.0 * spread * k as f32 / 5.0;
                            c + Vec2::angled(a) * radius
                        })
                        .collect();
                    painter.add(Shape::line(pts, Stroke::new(weight * 0.9, color)));
                }
            }
        }
        Icon::Community => {
            // Three outlined people: a taller centre figure with two behind.
            let person = |dx: f32, scale: f32, painter: &egui::Painter| {
                let head = c + Vec2::new(dx, -r * 0.38 * scale);
                painter.circle_stroke(head, r * 0.17 * scale, Stroke::new(weight * 0.9, color));
                let shoulders = c + Vec2::new(dx, r * 0.26 * scale);
                let radius = r * 0.31 * scale;
                let pts: Vec<Pos2> = (0..7)
                    .map(|k| {
                        let a = std::f32::consts::PI + std::f32::consts::PI * k as f32 / 6.0;
                        shoulders + Vec2::new(radius * a.cos(), radius * a.sin())
                    })
                    .collect();
                painter.add(Shape::line(pts, Stroke::new(weight * 0.9, color)));
            };
            person(-r * 0.52, 0.72, painter);
            person(r * 0.52, 0.72, painter);
            person(0.0, 1.0, painter);
        }
        Icon::Tray => {
            // Archive tray with a downward arrow, like WhatsApp's Archived.
            painter.add(Shape::line(
                vec![
                    c + Vec2::new(-r * 0.72, -r * 0.30),
                    c + Vec2::new(-r * 0.72, r * 0.62),
                    c + Vec2::new(r * 0.72, r * 0.62),
                    c + Vec2::new(r * 0.72, -r * 0.30),
                ],
                s,
            ));
            line(c + Vec2::new(0.0, -r * 0.72), c + Vec2::new(0.0, r * 0.30));
            line(
                c + Vec2::new(-r * 0.30, r * 0.02),
                c + Vec2::new(0.0, r * 0.30),
            );
            line(
                c + Vec2::new(r * 0.30, r * 0.02),
                c + Vec2::new(0.0, r * 0.30),
            );
        }
        Icon::Megaphone => {
            // Outlined horn pointing right, with a grip and two sound waves.
            painter.add(Shape::closed_line(
                vec![
                    c + Vec2::new(-r * 0.60, -r * 0.22),
                    c + Vec2::new(-r * 0.10, -r * 0.62),
                    c + Vec2::new(-r * 0.10, r * 0.22),
                    c + Vec2::new(-r * 0.60, r * 0.12),
                ],
                s,
            ));
            line(
                c + Vec2::new(-r * 0.50, r * 0.14),
                c + Vec2::new(-r * 0.40, r * 0.58),
            );
            for (radius, spread) in [(r * 0.35, 0.62f32), (r * 0.60, 0.52)] {
                let origin = c + Vec2::new(-r * 0.05, -r * 0.18);
                let pts: Vec<Pos2> = (0..5)
                    .map(|k| {
                        let a = -spread + 2.0 * spread * k as f32 / 4.0;
                        origin + Vec2::new(radius * a.cos(), radius * a.sin())
                    })
                    .collect();
                painter.add(Shape::line(pts, Stroke::new(weight * 0.85, color)));
            }
        }
        Icon::Image => {
            let frame = Rect::from_center_size(c, Vec2::new(r * 1.55, r * 1.30));
            painter.rect_stroke(
                frame,
                CornerRadius::same((r * 0.24) as u8),
                s,
                egui::StrokeKind::Inside,
            );
            painter.circle_filled(c + Vec2::new(r * 0.34, -r * 0.32), r * 0.13, color);
            painter.add(Shape::line(
                vec![
                    c + Vec2::new(-r * 0.55, r * 0.30),
                    c + Vec2::new(-r * 0.18, -r * 0.10),
                    c + Vec2::new(r * 0.02, r * 0.12),
                    c + Vec2::new(r * 0.22, -r * 0.06),
                    c + Vec2::new(r * 0.55, r * 0.30),
                ],
                Stroke::new(weight * 0.9, color),
            ));
        }
        Icon::Mic => {
            let cap = Rect::from_center_size(
                c + Vec2::new(0.0, -r * 0.24),
                Vec2::new(r * 0.62, r * 1.04),
            );
            painter.rect_stroke(
                cap,
                CornerRadius::same((r * 0.31) as u8),
                s,
                egui::StrokeKind::Inside,
            );
            let arc: Vec<Pos2> = (0..=12)
                .map(|i| {
                    let a = std::f32::consts::PI * i as f32 / 12.0;
                    c + Vec2::new(a.cos() * r * 0.66, a.sin() * r * 0.34 + r * 0.04)
                })
                .collect();
            painter.add(Shape::line(arc, s));
            line(c + Vec2::new(0.0, r * 0.38), c + Vec2::new(0.0, r * 0.66));
            line(
                c + Vec2::new(-r * 0.28, r * 0.66),
                c + Vec2::new(r * 0.28, r * 0.66),
            );
        }
        Icon::Headphones => {
            let arc: Vec<Pos2> = (0..=12)
                .map(|i| {
                    let a = std::f32::consts::PI + std::f32::consts::PI * i as f32 / 12.0;
                    c + Vec2::new(a.cos() * r * 0.78, a.sin() * r * 0.72 + r * 0.02)
                })
                .collect();
            painter.add(Shape::line(arc, s));
            for side in [-1.0, 1.0] {
                let cup = Rect::from_center_size(
                    c + Vec2::new(side * r * 0.74, r * 0.24),
                    Vec2::new(r * 0.34, r * 0.62),
                );
                painter.rect_filled(cup, CornerRadius::same(3), color);
            }
        }
    }
}

/// Chromeless circular icon button with an iOS-style hover fill.
pub fn button(ui: &mut Ui, icon: Icon, size: f32, color: Color32, hover: Color32) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
    if resp.hovered() {
        ui.painter().circle_filled(rect.center(), size * 0.5, hover);
    }
    paint(
        ui.painter(),
        Rect::from_center_size(rect.center(), Vec2::splat(size * 0.62)),
        icon,
        if resp.hovered() {
            color
        } else {
            color.gamma_multiply(0.9)
        },
        1.6,
    );
    resp
}

/// WhatsApp Web's own marks, rendered from the SVGs the web app ships. Rail
/// tabs carry a filled twin for the open state; the rest are one silhouette.
macro_rules! wa_icons {
    ($($variant:ident => $file:literal),* $(,)? ; $($idle:ident => $filled:ident),* $(,)?) => {
        #[derive(Clone, Copy, PartialEq, Eq, Hash)]
        pub enum WaIcon {
            $($variant),*
        }

        impl WaIcon {
            /// The filled mark WhatsApp swaps in for the open tab.
            pub fn active(self) -> Self {
                match self {
                    $(Self::$idle => Self::$filled,)*
                    other => other,
                }
            }

            fn png(self) -> &'static [u8] {
                match self {
                    $(Self::$variant => include_bytes!(concat!("../assets/wa/", $file, ".png"))),*
                }
            }
        }
    };
}

wa_icons! {
    Chat => "chat",
    ChatActive => "chat-filled",
    Call => "call",
    CallActive => "call-filled",
    Status => "status",
    StatusActive => "status-filled",
    Channels => "channels",
    ChannelsActive => "channels-filled",
    Communities => "communities",
    CommunitiesActive => "communities-filled",
    Store => "store",
    StoreActive => "store-filled",
    Advertise => "advertise",
    Media => "media",
    Settings => "settings",
    SettingsActive => "settings-filled",
    Search => "search",
    Add => "add",
    AddCircle => "add-circle",
    More => "more",
    Dropdown => "dropdown",
    Archive => "archive",
    Read => "read",
    Image => "image",
    Pin => "pin",
    VideoCam => "videocam",
    Description => "description",
    PersonAdd => "person-add",
    Mail => "mail",
    VideoCall => "video-call",
    Lock => "lock",
    AddCall => "add-call",
    CallMade => "call-made",
    CallCallback => "call-callback",
    Link => "link",
    Dialpad => "dialpad",
    Edit => "edit",
    Camera => "camera",
    Checkbox => "checkbox",
    Close => "close",
    Sort => "sort",
    ChevronDown => "chevron-down",
    MetaOne => "meta-one",
    Weave => "weave",
    Grid => "grid",
    GridOn => "grid-on",
    Receipt => "receipt",
    Broadcast => "broadcast",
    Bolt => "bolt",
    SwitchAccount => "switch-account",
    Help => "help",
    Key => "key",
    Notifications => "notifications",
    Keyboard => "keyboard",
    Logout => "logout",
    Notes => "notes",
    Location => "location",
    Schedule => "schedule",
    Category => "category",
    AccountCircle => "account-circle",
    GroupFilled => "group-filled",
    Sticker => "sticker",
    Block => "block",
    Verified => "verified",
    NewChat => "new-chat",
    LaptopCalling => "laptop-calling",
    ;
    Chat => ChatActive,
    Call => CallActive,
    Status => StatusActive,
    Channels => ChannelsActive,
    Communities => CommunitiesActive,
    Store => StoreActive,
    Settings => SettingsActive,
}

/// Decoded rail bitmaps, kept for the life of the window. White masks are
/// tinted at paint time, so one texture serves both the idle and open state.
#[derive(Default)]
pub struct WaTextures(std::collections::HashMap<WaIcon, egui::TextureHandle>);

impl WaTextures {
    pub fn paint(
        &mut self,
        ctx: &egui::Context,
        painter: &egui::Painter,
        rect: Rect,
        icon: WaIcon,
        color: Color32,
    ) {
        let texture = self.0.entry(icon).or_insert_with(|| {
            let decoded = image::load_from_memory(icon.png()).expect("bundled WhatsApp icon");
            let mut rgba = decoded.to_rgba8();
            // Most captured SVGs draw with `currentColor`, but a few carry an
            // explicit dark fill. Painting tints by multiplying, so flatten
            // every mask to white and let the caller's colour decide.
            if icon != WaIcon::LaptopCalling {
                for pixel in rgba.pixels_mut() {
                    pixel[0] = 255;
                    pixel[1] = 255;
                    pixel[2] = 255;
                }
            }
            let pixels = egui::ColorImage::from_rgba_unmultiplied(
                [rgba.width() as usize, rgba.height() as usize],
                rgba.as_raw(),
            );
            ctx.load_texture("wa-rail-icon", pixels, egui::TextureOptions::LINEAR)
        });
        painter.image(
            texture.id(),
            rect,
            Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            color,
        );
    }
}

// --------------------------------------------------------------- Telegram Web
// Telegram Web A ships its marks as an icon font. The private-use glyphs were
// rendered once from the live client into white masks, so they tint and scale
// like the vector icons above instead of dragging a 60 kB font into the UI.
macro_rules! tg_icons {
    ($($variant:ident => $file:literal),* $(,)?) => {
        #[derive(Clone, Copy, PartialEq, Eq, Hash)]
        #[allow(dead_code)]
        pub enum TgIcon { $($variant),* }

        impl TgIcon {
            fn png(self) -> &'static [u8] {
                match self {
                    $(TgIcon::$variant => include_bytes!(concat!("../assets/tg/", $file, ".png"))),*
                }
            }
        }
    };
}

tg_icons! {
    Add => "add",
    AddUser => "add-user",
    Animations => "animations",
    Archive => "archive",
    ArrowLeft => "arrow-left",
    ArrowRight => "arrow-right",
    Attach => "attach",
    Bug => "bug",
    Camera => "camera",
    Channel => "channel",
    Check => "check",
    CheckFilled => "check-filled",
    Close => "close",
    Copy => "copy",
    Darkmode => "darkmode",
    Delete => "delete",
    Devices => "devices-filled",
    Download => "download",
    Edit => "edit",
    Eye => "eye",
    EyeCrossed => "eye-crossed",
    Folder => "folder",
    FolderFilled => "folder-filled",
    Forward => "forward",
    Gift => "gift",
    Gifs => "gifs",
    Group => "group",
    Hamburger => "hamburger",
    Help => "help",
    Info => "info",
    Install => "install",
    Keyboard => "keyboard",
    KeyboardFilled => "keyboard-filled",
    Language => "language",
    Location => "location",
    Lock => "lock",
    LockFilled => "lock-filled",
    Logout => "logout",
    Mark => "mark",
    Megaphone => "megaphone-filled",
    Mention => "mention",
    MessageFailed => "message-failed",
    MessagePending => "message-pending",
    MessageRead => "message-read",
    Microphone => "microphone",
    More => "more",
    Muted => "muted",
    NewChat => "new-chat-filled",
    Notifications => "notifications-filled",
    Palette => "palette",
    Phone => "phone",
    Pin => "pin",
    Poll => "poll",
    Privacy => "privacy-policy",
    ReadChats => "readchats",
    Replies => "replies",
    Reply => "reply",
    SavedMessages => "saved-messages",
    Search => "search",
    Send => "send",
    SendOutline => "send-outline",
    Settings => "settings",
    Share => "share-filled",
    Smile => "smile",
    Star => "star",
    Stickers => "stickers",
    Support => "support-filled",
    Unpin => "unpin",
    Unread => "unread",
    Unarchive => "unarchive",
    User => "user",
    UserTag => "user-tag",
    Video => "video",
    Warning => "warning",
    Web => "web",
}

/// Decoded Telegram masks, one texture per glyph for the life of the window.
#[derive(Default)]
pub struct TgTextures(std::collections::HashMap<TgIcon, egui::TextureHandle>);

impl TgTextures {
    pub fn paint(
        &mut self,
        ctx: &egui::Context,
        painter: &egui::Painter,
        rect: Rect,
        icon: TgIcon,
        color: Color32,
    ) {
        let texture = self.0.entry(icon).or_insert_with(|| {
            let decoded = image::load_from_memory(icon.png()).expect("bundled Telegram icon");
            let pixels = egui::ColorImage::from_rgba_unmultiplied(
                [decoded.width() as usize, decoded.height() as usize],
                decoded.to_rgba8().as_raw(),
            );
            ctx.load_texture("tg-icon", pixels, egui::TextureOptions::LINEAR)
        });
        painter.image(
            texture.id(),
            rect,
            Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            color,
        );
    }
}

// ----------------------------------------------------------------- Discord Web
// Captured from the live client as white masks, like the WhatsApp rail. The
// guild rail, channel list and friends page all draw from this one set.
macro_rules! dc_icons {
    ($($variant:ident => $file:literal),* $(,)?) => {
        #[derive(Clone, Copy, PartialEq, Eq, Hash)]
        #[allow(dead_code)]
        pub enum DcIcon { $($variant),* }

        impl DcIcon {
            fn png(self) -> &'static [u8] {
                match self {
                    $(DcIcon::$variant => include_bytes!(concat!("../assets/dc/", $file, ".png"))),*
                }
            }
        }
    };
}

dc_icons! {
    AddServer => "addserver",
    Discover => "discover",
    Friends => "friends",
    Gear => "gear",
    Help => "help",
    Home => "home",
    Inbox => "inbox",
    Message => "message",
    More => "more",
    Nitro => "nitro",
    Plus => "plus",
    Quests => "quests",
    Search => "search",
    Shop => "shop",
}

/// Decoded Discord masks, one texture per glyph for the life of the window.
#[derive(Default)]
pub struct DcTextures(std::collections::HashMap<DcIcon, egui::TextureHandle>);

impl DcTextures {
    pub fn paint(
        &mut self,
        ctx: &egui::Context,
        painter: &egui::Painter,
        rect: Rect,
        icon: DcIcon,
        color: Color32,
    ) {
        let texture = self.0.entry(icon).or_insert_with(|| {
            let decoded = image::load_from_memory(icon.png()).expect("bundled Discord icon");
            let pixels = egui::ColorImage::from_rgba_unmultiplied(
                [decoded.width() as usize, decoded.height() as usize],
                decoded.to_rgba8().as_raw(),
            );
            ctx.load_texture("dc-icon", pixels, egui::TextureOptions::LINEAR)
        });
        painter.image(
            texture.id(),
            rect,
            Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            color,
        );
    }
}
