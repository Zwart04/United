use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::format;
use crate::icons::{self, Icon};
use crate::qr;
use crate::theme::{self, HAIRLINE, Mode, Palette};
use crate::widgets;
use egui::{Color32, CornerRadius, Frame, Margin, Rect, RichText, ScrollArea, Sense, Stroke, Vec2};
use tokio::sync::broadcast::error::TryRecvError;
use united_core::events::{AuthPrompt, CallState, CoreEvent, ServiceStatus};
use united_core::model::{Chat, MediaRef, Message, Service};
use united_core::{Adapter, AppSettings, AuthInput, EventBus, Store};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum View {
    Service(Service),
    Unified,
    Settings,
}

/// WhatsApp Web's own rail, shown only while the WhatsApp view is open.
/// Media and Advertise are sheets; Profile and Settings fill the view.
#[derive(Clone, Copy, PartialEq, Eq)]
enum WaSection {
    Chats,
    Calls,
    Status,
    Channels,
    Communities,
    Business,
    Profile,
    Settings,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum WaModal {
    Advertise,
    Media,
}

/// Telegram Web A's hamburger drawer. Contacts and Settings replace the chat
/// list column; the rest open a pane in the middle column.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum TgSection {
    #[default]
    Chats,
    Contacts,
    Settings,
    Profile,
    Info(TgInfo),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TgInfo {
    AddAccount,
    NewContact,
    Saved,
    UiFeatures,
    Features,
    Bug,
    KVersion,
    Install,
}

impl TgInfo {
    fn title(self) -> &'static str {
        match self {
            Self::AddAccount => "Add Account",
            Self::NewContact => "New Contact",
            Self::Saved => "Saved Messages",
            Self::UiFeatures => "UI Features",
            Self::Features => "Telegram Features",
            Self::Bug => "Report a Bug",
            Self::KVersion => "Switch to K Version",
            Self::Install => "Install App",
        }
    }

    fn body(self) -> &'static str {
        match self {
            Self::AddAccount => {
                "United keeps one Telegram session per install. Sign out from the rail to link another account."
            }
            Self::NewContact => {
                "Telegram contacts come from the phone's address book. Add the person there and the next sync will carry the name."
            }
            Self::Saved => {
                "Messages you forward here are kept on Telegram's servers. None arrived yet."
            }
            Self::UiFeatures => {
                "Animations, chat backgrounds and the compact layout are part of United's native renderer."
            }
            Self::Features => {
                "Voice chats, folders and secret chats need MTProto calls United does not make yet."
            }
            Self::Bug => {
                "Describe what happened in the United repository's issue tracker. Logs live under the app data folder."
            }
            Self::KVersion => {
                "Web K is the other Telegram client. United talks MTProto directly, so there is no version to switch."
            }
            Self::Install => {
                "United already runs as a desktop app. A desktop entry is created when launch at startup is enabled."
            }
        }
    }

    fn icon(self) -> icons::TgIcon {
        match self {
            Self::AddAccount => icons::TgIcon::AddUser,
            Self::NewContact => icons::TgIcon::AddUser,
            Self::Saved => icons::TgIcon::SavedMessages,
            Self::UiFeatures => icons::TgIcon::Animations,
            Self::Features => icons::TgIcon::Help,
            Self::Bug => icons::TgIcon::Bug,
            Self::KVersion => icons::TgIcon::Web,
            Self::Install => icons::TgIcon::Install,
        }
    }
}

/// Rows in Telegram's settings list. Only the ones with a native counterpart
/// get a detail page; the rest explain where Telegram keeps the switch.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum TgSettingsTab {
    #[default]
    Account,
    General,
    Notifications,
    Privacy,
    Data,
    Devices,
    Language,
    Stickers,
}

/// Discord Web's guild rail and channel sidebar. Guild channels are outside
/// the adapter's scope, so the rail's buttons explain that instead of lying.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum DcSection {
    #[default]
    Friends,
    Nitro,
    Shop,
    Quests,
    AddFriend,
    AddServer,
    Discover,
    Chat,
}

impl DcSection {
    fn title(self) -> &'static str {
        match self {
            Self::Friends | Self::Chat => "Friends",
            Self::Nitro => "Nitro",
            Self::Shop => "Shop",
            Self::Quests => "Quests",
            Self::AddFriend => "Add Friend",
            Self::AddServer => "Add a Server",
            Self::Discover => "Discover",
        }
    }

    fn body(self) -> &'static str {
        match self {
            Self::Nitro => {
                "Nitro subscriptions, boosts and premium stickers are sold by Discord. United reads messages with a bot token and cannot buy them."
            }
            Self::Shop => {
                "The Shop sells avatar decorations and profile effects. Those live on Discord's account API, outside United's adapter."
            }
            Self::Quests => {
                "Quests are promotional tasks shown inside Discord's own client. United does not render them."
            }
            Self::AddFriend => {
                "Friend requests need a Discord user session. United's adapter connects with a bot token and only lists conversations that already exist."
            }
            Self::AddServer => {
                "Guild channels are outside the adapter. United lists direct messages; server text and voice stay in Discord."
            }
            Self::Discover => {
                "Server discovery is Discord's directory of public guilds. United does not browse guilds."
            }
            _ => "",
        }
    }

    fn icon(self) -> icons::DcIcon {
        match self {
            Self::Nitro => icons::DcIcon::Nitro,
            Self::Shop => icons::DcIcon::Shop,
            Self::Quests => icons::DcIcon::Quests,
            Self::AddFriend => icons::DcIcon::Friends,
            Self::AddServer => icons::DcIcon::AddServer,
            Self::Discover => icons::DcIcon::Discover,
            _ => icons::DcIcon::Home,
        }
    }
}

impl WaSection {
    /// Exact WhatsApp rail mark; `None` for the profile, which is a photo.
    fn wa_icon(self) -> Option<icons::WaIcon> {
        match self {
            Self::Chats => Some(icons::WaIcon::Chat),
            Self::Calls => Some(icons::WaIcon::Call),
            Self::Status => Some(icons::WaIcon::Status),
            Self::Channels => Some(icons::WaIcon::Channels),
            Self::Communities => Some(icons::WaIcon::Communities),
            Self::Business => Some(icons::WaIcon::Store),
            Self::Settings => Some(icons::WaIcon::Settings),
            Self::Profile => None,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Chats => "Chat",
            Self::Calls => "Telepon",
            Self::Status => "Status",
            Self::Channels => "Saluran",
            Self::Communities => "Komunitas",
            Self::Business => "Fitur bisnis",
            Self::Settings => "Pengaturan",
            Self::Profile => "Profil",
        }
    }
}

/// Quick-pick emoji for the composer, spread over the faces and gestures
/// WhatsApp Web shows first. A full searchable set would need a dataset;
/// this covers the everyday reactions without shipping one.
const EMOJI: [&str; 48] = [
    "😀", "😁", "😂", "🤣", "😊", "😍", "😘", "😎", "🤔", "😅", "😭", "😡", "👍", "👎", "🙏", "👏",
    "💪", "👌", "✌️", "🤝", "❤️", "💔", "🔥", "🎉", "✨", "💯", "✅", "❌", "⭐", "🌹", "☀️", "🌙",
    "🍕", "☕", "🎂", "🎁", "⚽", "🏆", "🚀", "💡", "📌", "📷", "🎵", "😴", "🤒", "🤯", "🥳", "😇",
];

/// Which cache a decoded image belongs to.
#[derive(Clone, Copy, PartialEq, Eq)]
enum DecodeKind {
    Media,
    Avatar,
}

/// Where the worker gets the bytes.
enum DecodeSource {
    File(String),
}

struct DecodeDone {
    kind: DecodeKind,
    key: String,
    pixels: Option<egui::ColorImage>,
}

/// Keep a decoded-texture cache bounded. Photos cost several megabytes each,
/// so a long session cannot hold every picture it ever painted.
fn cap_cache<K: std::hash::Hash + Eq + Clone, V>(map: &mut HashMap<K, V>, cap: usize) {
    if map.len() <= cap {
        return;
    }
    let excess = map.len() - cap;
    let doomed: Vec<K> = map.keys().take(excess).cloned().collect();
    for key in doomed {
        map.remove(&key);
    }
}

/// One tile in WhatsApp's own rail. The open tab swaps in the filled mark
/// WhatsApp uses, on the same raised square the web client paints.
fn wa_tile(
    ui: &mut egui::Ui,
    textures: &mut icons::WaTextures,
    icon: icons::WaIcon,
    selected: bool,
    p: &Palette,
    badge: u32,
) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(40.0), Sense::click());
    if selected {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(10), p.selected);
    } else if resp.hovered() {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(10), p.hover);
    }
    textures.paint(
        ui.ctx(),
        ui.painter(),
        Rect::from_center_size(rect.center(), Vec2::splat(24.0)),
        if selected { icon.active() } else { icon },
        if selected { p.text } else { p.text_muted },
    );
    if badge > 0 {
        let pos = rect.center() + Vec2::new(14.0, -14.0);
        ui.painter()
            .circle_filled(pos, 8.0, Color32::from_rgb(0x25, 0xd3, 0x66));
        ui.painter().text(
            pos,
            egui::Align2::CENTER_CENTER,
            if badge > 99 {
                "99+".to_string()
            } else {
                badge.to_string()
            },
            theme::semibold(8.5),
            Color32::WHITE,
        );
    }
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

/// Round chromeless button using WhatsApp's own bitmap mark.
fn wa_icon_button(
    ui: &mut egui::Ui,
    textures: &mut icons::WaTextures,
    icon: icons::WaIcon,
    p: &Palette,
    diameter: f32,
    icon_size: f32,
    tint: Color32,
) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(diameter), Sense::click());
    if resp.hovered() {
        ui.painter()
            .circle_filled(rect.center(), diameter / 2.0, p.hover);
    }
    textures.paint(
        ui.ctx(),
        ui.painter(),
        Rect::from_center_size(rect.center(), Vec2::splat(icon_size)),
        icon,
        tint,
    );
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

/// WhatsApp's pill search field, icon inside the fill.
fn wa_search_box(
    ui: &mut egui::Ui,
    textures: &mut icons::WaTextures,
    p: &Palette,
    query: &mut String,
    hint: &str,
) {
    let width = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, 36.0), Sense::click());
    ui.painter()
        .rect_filled(rect, CornerRadius::same(8), p.input_bg);
    textures.paint(
        ui.ctx(),
        ui.painter(),
        Rect::from_center_size(
            egui::pos2(rect.left() + 18.0, rect.center().y),
            Vec2::splat(16.0),
        ),
        icons::WaIcon::Search,
        p.text_muted,
    );
    let edit_rect = Rect::from_min_max(
        egui::pos2(rect.left() + 38.0, rect.top() + 7.0),
        egui::pos2(rect.right() - 12.0, rect.bottom() - 7.0),
    );
    let edit = ui.put(
        edit_rect,
        egui::TextEdit::singleline(query)
            .hint_text(RichText::new(hint).color(p.text_muted))
            .frame(Frame::NONE)
            .text_color(p.text)
            .font(theme::regular(13.0)),
    );
    if resp.clicked() {
        edit.request_focus();
    }
}

/// Title row of a WhatsApp tab. Actions are painted right to left, exactly as
/// they sit in the web client; returns which one was pressed.
fn wa_header(
    ui: &mut egui::Ui,
    textures: &mut icons::WaTextures,
    p: &Palette,
    title: &str,
    actions: &[icons::WaIcon],
) -> Option<usize> {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 58.0), Sense::hover());
    ui.painter().text(
        egui::pos2(rect.left() + 16.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        title,
        theme::semibold(20.0),
        p.text,
    );
    let mut right = rect.right() - 16.0;
    let mut clicked = None;
    for (index, icon) in actions.iter().enumerate() {
        let center = egui::pos2(right - 18.0, rect.center().y);
        let hit = Rect::from_center_size(center, Vec2::splat(40.0));
        let resp = ui.interact(
            hit,
            ui.id().with(("wa-header", title, index)),
            Sense::click(),
        );
        if resp.hovered() {
            ui.painter().circle_filled(center, 20.0, p.hover);
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        textures.paint(
            ui.ctx(),
            ui.painter(),
            Rect::from_center_size(center, Vec2::splat(24.0)),
            *icon,
            p.text_muted,
        );
        if resp.clicked() {
            clicked = Some(index);
        }
        right -= 44.0;
    }
    clicked
}

/// Bold section label with an optional green link on the right.
fn wa_section_row(ui: &mut egui::Ui, p: &Palette, title: &str, action: Option<&str>) -> bool {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 34.0), Sense::hover());
    ui.painter().text(
        egui::pos2(rect.left() + 16.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        title,
        theme::semibold(13.0),
        p.text,
    );
    let mut clicked = false;
    if let Some(action) = action {
        let galley = ui
            .painter()
            .layout_no_wrap(action.to_string(), theme::medium(11.5), p.accent);
        let action_rect = Rect::from_center_size(
            egui::pos2(rect.right() - 16.0 - galley.size().x / 2.0, rect.center().y),
            galley.size() + Vec2::new(12.0, 16.0),
        );
        let resp = ui.interact(
            action_rect,
            ui.id().with(("wa-section", title)),
            Sense::click(),
        );
        if resp.hovered() {
            ui.painter()
                .rect_filled(action_rect, CornerRadius::same(6), p.hover);
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        ui.painter().galley(
            egui::pos2(
                rect.right() - 16.0 - galley.size().x,
                rect.center().y - galley.size().y / 2.0,
            ),
            galley,
            p.accent,
        );
        clicked = resp.clicked();
    }
    clicked
}

/// Sentence-case group heading, as used by the business tools list.
fn wa_group_label(ui: &mut egui::Ui, p: &Palette, title: &str) {
    ui.add_space(16.0);
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 26.0), Sense::hover());
    ui.painter().text(
        egui::pos2(rect.left() + 16.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        title,
        theme::semibold(13.0),
        p.text,
    );
    ui.add_space(4.0);
}

/// Circular avatar with the same fallback palette as the chat list. A ring
/// colour draws WhatsApp's status circle around it.
fn wa_avatar(
    ui: &egui::Ui,
    p: &Palette,
    center: egui::Pos2,
    size: f32,
    title: &str,
    picture: Option<&egui::TextureHandle>,
    ring: Option<Color32>,
) {
    let avatar = Rect::from_center_size(center, Vec2::splat(size));
    if let Some(texture) = picture {
        egui::Image::new((texture.id(), Vec2::splat(size)))
            .corner_radius(CornerRadius::same((size / 2.0) as u8))
            .paint_at(ui, avatar);
    } else {
        ui.painter()
            .circle_filled(center, size / 2.0, format::avatar_color(title));
        if title.chars().any(|c| c.is_alphabetic()) {
            ui.painter().text(
                center,
                egui::Align2::CENTER_CENTER,
                format::initials(title),
                theme::medium(size * 0.34),
                Color32::WHITE,
            );
        } else {
            icons::paint(
                ui.painter(),
                Rect::from_center_size(center, Vec2::splat(size * 0.5)),
                Icon::Person,
                Color32::WHITE,
                1.8,
            );
        }
    }
    if let Some(color) = ring {
        ui.painter()
            .circle_stroke(center, size / 2.0 + 2.5, Stroke::new(2.0, color));
    }
    let _ = p;
}

/// One row of the Calls tab: name and day, then the call direction with the
/// outgoing/incoming/missed mark WhatsApp uses.
fn wa_call_row(
    ui: &mut egui::Ui,
    textures: &mut icons::WaTextures,
    p: &Palette,
    title: &str,
    time: &str,
    picture: Option<&egui::TextureHandle>,
    outgoing: bool,
    answered: bool,
) -> egui::Response {
    let width = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, 64.0), Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(rect, CornerRadius::ZERO, p.hover);
    }
    let center_y = rect.center().y;
    wa_avatar(
        ui,
        p,
        egui::pos2(rect.left() + 40.0, center_y),
        44.0,
        title,
        picture,
        None,
    );
    let text_x = rect.left() + 74.0;
    ui.painter().text(
        egui::pos2(text_x, rect.top() + 22.0),
        egui::Align2::LEFT_CENTER,
        title,
        theme::regular(14.0),
        p.text,
    );
    ui.painter().text(
        egui::pos2(rect.right() - 16.0, rect.top() + 22.0),
        egui::Align2::RIGHT_CENTER,
        time,
        theme::regular(11.0),
        p.text_muted,
    );
    let label = if outgoing {
        "Keluar"
    } else if answered {
        "Masuk"
    } else {
        "Tak terjawab"
    };
    let tint = if !outgoing && !answered {
        Color32::from_rgb(0xea, 0x55, 0x55)
    } else {
        p.text_muted
    };
    textures.paint(
        ui.ctx(),
        ui.painter(),
        Rect::from_center_size(
            egui::pos2(text_x + 7.0, rect.top() + 43.0),
            Vec2::splat(14.0),
        ),
        if outgoing {
            icons::WaIcon::CallMade
        } else {
            icons::WaIcon::CallCallback
        },
        tint,
    );
    ui.painter().text(
        egui::pos2(text_x + 18.0, rect.top() + 43.0),
        egui::Align2::LEFT_CENTER,
        label,
        theme::regular(11.5),
        tint,
    );
    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    resp
}

/// One row of the Channels tab: channel name and clock, sender preview and the
/// green unread count.
fn wa_channel_row(
    ui: &mut egui::Ui,
    p: &Palette,
    chat: &Chat,
    time: &str,
    picture: Option<&egui::TextureHandle>,
) -> egui::Response {
    let width = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, 62.0), Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(rect, CornerRadius::ZERO, p.hover);
    }
    wa_avatar(
        ui,
        p,
        egui::pos2(rect.left() + 40.0, rect.center().y),
        44.0,
        &chat.title,
        picture,
        None,
    );
    let text_x = rect.left() + 74.0;
    let time_galley =
        ui.painter()
            .layout_no_wrap(time.to_string(), theme::regular(11.0), p.text_muted);
    let time_w = time_galley.size().x;
    ui.painter().text(
        egui::pos2(text_x, rect.top() + 19.0),
        egui::Align2::LEFT_CENTER,
        &chat.title,
        theme::medium(13.5),
        p.text,
    );
    ui.painter().galley(
        egui::pos2(
            rect.right() - 16.0 - time_w,
            rect.top() + 19.0 - time_galley.size().y / 2.0,
        ),
        time_galley,
        p.text_muted,
    );
    let preview = if chat.preview_sender.is_empty() {
        chat.preview.clone()
    } else {
        format!("{}: {}", chat.preview_sender, chat.preview)
    };
    // Newsletter previews arrive with hard line breaks; a single line with an
    // ellipsis is what WhatsApp shows.
    let preview = preview.replace(['\n', '\r'], " ");
    let badge_w = if chat.unread > 0 { 28.0 } else { 0.0 };
    let clip_w = (rect.right() - 16.0 - badge_w - text_x).max(0.0);
    let preview = elide(&preview, clip_w, 11.5);
    let clip = Rect::from_min_max(
        egui::pos2(text_x, rect.top() + 30.0),
        egui::pos2(rect.right() - 16.0 - badge_w, rect.bottom()),
    );
    ui.painter().with_clip_rect(clip).text(
        egui::pos2(text_x, rect.top() + 40.0),
        egui::Align2::LEFT_CENTER,
        preview,
        theme::regular(11.5),
        p.text_muted,
    );
    if chat.unread > 0 {
        let center = egui::pos2(rect.right() - 22.0, rect.top() + 41.0);
        ui.painter()
            .circle_filled(center, 9.0, Color32::from_rgb(0x25, 0xd3, 0x66));
        ui.painter().text(
            center,
            egui::Align2::CENTER_CENTER,
            if chat.unread > 99 {
                "99+".to_string()
            } else {
                chat.unread.to_string()
            },
            theme::semibold(9.0),
            Color32::WHITE,
        );
    }
    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    resp
}

/// A centred round action with a caption underneath, used in every detail pane.
fn wa_action_row(
    ui: &mut egui::Ui,
    textures: &mut icons::WaTextures,
    p: &Palette,
    actions: &[(icons::WaIcon, &str)],
) -> Option<usize> {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 74.0), Sense::hover());
    let step = 150.0;
    let start = rect.center().x - step * (actions.len() as f32 - 1.0) / 2.0;
    let mut clicked = None;
    for (index, (icon, label)) in actions.iter().enumerate() {
        let center = egui::pos2(start + step * index as f32, rect.top() + 24.0);
        let hit = Rect::from_center_size(center, Vec2::splat(64.0));
        let resp = ui.interact(hit, ui.id().with(("wa-action", label)), Sense::click());
        ui.painter().circle_filled(
            center,
            24.0,
            if resp.hovered() { p.selected } else { p.header },
        );
        textures.paint(
            ui.ctx(),
            ui.painter(),
            Rect::from_center_size(center, Vec2::splat(22.0)),
            *icon,
            p.text,
        );
        ui.painter().text(
            egui::pos2(center.x, rect.top() + 58.0),
            egui::Align2::CENTER_CENTER,
            *label,
            theme::regular(10.5),
            p.text_muted,
        );
        if resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if resp.clicked() {
            clicked = Some(index);
        }
    }
    clicked
}

/// End-to-end footer pinned to the bottom of the WhatsApp detail panes.
fn wa_e2e_footer(ui: &mut egui::Ui, textures: &mut icons::WaTextures, p: &Palette, text: &str) {
    let rest = ui.available_height();
    if rest > 60.0 {
        ui.add_space(rest - 48.0);
    }
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 28.0), Sense::hover());
    let font = theme::regular(11.5);
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_string(), font, p.text_muted);
    let total = galley.size().x + 22.0;
    let left = rect.center().x - total / 2.0;
    textures.paint(
        ui.ctx(),
        ui.painter(),
        Rect::from_center_size(egui::pos2(left + 8.0, rect.center().y), Vec2::splat(17.0)),
        icons::WaIcon::Lock,
        p.text_muted,
    );
    ui.painter().galley(
        egui::pos2(left + 20.0, rect.center().y - galley.size().y / 2.0),
        galley,
        p.text_muted,
    );
}

/// Approximate single-line truncation, matching the chat list's own helper.
fn elide(text: &str, width: f32, size: f32) -> String {
    if width <= 0.0 {
        return String::new();
    }
    let max_chars = (width / (size * 0.52)) as usize;
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let mut out: String = text.chars().take(max_chars.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// Group an Indonesian number the way WhatsApp prints it.
fn format_phone(raw: &str) -> String {
    let digits: String = raw
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == '+')
        .collect();
    if let Some(rest) = digits.strip_prefix("+62") {
        let rest = rest.trim_start_matches('0');
        if rest.len() >= 10 {
            return format!("+62 {}-{}-{}", &rest[..3], &rest[3..7], &rest[7..]);
        }
    }
    raw.to_string()
}

/// Menu row used by the business, settings and profile lists. A green dot
/// mirrors WhatsApp's "new" marker.
fn wa_menu_row(
    ui: &mut egui::Ui,
    textures: &mut icons::WaTextures,
    p: &Palette,
    icon: icons::WaIcon,
    title: &str,
    detail: &str,
    dot: bool,
) -> bool {
    let width = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, 56.0), Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(rect, CornerRadius::ZERO, p.hover);
    }
    textures.paint(
        ui.ctx(),
        ui.painter(),
        Rect::from_center_size(
            egui::pos2(rect.left() + 32.0, rect.center().y),
            Vec2::splat(22.0),
        ),
        icon,
        p.text_muted,
    );
    ui.painter().text(
        rect.left_top() + Vec2::new(56.0, 12.0),
        egui::Align2::LEFT_TOP,
        title,
        theme::regular(13.5),
        p.text,
    );
    ui.painter().text(
        rect.left_top() + Vec2::new(56.0, 31.0),
        egui::Align2::LEFT_TOP,
        detail,
        theme::regular(11.0),
        p.text_muted,
    );
    if dot {
        ui.painter().circle_filled(
            egui::pos2(rect.right() - 20.0, rect.center().y),
            4.0,
            Color32::from_rgb(0x25, 0xd3, 0x66),
        );
    }
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp.clicked()
}

/// Small green text button, the shape WhatsApp uses for Edit and Add links.
fn wa_text_link(ui: &mut egui::Ui, p: &Palette, label: &str) -> bool {
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_string(), theme::medium(12.0), p.accent);
    let (rect, resp) =
        ui.allocate_exact_size(galley.size() + Vec2::new(16.0, 12.0), Sense::click());
    if resp.hovered() {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(6), p.hover);
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    ui.painter()
        .galley(rect.center() - galley.size() / 2.0, galley, p.accent);
    resp.clicked()
}

/// Profile information row: icon, tiny caption and the value.
fn wa_info_row(
    ui: &mut egui::Ui,
    textures: &mut icons::WaTextures,
    p: &Palette,
    icon: icons::WaIcon,
    label: &str,
    value: &str,
) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 46.0), Sense::hover());
    textures.paint(
        ui.ctx(),
        ui.painter(),
        Rect::from_center_size(
            egui::pos2(rect.left() + 11.0, rect.center().y),
            Vec2::splat(20.0),
        ),
        icon,
        p.text_muted,
    );
    ui.painter().text(
        rect.left_top() + Vec2::new(34.0, 5.0),
        egui::Align2::LEFT_TOP,
        label,
        theme::regular(10.0),
        p.text_muted,
    );
    ui.painter().text(
        rect.left_top() + Vec2::new(34.0, 20.0),
        egui::Align2::LEFT_TOP,
        value,
        theme::regular(13.0),
        p.text,
    );
}

/// Rounded profile input: leading icon and muted placeholder inside the border.
fn wa_input_row(
    ui: &mut egui::Ui,
    textures: &mut icons::WaTextures,
    p: &Palette,
    icon: icons::WaIcon,
    placeholder: &str,
) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 46.0), Sense::hover());
    let field = Rect::from_min_max(
        egui::pos2(rect.left() + 34.0, rect.top() + 4.0),
        egui::pos2(rect.right(), rect.bottom() - 4.0),
    );
    textures.paint(
        ui.ctx(),
        ui.painter(),
        Rect::from_center_size(
            egui::pos2(rect.left() + 11.0, field.center().y),
            Vec2::splat(20.0),
        ),
        icon,
        p.text_muted,
    );
    ui.painter().rect_stroke(
        field,
        CornerRadius::same(8),
        Stroke::new(1.0, p.divider),
        egui::StrokeKind::Inside,
    );
    ui.painter().text(
        egui::pos2(field.left() + 14.0, field.center().y),
        egui::Align2::LEFT_CENTER,
        placeholder,
        theme::regular(12.5),
        p.text_muted,
    );
}

/// Caption above a rounded selector field, like "Area dalam cakupan".
fn wa_selector_row(
    ui: &mut egui::Ui,
    textures: &mut icons::WaTextures,
    p: &Palette,
    label: &str,
    value: &str,
) {
    let width = ui.available_width();
    let (caption, _) = ui.allocate_exact_size(Vec2::new(width, 22.0), Sense::hover());
    ui.painter().text(
        egui::pos2(caption.left() + 34.0, caption.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        theme::regular(10.0),
        p.text_muted,
    );
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 40.0), Sense::hover());
    let field = Rect::from_min_max(
        egui::pos2(rect.left() + 34.0, rect.top()),
        egui::pos2(rect.right(), rect.bottom() - 2.0),
    );
    ui.painter().rect_stroke(
        field,
        CornerRadius::same(8),
        Stroke::new(1.0, p.divider),
        egui::StrokeKind::Inside,
    );
    ui.painter().text(
        egui::pos2(field.left() + 14.0, field.center().y),
        egui::Align2::LEFT_CENTER,
        value,
        theme::regular(12.5),
        p.text,
    );
    textures.paint(
        ui.ctx(),
        ui.painter(),
        Rect::from_center_size(
            egui::pos2(field.right() - 16.0, field.center().y),
            Vec2::splat(18.0),
        ),
        icons::WaIcon::Dropdown,
        p.text_muted,
    );
}

/// Menu row with a switch on the right. Returns true when the value changed.
fn wa_toggle_row(
    ui: &mut egui::Ui,
    textures: &mut icons::WaTextures,
    p: &Palette,
    icon: icons::WaIcon,
    title: &str,
    detail: &str,
    value: &mut bool,
) -> bool {
    let width = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, 58.0), Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(rect, CornerRadius::ZERO, p.hover);
    }
    textures.paint(
        ui.ctx(),
        ui.painter(),
        Rect::from_center_size(
            egui::pos2(rect.left() + 32.0, rect.center().y),
            Vec2::splat(22.0),
        ),
        icon,
        p.text_muted,
    );
    ui.painter().text(
        rect.left_top() + Vec2::new(56.0, 13.0),
        egui::Align2::LEFT_TOP,
        title,
        theme::regular(13.5),
        p.text,
    );
    ui.painter().text(
        rect.left_top() + Vec2::new(56.0, 32.0),
        egui::Align2::LEFT_TOP,
        detail,
        theme::regular(11.0),
        p.text_muted,
    );
    let track = Rect::from_center_size(
        egui::pos2(rect.right() - 40.0, rect.center().y),
        Vec2::new(36.0, 20.0),
    );
    ui.painter().rect_filled(
        track,
        CornerRadius::same(10),
        if *value { p.accent } else { p.divider },
    );
    ui.painter().circle_filled(
        egui::pos2(
            if *value {
                track.right() - 10.0
            } else {
                track.left() + 10.0
            },
            track.center().y,
        ),
        8.0,
        Color32::WHITE,
    );
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    if resp.clicked() {
        *value = !*value;
        return true;
    }
    false
}

/// Centred WhatsApp detail pane: illustration card, optional round actions and
/// the end-to-end footer. Returns the pressed action index.
fn wa_detail_card(
    ui: &mut egui::Ui,
    textures: &mut icons::WaTextures,
    p: &Palette,
    icon: icons::WaIcon,
    title: &str,
    body: &str,
    actions: &[(icons::WaIcon, &str)],
    footer: Option<&str>,
) -> Option<usize> {
    let mut pressed = None;
    ui.add_space(100.0);
    ui.vertical_centered(|ui| {
        Frame::new()
            .fill(p.header)
            .corner_radius(12)
            .inner_margin(Margin::symmetric(20, 18))
            .show(ui, |ui| {
                ui.set_width(240.0);
                ui.vertical_centered(|ui| {
                    let (rect, _) = ui.allocate_exact_size(Vec2::new(132.0, 88.0), Sense::hover());
                    if icon == icons::WaIcon::LaptopCalling {
                        textures.paint(ui.ctx(), ui.painter(), rect, icon, Color32::WHITE);
                    } else {
                        textures.paint(
                            ui.ctx(),
                            ui.painter(),
                            Rect::from_center_size(rect.center(), Vec2::splat(56.0)),
                            icon,
                            p.text_muted.gamma_multiply(0.55),
                        );
                    }
                    ui.add_space(12.0);
                    ui.label(
                        RichText::new(title)
                            .font(theme::semibold(16.0))
                            .color(p.text),
                    );
                    ui.add_space(6.0);
                    ui.label(
                        RichText::new(body)
                            .font(theme::regular(11.5))
                            .color(p.text_muted),
                    );
                });
            });
        ui.add_space(20.0);
    });
    if !actions.is_empty() {
        pressed = wa_action_row(ui, textures, p, actions);
    }
    if let Some(footer) = footer {
        wa_e2e_footer(ui, textures, p, footer);
    }
    pressed
}

/// Centred icon + explanation for a tab that has no rows yet.
fn wa_empty(ui: &mut egui::Ui, p: &Palette, icon: Icon, title: &str, body: &str) {
    ui.add_space(54.0);
    ui.vertical_centered(|ui| {
        let (rect, _) = ui.allocate_exact_size(Vec2::splat(40.0), Sense::hover());
        icons::paint(
            ui.painter(),
            Rect::from_center_size(rect.center(), Vec2::splat(34.0)),
            icon,
            p.text_muted.gamma_multiply(0.6),
            1.5,
        );
        ui.add_space(10.0);
        ui.label(RichText::new(title).font(theme::medium(13.5)).color(p.text));
        ui.add_space(2.0);
        ui.label(
            RichText::new(body)
                .font(theme::regular(11.5))
                .color(p.text_muted),
        );
    });
}

/// One row in the Media sheet's Links tab.
fn wa_link_row(ui: &mut egui::Ui, p: &Palette, title: &str, body: &str, at: i64) -> bool {
    let width = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, 58.0), Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(rect, CornerRadius::ZERO, p.hover);
    }
    ui.painter().text(
        rect.left_top() + Vec2::new(16.0, 10.0),
        egui::Align2::LEFT_TOP,
        title,
        theme::medium(12.5),
        p.text,
    );
    ui.painter().text(
        rect.left_top() + Vec2::new(16.0, 30.0),
        egui::Align2::LEFT_TOP,
        body.chars().take(90).collect::<String>(),
        theme::regular(11.5),
        p.accent,
    );
    ui.painter().text(
        rect.right_top() + Vec2::new(-16.0, 10.0),
        egui::Align2::RIGHT_TOP,
        format::chat_time(at),
        theme::regular(10.5),
        p.text_muted,
    );
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp.clicked()
}

/// First http(s) URL in a message body, trimmed of trailing punctuation.
fn first_url(body: &str) -> Option<String> {
    let start = body.find("http://").or_else(|| body.find("https://"))?;
    let rest = &body[start..];
    let end = rest.find(|c: char| c.is_whitespace()).unwrap_or(rest.len());
    Some(
        rest[..end]
            .trim_end_matches(['.', ',', ')', ']'])
            .to_string(),
    )
}

// ------------------------------------------------------------------ Telegram

/// Round chromeless button with one of Telegram's own marks.
fn tg_button(
    ui: &mut egui::Ui,
    textures: &mut icons::TgTextures,
    icon: icons::TgIcon,
    diameter: f32,
    icon_size: f32,
    tint: Color32,
) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(diameter), Sense::click());
    if resp.hovered() {
        ui.painter()
            .circle_filled(rect.center(), diameter / 2.0, Color32::from_white_alpha(14));
    }
    textures.paint(
        ui.ctx(),
        ui.painter(),
        Rect::from_center_size(rect.center(), Vec2::splat(icon_size)),
        icon,
        tint,
    );
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

/// Telegram Web A's pill search field.
fn tg_search_box(
    ui: &mut egui::Ui,
    textures: &mut icons::TgTextures,
    p: &Palette,
    text: &mut String,
    hint: &str,
    width: f32,
) -> egui::Response {
    // One allocated pill, with the field placed inside it. A nested Frame let
    // the field's desired width push the header past the card's edge.
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, 40.0), Sense::click());
    ui.painter()
        .rect_filled(rect, CornerRadius::same(20), p.input_bg);
    textures.paint(
        ui.ctx(),
        ui.painter(),
        Rect::from_center_size(
            egui::pos2(rect.left() + 24.0, rect.center().y),
            Vec2::splat(20.0),
        ),
        icons::TgIcon::Search,
        p.text_muted,
    );
    let edit = Rect::from_min_max(
        egui::pos2(rect.left() + 42.0, rect.top() + 8.0),
        egui::pos2(rect.right() - 14.0, rect.bottom() - 8.0),
    );
    let field = ui.put(
        edit,
        egui::TextEdit::singleline(text)
            .hint_text(
                RichText::new(hint)
                    .color(p.text_muted)
                    .font(theme::regular(14.0)),
            )
            .frame(egui::Frame::NONE)
            .desired_width(edit.width())
            .font(theme::regular(14.0))
            .text_color(p.text),
    );
    if resp.clicked() {
        field.request_focus();
    }
    resp
}

/// One row in Telegram's hamburger drawer.
fn tg_menu_item(
    ui: &mut egui::Ui,
    textures: &mut icons::TgTextures,
    p: &Palette,
    icon: icons::TgIcon,
    label: &str,
    selected: bool,
) -> bool {
    let width = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, 32.0), Sense::click());
    if resp.hovered() {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(8), p.hover);
    }
    textures.paint(
        ui.ctx(),
        ui.painter(),
        Rect::from_center_size(
            egui::pos2(rect.left() + 18.0, rect.center().y),
            Vec2::splat(20.0),
        ),
        icon,
        if selected { p.accent } else { p.text_muted },
    );
    ui.painter().text(
        egui::pos2(rect.left() + 38.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        theme::regular(14.0),
        p.text,
    );
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp.clicked()
}

/// Drawer row with a switch on the right, for Night Mode.
fn tg_menu_toggle(
    ui: &mut egui::Ui,
    textures: &mut icons::TgTextures,
    p: &Palette,
    icon: icons::TgIcon,
    label: &str,
    value: bool,
) -> bool {
    let width = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, 32.0), Sense::click());
    if resp.hovered() {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(8), p.hover);
    }
    textures.paint(
        ui.ctx(),
        ui.painter(),
        Rect::from_center_size(
            egui::pos2(rect.left() + 18.0, rect.center().y),
            Vec2::splat(20.0),
        ),
        icon,
        p.text_muted,
    );
    ui.painter().text(
        egui::pos2(rect.left() + 38.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        theme::regular(14.0),
        p.text,
    );
    let track = Rect::from_center_size(
        egui::pos2(rect.right() - 22.0, rect.center().y),
        Vec2::new(34.0, 20.0),
    );
    ui.painter().rect_filled(
        track,
        CornerRadius::same(10),
        if value { p.accent } else { p.divider },
    );
    ui.painter().circle_filled(
        egui::pos2(
            if value {
                track.right() - 10.0
            } else {
                track.left() + 10.0
            },
            track.center().y,
        ),
        7.0,
        Color32::WHITE,
    );
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp.clicked()
}

/// Coloured round icon tile and two lines, the settings list's row.
fn tg_settings_row(
    ui: &mut egui::Ui,
    textures: &mut icons::TgTextures,
    p: &Palette,
    icon: icons::TgIcon,
    color: Color32,
    title: &str,
    subtitle: &str,
    selected: bool,
) -> bool {
    let width = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, 58.0), Sense::click());
    if selected || resp.hovered() {
        ui.painter().rect_filled(
            rect,
            CornerRadius::same(12),
            if selected { p.selected } else { p.hover },
        );
    }
    let tile = Rect::from_center_size(
        egui::pos2(rect.left() + 30.0, rect.center().y),
        Vec2::splat(38.0),
    );
    ui.painter().circle_filled(tile.center(), 19.0, color);
    textures.paint(
        ui.ctx(),
        ui.painter(),
        Rect::from_center_size(tile.center(), Vec2::splat(22.0)),
        icon,
        Color32::WHITE,
    );
    ui.painter().text(
        rect.left_top() + Vec2::new(56.0, 11.0),
        egui::Align2::LEFT_TOP,
        title,
        theme::medium(14.0),
        p.text,
    );
    ui.painter().text(
        rect.left_top() + Vec2::new(56.0, 31.0),
        egui::Align2::LEFT_TOP,
        subtitle,
        theme::regular(12.0),
        p.text_muted,
    );
    textures.paint(
        ui.ctx(),
        ui.painter(),
        Rect::from_center_size(
            egui::pos2(rect.right() - 16.0, rect.center().y),
            Vec2::splat(16.0),
        ),
        icons::TgIcon::ArrowRight,
        p.text_muted.gamma_multiply(0.7),
    );
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp.clicked()
}

/// Label/value line inside a Telegram settings detail card.
fn tg_fact_row(ui: &mut egui::Ui, p: &Palette, label: &str, value: &str) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 46.0), Sense::hover());
    ui.painter().text(
        rect.left_top() + Vec2::new(0.0, 7.0),
        egui::Align2::LEFT_TOP,
        label,
        theme::regular(11.0),
        p.text_muted,
    );
    ui.painter().text(
        rect.left_top() + Vec2::new(0.0, 23.0),
        egui::Align2::LEFT_TOP,
        value,
        theme::regular(14.0),
        p.text,
    );
}

/// Centred card used by Telegram's profile, contact and info panes.
fn tg_detail_frame(ui: &mut egui::Ui, p: &Palette, content: impl FnOnce(&mut egui::Ui)) {
    ScrollArea::vertical()
        .id_salt("tg-detail")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.add_space(40.0);
            ui.vertical_centered(|ui| {
                Frame::new()
                    .fill(p.header)
                    .corner_radius(14)
                    .inner_margin(Margin::symmetric(24, 22))
                    .show(ui, |ui| {
                        ui.set_width(320.0_f32.min(ui.available_width()));
                        content(ui);
                    });
            });
        });
}

fn tg_placeholder(ui: &mut egui::Ui, p: &Palette, text: &str) {
    ui.add_space(120.0);
    ui.vertical_centered(|ui| {
        ui.label(
            RichText::new(text)
                .font(theme::regular(13.0))
                .color(p.text_muted),
        );
    });
}

/// Telegram's blue pill button.
fn tg_pill_button(ui: &mut egui::Ui, p: &Palette, label: &str) -> bool {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(148.0, 36.0), Sense::click());
    ui.painter()
        .rect_filled(rect, CornerRadius::same(18), p.accent);
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        label,
        theme::medium(13.0),
        Color32::WHITE,
    );
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp.clicked()
}

/// Dim + click-away backdrop behind a modal sheet. Returns true on click.
fn modal_backdrop(ctx: &egui::Context) -> bool {
    let screen = ctx.content_rect();
    let mut clicked = false;
    egui::Area::new(egui::Id::new("wa-modal-dim"))
        .order(egui::Order::Middle)
        .fixed_pos(screen.min)
        .show(ctx, |ui| {
            let (rect, resp) = ui.allocate_exact_size(screen.size(), Sense::click());
            ui.painter()
                .rect_filled(rect, 0.0, Color32::from_black_alpha(120));
            clicked = resp.clicked();
        });
    clicked
}

// ------------------------------------------------------------------- Discord

/// Sample DMs for `UNITED_DC` screenshots, where no bot token is linked.
fn dc_demo_chats() -> Vec<Chat> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    [
        ("1", "Ryan", "heyheyryan", 0u32),
        ("2", "Rythm", "rythm.fm", 0),
        ("3", "Bewokz", "jote", 2),
        ("4", "Jockie Music", "m!help", 0),
        ("5", "sy4mei", "Online", 0),
    ]
    .into_iter()
    .enumerate()
    .map(|(n, (id, title, preview, unread))| Chat {
        id: id.to_string(),
        account: united_core::model::AccountId::new(Service::Discord, "me"),
        service: Service::Discord,
        title: title.to_string(),
        is_group: false,
        avatar: None,
        unread,
        muted: false,
        last_at: now - n as i64 * 600,
        preview: preview.to_string(),
        preview_sender: String::new(),
        target: id.to_string(),
    })
    .collect()
}

/// 48px guild tile with Discord's white pill indicator.
fn dc_guild_tile(
    ui: &mut egui::Ui,
    textures: &mut icons::DcTextures,
    p: &Palette,
    icon: icons::DcIcon,
    selected: bool,
) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(48.0), Sense::click());
    if selected {
        // The indicator hangs off the rail's left edge, so it is drawn from
        // the tile's position rather than from the allocated row.
        let pill = Rect::from_center_size(
            egui::pos2(rect.left() - 12.0, rect.center().y),
            Vec2::new(8.0, 32.0),
        );
        ui.painter()
            .rect_filled(pill, CornerRadius::same(4), Color32::WHITE);
        ui.painter()
            .rect_filled(rect, CornerRadius::same(16), p.accent);
    } else {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(16), p.hover);
    }
    textures.paint(
        ui.ctx(),
        ui.painter(),
        Rect::from_center_size(rect.center(), Vec2::splat(30.0)),
        icon,
        if selected {
            Color32::WHITE
        } else {
            p.text_muted
        },
    );
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

/// One nav row in Discord's DM sidebar (Friends, Nitro, Shop, Quests).
fn dc_nav_row(
    ui: &mut egui::Ui,
    textures: &mut icons::DcTextures,
    p: &Palette,
    icon: icons::DcIcon,
    label: &str,
    selected: bool,
) -> bool {
    let width = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, 38.0), Sense::click());
    if selected {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(8), p.selected);
    } else if resp.hovered() {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(8), p.hover);
    }
    textures.paint(
        ui.ctx(),
        ui.painter(),
        Rect::from_center_size(
            egui::pos2(rect.left() + 18.0, rect.center().y),
            Vec2::splat(20.0),
        ),
        icon,
        if selected { p.text } else { p.text_muted },
    );
    ui.painter().text(
        egui::pos2(rect.left() + 38.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        theme::medium(15.5),
        if selected { p.text } else { p.text_muted },
    );
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp.clicked()
}

/// Small round button for the user bar, with a slash when toggled off.
fn dc_voice_button(ui: &mut egui::Ui, icon: Icon, p: &Palette, active: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(32.0), Sense::click());
    if resp.hovered() {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(6), p.selected);
    }
    icons::paint(
        ui.painter(),
        Rect::from_center_size(rect.center(), Vec2::splat(20.0)),
        icon,
        if active { p.text } else { p.text_muted },
        1.8,
    );
    if !active {
        ui.painter().line_segment(
            [
                rect.center() + Vec2::new(-8.0, 8.0),
                rect.center() + Vec2::new(8.0, -8.0),
            ],
            Stroke::new(2.0, Color32::from_rgb(0xda, 0x37, 0x3c)),
        );
    }
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

/// Centred explanatory card for the rail's non-DM destinations.
fn dc_info_page(
    ui: &mut egui::Ui,
    textures: &mut icons::DcTextures,
    p: &Palette,
    section: DcSection,
) -> bool {
    let mut back = false;
    ui.add_space(80.0);
    ui.vertical_centered(|ui| {
        Frame::new()
            .fill(p.hover)
            .corner_radius(12)
            .inner_margin(Margin::symmetric(28, 26))
            .show(ui, |ui| {
                ui.set_width(340.0_f32.min(ui.available_width()));
                ui.vertical_centered(|ui| {
                    let (rect, _) = ui.allocate_exact_size(Vec2::splat(64.0), Sense::hover());
                    textures.paint(
                        ui.ctx(),
                        ui.painter(),
                        Rect::from_center_size(rect.center(), Vec2::splat(44.0)),
                        section.icon(),
                        p.text,
                    );
                    ui.add_space(12.0);
                    ui.label(
                        RichText::new(section.title())
                            .font(theme::semibold(18.0))
                            .color(p.text),
                    );
                    ui.add_space(8.0);
                    ui.label(
                        RichText::new(section.body())
                            .font(theme::regular(13.0))
                            .color(p.text_muted),
                    );
                    ui.add_space(18.0);
                    let (rect, resp) =
                        ui.allocate_exact_size(Vec2::new(180.0, 36.0), Sense::click());
                    ui.painter()
                        .rect_filled(rect, CornerRadius::same(8), p.accent);
                    ui.painter().text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        "Back to Friends",
                        theme::medium(13.5),
                        Color32::WHITE,
                    );
                    if resp.hovered() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                    back = resp.clicked();
                });
            });
    });
    back
}

/// Switch row with no leading mark, for settings United really stores.
fn tg_toggle_row(
    ui: &mut egui::Ui,
    p: &Palette,
    title: &str,
    detail: &str,
    value: &mut bool,
) -> bool {
    let width = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, 58.0), Sense::click());
    if resp.hovered() {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(8), p.hover);
    }
    ui.painter().text(
        rect.left_top() + Vec2::new(2.0, 12.0),
        egui::Align2::LEFT_TOP,
        title,
        theme::regular(14.0),
        p.text,
    );
    ui.painter().text(
        rect.left_top() + Vec2::new(2.0, 32.0),
        egui::Align2::LEFT_TOP,
        detail,
        theme::regular(11.5),
        p.text_muted,
    );
    let track = Rect::from_center_size(
        egui::pos2(rect.right() - 20.0, rect.center().y),
        Vec2::new(36.0, 20.0),
    );
    ui.painter().rect_filled(
        track,
        CornerRadius::same(10),
        if *value { p.accent } else { p.divider },
    );
    ui.painter().circle_filled(
        egui::pos2(
            if *value {
                track.right() - 10.0
            } else {
                track.left() + 10.0
            },
            track.center().y,
        ),
        7.0,
        Color32::WHITE,
    );
    if resp.clicked() {
        *value = !*value;
        return true;
    }
    false
}

/// Voice call painted over the chat, like WhatsApp Web's call screen.
struct CallUi {
    service: Service,
    chat_id: String,
    call_id: String,
    state: CallState,
    title: String,
    /// Set when media connected; the bar counts from here.
    connected: Option<Instant>,
}

pub struct UnitedApp {
    store: Arc<Store>,
    rx: tokio::sync::broadcast::Receiver<CoreEvent>,
    adapters: HashMap<Service, Arc<dyn Adapter>>,
    pub settings: AppSettings,
    pub mode: Mode,
    pub view: View,
    pub chats: Vec<Chat>,
    pub unified: Vec<Chat>,
    pub messages: HashMap<(String, String), Vec<Message>>,
    pub active_chat: HashMap<Service, (String, String)>,
    pub drafts: HashMap<(String, String), String>,
    pub search: String,
    /// Chat-list "Unread" pill, like WhatsApp and Telegram Web.
    unread_only: bool,
    /// WhatsApp Web's own rail inside the WhatsApp view. Telegram and Discord
    /// keep the plain chat list.
    wa_section: WaSection,
    wa_modal: Option<WaModal>,
    /// Calls tab rows, loaded when the tab is opened and after each call.
    wa_calls: Vec<united_core::CallEntry>,
    /// Channels tab rows, loaded on demand.
    wa_channels: Vec<Chat>,
    /// Media sheet contents, loaded when the sheet opens.
    wa_media: Vec<Message>,
    /// Messages carrying a URL, for the sheet's Links tab.
    wa_links: Vec<Message>,
    /// Media sheet tab: 0 media, 1 documents, 2 links.
    wa_media_tab: u8,
    /// Filter for the media sheet's search field.
    wa_media_query: String,
    /// Per-tab search text, cleared whenever the tab changes.
    wa_search: String,
    /// Which WhatsApp settings row is open, if any.
    wa_settings_page: Option<&'static str>,
    /// Dismissed the "Pilih notifikasi Anda" banner.
    wa_banner_hidden: bool,
    /// Exact WhatsApp rail bitmaps, decoded once per window.
    wa_textures: icons::WaTextures,
    /// Telegram Web A's drawer state and open pane.
    tg_section: TgSection,
    tg_settings_tab: TgSettingsTab,
    tg_drawer: bool,
    tg_more: bool,
    /// Address book loaded when Contacts opens, not queried every frame.
    tg_contacts: Vec<(String, String)>,
    tg_contact: Option<String>,
    /// Telegram glyph masks, decoded once per window.
    tg_textures: icons::TgTextures,
    /// Discord's own rail, sidebar page and Friends tab.
    dc_section: DcSection,
    /// Friends page tab: 0 All, 1 Online.
    dc_tab: u8,
    dc_muted: bool,
    dc_deafened: bool,
    /// Discord glyph masks, decoded once per window.
    dc_textures: icons::DcTextures,
    /// Set when the conversation header's search button is pressed; the chat
    /// list consumes it on the next frame to focus its field.
    search_focus: bool,
    pub statuses: HashMap<Service, (ServiceStatus, Option<String>)>,
    pub prompt: Option<(Service, AuthPrompt)>,
    pub prompt_buf: String,
    prompt_buf2: String,
    auth_errors: (
        std::sync::mpsc::Sender<String>,
        std::sync::mpsc::Receiver<String>,
    ),
    /// Decoded profile pictures, keyed by the cached file path.
    avatars: HashMap<String, Option<egui::TextureHandle>>,
    /// Cached on-disk path per `account|sender JID`; `None` means "asked, no
    /// picture", so the conversation does not query the store every frame.
    sender_avatars: HashMap<String, Option<String>>,
    /// Full-screen photo viewer: texture plus the file it came from, so the
    /// viewer can copy or save the original pixels.
    media_view: Option<(egui::TextureHandle, String)>,
    media_zoom: f32,
    media_pan: egui::Vec2,
    /// WhatsApp Web's "Select messages" mode for the open conversation.
    select_mode: bool,
    selected: std::collections::HashSet<String>,
    /// One-to-one voice call shown in the overlay: who, which call, and when
    /// media connected (for the duration clock).
    call: Option<CallUi>,
    /// Decoded photos and stickers, keyed by the downloaded file path.
    media_textures: HashMap<String, Option<egui::TextureHandle>>,
    /// Decoded protocol previews (video frames, sticker stills).
    media_thumbs: HashMap<String, Option<egui::TextureHandle>>,
    /// Resolved local path per message. Avoids a `stat` per media bubble on
    /// every frame, which is what made scrolling stutter.
    media_locals: HashMap<String, Option<String>>,
    /// Parsed descriptor per message. Long base64 thumbnails would otherwise
    /// be re-parsed as JSON on every frame.
    media_parsed: HashMap<String, Option<Arc<MediaRef>>>,
    /// Bumped per chat when a download lands, so the scroll signature changes
    /// without walking every message each frame.
    media_ready: HashMap<(String, String), u64>,
    /// Images decoded off the UI thread. Decoding a phone photo inside a frame
    /// stalls the window; the worker hands back pixels instead.
    decode_tx: std::sync::mpsc::Sender<(DecodeKind, String, DecodeSource)>,
    decode_rx: std::sync::mpsc::Receiver<DecodeDone>,
    /// Cache keys already queued for decoding.
    decoding: std::collections::HashSet<String>,
    /// Who is typing in a chat: `(name, recording, when seen)`.
    typing: HashMap<(String, String), (String, bool, Instant)>,
    /// Availability per chat: online plus the last time they were seen.
    presence: HashMap<(String, String), (bool, Option<i64>)>,
    /// Last typing notice sent per chat: when, and whether it said "typing".
    /// Keeps a keystroke per frame from becoming a stanza per frame.
    typing_sent: HashMap<(String, String), (Instant, bool)>,
    /// First unread message on open: `(account, chat, at, count)`. The store
    /// clears the counter immediately, so this snapshot drives the divider.
    unread_divider: Option<(String, String, i64, u32)>,
    /// Chat the user asked to jump to the bottom of.
    jump_bottom: Option<(String, String)>,
    /// The view follows the newest message until the user scrolls up.
    pinned_bottom: bool,
    /// Whether the last frame ended at the end of the transcript. Read before
    /// the scroll area exists, so a pinned view only asks for a jump when it
    /// actually fell behind instead of every frame.
    at_bottom: bool,
    /// `content - viewport` per conversation from the last painted frame.
    /// Feeding it back as the initial offset is what actually opens a chat at
    /// the newest message: `scroll_to_cursor` requests are dropped here.
    chat_bottom: HashMap<(String, String), f32>,
    /// Window focus from the last frame. A focused conversation counts as
    /// read, so it must not raise a desktop banner.
    window_focused: bool,
    /// Address-book names by id, so a group message can name its author and
    /// a mention token can become a person.
    contact_names: HashMap<String, String>,
    /// Contact deltas arrive one per person; rewriting titles and authors per
    /// delta made the first sync stutter. Debounce the repair.
    contacts_dirty: Option<Instant>,
    /// Roster per chat for the `@`-mention picker.
    members: HashMap<String, Vec<(String, String)>>,
    /// Last roster request per chat, so a failed fetch retries instead of
    /// being stuck forever.
    member_requests: HashMap<String, Instant>,
    members_tx: std::sync::mpsc::Sender<(String, Vec<(String, String)>)>,
    members_rx: std::sync::mpsc::Receiver<(String, Vec<(String, String)>)>,
    /// Highlighted row in the mention picker.
    mention_selected: usize,
    /// Draft text for which the picker was dismissed with Escape.
    mention_dismissed: Option<String>,
    /// Service whose backlog is landing right now, for the "syncing" pill.
    syncing: Option<(Service, Instant)>,
    /// Nothing was stored at launch, so the first history sync is a pairing
    /// backfill and worth a notice. On a populated account it is not.
    first_sync: bool,
    /// Set by chat upserts so the list is queried once per drain, not per row.
    lists_dirty: bool,
    /// Clipboard images resolved off the UI thread; X11 clipboard reads can
    /// block when the owner is slow.
    paste_tx: std::sync::mpsc::Sender<std::path::PathBuf>,
    paste_rx: std::sync::mpsc::Receiver<std::path::PathBuf>,
    /// Attachments already being downloaded, so scrolling cannot queue a
    /// hundred CDN requests for the same file.
    /// Downloads started per message with when they were attempted. Kept as a
    /// map, not a set: a failed or pre-connection attempt must be retried
    /// later instead of being remembered as "already tried" forever.
    media_requests: HashMap<String, Instant>,
    /// Chats whose protocol history was already requested this session.
    history_loaded: std::collections::HashSet<(String, String)>,
    /// Chats whose profile picture was already asked for this session.
    avatar_tried: std::collections::HashSet<(String, String)>,
    /// Group subjects resolved for placeholder names, plus the ones already
    /// asked for this session.
    title_tx: std::sync::mpsc::Sender<(String, String, String)>,
    title_rx: std::sync::mpsc::Receiver<(String, String, String)>,
    title_tried: std::collections::HashSet<(String, String)>,
    /// Group/conversation info pane docked on the right.
    info_open: bool,
    /// Message the composer is replying to; cleared when the chat changes
    /// or the reply is sent.
    reply_to: Option<Message>,
    /// Quoted rows older than the loaded window, looked up once.
    quote_cache: HashMap<String, Option<Message>>,
    /// Last "load older" request per chat, so scrolling to the top pages back
    /// instead of firing a request every frame.
    older_request: HashMap<(String, String), Instant>,
    /// Oldest timestamp loaded from SQLite per chat, for local paging.
    history_oldest: HashMap<(String, String), i64>,
    /// Chats whose local cache has no older row left.
    history_done: std::collections::HashSet<(String, String)>,
    /// Message to keep under the viewport after older rows are prepended.
    older_anchor: Option<String>,
    /// Pinned/archived flags, loaded with the chat lists.
    chat_flags: HashMap<(String, String), (bool, bool)>,
    show_archived: bool,
    /// In-chat search: query, results, and whether archived chats are shown.
    chat_search: String,
    chat_search_open: bool,
    chat_search_hits: Vec<Message>,
    /// Reactions per conversation, keyed by message id. Each entry is
    /// `(sender id, emoji)`; loaded with the transcript.
    reactions: HashMap<(String, String), HashMap<String, Vec<(String, String)>>>,
    /// Message being edited plus its draft text (edit dialog).
    edit_target: Option<(Message, String)>,
    /// Background notifier state. A minimized window may not paint, so the
    /// banner decision cannot wait for the next frame.
    notify_focus: Arc<AtomicBool>,
    notify_open: Arc<Mutex<Vec<(String, String)>>>,
    notify_enabled: Arc<AtomicBool>,
    emoji_open: bool,
    qr: Option<(String, egui::TextureHandle)>,
    pub toast: Option<(String, Instant)>,
    last_save: Instant,
    /// Something worth persisting changed since the last write. Saves used to
    /// run every five seconds whether or not anything had changed.
    settings_dirty: bool,
    started: Instant,
    last_title: String,
    shot_sent: bool,
    frames: u32,
    fps_mark: Instant,
    prof: [f64; 4],
}

impl UnitedApp {
    pub fn new(
        ctx: &egui::Context,
        store: Arc<Store>,
        bus: EventBus,
        adapters: HashMap<Service, Arc<dyn Adapter>>,
        settings: AppSettings,
    ) -> Self {
        let (members_tx, members_rx) = std::sync::mpsc::channel();
        let (title_tx, title_rx) = std::sync::mpsc::channel();
        let (paste_tx, paste_rx) = std::sync::mpsc::channel();
        // One decoding thread: photos are big and foveal, so a queue of small
        // jobs beats blocking the frame that requested them.
        let (decode_tx, job_rx) = std::sync::mpsc::channel::<(DecodeKind, String, DecodeSource)>();
        let (done_tx, decode_rx) = std::sync::mpsc::channel::<DecodeDone>();
        let repaint = ctx.clone();
        std::thread::spawn(move || {
            for (kind, key, source) in job_rx {
                let bytes = match source {
                    DecodeSource::File(path) => std::fs::read(path).ok(),
                };
                let pixels = bytes
                    .and_then(|bytes| image::load_from_memory(&bytes).ok())
                    .map(|picture| {
                        if kind == DecodeKind::Avatar {
                            // Group photos are often wide logos; crop the centre
                            // square instead of squeezing it into a circle.
                            let (w, h) = (picture.width(), picture.height());
                            let side = w.min(h).max(1);
                            let cropped = image::imageops::crop_imm(
                                &picture,
                                (w - side) / 2,
                                (h - side) / 2,
                                side,
                                side,
                            )
                            .to_image();
                            let resized = image::imageops::resize(
                                &cropped,
                                96,
                                96,
                                image::imageops::FilterType::Triangle,
                            );
                            return egui::ColorImage::from_rgba_unmultiplied(
                                [96, 96],
                                resized.as_raw(),
                            );
                        }
                        // Phone photos stay crisp at 1400 px but never allocate
                        // a 12-megapixel texture.
                        let picture = if picture.width() > 1400 || picture.height() > 1400 {
                            picture.thumbnail(1400, 1400)
                        } else {
                            picture
                        };
                        let rgba = picture.to_rgba8();
                        egui::ColorImage::from_rgba_unmultiplied(
                            [rgba.width() as usize, rgba.height() as usize],
                            rgba.as_raw(),
                        )
                    });
                if done_tx.send(DecodeDone { kind, key, pixels }).is_err() {
                    break;
                }
                // The texture is only attached on the UI thread; nudge it so
                // the placeholder is replaced without waiting for a stanza.
                repaint.request_repaint();
            }
        });
        let mode = match settings.theme {
            united_core::settings::Theme::Light => Mode::Light,
            united_core::settings::Theme::Dark => Mode::Dark,
            united_core::settings::Theme::System => detect_system_mode(ctx),
        };
        let rx = bus.subscribe();
        // Wake the window when an adapter publishes instead of polling on a
        // timer: an idle inbox should not burn a frame every 400 ms.
        {
            let ctx = ctx.clone();
            let mut wake = bus.subscribe();
            tokio::spawn(async move {
                while wake.recv().await.is_ok() {
                    ctx.request_repaint();
                }
            });
        }
        egui_extras::install_image_loaders(ctx);
        theme::install(ctx);
        apply_mode(ctx, mode);
        let view = settings
            .last_service
            .as_deref()
            .and_then(Service::from_id)
            .map(View::Service)
            .unwrap_or(View::Unified);

        // A device that has never stored anything shows the first sync; an
        // established account has thousands of rows and syncs in the
        // background silently.
        let first_sync = store.chats(None, 1).map(|c| c.is_empty()).unwrap_or(false);

        // Files downloaded before the path was stored (and by any earlier
        // build) are adopted instead of re-fetched: the phone no longer has
        // most of them, so a re-fetch would only fail.
        match store.link_local_media(&united_core::paths::service_dir("whatsapp").join("media")) {
            Ok(0) => {}
            Ok(n) => log::info!("linked {n} media file(s) already on disk"),
            Err(err) => log::warn!("media backfill failed: {err:#}"),
        }
        // Previews overwritten by an older history chunk before the upsert
        // learned to keep the newest one.
        match store.repair_previews() {
            Ok(0) => {}
            Ok(n) => log::info!("repaired {n} chat preview(s)"),
            Err(err) => log::warn!("preview repair failed: {err:#}"),
        }
        // Attachments are re-downloadable, so every launch trims the caches to
        // the same ceiling the adapters use after a download.
        for service in ["whatsapp", "telegram", "discord"] {
            united_core::paths::prune_media(
                &united_core::paths::service_dir(service).join("media"),
                united_core::paths::MEDIA_BUDGET,
            );
        }

        let notify_store = store.clone();
        let notify_enabled = Arc::new(AtomicBool::new(settings.desktop_notifications));
        let mut app = Self {
            store,
            rx,
            adapters,
            settings,
            mode,
            view,
            chats: Vec::new(),
            unified: Vec::new(),
            messages: HashMap::new(),
            active_chat: HashMap::new(),
            drafts: HashMap::new(),
            search: String::new(),
            unread_only: false,
            search_focus: false,
            wa_section: WaSection::Chats,
            wa_modal: None,
            wa_calls: Vec::new(),
            wa_channels: Vec::new(),
            wa_media: Vec::new(),
            wa_links: Vec::new(),
            wa_media_tab: 0,
            wa_media_query: String::new(),
            wa_search: String::new(),
            wa_settings_page: None,
            wa_banner_hidden: false,
            wa_textures: icons::WaTextures::default(),
            tg_section: TgSection::Chats,
            tg_settings_tab: TgSettingsTab::Account,
            tg_drawer: false,
            tg_more: false,
            tg_contacts: Vec::new(),
            tg_contact: None,
            tg_textures: icons::TgTextures::default(),
            dc_section: DcSection::Friends,
            dc_tab: 0,
            dc_muted: false,
            dc_deafened: false,
            dc_textures: icons::DcTextures::default(),
            statuses: HashMap::new(),
            prompt: None,
            prompt_buf: String::new(),
            prompt_buf2: String::new(),
            auth_errors: std::sync::mpsc::channel(),
            avatars: HashMap::new(),
            sender_avatars: HashMap::new(),
            media_view: None,
            media_zoom: 1.0,
            media_pan: egui::Vec2::ZERO,
            select_mode: false,
            call: None,
            selected: std::collections::HashSet::new(),
            media_textures: HashMap::new(),
            media_thumbs: HashMap::new(),
            media_locals: HashMap::new(),
            media_parsed: HashMap::new(),
            media_ready: HashMap::new(),
            decode_tx,
            decode_rx,
            decoding: std::collections::HashSet::new(),
            typing: HashMap::new(),
            presence: HashMap::new(),
            typing_sent: HashMap::new(),
            unread_divider: None,
            jump_bottom: None,
            pinned_bottom: true,
            at_bottom: true,
            chat_bottom: HashMap::new(),
            window_focused: true,
            contact_names: HashMap::new(),
            contacts_dirty: None,
            members: HashMap::new(),
            member_requests: HashMap::new(),
            members_tx: members_tx.clone(),
            members_rx,
            mention_selected: 0,
            mention_dismissed: None,
            syncing: None,
            first_sync,
            lists_dirty: false,
            paste_tx,
            paste_rx,
            media_requests: HashMap::new(),
            history_loaded: std::collections::HashSet::new(),
            avatar_tried: std::collections::HashSet::new(),
            title_tx: title_tx.clone(),
            title_rx,
            title_tried: std::collections::HashSet::new(),
            info_open: false,
            reply_to: None,
            quote_cache: HashMap::new(),
            older_request: HashMap::new(),
            history_oldest: HashMap::new(),
            history_done: std::collections::HashSet::new(),
            older_anchor: None,
            chat_flags: HashMap::new(),
            show_archived: false,
            chat_search: String::new(),
            chat_search_open: false,
            chat_search_hits: Vec::new(),
            reactions: HashMap::new(),
            edit_target: None,
            notify_focus: Arc::new(AtomicBool::new(true)),
            notify_open: Arc::new(Mutex::new(Vec::new())),
            notify_enabled,
            emoji_open: false,
            qr: None,
            toast: None,
            last_save: Instant::now(),
            settings_dirty: false,
            started: Instant::now(),
            last_title: String::new(),
            shot_sent: false,
            frames: 0,
            fps_mark: Instant::now(),
            prof: [0.0; 4],
        };
        // Replay whatever the adapters published before this window existed:
        // history sync, chat lists and the first auth prompts all land there.
        for event in bus.replay() {
            app.handle_event(ctx, event);
        }
        // Desktop banners run on their own subscription. The UI thread can be
        // asleep (minimized window, Wayland drops the redraw), and a message
        // must still notify before the next frame.
        {
            let store = notify_store;
            let focus = app.notify_focus.clone();
            let open = app.notify_open.clone();
            let enabled = app.notify_enabled.clone();
            let mut events = bus.subscribe();
            tokio::spawn(async move {
                while let Ok(event) = events.recv().await {
                    if !enabled.load(Ordering::Relaxed) {
                        continue;
                    }
                    match event {
                        CoreEvent::Call {
                            account,
                            chat_id,
                            state,
                            ..
                        } => {
                            if state != CallState::Incoming {
                                continue;
                            }
                            let title = store
                                .chat(&account, &chat_id)
                                .ok()
                                .flatten()
                                .map(|chat| chat.title)
                                .unwrap_or_else(|| chat_id.clone());
                            notify_banner(
                                &format!("{title} · panggilan masuk"),
                                "Ketuk Terima untuk menjawab",
                            );
                        }
                        CoreEvent::Message { message, live } => {
                            if !live || message.outgoing {
                                continue;
                            }
                            let open_now = focus.load(Ordering::Relaxed)
                                && open.lock().unwrap().iter().any(|(account, chat)| {
                                    account == &message.account.0 && chat == &message.chat_id
                                });
                            if open_now {
                                continue;
                            }
                            let chat = store
                                .chat(&message.account.0, &message.chat_id)
                                .ok()
                                .flatten();
                            if chat.as_ref().is_some_and(|chat| chat.muted) {
                                continue;
                            }
                            let title = chat
                                .map(|chat| chat.title)
                                .unwrap_or_else(|| message.sender_name.clone());
                            let body = if message.body.trim().is_empty() {
                                "Pesan baru".to_string()
                            } else {
                                message.body.clone()
                            };
                            notify_banner(&title, &body);
                        }
                        _ => {}
                    }
                }
            });
        }
        // Names synced on an earlier run are already on disk; fold them into
        // stored chat titles and sender names before the first list refresh.
        for account in ["whatsapp:me", "telegram:me", "discord:me"] {
            if let Ok(names) = app.store.contacts(account) {
                if !names.is_empty() {
                    let _ = app.store.repair_contact_names(account);
                }
                let _ = app.store.backfill_names(account);
                for (id, name) in names {
                    app.contact_names.insert(mention_key(&id), name);
                }
            }
        }
        app.refresh_lists();
        // Debug hook: UNITED_PASTE_TEST=1 reads the clipboard image once and
        // logs where it landed, without needing a connected account.
        if std::env::var("UNITED_PASTE_TEST").is_ok() {
            std::thread::spawn(|| match clipboard_image() {
                Some(path) => {
                    let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                    log::warn!("paste test: {} ({} bytes)", path.display(), size);
                }
                None => log::warn!("paste test: no image on clipboard"),
            });
        }
        // Debug hook: UNITED_FLOOD=N replays N history rows through the batch
        // path, to measure post-pairing responsiveness without a phone.
        if let Some(count) = std::env::var("UNITED_FLOOD")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
        {
            let started = Instant::now();
            let batch: Vec<(Message, bool)> = (0..count)
                .map(|i| {
                    (
                        Message {
                            id: format!("flood{i}"),
                            account: united_core::model::AccountId("whatsapp:me".into()),
                            chat_id: format!("flood{}@s.whatsapp.net", i % 40),
                            service: Service::WhatsApp,
                            sender_id: format!("62811{i}@s.whatsapp.net"),
                            sender_name: format!("User {}", i % 40),
                            body: format!("history row {i}"),
                            at: 1_700_000_000 + i as i64,
                            outgoing: false,
                            status: united_core::model::MessageStatus::Sent,
                            media: None,
                            reply_to: None,
                        },
                        false,
                    )
                })
                .collect();
            app.ingest_messages(batch);
            log::warn!("flood: {count} rows in {:?}", started.elapsed());
        }
        // Restore unsent drafts. The key carries the service so a Telegram and
        // a WhatsApp chat that happen to share an id cannot collide.
        for (spec, text) in app.settings.drafts.clone() {
            let mut parts = spec.splitn(3, '|');
            let (Some(_), Some(account), Some(chat_id)) =
                (parts.next(), parts.next(), parts.next())
            else {
                continue;
            };
            app.drafts
                .insert((account.to_string(), chat_id.to_string()), text);
        }
        // Adapters start before the window exists, so a QR or phone prompt can
        // be published before this receiver subscribes: seed it from the
        // adapter's own state instead of waiting for the next event.
        for (service, adapter) in &app.adapters {
            let Some(prompt) = adapter.prompt() else {
                continue;
            };
            if let AuthPrompt::Qr { payload } = &prompt {
                if !payload.is_empty() {
                    app.qr = qr::qr_texture(ctx, payload, "wa-qr")
                        .map(|texture| (payload.clone(), texture));
                }
            }
            app.statuses.insert(
                *service,
                (ServiceStatus::NeedsAuth, Some("Sign in required".into())),
            );
            app.prompt = Some((*service, prompt));
        }
        // Debug hook: UNITED_PROMPT=whatsapp shows the pairing sheet with a sample code.
        if std::env::var("UNITED_PROMPT").as_deref() == Ok("whatsapp") {
            app.prompt = Some((
                Service::WhatsApp,
                AuthPrompt::Qr {
                    payload: "https://wa.example/pair?code=UNITED-DEBUG".to_string(),
                },
            ));
        }
        // Debug hook: UNITED_PROMPT=discord shows the bot-token tutorial sheet.
        if std::env::var("UNITED_PROMPT").as_deref() == Ok("discord") {
            app.prompt = Some((Service::Discord, AuthPrompt::Token));
        }
        // Debug hook: UNITED_STATUS=online paints every service as connected.
        if std::env::var("UNITED_STATUS").as_deref() == Ok("online") {
            for service in Service::ALL {
                app.statuses
                    .insert(service, (ServiceStatus::Online, Some("debug".into())));
            }
        }
        // Debug hook: UNITED_VIEW=settings opens the settings pane on launch.
        if std::env::var("UNITED_VIEW").as_deref() == Ok("settings") {
            app.view = View::Settings;
        }
        // Debug hook: UNITED_WA_TAB=calls|status|channels|communities|business|
        // profile|media|advertise opens a WhatsApp tab for screenshots.
        if let Ok(tab) = std::env::var("UNITED_WA_TAB") {
            app.view = View::Service(Service::WhatsApp);
            match tab.as_str() {
                "media" | "advertise" => {
                    app.wa_media = app
                        .store
                        .media_messages(Service::WhatsApp, 120)
                        .unwrap_or_default();
                    app.wa_links = app
                        .store
                        .link_messages(Service::WhatsApp, 80)
                        .unwrap_or_default();
                    app.wa_modal = Some(if tab == "media" {
                        WaModal::Media
                    } else {
                        WaModal::Advertise
                    });
                }
                other => {
                    let section = match other {
                        "calls" => WaSection::Calls,
                        "status" => WaSection::Status,
                        "channels" => WaSection::Channels,
                        "communities" => WaSection::Communities,
                        "business" => WaSection::Business,
                        "profile" => WaSection::Profile,
                        "settings" => WaSection::Settings,
                        _ => WaSection::Chats,
                    };
                    app.wa_open(section);
                }
            }
        }
        // Debug hook: UNITED_OPEN_CHAT=account|chat opens a conversation on launch.
        if let Ok(spec) = std::env::var("UNITED_OPEN_CHAT") {
            if let Some((account, chat_id)) = spec.split_once('|') {
                if let Ok(Some(chat)) = app.store.chat(account, chat_id) {
                    app.open_chat(chat);
                }
            }
        } else if let Some([_, account, chat_id]) = &app.settings.last_chat
            && let Ok(Some(chat)) = app.store.chat(account, chat_id)
        {
            app.open_chat(chat);
        }
        // Debug hook: UNITED_INFO=1 docks the info pane for screenshots.
        if std::env::var("UNITED_INFO").is_ok() {
            app.info_open = true;
        }
        // Debug hook: UNITED_EMOJI=1 opens the composer picker for screenshots.
        if std::env::var("UNITED_EMOJI").is_ok() {
            app.emoji_open = true;
        }
        // Debug hook: UNITED_TG=drawer|more|contacts|settings|profile|info
        // opens a Telegram Web A pane for screenshots.
        if let Ok(tab) = std::env::var("UNITED_TG") {
            app.view = View::Service(Service::Telegram);
            match tab.as_str() {
                "drawer" => {
                    app.tg_section = TgSection::Chats;
                    app.tg_drawer = true;
                }
                "more" => {
                    app.tg_section = TgSection::Chats;
                    app.tg_drawer = true;
                    app.tg_more = true;
                }
                "contacts" => app.tg_open(TgSection::Contacts),
                "settings" => {
                    app.tg_open(TgSection::Settings);
                    app.tg_settings_tab = TgSettingsTab::General;
                }
                "account" => app.tg_open(TgSection::Settings),
                "profile" => app.tg_open(TgSection::Profile),
                "info" => app.tg_open(TgSection::Info(TgInfo::Features)),
                _ => app.tg_open(TgSection::Chats),
            }
        }
        // Debug hook: UNITED_DC=friends|nitro|shop|quests|addfriend|addserver|
        // discover opens a Discord pane for screenshots.
        if let Ok(tab) = std::env::var("UNITED_DC") {
            app.view = View::Service(Service::Discord);
            // Screenshots run without a bot token, so the sign-in sheet would
            // cover every pane.
            app.prompt = None;
            app.prompt_buf.clear();
            app.prompt_buf2.clear();
            app.dc_section = match tab.as_str() {
                "nitro" => DcSection::Nitro,
                "shop" => DcSection::Shop,
                "quests" => DcSection::Quests,
                "addfriend" => DcSection::AddFriend,
                "addserver" => DcSection::AddServer,
                "discover" => DcSection::Discover,
                "dm" => {
                    app.dc_section = DcSection::Chat;
                    // Demo rows only load in refresh_lists, so seed one here.
                    let chat = app
                        .chats
                        .iter()
                        .find(|c| c.service == Service::Discord)
                        .cloned()
                        .or_else(|| dc_demo_chats().into_iter().next());
                    if let Some(chat) = chat {
                        app.open_chat(chat);
                    }
                    return app;
                }
                _ => DcSection::Friends,
            };
        }
        app
    }

    fn refresh_lists(&mut self) {
        let service = match self.view {
            View::Service(s) => Some(s),
            _ => None,
        };
        self.chats = self.store.chats(service, 5000).unwrap_or_default();
        self.unified = self.store.chats(None, 5000).unwrap_or_default();
        self.chat_flags = self.store.chat_flags().unwrap_or_default();
        // Debug hook: UNITED_DC screenshots run without a linked bot, so a
        // later Chats event must not wipe the sample rows.
        if std::env::var("UNITED_DC").is_ok()
            && self.chats.iter().all(|c| c.service != Service::Discord)
        {
            self.chats.extend(dc_demo_chats());
        }
    }

    /// Patch one conversation in the loaded lists instead of re-reading
    /// thousands of rows. A live message only changes one preview; a full
    /// reload cost two queries per message burst and rebuilt every row.
    /// Unknown chats (first message, history sync) fall back to a reload.
    fn patch_chat(&mut self, chat: Chat) {
        let mut found = false;
        for list in [&mut self.chats, &mut self.unified] {
            if let Some(slot) = list
                .iter_mut()
                .find(|row| row.account.0 == chat.account.0 && row.id == chat.id)
            {
                *slot = chat.clone();
                found = true;
            }
        }
        if !found {
            self.lists_dirty = true;
        }
    }

    fn drain_events(&mut self, ctx: &egui::Context) {
        while let Ok((key, list)) = self.members_rx.try_recv() {
            log::debug!("roster {key}: {} member(s)", list.len());
            if list.is_empty() {
                // Empty means "not yet": allow another attempt later.
                self.member_requests.remove(&key);
            } else {
                self.members.insert(key, list);
            }
        }

        while let Ok((account, chat_id, title)) = self.title_rx.try_recv() {
            for list in [&mut self.chats, &mut self.unified] {
                for chat in list.iter_mut() {
                    if chat.id == chat_id && chat.account.0 == account {
                        chat.title = title.clone();
                    }
                }
            }
            if let Ok(Some(mut chat)) = self.store.chat(&account, &chat_id) {
                // The upsert guard ignores a placeholder that would replace a
                // real name; here the incoming one *is* the real name.
                if chat.title != title {
                    chat.title = title;
                    let _ = self.store.upsert_chat(&chat);
                }
            }
        }
        while let Ok(path) = self.paste_rx.try_recv() {
            self.send_path(path, true);
        }
        while let Ok(error) = self.auth_errors.1.try_recv() {
            self.toast = Some((error, Instant::now()));
        }
        let mut events = Vec::new();
        let mut more = false;
        loop {
            match self.rx.try_recv() {
                Ok(event) => {
                    events.push(event);
                    if events.len() >= 1200 {
                        more = true;
                        break;
                    }
                }
                Err(TryRecvError::Empty) | Err(TryRecvError::Closed) => break,
                Err(TryRecvError::Lagged(_)) => continue,
            }
        }
        let mut batch = Vec::new();
        for event in events {
            match event {
                CoreEvent::Message { message, live } => batch.push((message, live)),
                other => self.handle_event(ctx, other),
            }
        }
        self.ingest_messages(batch);
        self.drain_decodes(ctx);
        if self.lists_dirty {
            self.lists_dirty = false;
            self.refresh_lists();
        }
        if more {
            ctx.request_repaint();
        }
    }

    /// Attach textures the worker finished decoding. `ctx.load_texture` is
    /// cheap; the expensive part (JPEG/WebP decode and resize) already ran off
    /// the UI thread.
    fn drain_decodes(&mut self, ctx: &egui::Context) {
        while let Ok(done) = self.decode_rx.try_recv() {
            self.decoding.remove(&done.key);
            let texture = done.pixels.map(|pixels| {
                ctx.load_texture(done.key.clone(), pixels, egui::TextureOptions::LINEAR)
            });
            match done.kind {
                DecodeKind::Media => {
                    self.media_textures.insert(done.key, texture);
                    cap_cache(&mut self.media_textures, 240);
                }
                DecodeKind::Avatar => {
                    self.avatars.insert(done.key, texture);
                    cap_cache(&mut self.avatars, 320);
                }
            }
        }
    }

    fn handle_event(&mut self, ctx: &egui::Context, event: CoreEvent) {
        match event {
            CoreEvent::Status {
                service,
                status,
                detail,
                ..
            } => {
                let unlinked = status == ServiceStatus::SignedOut
                    && detail.as_deref() == Some("unlinked from phone");
                self.statuses.insert(service, (status, detail));
                if unlinked {
                    self.notify(format!(
                        "{} was unlinked. Press Link device to scan again.",
                        service.display_name()
                    ));
                }
                if status == ServiceStatus::Online {
                    // Sign-in succeeded: close the sheet and drop cached state.
                    if self.prompt.as_ref().map(|(s, _)| *s) == Some(service) {
                        self.prompt = None;
                        self.qr = None;
                        self.prompt_buf.clear();
                        self.prompt_buf2.clear();
                    }
                    self.refresh_lists();
                }
            }
            CoreEvent::Auth { service, prompt } => {
                // UNITED_DC screenshots run without a linked bot; the token
                // sheet would cover the pane under test.
                if service == Service::Discord && std::env::var("UNITED_DC").is_ok() {
                    return;
                }
                if let AuthPrompt::Qr { payload } = &prompt {
                    self.qr = qr::qr_texture(ctx, payload, "wa-qr").map(|t| (payload.clone(), t));
                } else {
                    self.qr = None;
                }
                self.prompt_buf.clear();
                self.prompt = Some((service, prompt));
            }
            CoreEvent::Chats { service, chats } => {
                let _ = self.store.upsert_chats(&chats);
                if let View::Service(active) = self.view {
                    if active == service {
                        // Goes through refresh_lists so the UNITED_DC sample
                        // rows survive a late Chats event.
                        self.refresh_lists();
                    }
                }
                self.unified = self.store.chats(None, 5000).unwrap_or_default();
            }
            CoreEvent::Contacts { account, rows } => {
                let _ = self.store.put_contacts(&account, &rows);
                for (id, name) in rows {
                    let key = mention_key(&id);
                    if !key.is_empty() {
                        self.contact_names.insert(key, name);
                    }
                }
                // Chats and group authors written before the phone's address
                // book arrived still carry the number; fix them once the
                // per-person deltas stop arriving.
                self.contacts_dirty = Some(Instant::now());
                ctx.request_repaint_after(Duration::from_millis(900));
            }
            CoreEvent::Avatar {
                account,
                service: _,
                chat_id,
                path,
            } => {
                let _ = self.store.set_chat_avatar(&account, &chat_id, &path);
                for list in [&mut self.chats, &mut self.unified] {
                    for chat in list.iter_mut() {
                        if chat.id == chat_id && chat.account.0 == account {
                            chat.avatar = Some(path.clone());
                        }
                    }
                }
            }
            CoreEvent::SenderAvatar {
                account,
                service: _,
                jid,
                path,
            } => {
                let _ = self.store.set_avatar(&account, &jid, &path);
                self.sender_avatars
                    .insert(format!("{account}|{jid}"), Some(path));
            }
            CoreEvent::ChatUpsert { chat } => {
                let _ = self.store.upsert_chat(&chat);
                self.patch_chat(chat);
            }
            // Replayed events (before the window existed) go through the same
            // batched path as live traffic.
            CoreEvent::Message { message, live } => {
                self.ingest_messages(vec![(message, live)]);
            }
            CoreEvent::MediaReady {
                account,
                chat_id,
                message_id,
                media,
            } => {
                // A silent failure here means the photo is on disk but the
                // row still points nowhere, so every restart re-downloads it.
                if let Err(err) =
                    self.store
                        .set_message_media(&account, &chat_id, &message_id, &media)
                {
                    log::warn!("media path not saved for {message_id}: {err:#}");
                }
                self.media_requests
                    .remove(&format!("{account}|{message_id}"));
                self.media_locals
                    .remove(&format!("{account}|{chat_id}|{message_id}"));
                self.media_parsed
                    .remove(&format!("{account}|{chat_id}|{message_id}"));
                *self
                    .media_ready
                    .entry((account.clone(), chat_id.clone()))
                    .or_default() += 1;
                if let Some(list) = self.messages.get_mut(&(account, chat_id)) {
                    for msg in list.iter_mut() {
                        if msg.id == message_id {
                            msg.media = Some(media.clone());
                        }
                    }
                }
            }
            CoreEvent::Read { account, chat_id } => {
                let _ = self.store.set_unread(&account, &chat_id, 0);
                for list in [&mut self.chats, &mut self.unified] {
                    if let Some(chat) = list
                        .iter_mut()
                        .find(|chat| chat.account.0 == account && chat.id == chat_id)
                    {
                        chat.unread = 0;
                    }
                }
            }
            // Delivery and read receipts. One stanza can cover a whole batch, so
            // the update is a single statement per id and the in-memory copy is
            // patched in place: a reload would rebuild the scroll position.
            CoreEvent::Receipt {
                account,
                chat_id,
                ids,
                status,
            } => {
                for id in &ids {
                    let _ = self.store.update_status(&account, &chat_id, id, status);
                }
                if let Some(list) = self.messages.get_mut(&(account, chat_id)) {
                    for msg in list.iter_mut() {
                        if msg.outgoing && ids.iter().any(|id| id == &msg.id) {
                            // Never downgrade blue ticks back to one check.
                            if status == united_core::model::MessageStatus::Read
                                || msg.status == united_core::model::MessageStatus::Sent
                            {
                                msg.status = status;
                            }
                        }
                    }
                }
            }
            CoreEvent::Typing {
                account,
                chat_id,
                who,
                recording,
                active,
            } => {
                let key = (account, chat_id);
                if active {
                    let name = self.name_of(&key.0, &who).unwrap_or_else(|| who.clone());
                    self.typing.insert(key, (name, recording, Instant::now()));
                } else {
                    self.typing.remove(&key);
                }
            }
            CoreEvent::Presence {
                account,
                chat_id,
                online,
                last_seen,
            } => {
                self.presence
                    .insert((account, chat_id), (online, last_seen));
            }
            CoreEvent::Call {
                account,
                chat_id,
                call_id,
                state,
            } => {
                let title = self
                    .chats
                    .iter()
                    .find(|chat| chat.id == chat_id)
                    .map(|chat| chat.title.clone())
                    .or_else(|| self.name_of(&account, &chat_id))
                    .unwrap_or_else(|| chat_id.clone());
                // Persist every state so the Calls tab shows history even
                // after the overlay is gone.
                let now = united_core::model::now_secs();
                let key = if call_id.is_empty() {
                    format!("{chat_id}:{now}")
                } else {
                    call_id.clone()
                };
                let _ = self.store.log_call(
                    &account,
                    &chat_id,
                    &key,
                    state == CallState::Ringing,
                    matches!(state, CallState::Connecting | CallState::Active),
                    state == CallState::Ended,
                    now,
                );
                if self.wa_section == WaSection::Calls {
                    self.wa_calls = self.store.calls("whatsapp:me", 200).unwrap_or_default();
                }
                match state {
                    CallState::Ended => {
                        let current = self.call.as_ref();
                        let ours = current.is_some_and(|call| {
                            call.call_id.is_empty() || call_id.is_empty() || call.call_id == call_id
                        });
                        if ours {
                            self.call = None;
                        }
                    }
                    _ => {
                        let connected = self
                            .call
                            .as_ref()
                            .filter(|call| call.call_id == call_id)
                            .and_then(|call| call.connected);
                        self.call = Some(CallUi {
                            service: Service::WhatsApp,
                            chat_id,
                            call_id,
                            state,
                            title,
                            connected: if state == CallState::Active {
                                connected.or(Some(Instant::now()))
                            } else {
                                connected
                            },
                        });
                    }
                }
            }
            CoreEvent::Reaction {
                account,
                chat_id,
                message_id,
                sender_id,
                emoji,
            } => {
                let _ =
                    self.store
                        .set_reaction(&account, &chat_id, &message_id, &sender_id, &emoji);
                let row = self
                    .reactions
                    .entry((account, chat_id))
                    .or_default()
                    .entry(message_id)
                    .or_default();
                row.retain(|(sender, _)| sender != &sender_id);
                if !emoji.is_empty() {
                    row.push((sender_id, emoji));
                }
            }
            CoreEvent::MessageEdit {
                account,
                chat_id,
                message_id,
                body,
            } => {
                let _ = self
                    .store
                    .set_message_body(&account, &chat_id, &message_id, &body);
                let mut at = None;
                if let Some(list) = self.messages.get_mut(&(account.clone(), chat_id.clone()))
                    && let Some(msg) = list.iter_mut().find(|msg| msg.id == message_id)
                {
                    at = Some(msg.at);
                    msg.body = body.clone();
                }
                // The list preview must follow an edit when it points at this
                // very message, exactly like WhatsApp Web.
                if let Ok(Some(mut chat)) = self.store.chat(&account, &chat_id)
                    && at.is_some_and(|at| chat.last_at == at)
                {
                    chat.preview = body;
                    let _ = self.store.upsert_chat(&chat);
                    self.patch_chat(chat);
                }
            }
            CoreEvent::MessageRevoke {
                account,
                chat_id,
                message_id,
            } => {
                const GONE: &str = "Pesan ini telah dihapus";
                let _ = self
                    .store
                    .scrub_message(&account, &chat_id, &message_id, GONE);
                self.media_locals
                    .remove(&format!("{account}|{chat_id}|{message_id}"));
                self.media_parsed
                    .remove(&format!("{account}|{chat_id}|{message_id}"));
                let mut at = None;
                if let Some(list) = self.messages.get_mut(&(account.clone(), chat_id.clone()))
                    && let Some(msg) = list.iter_mut().find(|msg| msg.id == message_id)
                {
                    at = Some(msg.at);
                    msg.body = GONE.to_string();
                    msg.media = None;
                }
                if let Ok(Some(mut chat)) = self.store.chat(&account, &chat_id)
                    && at.is_some_and(|at| chat.last_at == at)
                {
                    chat.preview = GONE.to_string();
                    let _ = self.store.upsert_chat(&chat);
                    self.patch_chat(chat);
                }
            }
        }
    }

    /// Persist a burst of messages in one transaction, then update the chat
    /// previews once per conversation. History sync can deliver thousands of
    /// rows after pairing; committing each one froze the window.
    fn ingest_messages(&mut self, batch: Vec<(Message, bool)>) {
        if batch.is_empty() {
            return;
        }
        let rows: Vec<Message> = batch.iter().map(|(m, _)| m.clone()).collect();
        let inserted = self.store.put_messages(&rows).unwrap_or_else(|err| {
            log::warn!("message batch insert failed: {err:#}");
            vec![false; rows.len()]
        });
        let mut touched: HashMap<(String, String), (Message, bool)> = HashMap::new();
        let mut updated_chats: Vec<Chat> = Vec::new();
        let mut any = false;
        let mut history: Option<Service> = None;
        for ((message, live), is_new) in batch.into_iter().zip(inserted) {
            if !live {
                history = Some(message.service);
            }
            if !is_new {
                continue;
            }
            any = true;
            let key = (message.account.0.clone(), message.chat_id.clone());
            // Keep the in-memory copy only for conversations already open:
            // history sync touches thousands of chats and the store has them.
            if let Some(list) = self.messages.get_mut(&key) {
                list.push(message.clone());
                // ponytail: keep the open conversation bounded; the store has
                // every row and `ensure_messages` re-reads on reopen. Swap in a
                // real pager if per-chat memory ever matters.
                if list.len() > 600 {
                    let excess = list.len() - 600;
                    list.drain(..excess);
                }
            }
            let entry = touched
                .entry(key)
                .or_insert_with(|| (message.clone(), live));
            if message.at >= entry.0.at {
                entry.0 = message.clone();
            }
            if live && !message.outgoing {
                entry.1 = true;
            }
            if live {
                self.request_media(&message);
            }
        }
        if !any {
            return;
        }
        for key in touched.keys() {
            if let Some(list) = self.messages.get_mut(key) {
                list.sort_by_key(|m| m.at);
            }
        }
        for ((account, chat_id), (message, live_out)) in touched {
            if let Ok(Some(mut chat)) = self.store.chat(&account, &chat_id) {
                let is_open = self
                    .active_chat
                    .get(&chat.service)
                    .map(|(a, c)| a == &account && c == &chat_id)
                    .unwrap_or(false);
                if message.at >= chat.last_at {
                    chat.last_at = message.at;
                    chat.preview = message.body.clone();
                    chat.preview_sender = if chat.is_group {
                        if message.outgoing {
                            "Anda".to_string()
                        } else {
                            message.sender_name.clone()
                        }
                    } else {
                        String::new()
                    };
                    if live_out && !is_open && !chat.muted {
                        chat.unread = chat.unread.saturating_add(1);
                    }
                }
                let _ = self.store.upsert_chat(&chat);
                updated_chats.push(chat);
            } else if !message.outgoing {
                let chat = Chat {
                    id: message.chat_id.clone(),
                    account: message.account.clone(),
                    service: message.service,
                    title: message.sender_name.clone(),
                    is_group: false,
                    avatar: None,
                    unread: if live_out { 1 } else { 0 },
                    muted: false,
                    last_at: message.at,
                    preview: message.body.clone(),
                    preview_sender: String::new(),
                    target: message.chat_id.clone(),
                };
                let _ = self.store.upsert_chat(&chat);
                updated_chats.push(chat);
            }
        }
        for chat in updated_chats {
            self.patch_chat(chat);
        }
        if let Some(service) = history {
            // Only a fresh device needs the notice. On an account that already
            // has chats this is background backfill; a pill that keeps coming
            // back for it is noise (and a repaint tax on every batch).
            if self.first_sync && self.started.elapsed().as_secs() < 300 {
                self.syncing = Some((service, Instant::now()));
            }
        }
    }

    /// Mirror the open conversations for the background notifier.
    fn sync_notify_open(&self) {
        let mut open = self.notify_open.lock().unwrap();
        open.clear();
        open.extend(self.active_chat.values().cloned());
    }

    pub fn ensure_messages(&mut self, chat: &Chat) {
        let key = (chat.account.0.clone(), chat.id.clone());
        if self.messages.contains_key(&key) {
            return;
        }
        let loaded = self
            .store
            .messages(&chat.account.0, &chat.id, 200)
            .unwrap_or_default();
        // Only the newest handful are fetched on open; older pages download
        // when the user scrolls back, keeping the first paint light.
        let pending: Vec<Message> = loaded
            .iter()
            .rev()
            .filter(|m| m.media.is_some())
            .take(8)
            .cloned()
            .collect();
        self.messages.insert(key.clone(), loaded);
        self.reactions.insert(
            key.clone(),
            self.store
                .reactions(&chat.account.0, &chat.id)
                .unwrap_or_default(),
        );
        if let Some(first) = self.messages.get(&key).and_then(|list| list.first()) {
            self.history_oldest.insert(key.clone(), first.at);
        }
        if self.messages.get(&key).is_some_and(|list| list.len() < 200) {
            self.history_done.insert(key.clone());
        }
        // A chat opens at its newest message, like every other client. Until
        // the content exists there is no offset to stick to, so the jump is
        // requested by key and applied on the first frame that renders it.
        self.jump_bottom = Some(key);
        for msg in &pending {
            self.request_media(msg);
        }
    }

    /// Page one older slab out of SQLite. Returns false when the local cache is
    /// exhausted, so the caller can fall back to the protocol.
    fn load_older_local(&mut self, chat: &Chat) -> bool {
        let key = (chat.account.0.clone(), chat.id.clone());
        if self.history_done.contains(&key) {
            return false;
        }
        let Some(before) = self.history_oldest.get(&key).copied() else {
            return false;
        };
        let page = self
            .store
            .messages_page(&chat.account.0, &chat.id, Some(before), 120)
            .unwrap_or_default();
        if page.is_empty() {
            self.history_done.insert(key);
            return false;
        }
        let oldest = page.first().map(|m| m.at).unwrap_or(before);
        self.history_oldest.insert(key.clone(), oldest);
        if page.len() < 120 {
            self.history_done.insert(key.clone());
        }
        // Keep the message that was at the top under the viewport: egui can
        // scroll a rect into place after the taller content is laid out.
        let anchor = self
            .messages
            .get(&key)
            .and_then(|list| list.first())
            .map(|m| m.id.clone());
        if let Some(list) = self.messages.get_mut(&key) {
            let mut merged = page;
            merged.append(list);
            *list = merged;
        }
        self.older_anchor = anchor;
        true
    }

    /// Jump to one stored hit: load a page ending at its timestamp and use the
    /// anchor mechanism to scroll it into view.
    fn open_message_at(&mut self, hit: &Message) {
        let key = (hit.account.0.clone(), hit.chat_id.clone());
        let page = self
            .store
            .messages_page(&hit.account.0, &hit.chat_id, Some(hit.at + 1), 200)
            .unwrap_or_default();
        if page.is_empty() {
            return;
        }
        if let Some(first) = page.first() {
            self.history_oldest.insert(key.clone(), first.at);
        }
        self.history_done.remove(&key);
        self.messages.insert(key, page);
        self.older_anchor = Some(hit.id.clone());
        self.pinned_bottom = false;
        self.at_bottom = false;
        self.chat_search_open = false;
        self.chat_search.clear();
        self.chat_search_hits.clear();
    }

    /// Ask the adapter to fetch one attachment in the background. `MediaReady`
    /// carries the local path back, so guards against duplicate work here.
    fn request_media(&mut self, message: &Message) {
        let Some(raw) = message.media.as_deref() else {
            return;
        };
        let Some(media) = MediaRef::parse(raw) else {
            return;
        };
        if media.local().is_some() {
            return;
        }
        // Skip while the service is still signing in: the media session IQ
        // fails with `not_connected` and the attempt would be wasted.
        if !matches!(
            self.status_of(message.service),
            ServiceStatus::Online | ServiceStatus::Connecting
        ) {
            return;
        }
        let key = format!("{}|{}", message.account.0, message.id);
        let fresh = |at: &Instant| at.elapsed() < Duration::from_secs(15);
        if self.media_requests.get(&key).is_some_and(fresh) {
            return;
        }
        self.media_requests.insert(key, Instant::now());
        let Some(adapter) = self.adapters.get(&message.service).cloned() else {
            return;
        };
        let message = message.clone();
        tokio::spawn(async move {
            if let Err(err) = adapter.download_media(&message).await {
                log::warn!("media download failed: {err:#}");
            }
        });
    }

    /// Local path of one stored descriptor, cached per message. Called from
    /// the scroll loop, so it must never touch the filesystem or hash a long
    /// base64 payload twice.
    fn media_local(&mut self, message: &Message) -> Option<String> {
        let key = message_key(message);
        if let Some(cached) = self.media_locals.get(&key) {
            return cached.clone();
        }
        let path = self
            .media_of(message)
            .and_then(|m| m.local().map(str::to_string));
        self.media_locals.insert(key, path.clone());
        path
    }

    /// Parsed media descriptor of one message, cached by message key. One JSON
    /// parse per message per session, not per frame.
    fn media_of(&mut self, message: &Message) -> Option<Arc<MediaRef>> {
        let key = message_key(message);
        if let Some(cached) = self.media_parsed.get(&key) {
            return cached.clone();
        }
        let parsed = message
            .media
            .as_deref()
            .and_then(MediaRef::parse)
            .map(Arc::new);
        self.media_parsed.insert(key, parsed.clone());
        parsed
    }

    /// Decode an embedded preview, either base64 from the protocol or a file
    /// written by the Telegram/WhatsApp downloader.
    ///
    /// Thumbnails are a few kilobytes, so they are decoded inline: queueing
    /// them made every bubble in a freshly opened chat pop from the 62 px
    /// placeholder to its real height for a few frames, which read as flicker.
    fn media_thumb(
        &mut self,
        ctx: &egui::Context,
        key: &str,
        raw: &str,
    ) -> Option<egui::TextureHandle> {
        if let Some(cached) = self.media_thumbs.get(key) {
            return cached.clone();
        }
        use base64::Engine as _;
        // Telegram stores a thumbnail file path; WhatsApp embeds base64.
        // JPEG base64 also starts with '/', so only trust an existing path.
        let bytes = if raw.starts_with('/') && std::path::Path::new(raw).exists() {
            std::fs::read(raw).ok()
        } else {
            base64::engine::general_purpose::STANDARD.decode(raw).ok()
        }?;
        let picture = image::load_from_memory(&bytes).ok()?;
        let rgba = picture.to_rgba8();
        let pixels = egui::ColorImage::from_rgba_unmultiplied(
            [rgba.width() as usize, rgba.height() as usize],
            rgba.as_raw(),
        );
        let texture =
            ctx.load_texture(format!("thumb:{key}"), pixels, egui::TextureOptions::LINEAR);
        self.media_thumbs
            .insert(key.to_string(), Some(texture.clone()));
        cap_cache(&mut self.media_thumbs, 240);
        Some(texture)
    }

    /// Decode one downloaded photo for the conversation view.
    fn media_texture(&mut self, _ctx: &egui::Context, path: &str) -> Option<egui::TextureHandle> {
        if let Some(cached) = self.media_textures.get(path) {
            return cached.clone();
        }
        if self.decoding.insert(path.to_string()) {
            let _ = self.decode_tx.send((
                DecodeKind::Media,
                path.to_string(),
                DecodeSource::File(path.to_string()),
            ));
        }
        None
    }

    /// Resolve a group that is still stored under `Group <id>`.
    fn resolve_title(&mut self, chat: &Chat) {
        let placeholder = {
            let id = chat.id.split('@').next().unwrap_or_default();
            let title = chat.title.trim();
            title == id || title == format!("Group {id}")
        };
        if !placeholder
            || !matches!(self.status_of(chat.service), ServiceStatus::Online)
            || !self
                .title_tried
                .insert((chat.account.0.clone(), chat.id.clone()))
        {
            return;
        }
        let Some(adapter) = self.adapters.get(&chat.service).cloned() else {
            return;
        };
        let account = chat.account.0.clone();
        let id = chat.id.clone();
        let tx = self.title_tx.clone();
        tokio::spawn(async move {
            if let Ok(Some(title)) = adapter.chat_title(&id).await {
                let _ = tx.send((account, id, title));
            }
        });
    }

    /// Ask the protocol for a chat's profile picture, once per session. Most
    /// chats never send a live message, so without this the list keeps
    /// initials for everyone the phone already knows about.
    fn preload_avatar(&mut self, chat: &Chat) {
        // The picture IQ fails with `not connected` while the socket is still
        // signing in, and the once-per-session guard would remember that.
        if !matches!(self.status_of(chat.service), ServiceStatus::Online) {
            return;
        }
        if chat.avatar.is_some()
            || !self
                .avatar_tried
                .insert((chat.account.0.clone(), chat.id.clone()))
        {
            return;
        }
        let Some(adapter) = self.adapters.get(&chat.service).cloned() else {
            return;
        };
        let id = chat.id.clone();
        tokio::spawn(async move {
            let _ = adapter.load_avatar(&id).await;
        });
    }

    pub fn open_chat(&mut self, chat: Chat) {
        self.info_open = false;
        self.reply_to = None;
        self.emoji_open = false;
        self.media_view = None;
        self.media_zoom = 1.0;
        self.media_pan = egui::Vec2::ZERO;
        self.select_mode = false;
        self.selected.clear();
        self.view = View::Service(chat.service);
        self.settings.last_service = Some(chat.service.id().to_string());
        self.settings.last_chat = Some([
            chat.service.id().to_string(),
            chat.account.0.clone(),
            chat.id.clone(),
        ]);
        self.settings_dirty = true;
        self.active_chat
            .insert(chat.service, (chat.account.0.clone(), chat.id.clone()));
        self.sync_notify_open();
        // Browsing dozens of conversations must not pin every transcript in
        // RAM; closed chats are re-read from SQLite when reopened.
        if self.messages.len() >= 24 {
            let open: Vec<_> = self.active_chat.values().cloned().collect();
            self.messages
                .retain(|(account, id), _| open.iter().any(|(a, c)| a == account && c == id));
        }
        self.ensure_messages(&chat);
        self.unread_divider = (chat.unread > 0)
            .then(|| {
                self.messages
                    .get(&(chat.account.0.clone(), chat.id.clone()))
                    .and_then(|list| list.iter().rev().take(chat.unread as usize).last())
                    .map(|first| {
                        (
                            chat.account.0.clone(),
                            chat.id.clone(),
                            first.at,
                            chat.unread,
                        )
                    })
            })
            .flatten();
        // Start at the newest message instead of restoring whatever offset the
        // previous conversation left behind.
        self.pinned_bottom = true;
        self.at_bottom = false;
        self.jump_bottom = Some((chat.account.0.clone(), chat.id.clone()));
        self.preload_avatar(&chat);
        self.resolve_title(&chat);
        // Debug hook: pre-fill the composer to check it grows instead of being
        // clipped by a long draft.
        if let Ok(text) = std::env::var("UNITED_DRAFT") {
            self.drafts
                .insert((chat.account.0.clone(), chat.id.clone()), text);
        }
        // Debug hook: UNITED_REPLY=1 quotes the newest text message so the
        // reply strip can be reviewed without a pointer.
        if std::env::var("UNITED_REPLY").is_ok() {
            if let Some(last) = self
                .messages
                .get(&(chat.account.0.clone(), chat.id.clone()))
                .and_then(|list| list.iter().rev().find(|m| !m.body.trim().is_empty()))
            {
                self.reply_to = Some(last.clone());
            }
        }
        let _ = self.store.set_unread(&chat.account.0, &chat.id, 0);
        if let Some(adapter) = self.adapters.get(&chat.service) {
            let adapter = adapter.clone();
            let target = chat.target.clone();
            let fetch_history = self
                .history_loaded
                .insert((chat.account.0.clone(), chat.id.clone()));
            let watch = !chat.is_group;
            tokio::spawn(async move {
                let _ = adapter.mark_read(&target).await;
                // Availability is only meaningful for a person, and only while
                // they are configured to share it.
                if watch {
                    let _ = adapter.watch_presence(&target).await;
                }
                // Protocols that can page backwards (Telegram, Discord) fill
                // the older half of the conversation on first open. WhatsApp
                // delivers it through history sync instead.
                if fetch_history {
                    let _ = adapter.load_history(&target, 50).await;
                }
            });
        }
        self.refresh_lists();
    }

    pub fn send_current(&mut self) {
        let View::Service(service) = self.view else {
            return;
        };
        let Some((account, chat_id)) = self.active_chat.get(&service).cloned() else {
            return;
        };
        let key = (account.clone(), chat_id.clone());
        let text = self.drafts.remove(&key).unwrap_or_default();
        self.settings
            .drafts
            .remove(&format!("{}|{}|{}", service.id(), account, chat_id));
        self.settings_dirty = true;
        let text = text.trim().to_string();
        if text.is_empty() {
            return;
        }
        let target = self
            .store
            .chat(&account, &chat_id)
            .ok()
            .flatten()
            .map(|c| c.target)
            .unwrap_or_else(|| chat_id.clone());
        let Some(adapter) = self.adapters.get(&service).cloned() else {
            self.notify("No native adapter for this service");
            return;
        };
        let reply = self.reply_to.take().filter(|msg| msg.chat_id == chat_id);
        tokio::spawn(async move {
            if let Err(err) = adapter.send_text(&target, &text, reply.as_ref()).await {
                log::warn!("send failed: {err:#}");
            }
            // The bubble replaced the draft, so the typing stop must go out or
            // the other phone shows "typing…" until it times out.
            let _ = adapter.typing(&target, false).await;
        });
    }

    /// Pick a file with the desktop portal and hand it to the adapter. The
    /// draft becomes the caption, matching how the web clients send media.
    fn attach(&mut self, media_only: bool) {
        if self.active_service().is_none() || self.active_chat.is_empty() {
            return;
        }
        let mut dialog = rfd::FileDialog::new();
        if media_only {
            dialog = dialog.add_filter(
                "Photos and videos",
                &["jpg", "jpeg", "png", "webp", "gif", "mp4", "mov", "mkv"],
            );
        }
        let Some(path) = dialog.pick_file() else {
            return;
        };
        self.send_path(path, media_only);
    }

    /// Upload one file to the open chat, using the draft as its caption.
    fn send_path(&mut self, path: std::path::PathBuf, media_only: bool) {
        let Some(service) = self.active_service() else {
            return;
        };
        let Some((account, chat_id)) = self.active_chat.get(&service).cloned() else {
            return;
        };
        let _ = media_only;
        let target = self
            .store
            .chat(&account, &chat_id)
            .ok()
            .flatten()
            .map(|c| c.target)
            .unwrap_or_else(|| chat_id.clone());
        let Some(adapter) = self.adapters.get(&service).cloned() else {
            self.notify("No native adapter for this service");
            return;
        };
        let caption = self
            .drafts
            .remove(&(account.clone(), chat_id.clone()))
            .unwrap_or_default();
        self.settings
            .drafts
            .remove(&format!("{}|{}|{}", service.id(), account, chat_id));
        self.settings_dirty = true;
        self.notify(format!(
            "Uploading {}…",
            path.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("attachment")
        ));
        let errors = self.auth_errors.0.clone();
        tokio::spawn(async move {
            if let Err(err) = adapter.send_file(&target, &path, &caption).await {
                log::warn!("attach failed: {err:#}");
                let _ = errors.send(format!("Upload failed: {err:#}"));
            }
        });
    }

    pub fn submit_auth(&mut self) {
        let Some((service, prompt)) = self.prompt.clone() else {
            return;
        };
        let Some(adapter) = self.adapters.get(&service).cloned() else {
            return;
        };
        let value = self.prompt_buf.trim().to_string();
        let input = match prompt {
            AuthPrompt::Qr { payload } if payload.is_empty() => AuthInput::Refresh,
            AuthPrompt::Qr { .. } => return,
            AuthPrompt::Phone => AuthInput::Phone(value),
            AuthPrompt::Code => AuthInput::Code(value),
            AuthPrompt::Password => AuthInput::Password(value),
            AuthPrompt::Token => AuthInput::Token(value),
            AuthPrompt::Credentials => {
                let api_hash = self.prompt_buf2.trim().to_string();
                match value.parse::<i32>() {
                    Ok(api_id) => AuthInput::ApiCredentials { api_id, api_hash },
                    Err(_) => {
                        self.toast = Some((
                            "api_id must be the numeric id from my.telegram.org".into(),
                            Instant::now(),
                        ));
                        return;
                    }
                }
            }
        };
        let errors = self.auth_errors.0.clone();
        tokio::spawn(async move {
            if let Err(err) = adapter.submit(input).await {
                log::warn!("auth submit failed: {err:#}");
                let _ = errors.send(format!("Sign in failed: {err}"));
            }
        });
        self.prompt_buf.clear();
        self.prompt_buf2.clear();
    }

    /// Desktop notification, off the UI thread: D-Bus calls must never stall a
    /// frame, and a missing notification daemon must not break the app.
    fn notify_desktop(&self, title: &str, body: &str) {
        if self.settings.desktop_notifications {
            notify_banner(title, body);
        }
    }

    /// Copy a downloaded photo's pixels to the clipboard. WhatsApp Web's
    /// right-click menu does the same on an image. arboard is tried first;
    /// GNOME's XWayland bridge often rejects it, so xclip gets the PNG.
    fn copy_image(&mut self, path: &str) {
        let Ok(picture) = image::open(path) else {
            self.notify("Gambar belum bisa dibaca");
            return;
        };
        let rgba = picture.into_rgba8();
        let (width, height) = (rgba.width() as usize, rgba.height() as usize);
        if let Ok(mut clipboard) = arboard::Clipboard::new()
            && clipboard
                .set_image(arboard::ImageData {
                    width,
                    height,
                    bytes: std::borrow::Cow::Owned(rgba.into_raw()),
                })
                .is_ok()
        {
            self.notify("Gambar disalin");
            return;
        }
        let Ok(png) = image::open(path) else {
            self.notify("Gambar belum bisa dibaca");
            return;
        };
        let mut bytes = Vec::new();
        if png
            .write_to(
                &mut std::io::Cursor::new(&mut bytes),
                image::ImageFormat::Png,
            )
            .is_err()
        {
            self.notify("Gambar belum bisa dibaca");
            return;
        }
        let Ok(mut child) = std::process::Command::new("xclip")
            .args(["-selection", "clipboard", "-t", "image/png", "-i"])
            .stdin(std::process::Stdio::piped())
            .spawn()
        else {
            self.notify("Clipboard tidak mendukung gambar");
            return;
        };
        use std::io::Write as _;
        let copied = child
            .stdin
            .take()
            .is_some_and(|mut stdin| stdin.write_all(&bytes).is_ok());
        let _ = child.wait();
        self.notify(if copied {
            "Gambar disalin"
        } else {
            "Clipboard tidak mendukung gambar"
        });
    }

    /// Save a downloaded photo or document where the user picks it, like
    /// WhatsApp Web's "Save image as…".
    fn save_media_as(&mut self, path: &str, name: &str) {
        let Some(target) = rfd::FileDialog::new()
            .set_file_name(if name.trim().is_empty() {
                "united-media"
            } else {
                name
            })
            .save_file()
        else {
            return;
        };
        match std::fs::copy(path, &target) {
            Ok(_) => self.notify("File disimpan"),
            Err(err) => self.notify(format!("Gagal menyimpan: {err}")),
        }
    }

    pub fn notify(&mut self, text: impl Into<String>) {
        self.toast = Some((text.into(), Instant::now()));
    }

    /// Lazily decode a cached profile picture into a texture.
    fn avatar_texture(
        &mut self,
        _ctx: &egui::Context,
        path: Option<&str>,
    ) -> Option<egui::TextureHandle> {
        let path = path?;
        if let Some(cached) = self.avatars.get(path) {
            return cached.clone();
        }
        if self.decoding.insert(path.to_string()) {
            let _ = self.decode_tx.send((
                DecodeKind::Avatar,
                path.to_string(),
                DecodeSource::File(path.to_string()),
            ));
        }
        None
    }

    /// Open the sign-in sheet for `service` from anywhere in the shell.
    /// Drop one account: revoke the session, forget cached chats and files.
    fn disconnect(&mut self, service: Service) {
        if let Some(adapter) = self.adapters.get(&service).cloned() {
            let errors = self.auth_errors.0.clone();
            tokio::spawn(async move {
                if let Err(err) = adapter.logout().await {
                    log::warn!("logout failed: {err:#}");
                    let _ = errors.send(format!("Disconnect failed: {err:#}"));
                }
            });
        }
        let _ = self.store.clear_service(service);
        let prefix = format!("{}:", service.id());
        self.messages
            .retain(|(account, _), _| !account.starts_with(&prefix));
        self.active_chat.remove(&service);
        self.sync_notify_open();
        self.statuses.insert(
            service,
            (ServiceStatus::SignedOut, Some("signed out".into())),
        );
        self.media_locals.clear();
        self.media_parsed.clear();
        self.media_ready.clear();
        self.media_textures.clear();
        self.media_thumbs.clear();
        self.media_requests.clear();
        self.sender_avatars.clear();
        self.refresh_lists();
        self.notify(format!("{} disconnected", service.display_name()));
    }

    fn begin_sign_in(&mut self, service: Service) {
        let prompt = self
            .adapters
            .get(&service)
            .and_then(|adapter| adapter.sign_in_prompt())
            .unwrap_or(match service {
                Service::Discord => AuthPrompt::Token,
                Service::Telegram => AuthPrompt::Phone,
                _ => AuthPrompt::Qr {
                    payload: String::new(),
                },
            });
        self.prompt = Some((service, prompt));
        self.prompt_buf.clear();
        self.prompt_buf2.clear();
        self.view = View::Service(service);
        self.settings.last_service = Some(service.id().to_string());
        self.settings_dirty = true;
        self.refresh_lists();
    }

    /// Cached on-disk path of one participant's profile picture. `None` is
    /// remembered so a chat with many messages asks the store only once.
    fn sender_avatar_path(&mut self, account: &str, sender: &str) -> Option<String> {
        if sender.is_empty() {
            return None;
        }
        let jid = normalize_sender(sender);
        let key = format!("{account}|{jid}");
        if let Some(cached) = self.sender_avatars.get(&key) {
            return cached.clone();
        }
        let path = self.store.avatar(account, &jid).ok().flatten();
        self.sender_avatars.insert(key, path.clone());
        path
    }

    fn palette(&self) -> Palette {
        theme::palette(self.mode)
    }

    pub fn active_service(&self) -> Option<Service> {
        match self.view {
            View::Service(s) => Some(s),
            View::Unified => None,
            View::Settings => None,
        }
    }

    fn active_chat_obj(&self) -> Option<Chat> {
        let service = self.active_service()?;
        let (account, chat_id) = self.active_chat.get(&service)?;
        // The chat list already holds every row; a SQLite round-trip per
        // frame was the single hottest query in the UI thread.
        self.unified
            .iter()
            .chain(self.chats.iter())
            .find(|c| c.service == service && &c.account.0 == account && &c.id == chat_id)
            .cloned()
            .or_else(|| self.store.chat(account, chat_id).ok().flatten())
    }

    fn status_of(&self, service: Service) -> ServiceStatus {
        self.statuses
            .get(&service)
            .map(|(s, _)| *s)
            .unwrap_or(ServiceStatus::SignedOut)
    }

    fn unread_by_service(&self) -> BTreeMap<String, u32> {
        let mut map = BTreeMap::new();
        for chat in &self.unified {
            *map.entry(chat.service.id().to_string()).or_insert(0) += chat.unread;
        }
        map
    }

    // ---------------------------------------------------------------- layout

    /// Custom title bar. Wayland compositors do not agree on server-side
    /// decorations, so the window draws its own minimise / maximise / close
    /// controls and keeps the X where people expect it.
    fn chrome(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let p = self.palette();
        egui::Panel::top("window-chrome")
            .exact_size(40.0)
            .frame(Frame::new().fill(if self.mode == theme::Mode::Dark {
                Color32::from_rgb(0x15, 0x15, 0x17)
            } else {
                Color32::from_rgb(0xec, 0xec, 0xef)
            }))
            .show(ui, |ui| {
                let painter = ui.painter().clone();
                let full = ui.max_rect();
                painter.hline(
                    full.x_range(),
                    full.bottom() - HAIRLINE / 2.0,
                    Stroke::new(HAIRLINE, p.divider),
                );
                painter.circle_filled(full.left_center() + Vec2::new(18.0, 0.0), 4.0, p.accent);
                painter.text(
                    full.left_center() + Vec2::new(30.0, 0.0),
                    egui::Align2::LEFT_CENTER,
                    "United",
                    theme::semibold(13.0),
                    p.text,
                );

                let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
                let btn = Vec2::new(46.0, full.height());
                let close = Rect::from_min_size(egui::pos2(full.right() - btn.x, full.top()), btn);
                let maximize =
                    Rect::from_min_size(egui::pos2(full.right() - btn.x * 2.0, full.top()), btn);
                let minimize =
                    Rect::from_min_size(egui::pos2(full.right() - btn.x * 3.0, full.top()), btn);

                let hover = |rect: Rect, danger: bool| {
                    if danger {
                        painter.rect_filled(
                            rect,
                            CornerRadius::same(0),
                            Color32::from_rgb(0xe8, 0x11, 0x23),
                        );
                    } else {
                        painter.rect_filled(rect, CornerRadius::same(0), p.hover);
                    }
                };

                let close_resp = ui.interact(close, egui::Id::new("win-close"), Sense::click());
                if close_resp.hovered() {
                    hover(close, true);
                }
                let ink = if close_resp.hovered() {
                    Color32::WHITE
                } else {
                    p.text
                };
                let c = close.center();
                let arm = 5.0;
                painter.line_segment(
                    [c + Vec2::new(-arm, -arm), c + Vec2::new(arm, arm)],
                    Stroke::new(1.4, ink),
                );
                painter.line_segment(
                    [c + Vec2::new(arm, -arm), c + Vec2::new(-arm, arm)],
                    Stroke::new(1.4, ink),
                );
                if close_resp.clicked() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }

                let max_resp = ui.interact(maximize, egui::Id::new("win-max"), Sense::click());
                if max_resp.hovered() {
                    hover(maximize, false);
                }
                let c = maximize.center();
                if maximized {
                    let a = Rect::from_center_size(c + Vec2::new(1.0, 1.0), Vec2::splat(9.0));
                    let b = Rect::from_center_size(c + Vec2::new(-1.0, -1.0), Vec2::splat(9.0));
                    painter.rect_stroke(
                        a,
                        CornerRadius::same(0),
                        Stroke::new(1.1, p.text),
                        egui::StrokeKind::Inside,
                    );
                    painter.rect_stroke(
                        b,
                        CornerRadius::same(0),
                        Stroke::new(1.1, p.text),
                        egui::StrokeKind::Inside,
                    );
                } else {
                    painter.rect_stroke(
                        Rect::from_center_size(c, Vec2::splat(10.0)),
                        CornerRadius::same(0),
                        Stroke::new(1.1, p.text),
                        egui::StrokeKind::Inside,
                    );
                }
                if max_resp.clicked() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
                }

                let min_resp = ui.interact(minimize, egui::Id::new("win-min"), Sense::click());
                if min_resp.hovered() {
                    hover(minimize, false);
                }
                let c = minimize.center();
                painter.hline(
                    (c.x - 5.0)..=(c.x + 5.0),
                    c.y + 0.5,
                    Stroke::new(1.2, p.text),
                );
                if min_resp.clicked() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                }

                // Drag the empty part of the bar; double-click toggles maximise.
                let drag =
                    Rect::from_min_max(full.min, egui::pos2(minimize.left() - 6.0, full.bottom()));
                let drag_resp =
                    ui.interact(drag, egui::Id::new("win-drag"), Sense::click_and_drag());
                if drag_resp.drag_started() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                }
                if drag_resp.double_clicked() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
                }
            });

        self.resize_edges(ui);
    }

    /// Borderless windows lose the compositor's resize handles; keep the hot
    /// zones at the edges and corners.
    fn resize_edges(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let r = ctx.content_rect();
        let edge = 5.0;
        let corner = 10.0;
        let zones = [
            (
                "res-w",
                Rect::from_min_max(
                    egui::pos2(r.left(), r.top() + 40.0),
                    egui::pos2(r.left() + edge, r.bottom() - corner),
                ),
                egui::ResizeDirection::West,
            ),
            (
                "res-e",
                Rect::from_min_max(
                    egui::pos2(r.right() - edge, r.top() + 40.0),
                    egui::pos2(r.right(), r.bottom() - corner),
                ),
                egui::ResizeDirection::East,
            ),
            (
                "res-s",
                Rect::from_min_max(
                    egui::pos2(r.left() + corner, r.bottom() - edge),
                    egui::pos2(r.right() - corner, r.bottom()),
                ),
                egui::ResizeDirection::South,
            ),
            (
                "res-sw",
                Rect::from_min_max(
                    egui::pos2(r.left(), r.bottom() - corner),
                    egui::pos2(r.left() + corner, r.bottom()),
                ),
                egui::ResizeDirection::SouthWest,
            ),
            (
                "res-se",
                Rect::from_min_max(egui::pos2(r.right() - corner, r.bottom() - corner), r.max),
                egui::ResizeDirection::SouthEast,
            ),
        ];
        for (id, rect, dir) in zones {
            let resp = ui.interact(rect, egui::Id::new(id), Sense::drag());
            if resp.hovered() {
                ctx.set_cursor_icon(match dir {
                    egui::ResizeDirection::West | egui::ResizeDirection::East => {
                        egui::CursorIcon::ResizeHorizontal
                    }
                    egui::ResizeDirection::South => egui::CursorIcon::ResizeVertical,
                    _ => egui::CursorIcon::ResizeNwSe,
                });
            }
            if resp.drag_started() {
                ctx.send_viewport_cmd(egui::ViewportCommand::BeginResize(dir));
            }
        }
    }

    fn rail(&mut self, ui: &mut egui::Ui) {
        let p = self.palette();
        egui::Panel::left("rail")
            .exact_size(76.0)
            .frame(Frame::new().fill(p.rail))
            .show(ui, |ui| {
                let full = ui.max_rect();
                ui.painter().vline(
                    full.right() - HAIRLINE / 2.0,
                    full.y_range(),
                    Stroke::new(HAIRLINE, p.divider),
                );
                ui.add_space(14.0);
                ui.vertical_centered(|ui| {
                    // Unified inbox: the Messages glyph.
                    let unread: u32 = self.unified.iter().map(|c| c.unread).sum();
                    let selected = self.view == View::Unified;
                    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(56.0), Sense::click());
                    let tile = Rect::from_center_size(rect.center(), Vec2::splat(44.0));
                    if selected {
                        ui.painter()
                            .rect_filled(tile, CornerRadius::same(11), p.selected);
                    } else if resp.hovered() {
                        ui.painter()
                            .rect_filled(tile, CornerRadius::same(11), p.hover);
                    }
                    icons::paint(
                        ui.painter(),
                        Rect::from_center_size(rect.center(), Vec2::splat(24.0)),
                        Icon::Bubbles,
                        if selected {
                            p.accent
                        } else {
                            p.text.gamma_multiply(0.75)
                        },
                        1.8,
                    );
                    if unread > 0 {
                        let pos = rect.center() + Vec2::new(15.0, -15.0);
                        ui.painter()
                            .circle_filled(pos, 7.0, Color32::from_rgb(0xff, 0x3b, 0x30));
                        ui.painter().text(
                            pos,
                            egui::Align2::CENTER_CENTER,
                            if unread > 9 {
                                "9+".to_string()
                            } else {
                                unread.to_string()
                            },
                            theme::semibold(8.5),
                            Color32::WHITE,
                        );
                    }
                    if resp.clicked() {
                        self.view = View::Unified;
                        self.settings.last_service = None;
                        self.settings_dirty = true;
                        self.refresh_lists();
                    }
                    resp.on_hover_text("All Messages");

                    ui.add_space(10.0);
                    let enabled: Vec<Service> = Service::ALL
                        .into_iter()
                        .filter(|s| self.settings.is_enabled(s.id()))
                        .collect();
                    let unread_map = self.unread_by_service();
                    let mut unlink: Option<Service> = None;
                    for service in enabled {
                        let count = *unread_map.get(service.id()).unwrap_or(&0);
                        let chip = widgets::service_chip(
                            ui,
                            service,
                            34.0,
                            self.view == View::Service(service),
                            count,
                            &p,
                        );
                        if chip.clicked() {
                            self.view = View::Service(service);
                            self.settings.last_service = Some(service.id().to_string());
                            self.settings_dirty = true;
                            self.refresh_lists();
                        }
                        chip.clone().on_hover_text(format!(
                            "{} · right-click to unlink",
                            service.display_name()
                        ));
                        // Debug hook: UNITED_CONTEXT=whatsapp opens the menu for
                        // a screenshot without a real right-click.
                        let forced = std::env::var("UNITED_CONTEXT")
                            .ok()
                            .filter(|value| value == service.id());
                        let menu = egui::Popup::context_menu(&chip);
                        if forced.is_some() {
                            log::debug!("context menu forced for {}", service.id());
                        }
                        let menu = if forced.is_some() {
                            menu.open_memory(Some(egui::containers::SetOpenCommand::Bool(true)))
                                .at_position(egui::pos2(140.0, 250.0))
                        } else {
                            menu
                        };
                        menu.show(|ui| {
                            if forced.is_some() {
                                log::debug!("context menu content drawn");
                            }
                            ui.set_min_width(190.0);
                            ui.label(
                                RichText::new(service.display_name())
                                    .font(theme::semibold(12.0))
                                    .color(p.text),
                            );
                            if ui.button("Unlink account").clicked() {
                                unlink = Some(service);
                                ui.close();
                            }
                        });
                        ui.add_space(2.0);
                    }
                    if let Some(service) = unlink {
                        self.disconnect(service);
                    }

                    let remaining = ui.available_height();
                    if remaining > 60.0 {
                        ui.add_space(remaining - 56.0);
                    }
                    let gear = icons::button(ui, Icon::Gear, 34.0, p.text_muted, p.hover);
                    if gear.clicked() {
                        self.view = View::Settings;
                    }
                    gear.on_hover_text("Settings");
                    ui.add_space(10.0);
                });
            });
    }

    /// WhatsApp Web's own rail. Media and Advertise are sheets; everything
    /// else swaps the list pane or the full-width page.
    fn wa_rail(&mut self, ui: &mut egui::Ui) {
        let p = theme::service_palette(self.mode, Service::WhatsApp);
        egui::Panel::left("wa-rail")
            .exact_size(64.0)
            .frame(Frame::new().fill(p.panel))
            .show(ui, |ui| {
                let full = ui.max_rect();
                ui.painter().vline(
                    full.right() - HAIRLINE / 2.0,
                    full.y_range(),
                    Stroke::new(HAIRLINE, p.divider),
                );
                ui.add_space(10.0);
                ui.vertical_centered(|ui| {
                    let unread: u32 = self.chats.iter().map(|c| c.unread).sum();
                    for section in [
                        WaSection::Chats,
                        WaSection::Calls,
                        WaSection::Status,
                        WaSection::Channels,
                        WaSection::Communities,
                    ] {
                        let badge = if section == WaSection::Chats {
                            unread
                        } else {
                            0
                        };
                        let Some(icon) = section.wa_icon() else {
                            continue;
                        };
                        let resp = wa_tile(
                            ui,
                            &mut self.wa_textures,
                            icon,
                            self.wa_section == section,
                            &p,
                            badge,
                        );
                        if resp.clicked() {
                            self.wa_open(section);
                        }
                        resp.on_hover_text(section.label());
                        ui.add_space(4.0);
                    }
                    ui.add_space(8.0);
                    let line = ui.max_rect();
                    ui.painter().hline(
                        egui::Rangef::new(line.left() + 14.0, line.right() - 14.0),
                        line.top(),
                        Stroke::new(HAIRLINE, p.divider),
                    );
                    ui.add_space(12.0);
                    let business = wa_tile(
                        ui,
                        &mut self.wa_textures,
                        icons::WaIcon::Store,
                        self.wa_section == WaSection::Business,
                        &p,
                        0,
                    );
                    if business.clicked() {
                        self.wa_open(WaSection::Business);
                    }
                    business.on_hover_text("Fitur bisnis");
                    ui.add_space(4.0);
                    let advertise = wa_tile(
                        ui,
                        &mut self.wa_textures,
                        icons::WaIcon::Advertise,
                        false,
                        &p,
                        0,
                    );
                    if advertise.clicked() {
                        self.wa_modal = Some(WaModal::Advertise);
                    }
                    advertise.on_hover_text("Iklankan");
                });
                ui.with_layout(egui::Layout::bottom_up(egui::Align::Center), |ui| {
                    ui.add_space(12.0);
                    let profile = self.wa_profile_tile(ui, &p);
                    if profile.clicked() {
                        self.wa_open(WaSection::Profile);
                    }
                    profile.on_hover_text("Profil");
                    ui.add_space(4.0);
                    let settings = wa_tile(
                        ui,
                        &mut self.wa_textures,
                        icons::WaIcon::Settings,
                        self.wa_section == WaSection::Settings,
                        &p,
                        0,
                    );
                    if settings.clicked() {
                        self.wa_open(WaSection::Settings);
                    }
                    settings.on_hover_text("Pengaturan");
                    ui.add_space(4.0);
                    let media = wa_tile(
                        ui,
                        &mut self.wa_textures,
                        icons::WaIcon::Media,
                        false,
                        &p,
                        0,
                    );
                    if media.clicked() {
                        self.wa_media = self
                            .store
                            .media_messages(Service::WhatsApp, 120)
                            .unwrap_or_default();
                        self.wa_links = self
                            .store
                            .link_messages(Service::WhatsApp, 80)
                            .unwrap_or_default();
                        self.wa_media_tab = 0;
                        self.wa_media_query.clear();
                        self.wa_modal = Some(WaModal::Media);
                    }
                    media.on_hover_text("Media");
                });
            });
    }

    /// Bottom rail tile: the account's own profile picture when the adapter
    /// has fetched it, otherwise a person mark.
    fn wa_profile_tile(&mut self, ui: &mut egui::Ui, p: &Palette) -> egui::Response {
        let (rect, resp) = ui.allocate_exact_size(Vec2::splat(40.0), Sense::click());
        if self.wa_section == WaSection::Profile {
            ui.painter()
                .rect_filled(rect, CornerRadius::same(10), p.selected);
        } else if resp.hovered() {
            ui.painter()
                .rect_filled(rect, CornerRadius::same(10), p.hover);
        }
        let path = self.sender_avatar_path("whatsapp:me", "me");
        let avatar = self.avatar_texture(ui.ctx(), path.as_deref());
        let avatar_rect = Rect::from_center_size(rect.center(), Vec2::splat(28.0));
        if let Some(texture) = avatar {
            egui::Image::new((texture.id(), Vec2::splat(28.0)))
                .corner_radius(14)
                .paint_at(ui, avatar_rect);
        } else {
            ui.painter()
                .circle_filled(avatar_rect.center(), 14.0, p.input_bg);
            icons::paint(
                ui.painter(),
                Rect::from_center_size(avatar_rect.center(), Vec2::splat(18.0)),
                Icon::Person,
                p.text_muted,
                1.5,
            );
        }
        if resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        resp
    }

    fn wa_open(&mut self, section: WaSection) {
        self.wa_section = section;
        self.wa_search.clear();
        self.wa_settings_page = None;
        self.search.clear();
        self.unread_only = false;
        match section {
            WaSection::Calls => {
                self.wa_calls = self.store.calls("whatsapp:me", 200).unwrap_or_default();
            }
            WaSection::Channels => {
                self.wa_channels = self
                    .store
                    .channels(Service::WhatsApp, 500)
                    .unwrap_or_default();
            }
            _ => {}
        }
    }

    /// List pane for the non-chat WhatsApp tabs. Chat keeps the existing list
    /// so search, filters, pinned rows and archived chats stay untouched.
    fn wa_pane(&mut self, ui: &mut egui::Ui, p: &Palette) {
        match self.wa_section {
            WaSection::Calls => self.wa_calls_pane(ui, p),
            WaSection::Status => self.wa_status_pane(ui, p),
            WaSection::Channels => self.wa_channels_pane(ui, p),
            WaSection::Communities => self.wa_communities_pane(ui, p),
            WaSection::Settings => self.wa_settings_pane(ui, p),
            _ => {}
        }
    }

    /// Business tools split into a 340px menu and a centred detail card; the
    /// profile is a full-width page with its own top bar.
    fn wa_page(&mut self, ui: &mut egui::Ui) {
        let p = theme::service_palette(self.mode, Service::WhatsApp);
        match self.wa_section {
            WaSection::Business => {
                egui::Panel::left("wa-business-list")
                    .exact_size(340.0)
                    .resizable(false)
                    .frame(Frame::new().fill(p.panel))
                    .show(ui, |ui| {
                        let full = ui.max_rect();
                        ui.painter().vline(
                            full.right() - HAIRLINE / 2.0,
                            full.y_range(),
                            Stroke::new(HAIRLINE, p.divider),
                        );
                        self.wa_business_pane(ui, &p);
                    });
                egui::CentralPanel::default()
                    .frame(Frame::new().fill(p.chat_bg))
                    .show(ui, |ui| {
                        ui.add_space(140.0);
                        ui.vertical_centered(|ui| {
                            let (rect, _) =
                                ui.allocate_exact_size(Vec2::splat(64.0), Sense::hover());
                            self.wa_textures.paint(
                                ui.ctx(),
                                ui.painter(),
                                Rect::from_center_size(rect.center(), Vec2::splat(56.0)),
                                icons::WaIcon::Store,
                                p.text_muted.gamma_multiply(0.55),
                            );
                            ui.add_space(12.0);
                            ui.label(
                                RichText::new("Alat bisnis")
                                    .font(theme::semibold(20.0))
                                    .color(p.text),
                            );
                            ui.add_space(6.0);
                            ui.label(
                                RichText::new(
                                    "Kelola bisnis dan jangkau lebih banyak pelanggan, semua \
                                     di satu tempat.",
                                )
                                .font(theme::regular(12.0))
                                .color(p.text_muted),
                            );
                        });
                    });
            }
            WaSection::Profile => {
                egui::CentralPanel::default()
                    .frame(Frame::new().fill(p.panel))
                    .show(ui, |ui| self.wa_profile_pane(ui, &p));
            }
            _ => {}
        }
    }

    /// Right-hand detail for every WhatsApp tab that keeps the list column.
    fn wa_detail(&mut self, ui: &mut egui::Ui) {
        let p = theme::service_palette(self.mode, Service::WhatsApp);
        let section = self.wa_section;
        let mut pressed = None;
        egui::CentralPanel::default()
            .frame(Frame::new().fill(p.chat_bg))
            .show(ui, |ui| {
                pressed = match section {
                    WaSection::Calls => wa_detail_card(
                        ui,
                        &mut self.wa_textures,
                        &p,
                        icons::WaIcon::LaptopCalling,
                        "Telepon suara dan video",
                        "Bagikan layar, beri reaksi dengan emoji, dan banyak lagi, dengan \
                         maksimum 32 orang.",
                        &[
                            (icons::WaIcon::VideoCall, "Mulai telepon"),
                            (icons::WaIcon::Link, "Tautan menelepon baru"),
                            (icons::WaIcon::Dialpad, "Telepon nomor"),
                        ],
                        Some("Telepon pribadi Anda terenkripsi secara end-to-end"),
                    ),
                    WaSection::Status => wa_detail_card(
                        ui,
                        &mut self.wa_textures,
                        &p,
                        icons::WaIcon::Status,
                        "Bagikan status",
                        "Bagikan foto, video, dan teks yang hilang setelah 24 jam.",
                        &[
                            (icons::WaIcon::Edit, "Status teks"),
                            (icons::WaIcon::Camera, "Foto dan video"),
                        ],
                        None,
                    ),
                    WaSection::Channels => wa_detail_card(
                        ui,
                        &mut self.wa_textures,
                        &p,
                        icons::WaIcon::Channels,
                        "Temukan saluran",
                        "Hiburan, olahraga, berita, gaya hidup, toko, dan lainnya. Ikuti \
                         saluran yang Anda minati.",
                        &[
                            (icons::WaIcon::Add, "Buat saluran"),
                            (icons::WaIcon::Grid, "Jelajahi saluran"),
                        ],
                        None,
                    ),
                    WaSection::Communities => wa_detail_card(
                        ui,
                        &mut self.wa_textures,
                        &p,
                        icons::WaIcon::Communities,
                        "Buat komunitas",
                        "Kumpulkan anggota dalam grup-grup berdasarkan topik dan kirim \
                         pengumuman admin dengan mudah.",
                        &[],
                        Some("Pesan pribadi Anda di komunitas terenkripsi secara end-to-end"),
                    ),
                    WaSection::Settings => {
                        self.wa_settings_detail(ui, &p);
                        None
                    }
                    _ => {
                        let icon = section.wa_icon().unwrap_or(icons::WaIcon::AccountCircle);
                        ui.add_space(150.0);
                        ui.vertical_centered(|ui| {
                            let (rect, _) =
                                ui.allocate_exact_size(Vec2::splat(64.0), Sense::hover());
                            self.wa_textures.paint(
                                ui.ctx(),
                                ui.painter(),
                                Rect::from_center_size(rect.center(), Vec2::splat(56.0)),
                                icon,
                                p.text_muted.gamma_multiply(0.5),
                            );
                            ui.add_space(14.0);
                            ui.label(
                                RichText::new(section.label())
                                    .font(theme::semibold(20.0))
                                    .color(p.text),
                            );
                        });
                        None
                    }
                };
            });
        match (section, pressed) {
            (WaSection::Calls, Some(0)) => self.notify("Pilih obrolan untuk memulai panggilan."),
            (WaSection::Calls, Some(1)) => self.notify("Tautan panggilan dibuat dari ponsel Anda."),
            (WaSection::Calls, Some(_)) => self.notify("Telepon nomor dari ponsel Anda."),
            (WaSection::Status, Some(0)) => self.notify("Status teks dibuat dari ponsel."),
            (WaSection::Status, Some(_)) => {
                self.notify("Pilih foto atau video status dari ponsel.")
            }
            (WaSection::Channels, Some(0)) => self.notify("Buat saluran dari ponsel Anda."),
            (WaSection::Channels, Some(_)) => self.notify("Jelajahi saluran dari ponsel Anda."),
            _ => {}
        }
    }

    fn wa_calls_pane(&mut self, ui: &mut egui::Ui, p: &Palette) {
        let header = wa_header(
            ui,
            &mut self.wa_textures,
            p,
            "Panggilan",
            &[icons::WaIcon::AddCall, icons::WaIcon::More],
        );
        if header == Some(0) {
            self.notify("Mulai panggilan dari sebuah obrolan.");
        }
        Frame::new()
            .inner_margin(Margin {
                left: 16,
                right: 16,
                top: 0,
                bottom: 10,
            })
            .show(ui, |ui| {
                wa_search_box(
                    ui,
                    &mut self.wa_textures,
                    p,
                    &mut self.wa_search,
                    "Cari nama, nomor, atau @namapengguna",
                );
            });
        let query = self.wa_search.trim().to_lowercase();
        let rows = self.wa_calls.clone();
        let mut open = None;
        ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if wa_section_row(ui, p, "Favorit", Some("Lihat semua")) {
                    self.notify("Favorit dikelola dari ponsel Anda.");
                }
                let own = self.wa_account_label();
                let path = self.sender_avatar_path("whatsapp:me", "me");
                let picture = self.avatar_texture(ui.ctx(), path.as_deref());
                let width = ui.available_width();
                let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, 62.0), Sense::click());
                if resp.hovered() {
                    ui.painter().rect_filled(rect, CornerRadius::ZERO, p.hover);
                }
                wa_avatar(
                    ui,
                    p,
                    egui::pos2(rect.left() + 40.0, rect.center().y),
                    44.0,
                    &own,
                    picture.as_ref(),
                    None,
                );
                ui.painter().text(
                    egui::pos2(rect.left() + 74.0, rect.center().y),
                    egui::Align2::LEFT_CENTER,
                    format!("{} (Anda)", format_phone(&own)),
                    theme::regular(14.0),
                    p.text,
                );
                if resp.clicked() {
                    self.notify("Ini nomor WhatsApp Anda.");
                }
                wa_section_row(ui, p, "Terbaru", None);
                let mut shown = 0usize;
                for entry in rows {
                    let known = self.chats.iter().find(|c| c.id == entry.chat_id).cloned();
                    let title = known.as_ref().map(|c| c.title.clone()).unwrap_or_else(|| {
                        entry
                            .chat_id
                            .split('@')
                            .next()
                            .unwrap_or(&entry.chat_id)
                            .to_string()
                    });
                    if !query.is_empty() && !title.to_lowercase().contains(&query) {
                        continue;
                    }
                    shown += 1;
                    let outgoing = entry.outgoing;
                    let answered = entry.answered;
                    let at = entry.at;
                    let chat = Chat {
                        id: entry.chat_id.clone(),
                        account: united_core::AccountId("whatsapp:me".into()),
                        service: Service::WhatsApp,
                        title: title.clone(),
                        is_group: known.as_ref().is_some_and(|c| c.is_group),
                        avatar: known.as_ref().and_then(|c| c.avatar.clone()),
                        unread: 0,
                        muted: false,
                        last_at: at,
                        preview: String::new(),
                        preview_sender: String::new(),
                        target: entry.chat_id.clone(),
                    };
                    self.preload_avatar(&chat);
                    let picture = self.avatar_texture(ui.ctx(), chat.avatar.as_deref());
                    let time = format::chat_time(at);
                    if wa_call_row(
                        ui,
                        &mut self.wa_textures,
                        p,
                        &title,
                        &time,
                        picture.as_ref(),
                        outgoing,
                        answered,
                    )
                    .clicked()
                    {
                        open = Some(chat);
                    }
                }
                if shown == 0 {
                    ui.add_space(14.0);
                    Frame::new()
                        .inner_margin(Margin::symmetric(16, 0))
                        .show(ui, |ui| {
                            ui.label(
                                RichText::new(if query.is_empty() {
                                    "Belum ada panggilan"
                                } else {
                                    "Tidak ada panggilan yang cocok"
                                })
                                .font(theme::regular(12.0))
                                .color(p.text_muted),
                            );
                        });
                }
            });
        if let Some(chat) = open {
            self.open_chat(chat);
            self.wa_section = WaSection::Chats;
            self.wa_search.clear();
        }
    }

    fn wa_status_pane(&mut self, ui: &mut egui::Ui, p: &Palette) {
        let header = wa_header(
            ui,
            &mut self.wa_textures,
            p,
            "Status",
            &[icons::WaIcon::AddCircle, icons::WaIcon::More],
        );
        if header == Some(0) {
            self.notify("Pembaruan status dibuat dari ponsel Anda.");
        }
        let width = ui.available_width();
        let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, 68.0), Sense::click());
        if resp.hovered() {
            ui.painter().rect_filled(rect, CornerRadius::ZERO, p.hover);
        }
        let path = self.sender_avatar_path("whatsapp:me", "me");
        let picture = self.avatar_texture(ui.ctx(), path.as_deref());
        let own = self.wa_account_label();
        wa_avatar(
            ui,
            p,
            egui::pos2(rect.left() + 40.0, rect.center().y),
            44.0,
            &own,
            picture.as_ref(),
            None,
        );
        let badge = egui::pos2(rect.left() + 58.0, rect.center().y + 15.0);
        ui.painter()
            .circle_filled(badge, 9.0, Color32::from_rgb(0x25, 0xd3, 0x66));
        ui.painter().text(
            badge,
            egui::Align2::CENTER_CENTER,
            "+",
            theme::semibold(12.0),
            Color32::WHITE,
        );
        ui.painter().text(
            egui::pos2(rect.left() + 74.0, rect.top() + 24.0),
            egui::Align2::LEFT_CENTER,
            "Status saya",
            theme::medium(14.0),
            p.text,
        );
        ui.painter().text(
            egui::pos2(rect.left() + 74.0, rect.top() + 44.0),
            egui::Align2::LEFT_CENTER,
            "Klik untuk membuat pembaruan status",
            theme::regular(11.5),
            p.text_muted,
        );
        if resp.clicked() {
            self.notify("Status baru dibuat dari ponsel Anda.");
        }
        wa_section_row(ui, p, "TERBARU", None);
        ui.add_space(18.0);
        Frame::new()
            .inner_margin(Margin::symmetric(16, 0))
            .show(ui, |ui| {
                ui.label(
                    RichText::new(
                        "Pembaruan status dari kontak Anda akan muncul di sini. Status \
                         hanya tersedia di ponsel.",
                    )
                    .font(theme::regular(11.5))
                    .color(p.text_muted),
                );
            });
    }

    fn wa_channels_pane(&mut self, ui: &mut egui::Ui, p: &Palette) {
        let header = wa_header(
            ui,
            &mut self.wa_textures,
            p,
            "Saluran",
            &[icons::WaIcon::AddCircle],
        );
        if header == Some(0) {
            self.notify("Buat saluran dari ponsel Anda.");
        }
        Frame::new()
            .inner_margin(Margin {
                left: 16,
                right: 16,
                top: 0,
                bottom: 10,
            })
            .show(ui, |ui| {
                wa_search_box(ui, &mut self.wa_textures, p, &mut self.wa_search, "Cari");
            });
        let query = self.wa_search.trim().to_lowercase();
        let rows = self.wa_channels.clone();
        let mut open = None;
        let mut shown = 0usize;
        ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for chat in rows {
                    if !query.is_empty() && !chat.title.to_lowercase().contains(&query) {
                        continue;
                    }
                    shown += 1;
                    self.preload_avatar(&chat);
                    let picture = self.avatar_texture(ui.ctx(), chat.avatar.as_deref());
                    let time = format::chat_time(chat.last_at);
                    if wa_channel_row(ui, p, &chat, &time, picture.as_ref()).clicked() {
                        open = Some(chat);
                    }
                }
                if shown == 0 {
                    ui.add_space(14.0);
                    Frame::new()
                        .inner_margin(Margin::symmetric(16, 0))
                        .show(ui, |ui| {
                            ui.label(
                                RichText::new(if query.is_empty() {
                                    "Belum ada saluran"
                                } else {
                                    "Tidak ada saluran yang cocok"
                                })
                                .font(theme::regular(12.0))
                                .color(p.text_muted),
                            );
                        });
                }
            });
        if let Some(chat) = open {
            self.open_chat(chat);
            self.wa_section = WaSection::Chats;
            self.wa_search.clear();
        }
    }

    fn wa_communities_pane(&mut self, ui: &mut egui::Ui, p: &Palette) {
        wa_header(ui, &mut self.wa_textures, p, "Komunitas", &[]);
        ui.add_space(42.0);
        ui.vertical_centered(|ui| {
            let (rect, _) = ui.allocate_exact_size(Vec2::splat(64.0), Sense::hover());
            self.wa_textures.paint(
                ui.ctx(),
                ui.painter(),
                Rect::from_center_size(rect.center(), Vec2::splat(56.0)),
                icons::WaIcon::GroupFilled,
                p.text_muted.gamma_multiply(0.55),
            );
            ui.add_space(12.0);
            ui.label(
                RichText::new("Belum ada komunitas")
                    .font(theme::medium(13.5))
                    .color(p.text),
            );
            ui.add_space(4.0);
            ui.label(
                RichText::new("Komunitas yang Anda ikuti akan muncul di sini.")
                    .font(theme::regular(11.5))
                    .color(p.text_muted),
            );
        });
    }

    fn wa_business_pane(&mut self, ui: &mut egui::Ui, p: &Palette) {
        wa_header(ui, &mut self.wa_textures, p, "Fitur bisnis", &[]);
        ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                wa_group_label(ui, p, "Kembangkan bisnis Anda");
                if wa_menu_row(
                    ui,
                    &mut self.wa_textures,
                    p,
                    icons::WaIcon::MetaOne,
                    "Meta One",
                    "Buka fitur premium di seluruh aplikasi Meta",
                    false,
                ) {
                    self.notify("Meta One tersedia di ponsel Anda.");
                }
                if wa_menu_row(
                    ui,
                    &mut self.wa_textures,
                    p,
                    icons::WaIcon::Weave,
                    "Meta Business Agent",
                    "Balas cepat ke pelanggan 24/7",
                    true,
                ) {
                    self.notify("Meta Business Agent tersedia di ponsel Anda.");
                }
                if wa_menu_row(
                    ui,
                    &mut self.wa_textures,
                    p,
                    icons::WaIcon::Store,
                    "Profil bisnis",
                    "Kelola alamat, jam kerja, dan situs web",
                    false,
                ) {
                    self.wa_open(WaSection::Profile);
                }
                if wa_menu_row(
                    ui,
                    &mut self.wa_textures,
                    p,
                    icons::WaIcon::Grid,
                    "Katalog",
                    "Tampilkan produk dan layanan",
                    false,
                ) {
                    self.notify("Katalog tersedia di ponsel Anda.");
                }
                if wa_menu_row(
                    ui,
                    &mut self.wa_textures,
                    p,
                    icons::WaIcon::Receipt,
                    "Order",
                    "Kelola orderan dan pembayaran",
                    false,
                ) {
                    self.notify("Order tersedia di ponsel Anda.");
                }
                wa_group_label(ui, p, "Jangkau lebih banyak pelanggan");
                if wa_menu_row(
                    ui,
                    &mut self.wa_textures,
                    p,
                    icons::WaIcon::Advertise,
                    "Iklankan",
                    "Buat iklan yang mengarahkan pelanggan ke WhatsApp",
                    false,
                ) {
                    self.wa_modal = Some(WaModal::Advertise);
                }
                if wa_menu_row(
                    ui,
                    &mut self.wa_textures,
                    p,
                    icons::WaIcon::Broadcast,
                    "Siaran bisnis",
                    "Lihat semua siaran dan audiens",
                    true,
                ) {
                    self.notify("Siaran bisnis tersedia di ponsel Anda.");
                }
                wa_group_label(ui, p, "Perpesanan");
                if wa_menu_row(
                    ui,
                    &mut self.wa_textures,
                    p,
                    icons::WaIcon::Bolt,
                    "Balasan cepat",
                    "Gunakan kembali pesan yang sering dikirim",
                    false,
                ) {
                    self.notify("Balasan cepat tersedia di ponsel Anda.");
                }
                if wa_menu_row(
                    ui,
                    &mut self.wa_textures,
                    p,
                    icons::WaIcon::GridOn,
                    "Daftar",
                    "Kelola orang dan grup",
                    false,
                ) {
                    self.notify("Daftar tersedia di ponsel Anda.");
                }
                if wa_menu_row(
                    ui,
                    &mut self.wa_textures,
                    p,
                    icons::WaIcon::Help,
                    "Pusat bantuan bisnis",
                    "Dapatkan bantuan, hubungi kami",
                    false,
                ) {
                    widgets::open_url("https://faq.whatsapp.com/");
                }
                ui.add_space(20.0);
            });
    }

    fn wa_settings_pane(&mut self, ui: &mut egui::Ui, p: &Palette) {
        wa_header(ui, &mut self.wa_textures, p, "Pengaturan", &[]);
        Frame::new()
            .inner_margin(Margin {
                left: 16,
                right: 16,
                top: 0,
                bottom: 10,
            })
            .show(ui, |ui| {
                wa_search_box(
                    ui,
                    &mut self.wa_textures,
                    p,
                    &mut self.wa_search,
                    "Cari di pengaturan",
                );
            });
        let query = self.wa_search.trim().to_lowercase();
        let mut open_profile = false;
        let mut open_business = false;
        ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if !self.wa_banner_hidden && query.is_empty() {
                    Frame::new()
                        .inner_margin(Margin::symmetric(16, 6))
                        .show(ui, |ui| {
                            Frame::new()
                                .fill(p.input_bg)
                                .corner_radius(10)
                                .inner_margin(Margin::symmetric(12, 12))
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        let (icon_rect, _) = ui
                                            .allocate_exact_size(Vec2::splat(28.0), Sense::hover());
                                        self.wa_textures.paint(
                                            ui.ctx(),
                                            ui.painter(),
                                            Rect::from_center_size(
                                                icon_rect.center(),
                                                Vec2::splat(24.0),
                                            ),
                                            icons::WaIcon::Notifications,
                                            p.text,
                                        );
                                        ui.add_space(4.0);
                                        ui.vertical(|ui| {
                                            ui.label(
                                                RichText::new("Pilih notifikasi Anda")
                                                    .font(theme::medium(13.0))
                                                    .color(p.text),
                                            );
                                            ui.label(
                                                RichText::new(
                                                    "Dapatkan notifikasi untuk pesan, grup, \
                                                     atau status Anda.",
                                                )
                                                .font(theme::regular(11.0))
                                                .color(p.text_muted),
                                            );
                                            if wa_text_link(ui, p, "Pilih sekarang") {
                                                self.wa_settings_page = Some("Notifikasi");
                                            }
                                        });
                                        ui.with_layout(
                                            egui::Layout::right_to_left(egui::Align::Min),
                                            |ui| {
                                                if wa_icon_button(
                                                    ui,
                                                    &mut self.wa_textures,
                                                    icons::WaIcon::Close,
                                                    p,
                                                    26.0,
                                                    16.0,
                                                    p.text_muted,
                                                )
                                                .clicked()
                                                {
                                                    self.wa_banner_hidden = true;
                                                }
                                            },
                                        );
                                    });
                                });
                        });
                    ui.add_space(6.0);
                }
                let own = self.wa_account_label();
                let path = self.sender_avatar_path("whatsapp:me", "me");
                let picture = self.avatar_texture(ui.ctx(), path.as_deref());
                let width = ui.available_width();
                let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, 84.0), Sense::click());
                if resp.hovered() {
                    ui.painter().rect_filled(rect, CornerRadius::ZERO, p.hover);
                }
                wa_avatar(
                    ui,
                    p,
                    egui::pos2(rect.left() + 54.0, rect.center().y),
                    64.0,
                    &own,
                    picture.as_ref(),
                    None,
                );
                ui.painter().text(
                    egui::pos2(rect.left() + 100.0, rect.center().y),
                    egui::Align2::LEFT_CENTER,
                    format_phone(&own),
                    theme::medium(15.0),
                    p.text,
                );
                if resp.clicked() {
                    open_profile = true;
                }
                let line = ui.available_width();
                let (rule, _) = ui.allocate_exact_size(Vec2::new(line, 1.0), Sense::hover());
                ui.painter().hline(
                    egui::Rangef::new(rule.left() + 16.0, rule.right() - 16.0),
                    rule.center().y,
                    Stroke::new(HAIRLINE, p.divider),
                );
                let rows: [(&str, &str, icons::WaIcon); 9] = [
                    (
                        "Meta One",
                        "Buka fitur premium di seluruh aplikasi Meta",
                        icons::WaIcon::MetaOne,
                    ),
                    (
                        "Fitur bisnis",
                        "Balasan cepat, label, katalog",
                        icons::WaIcon::Store,
                    ),
                    ("Akun", "Notifikasi keamanan, info akun", icons::WaIcon::Key),
                    (
                        "Privasi",
                        "Kontak diblokir, pesan sementara",
                        icons::WaIcon::Lock,
                    ),
                    (
                        "Chat",
                        "Tema, wallpaper, pengaturan obrolan",
                        icons::WaIcon::Notes,
                    ),
                    (
                        "Notifikasi",
                        "Pesan, grup, suara",
                        icons::WaIcon::Notifications,
                    ),
                    (
                        "Pintasan keyboard",
                        "Tindakan cepat",
                        icons::WaIcon::Keyboard,
                    ),
                    (
                        "Bantuan dan masukan",
                        "Pusat Bantuan, hubungi kami, Kebijakan Privasi",
                        icons::WaIcon::Help,
                    ),
                    (
                        "Keluar",
                        "Putuskan tautan perangkat ini",
                        icons::WaIcon::Logout,
                    ),
                ];
                for (title, detail, icon) in rows {
                    if !query.is_empty()
                        && !title.to_lowercase().contains(&query)
                        && !detail.to_lowercase().contains(&query)
                    {
                        continue;
                    }
                    if wa_menu_row(ui, &mut self.wa_textures, p, icon, title, detail, false) {
                        if title == "Fitur bisnis" {
                            open_business = true;
                        } else if title == "Keluar" {
                            self.notify("Keluar dari WhatsApp dilakukan dari ponsel.");
                        } else {
                            self.wa_settings_page = Some(title);
                        }
                    }
                }
                ui.add_space(20.0);
            });
        if open_profile {
            self.wa_open(WaSection::Profile);
        } else if open_business {
            self.wa_open(WaSection::Business);
        }
    }

    /// Right side of the WhatsApp settings view: gear when nothing is chosen,
    /// otherwise the real controls for the selected row.
    fn wa_settings_detail(&mut self, ui: &mut egui::Ui, p: &Palette) {
        let Some(page) = self.wa_settings_page else {
            ui.add_space(170.0);
            ui.vertical_centered(|ui| {
                let (rect, _) = ui.allocate_exact_size(Vec2::splat(64.0), Sense::hover());
                self.wa_textures.paint(
                    ui.ctx(),
                    ui.painter(),
                    Rect::from_center_size(rect.center(), Vec2::splat(56.0)),
                    icons::WaIcon::Settings,
                    p.text_muted.gamma_multiply(0.55),
                );
                ui.add_space(14.0);
                ui.label(
                    RichText::new("Pengaturan")
                        .font(theme::semibold(20.0))
                        .color(p.text),
                );
            });
            return;
        };
        let width = ui.available_width();
        let (bar, _) = ui.allocate_exact_size(Vec2::new(width, 58.0), Sense::hover());
        let back_rect = Rect::from_center_size(
            egui::pos2(bar.left() + 28.0, bar.center().y),
            Vec2::splat(38.0),
        );
        let back = ui.interact(back_rect, ui.id().with("wa-settings-back"), Sense::click());
        if back.hovered() {
            ui.painter()
                .circle_filled(back_rect.center(), 19.0, p.hover);
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        ui.painter().text(
            back_rect.center(),
            egui::Align2::CENTER_CENTER,
            "\u{2190}",
            theme::semibold(18.0),
            p.text,
        );
        ui.painter().text(
            egui::pos2(bar.left() + 52.0, bar.center().y),
            egui::Align2::LEFT_CENTER,
            page,
            theme::semibold(16.0),
            p.text,
        );
        if back.clicked() {
            self.wa_settings_page = None;
            return;
        }
        ui.painter().hline(
            egui::Rangef::new(bar.left() + 16.0, bar.right() - 16.0),
            bar.bottom() - 0.5,
            Stroke::new(HAIRLINE, p.divider),
        );
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add_space(12.0);
                match page {
                    "Notifikasi" => {
                        let mut enabled = self.settings.desktop_notifications;
                        if wa_toggle_row(
                            ui,
                            &mut self.wa_textures,
                            p,
                            icons::WaIcon::Notifications,
                            "Notifikasi desktop",
                            "Tampilkan pesan baru saat United berjalan di latar belakang",
                            &mut enabled,
                        ) {
                            self.settings.desktop_notifications = enabled;
                            self.settings_dirty = true;
                        }
                        wa_info_row(
                            ui,
                            &mut self.wa_textures,
                            p,
                            icons::WaIcon::Mail,
                            "Pesan",
                            "Semua pesan langsung",
                        );
                        wa_info_row(
                            ui,
                            &mut self.wa_textures,
                            p,
                            icons::WaIcon::GroupFilled,
                            "Grup",
                            "Semua pesan grup",
                        );
                        self.wa_settings_link(ui, p);
                    }
                    "Chat" => {
                        wa_info_row(
                            ui,
                            &mut self.wa_textures,
                            p,
                            icons::WaIcon::Notes,
                            "Tema",
                            match self.mode {
                                Mode::Dark => "Gelap",
                                Mode::Light => "Terang",
                            },
                        );
                        wa_info_row(
                            ui,
                            &mut self.wa_textures,
                            p,
                            icons::WaIcon::Image,
                            "Wallpaper obrolan",
                            "Bawaan WhatsApp",
                        );
                        self.wa_settings_link(ui, p);
                    }
                    "Akun" => {
                        let label = self.wa_account_label();
                        wa_info_row(
                            ui,
                            &mut self.wa_textures,
                            p,
                            icons::WaIcon::AccountCircle,
                            "Nama profil",
                            &label,
                        );
                        wa_info_row(
                            ui,
                            &mut self.wa_textures,
                            p,
                            icons::WaIcon::Key,
                            "Verifikasi dua langkah",
                            "Aktif di ponsel",
                        );
                        self.wa_settings_link(ui, p);
                    }
                    "Privasi" => {
                        wa_info_row(
                            ui,
                            &mut self.wa_textures,
                            p,
                            icons::WaIcon::Block,
                            "Kontak diblokir",
                            "Kelola dari ponsel",
                        );
                        wa_info_row(
                            ui,
                            &mut self.wa_textures,
                            p,
                            icons::WaIcon::Schedule,
                            "Pesan sementara",
                            "Nonaktif",
                        );
                        self.wa_settings_link(ui, p);
                    }
                    "Pintasan keyboard" => {
                        wa_info_row(
                            ui,
                            &mut self.wa_textures,
                            p,
                            icons::WaIcon::Keyboard,
                            "Cari obrolan",
                            "Ctrl + F",
                        );
                        wa_info_row(
                            ui,
                            &mut self.wa_textures,
                            p,
                            icons::WaIcon::Keyboard,
                            "Obrolan berikutnya",
                            "Ctrl + Tab",
                        );
                        wa_info_row(
                            ui,
                            &mut self.wa_textures,
                            p,
                            icons::WaIcon::Keyboard,
                            "Tutup obrolan",
                            "Esc",
                        );
                    }
                    _ => {
                        ui.add_space(40.0);
                        ui.vertical_centered(|ui| {
                            ui.label(
                                RichText::new("Pengaturan ini dikelola dari ponsel Anda.")
                                    .font(theme::regular(12.5))
                                    .color(p.text_muted),
                            );
                        });
                        self.wa_settings_link(ui, p);
                    }
                }
                ui.add_space(24.0);
            });
    }

    fn wa_settings_link(&mut self, ui: &mut egui::Ui, p: &Palette) {
        ui.add_space(16.0);
        Frame::new()
            .inner_margin(Margin::symmetric(16, 0))
            .show(ui, |ui| {
                if primary_button(ui, p, "Buka pengaturan United") {
                    self.view = View::Settings;
                    self.settings_dirty = true;
                }
            });
    }

    fn wa_profile_pane(&mut self, ui: &mut egui::Ui, p: &Palette) {
        let width = ui.available_width();
        let (bar, _) = ui.allocate_exact_size(Vec2::new(width, 56.0), Sense::hover());
        let close_rect = Rect::from_center_size(
            egui::pos2(bar.left() + 30.0, bar.center().y),
            Vec2::splat(40.0),
        );
        let close = ui.interact(close_rect, ui.id().with("wa-profile-close"), Sense::click());
        if close.hovered() {
            ui.painter()
                .circle_filled(close_rect.center(), 20.0, p.hover);
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        self.wa_textures.paint(
            ui.ctx(),
            ui.painter(),
            Rect::from_center_size(close_rect.center(), Vec2::splat(22.0)),
            icons::WaIcon::Close,
            p.text,
        );
        ui.painter().text(
            egui::pos2(bar.left() + 54.0, bar.center().y),
            egui::Align2::LEFT_CENTER,
            "Profil",
            theme::medium(15.0),
            p.text,
        );
        let save_rect = Rect::from_center_size(
            egui::pos2(bar.right() - 52.0, bar.center().y),
            Vec2::new(76.0, 32.0),
        );
        let save = ui.interact(save_rect, ui.id().with("wa-profile-save"), Sense::click());
        ui.painter()
            .rect_filled(save_rect, CornerRadius::same(16), p.input_bg);
        ui.painter().text(
            save_rect.center(),
            egui::Align2::CENTER_CENTER,
            "Simpan",
            theme::medium(12.5),
            p.text_muted,
        );
        ui.painter().hline(
            egui::Rangef::new(bar.left(), bar.right()),
            bar.bottom() - 0.5,
            Stroke::new(HAIRLINE, p.divider),
        );
        if close.clicked() {
            self.wa_open(WaSection::Chats);
            return;
        }
        if save.clicked() {
            self.notify("Profil bisnis disimpan.");
        }
        let name = self.wa_account_label();
        let path = self.sender_avatar_path("whatsapp:me", "me");
        let picture = self.avatar_texture(ui.ctx(), path.as_deref());
        let mut action = false;
        egui::ScrollArea::vertical()
            .id_salt("wa-profile-scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let column = 680.0_f32.min(ui.available_width() - 48.0);
                ui.add_space(22.0);
                ui.horizontal(|ui| {
                    ui.add_space((ui.available_width() - column) / 2.0);
                    ui.vertical(|ui| {
                        ui.set_width(column);
                        let (cover, _) =
                            ui.allocate_exact_size(Vec2::new(column, 150.0), Sense::hover());
                        ui.painter()
                            .rect_filled(cover, CornerRadius::same(10), p.input_bg);
                        let cover_edit = Rect::from_center_size(
                            egui::pos2(cover.right() - 48.0, cover.top() + 22.0),
                            Vec2::new(78.0, 30.0),
                        );
                        ui.painter()
                            .rect_filled(cover_edit, CornerRadius::same(15), p.header);
                        self.wa_textures.paint(
                            ui.ctx(),
                            ui.painter(),
                            Rect::from_center_size(
                                egui::pos2(cover_edit.left() + 17.0, cover_edit.center().y),
                                Vec2::splat(16.0),
                            ),
                            icons::WaIcon::Camera,
                            p.accent,
                        );
                        ui.painter().text(
                            egui::pos2(cover_edit.left() + 30.0, cover_edit.center().y),
                            egui::Align2::LEFT_CENTER,
                            "Edit",
                            theme::medium(11.5),
                            p.accent,
                        );
                        wa_avatar(
                            ui,
                            p,
                            egui::pos2(cover.center().x, cover.bottom()),
                            124.0,
                            &name,
                            picture.as_ref(),
                            None,
                        );
                        ui.add_space(70.0);
                        ui.vertical_centered(|ui| {
                            if wa_text_link(ui, p, "Edit") {
                                action = true;
                            }
                        });
                        ui.add_space(14.0);
                        ui.label(
                            RichText::new("Informasi bisnis")
                                .font(theme::semibold(16.0))
                                .color(p.text),
                        );
                        ui.add_space(8.0);
                        wa_info_row(
                            ui,
                            &mut self.wa_textures,
                            p,
                            icons::WaIcon::Notes,
                            "Nama bisnis",
                            &format_phone(&name),
                        );
                        ui.add_space(8.0);
                        wa_input_row(
                            ui,
                            &mut self.wa_textures,
                            p,
                            icons::WaIcon::Grid,
                            "Deskripsi",
                        );
                        wa_input_row(
                            ui,
                            &mut self.wa_textures,
                            p,
                            icons::WaIcon::Location,
                            "Alamat",
                        );
                        wa_selector_row(
                            ui,
                            &mut self.wa_textures,
                            p,
                            "Area dalam cakupan",
                            "Lokasi persis",
                        );
                        wa_selector_row(
                            ui,
                            &mut self.wa_textures,
                            p,
                            "Catatan lokasi",
                            "Tambah catatan",
                        );
                        ui.add_space(20.0);
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new("Jam kerja")
                                    .font(theme::semibold(14.0))
                                    .color(p.text),
                            );
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.label(
                                        RichText::new("Selalu buka")
                                            .font(theme::regular(11.5))
                                            .color(p.text_muted),
                                    );
                                },
                            );
                        });
                        for day in [
                            "Minggu", "Senin", "Selasa", "Rabu", "Kamis", "Jumat", "Sabtu",
                        ] {
                            let (row, _) =
                                ui.allocate_exact_size(Vec2::new(column, 36.0), Sense::hover());
                            ui.painter().text(
                                egui::pos2(row.left(), row.center().y),
                                egui::Align2::LEFT_CENTER,
                                day,
                                theme::regular(12.5),
                                p.text,
                            );
                            ui.painter().text(
                                egui::pos2(row.right() - 60.0, row.center().y),
                                egui::Align2::RIGHT_CENTER,
                                "Buka 24 jam",
                                theme::regular(11.5),
                                p.text_muted,
                            );
                            let edit_rect = Rect::from_center_size(
                                egui::pos2(row.right() - 22.0, row.center().y),
                                Vec2::new(40.0, 26.0),
                            );
                            let edit = ui.interact(
                                edit_rect,
                                ui.id().with(("wa-day", day)),
                                Sense::click(),
                            );
                            ui.painter().text(
                                edit_rect.center(),
                                egui::Align2::CENTER_CENTER,
                                "Edit",
                                theme::medium(11.0),
                                p.accent,
                            );
                            if edit.clicked() {
                                action = true;
                            }
                            ui.painter().hline(
                                egui::Rangef::new(row.left(), row.right()),
                                row.bottom() - 0.5,
                                Stroke::new(HAIRLINE, p.divider),
                            );
                        }
                        ui.add_space(20.0);
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new("Kategori")
                                    .font(theme::semibold(14.0))
                                    .color(p.text),
                            );
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    let mut clicked = false;
                                    if wa_text_link(ui, p, "Edit") {
                                        clicked = true;
                                    }
                                    ui.add_space(12.0);
                                    ui.label(
                                        RichText::new("Bisnis lain")
                                            .font(theme::regular(12.5))
                                            .color(p.text),
                                    );
                                    action |= clicked;
                                },
                            );
                        });
                        ui.add_space(20.0);
                        ui.label(
                            RichText::new("Produk dan layanan")
                                .font(theme::semibold(14.0))
                                .color(p.text),
                        );
                        if wa_text_link(ui, p, "Tambah katalog") {
                            action = true;
                        }
                        ui.add_space(16.0);
                        ui.label(
                            RichText::new("Tautan")
                                .font(theme::semibold(14.0))
                                .color(p.text),
                        );
                        wa_info_row(
                            ui,
                            &mut self.wa_textures,
                            p,
                            icons::WaIcon::Link,
                            "Situs web",
                            "Tambah situs web lain",
                        );
                        ui.add_space(16.0);
                        ui.label(
                            RichText::new("Informasi kontak")
                                .font(theme::semibold(14.0))
                                .color(p.text),
                        );
                        wa_info_row(
                            ui,
                            &mut self.wa_textures,
                            p,
                            icons::WaIcon::Mail,
                            "Email",
                            "Tambah email",
                        );
                        wa_info_row(
                            ui,
                            &mut self.wa_textures,
                            p,
                            icons::WaIcon::Call,
                            "Nomor telepon",
                            &name,
                        );
                        ui.add_space(40.0);
                    });
                });
            });
        if action {
            self.notify("Edit profil bisnis dari ponsel Anda.");
        }
    }

    fn wa_account_label(&self) -> String {
        self.adapters
            .get(&Service::WhatsApp)
            .map(|a| a.label())
            .filter(|label| !label.is_empty())
            .unwrap_or_else(|| "Belum tertaut".to_string())
    }

    fn wa_media_sheet(&mut self, ctx: &egui::Context) {
        let p = theme::service_palette(self.mode, Service::WhatsApp);
        if modal_backdrop(ctx) {
            self.wa_modal = None;
            return;
        }
        let mut open = true;
        egui::Window::new("wa-media")
            .id(egui::Id::new("wa-media"))
            .order(egui::Order::Foreground)
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .fixed_size(Vec2::new(760.0, 500.0))
            .anchor(egui::Align2::CENTER_CENTER, Vec2::new(0.0, 20.0))
            .frame(
                Frame::new()
                    .fill(p.panel)
                    .corner_radius(14)
                    .stroke(Stroke::new(1.0, p.divider)),
            )
            .show(ctx, |ui| {
                Frame::new()
                    .inner_margin(Margin::symmetric(16, 10))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            for (index, label) in ["Media", "Dokumen", "Tautan"].iter().enumerate()
                            {
                                let active = self.wa_media_tab as usize == index;
                                let text = RichText::new(*label)
                                    .font(theme::medium(13.5))
                                    .color(if active { p.accent } else { p.text_muted });
                                if ui
                                    .add(
                                        egui::Button::new(text)
                                            .fill(Color32::TRANSPARENT)
                                            .corner_radius(6),
                                    )
                                    .clicked()
                                {
                                    self.wa_media_tab = index as u8;
                                }
                            }
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui.small_button("✕").clicked() {
                                        open = false;
                                    }
                                    // Search, as in Web's media sheet.
                                    let (pill, _) = ui.allocate_exact_size(
                                        Vec2::new(210.0, 30.0),
                                        Sense::hover(),
                                    );
                                    ui.painter().rect_filled(
                                        pill,
                                        CornerRadius::same(15),
                                        p.input_bg,
                                    );
                                    icons::paint(
                                        ui.painter(),
                                        Rect::from_center_size(
                                            egui::pos2(pill.left() + 16.0, pill.center().y),
                                            Vec2::splat(15.0),
                                        ),
                                        Icon::Search,
                                        p.text_muted,
                                        1.5,
                                    );
                                    let field = Rect::from_min_max(
                                        egui::pos2(pill.left() + 28.0, pill.top() + 6.0),
                                        egui::pos2(pill.right() - 10.0, pill.bottom() - 6.0),
                                    );
                                    ui.put(
                                        field,
                                        egui::TextEdit::singleline(&mut self.wa_media_query)
                                            .id(egui::Id::new("wa-media-search"))
                                            .hint_text(
                                                RichText::new("Cari")
                                                    .font(theme::regular(12.5))
                                                    .color(p.text_muted),
                                            )
                                            .frame(egui::Frame::NONE)
                                            .desired_width(field.width())
                                            .font(theme::regular(12.5))
                                            .text_color(p.text),
                                    );
                                },
                            );
                        });
                    });
                ui.separator();
                let query = self.wa_media_query.trim().to_lowercase();
                let titles: HashMap<String, String> = self
                    .chats
                    .iter()
                    .chain(self.wa_channels.iter())
                    .map(|c| (c.id.clone(), c.title.clone()))
                    .collect();
                let rows: Vec<Message> = match self.wa_media_tab {
                    1 => self
                        .wa_media
                        .iter()
                        .filter(|m| {
                            m.media
                                .as_deref()
                                .and_then(MediaRef::parse)
                                .is_some_and(|media| media.kind == "document")
                        })
                        .cloned()
                        .collect(),
                    2 => self.wa_links.clone(),
                    _ => self
                        .wa_media
                        .iter()
                        .filter(|m| {
                            m.media
                                .as_deref()
                                .and_then(MediaRef::parse)
                                .is_some_and(|media| media.kind != "document")
                        })
                        .cloned()
                        .collect(),
                }
                .into_iter()
                .filter(|m| {
                    query.is_empty()
                        || titles
                            .get(&m.chat_id)
                            .is_some_and(|t| t.to_lowercase().contains(&query))
                        || m.sender_name.to_lowercase().contains(&query)
                        || m.body.to_lowercase().contains(&query)
                })
                .collect();
                if rows.is_empty() {
                    wa_empty(
                        ui,
                        &p,
                        Icon::Image,
                        "Tidak ada media",
                        "Lampiran dari obrolan WhatsApp akan muncul di sini.",
                    );
                    return;
                }
                ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        if self.wa_media_tab == 2 {
                            for message in rows {
                                let title = titles
                                    .get(&message.chat_id)
                                    .cloned()
                                    .unwrap_or_else(|| message.sender_name.clone());
                                if wa_link_row(ui, &p, &title, &message.body, message.at) {
                                    if let Some(url) = first_url(&message.body) {
                                        widgets::open_url(&url);
                                    }
                                }
                            }
                        } else {
                            ui.horizontal_wrapped(|ui| {
                                for message in rows {
                                    let title = titles
                                        .get(&message.chat_id)
                                        .cloned()
                                        .unwrap_or_else(|| message.sender_name.clone());
                                    let Some(media) = self.media_of(&message) else {
                                        continue;
                                    };
                                    let key = message_key(&message);
                                    let thumb = media
                                        .thumb
                                        .as_deref()
                                        .and_then(|raw| self.media_thumb(ui.ctx(), &key, raw));
                                    let local = self.media_local(&message);
                                    let texture = match (&thumb, &local) {
                                        (Some(texture), _) => Some(texture.clone()),
                                        (None, Some(path)) => self.media_texture(ui.ctx(), path),
                                        _ => None,
                                    };
                                    let (rect, resp) = ui.allocate_exact_size(
                                        Vec2::new(168.0, 118.0),
                                        Sense::click(),
                                    );
                                    ui.painter().rect_filled(
                                        rect,
                                        CornerRadius::same(6),
                                        p.input_bg,
                                    );
                                    if let Some(texture) = &texture {
                                        let size = texture.size_vec2();
                                        let scale =
                                            (rect.width() / size.x).min(rect.height() / size.y);
                                        egui::Image::new((texture.id(), size * scale)).paint_at(
                                            ui,
                                            Rect::from_center_size(rect.center(), size * scale),
                                        );
                                    } else {
                                        icons::paint(
                                            ui.painter(),
                                            Rect::from_center_size(
                                                rect.center(),
                                                Vec2::splat(28.0),
                                            ),
                                            Icon::Image,
                                            p.text_muted,
                                            1.4,
                                        );
                                    }
                                    if resp.hovered() {
                                        ui.painter().rect_filled(
                                            Rect::from_min_max(
                                                egui::pos2(rect.left(), rect.bottom() - 22.0),
                                                rect.right_bottom(),
                                            ),
                                            CornerRadius {
                                                nw: 0,
                                                ne: 0,
                                                sw: 6,
                                                se: 6,
                                            },
                                            Color32::from_black_alpha(150),
                                        );
                                        ui.painter().text(
                                            rect.left_bottom() + Vec2::new(6.0, -11.0),
                                            egui::Align2::LEFT_CENTER,
                                            &title,
                                            theme::regular(10.5),
                                            Color32::WHITE,
                                        );
                                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                                    }
                                    if resp.clicked() {
                                        match local {
                                            Some(path) => {
                                                if let Some(texture) =
                                                    self.media_texture(ui.ctx(), &path)
                                                {
                                                    self.media_view = Some((texture, path));
                                                }
                                            }
                                            None => {
                                                self.request_media(&message);
                                                self.notify("Mengunduh media…");
                                            }
                                        }
                                    }
                                }
                            });
                        }
                    });
            });
        if !open {
            self.wa_modal = None;
        }
    }

    fn wa_advertise_sheet(&mut self, ctx: &egui::Context) {
        let p = theme::service_palette(self.mode, Service::WhatsApp);
        if modal_backdrop(ctx) {
            self.wa_modal = None;
            return;
        }
        let mut open = true;
        egui::Window::new("wa-advertise")
            .id(egui::Id::new("wa-advertise"))
            .order(egui::Order::Foreground)
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .fixed_size(Vec2::new(520.0, 540.0))
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .frame(
                Frame::new()
                    .fill(p.panel)
                    .corner_radius(16)
                    .stroke(Stroke::new(1.0, p.divider))
                    .inner_margin(Margin::same(24)),
            )
            .show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    let (rect, _) = ui.allocate_exact_size(Vec2::new(150.0, 92.0), Sense::hover());
                    // Web's illustration: a tilted ad card with a pin, ruled
                    // lines and a green chat bubble on the corner.
                    let c = rect.center() + Vec2::new(-8.0, 0.0);
                    let angle = -0.18f32;
                    let rot = |x: f32, y: f32| {
                        c + Vec2::new(
                            x * angle.cos() - y * angle.sin(),
                            x * angle.sin() + y * angle.cos(),
                        )
                    };
                    let card = vec![
                        rot(-46.0, -30.0),
                        rot(46.0, -30.0),
                        rot(46.0, 30.0),
                        rot(-46.0, 30.0),
                    ];
                    ui.painter().add(egui::Shape::convex_polygon(
                        card.clone(),
                        Color32::from_rgb(0xf3, 0xed, 0xdf),
                        Stroke::NONE,
                    ));
                    ui.painter().add(egui::Shape::closed_line(
                        card,
                        Stroke::new(1.0, Color32::from_rgb(0xc9, 0xc2, 0xb0)),
                    ));
                    icons::paint(
                        ui.painter(),
                        Rect::from_center_size(rot(-24.0, -8.0), Vec2::splat(24.0)),
                        Icon::Pin,
                        Color32::from_rgb(0x1f, 0x2c, 0x33),
                        1.8,
                    );
                    for (index, width) in [24.0f32, 18.0, 28.0].into_iter().enumerate() {
                        let y = 4.0 + index as f32 * 7.0;
                        ui.painter().line_segment(
                            [rot(-4.0, y), rot(-4.0 + width, y)],
                            Stroke::new(2.0, Color32::from_rgb(0xd6, 0xcf, 0xbe)),
                        );
                    }
                    let bubble = c + Vec2::new(46.0, 28.0);
                    ui.painter()
                        .circle_filled(bubble, 20.0, Color32::from_rgb(0x25, 0xd3, 0x66));
                    icons::paint(
                        ui.painter(),
                        Rect::from_center_size(bubble, Vec2::splat(20.0)),
                        Icon::Chat,
                        Color32::WHITE,
                        1.8,
                    );
                    ui.add_space(8.0);
                    ui.label(
                        RichText::new("Dapatkan pelanggan baru melalui iklan Facebook & Instagram")
                            .font(theme::semibold(18.0))
                            .color(p.text),
                    );
                    ui.add_space(8.0);
                    ui.label(
                        RichText::new(
                            "Buat iklan yang ditampilkan di Facebook & Instagram dengan mudah \
                             dan ajak orang-orang chat dengan Anda di WhatsApp.",
                        )
                        .font(theme::regular(12.5))
                        .color(p.text_muted),
                    );
                });
                ui.add_space(16.0);
                for (icon, text) in [
                    (Icon::Pin, "Jangkau pelanggan lokal"),
                    (Icon::Tray, "Kendalikan anggaran iklan Anda"),
                    (Icon::Status, "Jeda atau hentikan iklan kapan saja"),
                ] {
                    ui.horizontal(|ui| {
                        let (rect, _) = ui.allocate_exact_size(Vec2::splat(28.0), Sense::hover());
                        icons::paint(
                            ui.painter(),
                            Rect::from_center_size(rect.center(), Vec2::splat(18.0)),
                            icon,
                            p.text_muted,
                            1.5,
                        );
                        ui.label(RichText::new(text).font(theme::regular(13.0)).color(p.text));
                    });
                    ui.add_space(6.0);
                }
                ui.add_space(10.0);
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 3.0;
                    ui.label(
                        RichText::new(
                            "Nama dan foto bisnis Anda akan dibagikan dengan Meta untuk \
                             membuat iklan.",
                        )
                        .font(theme::regular(11.5))
                        .color(p.text_muted),
                    );
                    if ui
                        .link(
                            RichText::new("Pelajari selengkapnya")
                                .font(theme::regular(11.5))
                                .color(p.accent),
                        )
                        .clicked()
                    {
                        widgets::open_url("https://www.facebook.com/business/ads");
                    }
                });
                ui.add_space(18.0);
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add(
                                egui::Button::new(
                                    RichText::new("Lanjutkan di Facebook")
                                        .font(theme::medium(12.5))
                                        .color(Color32::BLACK),
                                )
                                .fill(Color32::WHITE)
                                .corner_radius(18)
                                .min_size(Vec2::new(160.0, 36.0)),
                            )
                            .clicked()
                        {
                            widgets::open_url("https://www.facebook.com/business/ads");
                            open = false;
                        }
                        if ui
                            .add(
                                egui::Button::new(
                                    RichText::new("Batal")
                                        .font(theme::medium(12.5))
                                        .color(p.accent),
                                )
                                .fill(Color32::TRANSPARENT)
                                .corner_radius(18)
                                .min_size(Vec2::new(72.0, 36.0)),
                            )
                            .clicked()
                        {
                            open = false;
                        }
                    });
                });
            });
        if !open {
            self.wa_modal = None;
        }
    }

    // -------------------------------------------------------------- Telegram

    fn tg_name(&self) -> String {
        self.adapters
            .get(&Service::Telegram)
            .map(|a| a.label())
            .filter(|l| !l.is_empty())
            .unwrap_or_else(|| "Telegram".to_string())
    }

    fn tg_id(&self) -> String {
        self.chats
            .iter()
            .find(|c| c.service == Service::Telegram)
            .map(|c| c.account.0.trim_start_matches("telegram:").to_string())
            .filter(|id| !id.is_empty())
            .unwrap_or_else(|| "—".to_string())
    }

    fn tg_open(&mut self, section: TgSection) {
        self.tg_section = section;
        self.tg_drawer = false;
        self.tg_more = false;
        self.search.clear();
        if section == TgSection::Contacts {
            self.tg_contacts = self
                .store
                .contacts("telegram:me")
                .unwrap_or_default()
                .into_iter()
                .collect();
            self.tg_contacts.sort_by(|a, b| a.1.cmp(&b.1));
        }
    }

    /// Telegram Web A's floating left column: a rounded card on the wallpaper,
    /// with the hamburger drawer painted over the chat list.
    fn tg_column(&mut self, ui: &mut egui::Ui) {
        let p = theme::service_palette(self.mode, Service::Telegram);
        egui::Panel::left("tg-column")
            .exact_size(406.0)
            .frame(Frame::new().fill(p.chat_bg))
            .show(ui, |ui| {
                let full = ui.max_rect();
                let card =
                    Rect::from_min_max(full.min + Vec2::splat(16.0), full.max - Vec2::splat(16.0));
                // Telegram's card sits a few pixels above the wallpaper, so it
                // casts a soft shadow instead of a hairline.
                ui.painter().rect_filled(
                    card.translate(Vec2::new(0.0, 2.0)).expand(2.0),
                    CornerRadius::same(18),
                    Color32::from_black_alpha(70),
                );
                ui.painter()
                    .rect_filled(card, CornerRadius::same(16), p.panel);
                ui.scope_builder(egui::UiBuilder::new().max_rect(card), |ui| {
                    ui.set_clip_rect(card);
                    match self.tg_section {
                        TgSection::Contacts => self.tg_contacts_pane(ui, &p),
                        TgSection::Settings => self.tg_settings_pane(ui, &p),
                        _ => self.tg_chats_pane(ui, &p),
                    }
                    if self.tg_drawer {
                        self.tg_drawer_pane(ui, &p);
                    }
                });
            });
    }

    fn tg_chats_pane(&mut self, ui: &mut egui::Ui, p: &Palette) {
        Frame::new()
            .inner_margin(Margin::symmetric(12, 12))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    if tg_button(
                        ui,
                        &mut self.tg_textures,
                        icons::TgIcon::Hamburger,
                        40.0,
                        24.0,
                        p.text,
                    )
                    .clicked()
                    {
                        self.tg_drawer = !self.tg_drawer;
                        self.tg_more = false;
                    }
                    ui.add_space(12.0);
                    let width = ui.available_width().max(120.0);
                    self.tg_search_box(ui, p, "Search", width);
                });
            });
        ui.add_space(4.0);
        let query = self.search.trim().to_lowercase();
        let mut rows: Vec<Chat> = self
            .chats
            .iter()
            .filter(|c| {
                let archived = self
                    .chat_flags
                    .get(&(c.account.0.clone(), c.id.clone()))
                    .is_some_and(|flags| flags.1);
                !archived
                    && (query.is_empty()
                        || c.title.to_lowercase().contains(&query)
                        || c.preview.to_lowercase().contains(&query))
            })
            .cloned()
            .collect();
        let pinned = |c: &Chat| {
            self.chat_flags
                .get(&(c.account.0.clone(), c.id.clone()))
                .is_some_and(|flags| flags.0)
        };
        rows.sort_by(|a, b| pinned(b).cmp(&pinned(a)));
        let active = self.active_chat.get(&Service::Telegram).cloned();
        let mut open = None;
        ScrollArea::vertical()
            .id_salt("tg-chats")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add_space(8.0);
                for chat in &rows {
                    let selected = active
                        .as_ref()
                        .is_some_and(|(a, id)| *a == chat.account.0 && *id == chat.id);
                    let flags = self
                        .chat_flags
                        .get(&(chat.account.0.clone(), chat.id.clone()))
                        .copied()
                        .unwrap_or((false, false));
                    if self
                        .tg_chat_row(ui, p, chat, selected, flags.0, flags.1)
                        .clicked()
                    {
                        open = Some(chat.clone());
                    }
                }
                if rows.is_empty() {
                    ui.add_space(80.0);
                    ui.vertical_centered(|ui| {
                        ui.label(
                            RichText::new("No chats yet")
                                .font(theme::medium(14.0))
                                .color(p.text_muted),
                        );
                    });
                }
                ui.add_space(8.0);
            });
        if let Some(chat) = open {
            self.tg_section = TgSection::Chats;
            self.open_chat(chat);
        }
    }

    fn tg_search_box(&mut self, ui: &mut egui::Ui, p: &Palette, hint: &str, width: f32) {
        tg_search_box(ui, &mut self.tg_textures, p, &mut self.search, hint, width);
    }

    /// One Telegram chat row: 54px avatar, two 16px lines and the blue badge.
    fn tg_chat_row(
        &mut self,
        ui: &mut egui::Ui,
        p: &Palette,
        chat: &Chat,
        selected: bool,
        pinned: bool,
        muted: bool,
    ) -> egui::Response {
        let width = ui.available_width();
        let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, 72.0), Sense::click());
        if selected {
            ui.painter()
                .rect_filled(rect, CornerRadius::same(10), p.selected);
        } else if resp.hovered() {
            ui.painter()
                .rect_filled(rect, CornerRadius::same(10), p.hover);
        }
        let picture = self.avatar_texture(ui.ctx(), chat.avatar.as_deref());
        wa_avatar(
            ui,
            p,
            egui::pos2(rect.left() + 36.0, rect.center().y),
            54.0,
            &chat.title,
            picture.as_ref(),
            None,
        );
        let text_x = rect.left() + 71.0;
        let time = if chat.last_at > 0 {
            format::clock(chat.last_at)
        } else {
            String::new()
        };
        let time_w = ui
            .painter()
            .layout_no_wrap(time.clone(), theme::regular(12.0), p.text_muted)
            .size()
            .x;
        let mut title_x = text_x;
        if pinned {
            self.tg_textures.paint(
                ui.ctx(),
                ui.painter(),
                Rect::from_min_size(egui::pos2(title_x, rect.top() + 14.0), Vec2::splat(14.0)),
                icons::TgIcon::Pin,
                p.text_muted,
            );
            title_x += 18.0;
        }
        if muted {
            self.tg_textures.paint(
                ui.ctx(),
                ui.painter(),
                Rect::from_min_size(egui::pos2(title_x, rect.top() + 14.0), Vec2::splat(14.0)),
                icons::TgIcon::Muted,
                p.text_muted,
            );
            title_x += 18.0;
        }
        let title_room = (rect.right() - 10.0 - time_w - 8.0 - title_x).max(20.0);
        ui.painter().text(
            egui::pos2(title_x, rect.top() + 21.0),
            egui::Align2::LEFT_CENTER,
            elide(&chat.title, title_room, 15.0),
            theme::medium(15.0),
            p.text,
        );
        if !time.is_empty() {
            ui.painter().text(
                egui::pos2(rect.right() - 10.0, rect.top() + 21.0),
                egui::Align2::RIGHT_CENTER,
                time,
                theme::regular(12.0),
                p.text_muted,
            );
        }
        let preview = chat.preview.replace('\n', " ");
        let room =
            (rect.right() - 10.0 - text_x - if chat.unread > 0 { 30.0 } else { 0.0 }).max(20.0);
        ui.painter().text(
            egui::pos2(text_x, rect.top() + 45.0),
            egui::Align2::LEFT_CENTER,
            elide(&preview, room, 13.5),
            theme::regular(13.5),
            p.text_muted,
        );
        if chat.unread > 0 {
            let badge = Rect::from_center_size(
                egui::pos2(rect.right() - 22.0, rect.top() + 52.0),
                Vec2::new(24.0, 20.0),
            );
            ui.painter()
                .rect_filled(badge, CornerRadius::same(10), p.accent);
            ui.painter().text(
                badge.center(),
                egui::Align2::CENTER_CENTER,
                if chat.unread > 99 {
                    "99+".to_string()
                } else {
                    chat.unread.to_string()
                },
                theme::semibold(11.0),
                Color32::WHITE,
            );
        }
        if resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        resp
    }

    fn tg_drawer_pane(&mut self, ui: &mut egui::Ui, p: &Palette) {
        let full = ui.max_rect();
        let catcher = ui.interact(full, ui.id().with("tg-drawer-catcher"), Sense::click());
        if catcher.clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.tg_drawer = false;
            self.tg_more = false;
            return;
        }
        // Exact height: the auto-sized Frame kept growing to the scope's
        // height, leaving a long empty tail under the last item. More opens a
        // second card to the right, as Web A does, so this one never grows.
        let drawer_h: f32 = 260.0_f32.min(full.height() - 60.0);
        let drawer =
            Rect::from_min_size(full.min + Vec2::new(29.0, 60.0), Vec2::new(216.0, drawer_h));
        let mut open = None;
        let mut night = false;
        // The drawer floats over the list; the shadow is what separates it,
        // since Web A paints it in the same #212121 as the card.
        for (offset, alpha) in [(6.0, 40), (3.0, 60)] {
            ui.painter().rect_filled(
                drawer.translate(Vec2::new(0.0, offset)),
                CornerRadius::same(18),
                Color32::from_black_alpha(alpha),
            );
        }
        ui.painter()
            .rect_filled(drawer, CornerRadius::same(16), p.hover);
        ui.painter().rect_stroke(
            drawer,
            CornerRadius::same(16),
            Stroke::new(1.0, p.divider),
            egui::StrokeKind::Inside,
        );
        ui.scope_builder(egui::UiBuilder::new().max_rect(drawer.shrink(4.0)), |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            ui.set_width(208.0);
            let (row, resp) = ui.allocate_exact_size(Vec2::new(208.0, 40.0), Sense::click());
            if resp.hovered() {
                ui.painter()
                    .rect_filled(row, CornerRadius::same(8), p.hover);
            }
            let picture = None;
            wa_avatar(
                ui,
                p,
                egui::pos2(row.left() + 18.0, row.center().y),
                26.0,
                &self.tg_name(),
                picture,
                None,
            );
            ui.painter().text(
                egui::pos2(row.left() + 38.0, row.center().y),
                egui::Align2::LEFT_CENTER,
                self.tg_name(),
                theme::medium(14.0),
                p.text,
            );
            if resp.clicked() {
                open = Some(TgSection::Profile);
            }
            ui.add_space(4.0);
            let line = ui.available_width();
            let (bar, _) = ui.allocate_exact_size(Vec2::new(line, 9.0), Sense::hover());
            ui.painter().hline(
                bar.x_range().shrink(8.0),
                bar.center().y,
                Stroke::new(1.0, p.divider),
            );
            // A macro keeps the shared drawer row inline; a closure
            // would hold `ui` and `tg_textures` borrowed for the whole
            // menu.
            macro_rules! item {
                ($icon:expr, $label:expr, $section:expr, $active:expr) => {{
                    if tg_menu_item(ui, &mut self.tg_textures, p, $icon, $label, $active) {
                        open = Some($section);
                    }
                }};
            }
            item!(
                icons::TgIcon::Add,
                "Add Account",
                TgSection::Info(TgInfo::AddAccount),
                false
            );
            item!(
                icons::TgIcon::User,
                "My Profile",
                TgSection::Profile,
                matches!(self.tg_section, TgSection::Profile)
            );
            item!(
                icons::TgIcon::SavedMessages,
                "Saved Messages",
                TgSection::Info(TgInfo::Saved),
                false
            );
            item!(
                icons::TgIcon::Group,
                "Contacts",
                TgSection::Contacts,
                matches!(self.tg_section, TgSection::Contacts)
            );
            item!(
                icons::TgIcon::Settings,
                "Settings",
                TgSection::Settings,
                matches!(self.tg_section, TgSection::Settings)
            );
            let more = tg_menu_item(
                ui,
                &mut self.tg_textures,
                p,
                icons::TgIcon::More,
                "More",
                self.tg_more,
            );
            if more {
                self.tg_more = !self.tg_more;
            }
        });
        if self.tg_more {
            // Second popup, top-aligned with the More row. It floats over the
            // chat list, so it is an Area rather than part of the drawer.
            let at = egui::pos2(drawer.right() + 6.0, drawer.top() + 217.0);
            let mut pick = None;
            egui::Area::new(egui::Id::new("tg-more-menu"))
                .order(egui::Order::Foreground)
                .fixed_pos(at)
                .show(ui.ctx(), |ui| {
                    for (offset, alpha) in [(6.0, 40), (3.0, 60)] {
                        ui.painter().rect_filled(
                            Rect::from_min_size(
                                at + Vec2::new(0.0, offset),
                                Vec2::new(216.0, 238.0),
                            ),
                            CornerRadius::same(18),
                            Color32::from_black_alpha(alpha),
                        );
                    }
                    Frame::new()
                        .fill(p.hover)
                        .stroke(Stroke::new(1.0, p.divider))
                        .corner_radius(16)
                        .inner_margin(Margin::symmetric(4, 4))
                        .show(ui, |ui| {
                            ui.spacing_mut().item_spacing.y = 0.0;
                            ui.set_width(208.0);
                            if tg_menu_toggle(
                                ui,
                                &mut self.tg_textures,
                                p,
                                icons::TgIcon::Darkmode,
                                if self.mode == Mode::Dark {
                                    "Night Mode"
                                } else {
                                    "Day Mode"
                                },
                                self.mode == Mode::Dark,
                            ) {
                                night = true;
                            }
                            if tg_menu_item(
                                ui,
                                &mut self.tg_textures,
                                p,
                                icons::TgIcon::Animations,
                                "UI Features",
                                false,
                            ) {
                                pick = Some(TgSection::Info(TgInfo::UiFeatures));
                            }
                            let line = ui.available_width();
                            let (bar, _) =
                                ui.allocate_exact_size(Vec2::new(line, 7.0), Sense::hover());
                            ui.painter().hline(
                                bar.x_range().shrink(8.0),
                                bar.center().y,
                                Stroke::new(1.0, p.divider),
                            );
                            if tg_menu_item(
                                ui,
                                &mut self.tg_textures,
                                p,
                                icons::TgIcon::Help,
                                "Telegram Features",
                                false,
                            ) {
                                pick = Some(TgSection::Info(TgInfo::Features));
                            }
                            if tg_menu_item(
                                ui,
                                &mut self.tg_textures,
                                p,
                                icons::TgIcon::Bug,
                                "Report a Bug",
                                false,
                            ) {
                                pick = Some(TgSection::Info(TgInfo::Bug));
                            }
                            if tg_menu_item(
                                ui,
                                &mut self.tg_textures,
                                p,
                                icons::TgIcon::Web,
                                "Switch to K Version",
                                false,
                            ) {
                                pick = Some(TgSection::Info(TgInfo::KVersion));
                            }
                            if tg_menu_item(
                                ui,
                                &mut self.tg_textures,
                                p,
                                icons::TgIcon::Install,
                                "Install App",
                                false,
                            ) {
                                pick = Some(TgSection::Info(TgInfo::Install));
                            }
                            let (foot, _) =
                                ui.allocate_exact_size(Vec2::new(208.0, 26.0), Sense::hover());
                            ui.painter().text(
                                foot.center(),
                                egui::Align2::CENTER_CENTER,
                                "Telegram Web A 13.0.2",
                                theme::regular(11.0),
                                p.text_muted,
                            );
                        });
                });
            if let Some(section) = pick {
                open = Some(section);
            }
        }
        if night {
            let value = if self.mode == Mode::Dark {
                united_core::settings::Theme::Light
            } else {
                united_core::settings::Theme::Dark
            };
            self.settings.theme = value;
            self.mode = if value == united_core::settings::Theme::Dark {
                Mode::Dark
            } else {
                Mode::Light
            };
            apply_mode(ui.ctx(), self.mode);
            self.settings.save();
        }
        if let Some(section) = open {
            self.tg_open(section);
        }
    }

    fn tg_contacts_pane(&mut self, ui: &mut egui::Ui, p: &Palette) {
        Frame::new()
            .inner_margin(Margin::symmetric(12, 12))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    if tg_button(
                        ui,
                        &mut self.tg_textures,
                        icons::TgIcon::ArrowLeft,
                        40.0,
                        24.0,
                        p.text,
                    )
                    .clicked()
                    {
                        self.tg_open(TgSection::Chats);
                    }
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new("Contacts")
                            .font(theme::semibold(17.0))
                            .color(p.text),
                    );
                });
                ui.add_space(6.0);
                self.tg_search_box(ui, p, "Search contacts", ui.available_width());
            });
        let query = self.search.trim().to_lowercase();
        let mut contacts: Vec<(String, String)> = self
            .tg_contacts
            .iter()
            .filter(|(id, name)| {
                query.is_empty() || name.to_lowercase().contains(&query) || id.contains(&query)
            })
            .cloned()
            .collect();
        contacts.sort_by(|a, b| a.1.cmp(&b.1));
        let mut open_contact = None;
        ScrollArea::vertical()
            .id_salt("tg-contacts")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add_space(4.0);
                Frame::new()
                    .inner_margin(Margin::symmetric(8, 0))
                    .show(ui, |ui| {
                        let (rect, resp) = ui.allocate_exact_size(
                            Vec2::new(ui.available_width(), 56.0),
                            Sense::click(),
                        );
                        if resp.hovered() {
                            ui.painter()
                                .rect_filled(rect, CornerRadius::same(10), p.hover);
                        }
                        ui.painter().circle_filled(
                            egui::pos2(rect.left() + 28.0, rect.center().y),
                            18.0,
                            p.accent,
                        );
                        self.tg_textures.paint(
                            ui.ctx(),
                            ui.painter(),
                            Rect::from_center_size(
                                egui::pos2(rect.left() + 28.0, rect.center().y),
                                Vec2::splat(20.0),
                            ),
                            icons::TgIcon::AddUser,
                            Color32::WHITE,
                        );
                        ui.painter().text(
                            egui::pos2(rect.left() + 56.0, rect.center().y),
                            egui::Align2::LEFT_CENTER,
                            "New Contact",
                            theme::medium(14.0),
                            p.accent,
                        );
                        if resp.clicked() {
                            open_contact = Some(String::new());
                        }
                    });
                for (id, name) in &contacts {
                    Frame::new()
                        .inner_margin(Margin::symmetric(8, 0))
                        .show(ui, |ui| {
                            let (rect, resp) = ui.allocate_exact_size(
                                Vec2::new(ui.available_width(), 56.0),
                                Sense::click(),
                            );
                            if resp.hovered() {
                                ui.painter()
                                    .rect_filled(rect, CornerRadius::same(10), p.hover);
                            }
                            wa_avatar(
                                ui,
                                p,
                                egui::pos2(rect.left() + 28.0, rect.center().y),
                                40.0,
                                name,
                                None,
                                None,
                            );
                            ui.painter().text(
                                egui::pos2(rect.left() + 56.0, rect.center().y - 8.0),
                                egui::Align2::LEFT_CENTER,
                                elide(name, rect.width() - 70.0, 14.0),
                                theme::regular(14.0),
                                p.text,
                            );
                            ui.painter().text(
                                egui::pos2(rect.left() + 56.0, rect.center().y + 10.0),
                                egui::Align2::LEFT_CENTER,
                                elide(id, rect.width() - 70.0, 11.5),
                                theme::regular(11.5),
                                p.text_muted,
                            );
                            if resp.clicked() {
                                open_contact = Some(id.clone());
                            }
                        });
                }
                if contacts.is_empty() {
                    ui.add_space(60.0);
                    ui.vertical_centered(|ui| {
                        ui.label(
                            RichText::new("No contacts synced")
                                .font(theme::medium(14.0))
                                .color(p.text_muted),
                        );
                    });
                }
                ui.add_space(8.0);
            });
        if let Some(id) = open_contact {
            if id.is_empty() {
                self.tg_open(TgSection::Info(TgInfo::NewContact));
            } else {
                self.tg_contact = Some(id);
            }
        }
    }

    fn tg_settings_pane(&mut self, ui: &mut egui::Ui, p: &Palette) {
        Frame::new()
            .inner_margin(Margin::symmetric(12, 12))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    if tg_button(
                        ui,
                        &mut self.tg_textures,
                        icons::TgIcon::ArrowLeft,
                        40.0,
                        24.0,
                        p.text,
                    )
                    .clicked()
                    {
                        self.tg_open(TgSection::Chats);
                    }
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new("Settings")
                            .font(theme::semibold(17.0))
                            .color(p.text),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let _ = tg_button(
                            ui,
                            &mut self.tg_textures,
                            icons::TgIcon::More,
                            32.0,
                            18.0,
                            p.text_muted,
                        );
                        let _ = tg_button(
                            ui,
                            &mut self.tg_textures,
                            icons::TgIcon::Edit,
                            32.0,
                            18.0,
                            p.text_muted,
                        );
                    });
                });
                ui.add_space(6.0);
                self.tg_search_box(ui, p, "Search settings", ui.available_width());
            });
        ScrollArea::vertical()
            .id_salt("tg-settings")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add_space(10.0);
                ui.vertical_centered(|ui| {
                    let picture = None;
                    wa_avatar(
                        ui,
                        p,
                        ui.cursor().min + Vec2::new(ui.available_width() / 2.0, 54.0),
                        108.0,
                        &self.tg_name(),
                        picture,
                        None,
                    );
                    ui.add_space(112.0);
                    ui.label(
                        RichText::new(self.tg_name())
                            .font(theme::semibold(20.0))
                            .color(p.text),
                    );
                    ui.label(
                        RichText::new("online")
                            .font(theme::regular(13.0))
                            .color(p.text_muted),
                    );
                });
                ui.add_space(14.0);
                Frame::new()
                    .inner_margin(Margin::symmetric(10, 0))
                    .show(ui, |ui| {
                        let query = self.search.trim().to_lowercase();
                        let rows: [(TgSettingsTab, icons::TgIcon, Color32, &str, &str); 8] = [
                            (
                                TgSettingsTab::Account,
                                icons::TgIcon::User,
                                Color32::from_rgb(0x33, 0x90, 0xec),
                                "Account",
                                "ID, username, bio",
                            ),
                            (
                                TgSettingsTab::General,
                                icons::TgIcon::Settings,
                                Color32::from_rgb(0xe0, 0xa1, 0x3a),
                                "General Settings",
                                "Wallpaper, theme, keyboard",
                            ),
                            (
                                TgSettingsTab::Notifications,
                                icons::TgIcon::Notifications,
                                Color32::from_rgb(0xe5, 0x53, 0x4b),
                                "Notifications",
                                "Sounds, badges",
                            ),
                            (
                                TgSettingsTab::Privacy,
                                icons::TgIcon::Lock,
                                Color32::from_rgb(0x3a, 0x9e, 0x9a),
                                "Privacy and Security",
                                "Last seen, blocked users",
                            ),
                            (
                                TgSettingsTab::Data,
                                icons::TgIcon::Folder,
                                Color32::from_rgb(0x4f, 0xae, 0x4e),
                                "Data and Storage",
                                "Network, media cache",
                            ),
                            (
                                TgSettingsTab::Devices,
                                icons::TgIcon::Devices,
                                Color32::from_rgb(0x8b, 0x6e, 0xe8),
                                "Devices",
                                "Active sessions",
                            ),
                            (
                                TgSettingsTab::Language,
                                icons::TgIcon::Language,
                                Color32::from_rgb(0x33, 0x90, 0xec),
                                "Language",
                                "English",
                            ),
                            (
                                TgSettingsTab::Stickers,
                                icons::TgIcon::Stickers,
                                Color32::from_rgb(0xe0, 0xa1, 0x3a),
                                "Stickers and Emoji",
                                "Masks, animated",
                            ),
                        ];
                        for (tab, icon, color, title, subtitle) in rows {
                            if !query.is_empty() && !title.to_lowercase().contains(&query) {
                                continue;
                            }
                            if tg_settings_row(
                                ui,
                                &mut self.tg_textures,
                                p,
                                icon,
                                color,
                                title,
                                subtitle,
                                self.tg_settings_tab == tab,
                            ) {
                                self.tg_settings_tab = tab;
                                self.tg_section = TgSection::Settings;
                            }
                        }
                    });
                ui.add_space(16.0);
            });
    }

    fn tg_detail(&mut self, ui: &mut egui::Ui) {
        let p = theme::service_palette(self.mode, Service::Telegram);
        egui::CentralPanel::default()
            .frame(Frame::new().fill(p.chat_bg))
            .show(ui, |ui| match self.tg_section {
                TgSection::Contacts => self.tg_contact_detail(ui, &p),
                TgSection::Settings => self.tg_settings_detail(ui, &p),
                TgSection::Profile => self.tg_profile_detail(ui, &p),
                TgSection::Info(kind) => self.tg_info_detail(ui, &p, kind),
                TgSection::Chats => {}
            });
    }

    fn tg_contact_detail(&mut self, ui: &mut egui::Ui, p: &Palette) {
        let Some(id) = self.tg_contact.clone() else {
            tg_placeholder(ui, p, "Select a contact to see their profile.");
            return;
        };
        let name = self
            .tg_contacts
            .iter()
            .find(|(contact, _)| *contact == id)
            .map(|(_, name)| name.clone())
            .unwrap_or_else(|| id.clone());
        let mut message = false;
        tg_detail_frame(ui, p, |ui| {
            let picture = None;
            wa_avatar(
                ui,
                p,
                ui.cursor().min + Vec2::new(ui.available_width() / 2.0, 54.0),
                108.0,
                &name,
                picture,
                None,
            );
            ui.add_space(112.0);
            ui.label(
                RichText::new(&name)
                    .font(theme::semibold(21.0))
                    .color(p.text),
            );
            ui.label(
                RichText::new("Telegram contact")
                    .font(theme::regular(13.0))
                    .color(p.text_muted),
            );
            ui.add_space(18.0);
            tg_fact_row(ui, p, "User ID", &id);
            ui.add_space(10.0);
            message = tg_pill_button(ui, p, "Send Message");
        });
        if message {
            if let Some(chat) = self.chats.iter().find(|c| c.title == name).cloned() {
                self.tg_open(TgSection::Chats);
                self.open_chat(chat);
            } else {
                self.toast = Some((format!("No conversation with {name} yet"), Instant::now()));
            }
        }
    }

    fn tg_profile_detail(&mut self, ui: &mut egui::Ui, p: &Palette) {
        let name = self.tg_name();
        let id = self.tg_id();
        tg_detail_frame(ui, p, |ui| {
            let picture = None;
            wa_avatar(
                ui,
                p,
                ui.cursor().min + Vec2::new(ui.available_width() / 2.0, 54.0),
                108.0,
                &name,
                picture,
                None,
            );
            ui.add_space(112.0);
            ui.label(
                RichText::new(&name)
                    .font(theme::semibold(21.0))
                    .color(p.text),
            );
            ui.label(
                RichText::new("online")
                    .font(theme::regular(13.0))
                    .color(p.text_muted),
            );
            ui.add_space(18.0);
            tg_fact_row(ui, p, "Telegram ID", &id);
            tg_fact_row(ui, p, "Username", "Not set");
            tg_fact_row(ui, p, "Bio", "Not set");
            tg_fact_row(ui, p, "Session", "MTProto, this install");
        });
    }

    fn tg_info_detail(&mut self, ui: &mut egui::Ui, p: &Palette, kind: TgInfo) {
        let mut back = false;
        ui.add_space(70.0);
        ui.vertical_centered(|ui| {
            Frame::new()
                .fill(p.header)
                .corner_radius(14)
                .inner_margin(Margin::symmetric(24, 22))
                .show(ui, |ui| {
                    ui.set_width(280.0);
                    ui.vertical_centered(|ui| {
                        let (rect, _) = ui.allocate_exact_size(Vec2::splat(64.0), Sense::hover());
                        self.tg_textures.paint(
                            ui.ctx(),
                            ui.painter(),
                            Rect::from_center_size(rect.center(), Vec2::splat(52.0)),
                            kind.icon(),
                            p.accent,
                        );
                        ui.add_space(10.0);
                        ui.label(
                            RichText::new(kind.title())
                                .font(theme::semibold(17.0))
                                .color(p.text),
                        );
                        ui.add_space(8.0);
                        ui.label(
                            RichText::new(kind.body())
                                .font(theme::regular(12.5))
                                .color(p.text_muted),
                        );
                        ui.add_space(16.0);
                        back = tg_pill_button(ui, p, "Back to Chats");
                    });
                });
        });
        if back {
            self.tg_open(TgSection::Chats);
        }
    }

    fn tg_settings_detail(&mut self, ui: &mut egui::Ui, p: &Palette) {
        let title = match self.tg_settings_tab {
            TgSettingsTab::Account => "Account",
            TgSettingsTab::General => "General Settings",
            TgSettingsTab::Notifications => "Notifications",
            TgSettingsTab::Privacy => "Privacy and Security",
            TgSettingsTab::Data => "Data and Storage",
            TgSettingsTab::Devices => "Devices",
            TgSettingsTab::Language => "Language",
            TgSettingsTab::Stickers => "Stickers and Emoji",
        };
        ScrollArea::vertical()
            .id_salt("tg-settings-detail")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add_space(24.0);
                let avail = ui.available_width();
                let card_w = 556.0_f32.min((avail - 32.0).max(280.0));
                ui.horizontal(|ui| {
                    ui.add_space(((avail - card_w) / 2.0).max(16.0));
                    ui.vertical(|ui| {
                        ui.set_width(card_w);
                        ui.label(
                            RichText::new(title)
                                .font(theme::semibold(20.0))
                                .color(p.text),
                        );
                        ui.add_space(12.0);
                        Frame::new()
                            .fill(p.header)
                            .corner_radius(14)
                            .inner_margin(Margin::symmetric(18, 12))
                            .show(ui, |ui| {
                                ui.set_width(card_w - 36.0);
                                match self.tg_settings_tab {
                                TgSettingsTab::Account => {
                                    let name = self.tg_name();
                                    let id = self.tg_id();
                                    ui.horizontal(|ui| {
                                        let picture = None;
                                        wa_avatar(
                                            ui,
                                            p,
                                            ui.cursor().min + Vec2::new(28.0, 28.0),
                                            56.0,
                                            &name,
                                            picture,
                                            None,
                                        );
                                        ui.add_space(64.0);
                                        ui.vertical(|ui| {
                                            ui.label(
                                                RichText::new(&name)
                                                    .font(theme::semibold(16.0))
                                                    .color(p.text),
                                            );
                                            ui.label(
                                                RichText::new("online")
                                                    .font(theme::regular(12.0))
                                                    .color(p.text_muted),
                                            );
                                        });
                                    });
                                    ui.add_space(8.0);
                                    tg_fact_row(ui, p, "Telegram ID", &id);
                                    tg_fact_row(ui, p, "Username", "Not set");
                                    tg_fact_row(ui, p, "Bio", "Not set");
                                }
                                TgSettingsTab::General => {
                                    ui.add_space(6.0);
                                    ui.label(
                                        RichText::new("Theme")
                                            .font(theme::regular(13.0))
                                            .color(p.text_muted),
                                    );
                                    ui.add_space(6.0);
                                    let current = match self.settings.theme {
                                        united_core::settings::Theme::System => 0,
                                        united_core::settings::Theme::Light => 1,
                                        united_core::settings::Theme::Dark => 2,
                                    };
                                    if let Some(picked) =
                                        widgets::segmented(ui, p, current, &["System", "Light", "Dark"])
                                    {
                                        let value = match picked {
                                            1 => united_core::settings::Theme::Light,
                                            2 => united_core::settings::Theme::Dark,
                                            _ => united_core::settings::Theme::System,
                                        };
                                        self.settings.theme = value;
                                        self.mode = match value {
                                            united_core::settings::Theme::Light => Mode::Light,
                                            united_core::settings::Theme::Dark => Mode::Dark,
                                            united_core::settings::Theme::System => {
                                                detect_system_mode(ui.ctx())
                                            }
                                        };
                                        apply_mode(ui.ctx(), self.mode);
                                        self.settings.save();
                                    }
                                    ui.add_space(10.0);
                                    tg_fact_row(ui, p, "Chat wallpaper", "Doodle pattern, dark");
                                    tg_fact_row(ui, p, "Message text size", "Default");
                                }
                                TgSettingsTab::Notifications => {
                                    let mut desktop = self.settings.desktop_notifications;
                                    if tg_toggle_row(
                                        ui,
                                        p,
                                        "Desktop notifications",
                                        "Show a system banner when the window is not focused",
                                        &mut desktop,
                                    ) {
                                        self.settings.desktop_notifications = desktop;
                                        self.settings.save();
                                    }
                                    let mut close = self.settings.minimize_on_close;
                                    if tg_toggle_row(
                                        ui,
                                        p,
                                        "Close to tray",
                                        "Keep United running when the window closes",
                                        &mut close,
                                    ) {
                                        self.settings.minimize_on_close = close;
                                        self.settings.save();
                                    }
                                    let mut startup = self.settings.launch_at_startup;
                                    if tg_toggle_row(
                                        ui,
                                        p,
                                        "Launch at startup",
                                        "Start United with the desktop session",
                                        &mut startup,
                                    ) {
                                        self.settings.launch_at_startup = startup;
                                        AppSettings::apply_autostart(startup);
                                        self.settings.save();
                                    }
                                }
                                TgSettingsTab::Privacy => {
                                    tg_fact_row(ui, p, "Last seen", "Managed by Telegram");
                                    tg_fact_row(ui, p, "Read receipts", "Sent with every message");
                                    tg_fact_row(ui, p, "Blocked users", "Manage in Telegram");
                                    ui.add_space(8.0);
                                    ui.label(
                                        RichText::new(
                                            "These switches live on Telegram's servers; changing them here would not reach the phone.",
                                        )
                                        .font(theme::regular(12.0))
                                        .color(p.text_muted),
                                    );
                                }
                                TgSettingsTab::Data => {
                                    let chats = self.chats.len();
                                    let cached: usize = self.messages.values().map(Vec::len).sum();
                                    tg_fact_row(ui, p, "Chats", &chats.to_string());
                                    tg_fact_row(ui, p, "Messages in memory", &cached.to_string());
                                    tg_fact_row(ui, p, "Media cache", "Managed automatically");
                                }
                                TgSettingsTab::Devices => {
                                    tg_fact_row(ui, p, "This device", "United on Linux");
                                    tg_fact_row(ui, p, "Session", "MTProto, this install");
                                    tg_fact_row(ui, p, "Other devices", "Not listed by the adapter");
                                }
                                TgSettingsTab::Language => {
                                    tg_fact_row(ui, p, "Interface", "English");
                                    tg_fact_row(ui, p, "Messages", "As sent");
                                }
                                TgSettingsTab::Stickers => {
                                    tg_fact_row(ui, p, "Stickers", "Rendered as images");
                                    tg_fact_row(ui, p, "Animated stickers", "First frame only");
                                    tg_fact_row(ui, p, "Emoji", "System font");
                                }
                                }
                            });
                        ui.add_space(24.0);
                    });
                });
            });
    }

    // --------------------------------------------------------------- Discord

    fn dc_open(&mut self, section: DcSection) {
        self.dc_section = section;
        self.search.clear();
    }

    /// Discord's 72px guild rail and 300px DM sidebar, on top of United's
    /// global rail like the WhatsApp and Telegram columns.
    fn dc_columns(&mut self, ui: &mut egui::Ui) {
        let p = theme::service_palette(self.mode, Service::Discord);
        egui::Panel::left("dc-rail")
            .exact_size(72.0)
            .frame(Frame::new().fill(p.panel))
            .show(ui, |ui| self.dc_rail(ui, &p));
        egui::Panel::left("dc-sidebar")
            .exact_size(300.0)
            .frame(Frame::new().fill(p.panel))
            .show(ui, |ui| self.dc_sidebar(ui, &p));
    }

    fn dc_rail(&mut self, ui: &mut egui::Ui, p: &Palette) {
        let mut go = None;
        ui.vertical_centered(|ui| {
            ui.add_space(12.0);
            if dc_guild_tile(
                ui,
                &mut self.dc_textures,
                p,
                icons::DcIcon::Home,
                !matches!(self.dc_section, DcSection::AddServer | DcSection::Discover),
            )
            .on_hover_text("Direct Messages")
            .clicked()
            {
                go = Some(DcSection::Friends);
            }
            ui.add_space(10.0);
            let (line, _) = ui.allocate_exact_size(Vec2::new(32.0, 2.0), Sense::hover());
            ui.painter()
                .rect_filled(line, CornerRadius::same(1), p.divider);
            ui.add_space(10.0);
            if dc_guild_tile(
                ui,
                &mut self.dc_textures,
                p,
                icons::DcIcon::AddServer,
                matches!(self.dc_section, DcSection::AddServer),
            )
            .on_hover_text("Add a Server")
            .clicked()
            {
                go = Some(DcSection::AddServer);
            }
            ui.add_space(8.0);
            if dc_guild_tile(
                ui,
                &mut self.dc_textures,
                p,
                icons::DcIcon::Discover,
                matches!(self.dc_section, DcSection::Discover),
            )
            .on_hover_text("Discover")
            .clicked()
            {
                go = Some(DcSection::Discover);
            }
        });
        if let Some(section) = go {
            self.dc_open(section);
        }
    }

    fn dc_sidebar(&mut self, ui: &mut egui::Ui, p: &Palette) {
        let full = ui.max_rect();
        let mut go = None;
        let mut open_chat = None;
        let mut focus_search = false;
        let mut settings = false;
        Frame::new()
            .inner_margin(Margin::symmetric(8, 8))
            .show(ui, |ui| {
                // Discord stacks its rows flush; leftover item spacing would
                // push the last DM under the pinned user bar.
                ui.spacing_mut().item_spacing.y = 0.0;
                // Search pill. Discord centres the hint, so the field is
                // placed over a painted pill instead of being the pill.
                let (rect, resp) =
                    ui.allocate_exact_size(Vec2::new(ui.available_width(), 40.0), Sense::click());
                ui.painter()
                    .rect_filled(rect, CornerRadius::same(8), p.input_bg);
                let field_rect = Rect::from_min_max(
                    egui::pos2(rect.left() + 12.0, rect.top() + 9.0),
                    egui::pos2(rect.right() - 12.0, rect.bottom() - 9.0),
                );
                let field = ui.put(
                    field_rect,
                    egui::TextEdit::singleline(&mut self.search)
                        .id(egui::Id::new("dc-search"))
                        .hint_text(
                            RichText::new("Find or start a conversation")
                                .font(theme::regular(13.5))
                                .color(p.text_muted),
                        )
                        .frame(egui::Frame::NONE)
                        .desired_width(field_rect.width())
                        .font(theme::regular(13.5))
                        .text_color(p.text),
                );
                if resp.clicked() {
                    field.request_focus();
                }
                ui.add_space(8.0);
                if dc_nav_row(
                    ui,
                    &mut self.dc_textures,
                    p,
                    icons::DcIcon::Friends,
                    "Friends",
                    matches!(self.dc_section, DcSection::Friends),
                ) {
                    go = Some(DcSection::Friends);
                }
                if dc_nav_row(
                    ui,
                    &mut self.dc_textures,
                    p,
                    icons::DcIcon::Nitro,
                    "Nitro",
                    matches!(self.dc_section, DcSection::Nitro),
                ) {
                    go = Some(DcSection::Nitro);
                }
                if dc_nav_row(
                    ui,
                    &mut self.dc_textures,
                    p,
                    icons::DcIcon::Shop,
                    "Shop",
                    matches!(self.dc_section, DcSection::Shop),
                ) {
                    go = Some(DcSection::Shop);
                }
                if dc_nav_row(
                    ui,
                    &mut self.dc_textures,
                    p,
                    icons::DcIcon::Quests,
                    "Quests",
                    matches!(self.dc_section, DcSection::Quests),
                ) {
                    go = Some(DcSection::Quests);
                }
                ui.add_space(8.0);
                // "Direct Messages" heading with Discord's plus button.
                let (head, _) =
                    ui.allocate_exact_size(Vec2::new(ui.available_width(), 28.0), Sense::hover());
                ui.painter().text(
                    egui::pos2(head.left() + 4.0, head.center().y),
                    egui::Align2::LEFT_CENTER,
                    "Direct Messages",
                    theme::semibold(11.5),
                    p.text_muted,
                );
                let plus = Rect::from_center_size(
                    egui::pos2(head.right() - 10.0, head.center().y),
                    Vec2::splat(24.0),
                );
                let plus_resp = ui.interact(plus, ui.id().with("dc-new-dm"), Sense::click());
                if plus_resp.hovered() {
                    ui.painter()
                        .rect_filled(plus, CornerRadius::same(6), p.selected);
                }
                self.dc_textures.paint(
                    ui.ctx(),
                    ui.painter(),
                    Rect::from_center_size(plus.center(), Vec2::splat(16.0)),
                    icons::DcIcon::Plus,
                    p.text_muted,
                );
                if plus_resp.clicked() {
                    focus_search = true;
                }
                let query = self.search.trim().to_lowercase();
                let mut rows: Vec<Chat> = self
                    .chats
                    .iter()
                    .filter(|c| {
                        query.is_empty()
                            || c.title.to_lowercase().contains(&query)
                            || c.preview.to_lowercase().contains(&query)
                    })
                    .cloned()
                    .collect();
                rows.sort_by_key(|c| std::cmp::Reverse(c.last_at));
                let active = self.active_chat.get(&Service::Discord).cloned();
                // Stop above the pinned user bar instead of letting the last
                // row sit under it.
                let list_h = (full.bottom() - 76.0 - ui.cursor().min.y).max(80.0);
                ScrollArea::vertical()
                    .id_salt("dc-dms")
                    .max_height(list_h)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        // Discord's rows sit flush; egui's default 8px gap
                        // would push the fifth DM under the user bar.
                        ui.spacing_mut().item_spacing.y = 0.0;
                        ui.add_space(2.0);
                        for chat in &rows {
                            let selected = active
                                .as_ref()
                                .is_some_and(|(a, id)| *a == chat.account.0 && *id == chat.id);
                            if self.dc_dm_row(ui, p, chat, selected).clicked() {
                                open_chat = Some(chat.clone());
                            }
                        }
                        if rows.is_empty() {
                            ui.add_space(40.0);
                            ui.vertical_centered(|ui| {
                                ui.label(
                                    RichText::new(if query.is_empty() {
                                        "No direct messages"
                                    } else {
                                        "No matches"
                                    })
                                    .font(theme::medium(13.0))
                                    .color(p.text_muted),
                                );
                            });
                        }
                        ui.add_space(4.0);
                    });
            });

        // User bar pinned to the sidebar's bottom edge.
        let bar = Rect::from_min_max(
            egui::pos2(full.left() + 8.0, full.bottom() - 66.0),
            egui::pos2(full.right() - 8.0, full.bottom() - 8.0),
        );
        ui.painter()
            .rect_filled(bar, CornerRadius::same(8), p.hover);
        let picture = None;
        wa_avatar(
            ui,
            p,
            egui::pos2(bar.left() + 24.0, bar.center().y),
            32.0,
            &self.dc_name(),
            picture,
            None,
        );
        ui.painter().text(
            egui::pos2(bar.left() + 48.0, bar.center().y - 8.0),
            egui::Align2::LEFT_CENTER,
            elide(&self.dc_name(), bar.width() - 170.0, 13.0),
            theme::medium(13.5),
            p.text,
        );
        ui.painter().text(
            egui::pos2(bar.left() + 48.0, bar.center().y + 8.0),
            egui::Align2::LEFT_CENTER,
            "Online",
            theme::regular(11.0),
            p.text_muted,
        );
        let mut x = bar.right() - 8.0 - 16.0;
        for (icon, on, which) in [
            (Icon::Mic, !self.dc_muted, 0u8),
            (Icon::Headphones, !self.dc_deafened, 1u8),
            (Icon::Gear, true, 2u8),
        ] {
            let at = egui::pos2(x, bar.center().y);
            x -= 36.0;
            let rect = Rect::from_center_size(at, Vec2::splat(32.0));
            let resp = ui.interact(rect, ui.id().with(("dc-voice", which)), Sense::click());
            if resp.hovered() {
                ui.painter()
                    .rect_filled(rect, CornerRadius::same(6), p.selected);
            }
            icons::paint(
                ui.painter(),
                Rect::from_center_size(at, Vec2::splat(20.0)),
                icon,
                if on { p.text } else { p.text_muted },
                1.8,
            );
            if !on {
                ui.painter().line_segment(
                    [at + Vec2::new(-8.0, 8.0), at + Vec2::new(8.0, -8.0)],
                    Stroke::new(2.0, Color32::from_rgb(0xda, 0x37, 0x3c)),
                );
            }
            if resp.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            if resp.clicked() {
                match which {
                    0 => self.dc_muted = !self.dc_muted,
                    1 => self.dc_deafened = !self.dc_deafened,
                    _ => settings = true,
                }
            }
        }
        if focus_search {
            ui.memory_mut(|m| m.request_focus(egui::Id::new("dc-search")));
        }
        if let Some(chat) = open_chat {
            self.dc_section = DcSection::Chat;
            self.open_chat(chat);
        } else if let Some(section) = go {
            self.dc_open(section);
        }
        if settings {
            self.view = View::Settings;
        }
    }

    fn dc_name(&self) -> String {
        self.adapters
            .get(&Service::Discord)
            .map(|a| a.label())
            .filter(|l| !l.is_empty())
            .unwrap_or_else(|| "Discord".to_string())
    }

    fn dc_dm_row(
        &mut self,
        ui: &mut egui::Ui,
        p: &Palette,
        chat: &Chat,
        selected: bool,
    ) -> egui::Response {
        let width = ui.available_width();
        let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, 42.0), Sense::click());
        if selected {
            ui.painter()
                .rect_filled(rect, CornerRadius::same(8), p.selected);
        } else if resp.hovered() {
            ui.painter()
                .rect_filled(rect, CornerRadius::same(8), p.hover);
        }
        let picture = self.avatar_texture(ui.ctx(), chat.avatar.as_deref());
        wa_avatar(
            ui,
            p,
            egui::pos2(rect.left() + 24.0, rect.center().y),
            32.0,
            &chat.title,
            picture.as_ref(),
            None,
        );
        let badge = if chat.unread > 0 { 26.0 } else { 0.0 };
        ui.painter().text(
            egui::pos2(rect.left() + 48.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            elide(&chat.title, rect.width() - 58.0 - badge, 15.0),
            theme::medium(15.0),
            p.text,
        );
        if chat.unread > 0 {
            let pill = Rect::from_center_size(
                egui::pos2(rect.right() - 16.0, rect.center().y),
                Vec2::new(20.0, 16.0),
            );
            ui.painter().rect_filled(
                pill,
                CornerRadius::same(8),
                Color32::from_rgb(0xf2, 0x3f, 0x43),
            );
            ui.painter().text(
                pill.center(),
                egui::Align2::CENTER_CENTER,
                if chat.unread > 99 {
                    "99+".to_string()
                } else {
                    chat.unread.to_string()
                },
                theme::semibold(10.0),
                Color32::WHITE,
            );
        }
        if resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        resp
    }

    fn dc_page(&mut self, ui: &mut egui::Ui) {
        let p = theme::service_palette(self.mode, Service::Discord);
        egui::CentralPanel::default()
            .frame(Frame::new().fill(p.chat_bg))
            .show(ui, |ui| match self.dc_section {
                DcSection::Friends => self.dc_friends(ui, &p),
                DcSection::Chat => {}
                section => {
                    let back = dc_info_page(ui, &mut self.dc_textures, &p, section);
                    if back {
                        self.dc_open(DcSection::Friends);
                    }
                }
            });
    }

    fn dc_friends(&mut self, ui: &mut egui::Ui, p: &Palette) {
        let mut open = None;
        let mut add = false;
        Frame::new()
            .inner_margin(Margin::symmetric(16, 0))
            .show(ui, |ui| {
                let (rect, _) =
                    ui.allocate_exact_size(Vec2::new(ui.available_width(), 48.0), Sense::hover());
                self.dc_textures.paint(
                    ui.ctx(),
                    ui.painter(),
                    Rect::from_center_size(
                        egui::pos2(rect.left() + 10.0, rect.center().y),
                        Vec2::splat(20.0),
                    ),
                    icons::DcIcon::Friends,
                    p.text_muted,
                );
                ui.painter().text(
                    egui::pos2(rect.left() + 28.0, rect.center().y),
                    egui::Align2::LEFT_CENTER,
                    "Friends",
                    theme::medium(15.0),
                    p.text,
                );
                ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let (btn, resp) =
                            ui.allocate_exact_size(Vec2::new(100.0, 32.0), Sense::click());
                        ui.painter()
                            .rect_filled(btn, CornerRadius::same(8), p.accent);
                        ui.painter().text(
                            btn.center(),
                            egui::Align2::CENTER_CENTER,
                            "Add Friend",
                            theme::medium(13.0),
                            Color32::WHITE,
                        );
                        if resp.hovered() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                        if resp.clicked() {
                            add = true;
                        }
                        ui.add_space(10.0);
                        for (label, tab, width) in [("All", 0u8, 48.0), ("Online", 1u8, 64.0)] {
                            let (tab_rect, tab_resp) =
                                ui.allocate_exact_size(Vec2::new(width, 32.0), Sense::click());
                            let active = self.dc_tab == tab;
                            if active {
                                ui.painter().rect_filled(
                                    tab_rect,
                                    CornerRadius::same(8),
                                    p.selected,
                                );
                            }
                            ui.painter().text(
                                tab_rect.center(),
                                egui::Align2::CENTER_CENTER,
                                label,
                                theme::regular(13.0),
                                if active { p.text } else { p.text_muted },
                            );
                            if tab_resp.clicked() {
                                self.dc_tab = tab;
                            }
                            ui.add_space(4.0);
                        }
                    });
                });
                let (line, _) =
                    ui.allocate_exact_size(Vec2::new(ui.available_width(), 1.0), Sense::hover());
                ui.painter()
                    .hline(line.x_range(), line.center().y, Stroke::new(1.0, p.divider));
            });

        // Active Now column, Discord's empty right rail.
        egui::Panel::right("dc-now")
            .exact_size(300.0)
            .resizable(false)
            .frame(Frame::new().fill(p.chat_bg))
            .show(ui, |ui| {
                Frame::new()
                    .inner_margin(Margin::symmetric(16, 14))
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new("Active Now")
                                .font(theme::semibold(16.0))
                                .color(p.text),
                        );
                        ui.add_space(10.0);
                        let (line, _) = ui.allocate_exact_size(
                            Vec2::new(ui.available_width(), 1.0),
                            Sense::hover(),
                        );
                        ui.painter().hline(
                            line.x_range(),
                            line.center().y,
                            Stroke::new(1.0, p.divider),
                        );
                        ui.add_space(14.0);
                        Frame::new()
                            .fill(p.hover)
                            .stroke(Stroke::new(1.0, p.divider))
                            .corner_radius(8)
                            .inner_margin(Margin::symmetric(18, 20))
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.vertical_centered(|ui| {
                                    ui.label(
                                        RichText::new("It's quiet for now...")
                                            .font(theme::semibold(13.5))
                                            .color(p.text),
                                    );
                                    ui.add_space(6.0);
                                    ui.label(
                                        RichText::new(
                                            "When a friend starts an activity—like playing a game or hanging out on voice—we'll show it here!",
                                        )
                                        .font(theme::regular(12.0))
                                        .color(p.text_muted),
                                    );
                                });
                            });
                    });
            });

        // Friends list.
        Frame::new()
            .inner_margin(Margin::symmetric(16, 12))
            .show(ui, |ui| {
                let (rect, resp) =
                    ui.allocate_exact_size(Vec2::new(ui.available_width(), 36.0), Sense::click());
                ui.painter()
                    .rect_filled(rect, CornerRadius::same(8), p.input_bg);
                self.dc_textures.paint(
                    ui.ctx(),
                    ui.painter(),
                    Rect::from_center_size(
                        egui::pos2(rect.left() + 16.0, rect.center().y),
                        Vec2::splat(16.0),
                    ),
                    icons::DcIcon::Search,
                    p.text_muted,
                );
                let field_rect = Rect::from_min_max(
                    egui::pos2(rect.left() + 32.0, rect.top() + 7.0),
                    egui::pos2(rect.right() - 10.0, rect.bottom() - 7.0),
                );
                let field = ui.put(
                    field_rect,
                    egui::TextEdit::singleline(&mut self.search)
                        .id(egui::Id::new("dc-friend-search"))
                        .hint_text(
                            RichText::new("Search")
                                .font(theme::regular(13.5))
                                .color(p.text_muted),
                        )
                        .frame(egui::Frame::NONE)
                        .desired_width(field_rect.width())
                        .font(theme::regular(13.5))
                        .text_color(p.text),
                );
                if resp.clicked() {
                    field.request_focus();
                }
                ui.add_space(10.0);
                let query = self.search.trim().to_lowercase();
                let mut rows: Vec<Chat> = self
                    .chats
                    .iter()
                    .filter(|c| {
                        query.is_empty()
                            || c.title.to_lowercase().contains(&query)
                            || c.preview.to_lowercase().contains(&query)
                    })
                    .cloned()
                    .collect();
                rows.sort_by_key(|c| std::cmp::Reverse(c.last_at));
                let online = self.dc_tab == 1;
                ui.painter().text(
                    ui.cursor().min,
                    egui::Align2::LEFT_TOP,
                    if online {
                        "Online — 0".to_string()
                    } else {
                        format!("All — {}", rows.len())
                    },
                    theme::semibold(11.5),
                    p.text_muted,
                );
                ui.add_space(18.0);
                ScrollArea::vertical()
                    .id_salt("dc-friends")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        if online {
                            ui.add_space(30.0);
                            ui.vertical_centered(|ui| {
                                ui.label(
                                    RichText::new("No one is online")
                                        .font(theme::semibold(13.5))
                                        .color(p.text),
                                );
                                ui.add_space(4.0);
                                ui.label(
                                    RichText::new(
                                        "Presence is not exposed by United's Discord adapter.",
                                    )
                                    .font(theme::regular(12.0))
                                    .color(p.text_muted),
                                );
                            });
                        }
                        for chat in &rows {
                            let (row, row_resp) = ui.allocate_exact_size(
                                Vec2::new(ui.available_width(), 48.0),
                                Sense::click(),
                            );
                            if row_resp.hovered() {
                                ui.painter()
                                    .rect_filled(row, CornerRadius::same(8), p.hover);
                            }
                            let picture = self.avatar_texture(ui.ctx(), chat.avatar.as_deref());
                            wa_avatar(
                                ui,
                                p,
                                egui::pos2(row.left() + 24.0, row.center().y),
                                32.0,
                                &chat.title,
                                picture.as_ref(),
                                None,
                            );
                            ui.painter().text(
                                egui::pos2(row.left() + 48.0, row.center().y - 8.0),
                                egui::Align2::LEFT_CENTER,
                                elide(&chat.title, row.width() - 60.0, 14.0),
                                theme::medium(14.5),
                                p.text,
                            );
                            ui.painter().text(
                                egui::pos2(row.left() + 48.0, row.center().y + 9.0),
                                egui::Align2::LEFT_CENTER,
                                elide(
                                    if chat.preview.is_empty() {
                                        "Direct message"
                                    } else {
                                        &chat.preview
                                    },
                                    row.width() - 60.0,
                                    12.0,
                                ),
                                theme::regular(12.0),
                                p.text_muted,
                            );
                            if row_resp.clicked() {
                                open = Some(chat.clone());
                            }
                        }
                    });
            });
        if add {
            self.dc_section = DcSection::AddFriend;
        } else if let Some(chat) = open {
            self.dc_section = DcSection::Chat;
            self.open_chat(chat);
        }
    }

    fn chat_list(&mut self, ui: &mut egui::Ui) {
        // Telegram Web A replaces the whole list column with a floating card,
        // so it brings its own panel instead of sharing the 340px one.
        if matches!(self.view, View::Service(Service::Telegram)) {
            self.tg_column(ui);
            return;
        }
        // Discord keeps its own guild rail and DM sidebar.
        if matches!(self.view, View::Service(Service::Discord)) {
            self.dc_columns(ui);
            return;
        }
        // A service view wears that client's own pane colours, so the list and
        // the conversation read as one window instead of two themes.
        let p = match self.view {
            View::Service(service) => theme::service_palette(self.mode, service),
            _ => self.palette(),
        };
        let unified = self.view == View::Unified;
        let wa = matches!(self.view, View::Service(Service::WhatsApp));
        if wa {
            self.wa_rail(ui);
        }
        // Business tools and the profile use the whole window, not the list
        // column, so the rail is all that is drawn here.
        if wa && matches!(self.wa_section, WaSection::Business | WaSection::Profile) {
            return;
        }
        let width = 340.0;
        egui::Panel::left("chats")
            .exact_size(width)
            .resizable(false)
            .frame(Frame::new().fill(p.panel))
            .show(ui, |ui| {
                let full = ui.max_rect();
                ui.painter().vline(
                    full.right() - HAIRLINE / 2.0,
                    full.y_range(),
                    Stroke::new(HAIRLINE, p.divider),
                );
                if wa && self.wa_section != WaSection::Chats {
                    self.wa_pane(ui, &p);
                    return;
                }

                // Title row: large title left, account label right.
                Frame::new()
                    .inner_margin(Margin::symmetric(16, 14))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            let title = match self.view {
                                View::Service(s) => s.display_name().to_string(),
                                _ => "All Messages".to_string(),
                            };
                            ui.label(
                                RichText::new(title)
                                    .font(theme::semibold(20.0))
                                    .color(p.text),
                            );
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    let account = match self.view {
                                        View::Service(s) => self
                                            .adapters
                                            .get(&s)
                                            .map(|a| a.label())
                                            .unwrap_or_default(),
                                        _ => String::new(),
                                    };
                                    let title = match self.view {
                                        View::Service(s) => s.display_name(),
                                        _ => "All Messages",
                                    };
                                    if let View::Service(service) = self.view {
                                        let connected = matches!(
                                            self.status_of(service),
                                            ServiceStatus::Online | ServiceStatus::Connecting
                                        );
                                        if self.adapters.contains_key(&service) && !connected {
                                            if ui
                                                .add(
                                                    egui::Button::new(
                                                        RichText::new("Link device")
                                                            .font(theme::medium(11.5))
                                                            .color(Color32::WHITE),
                                                    )
                                                    .fill(p.accent)
                                                    .corner_radius(8)
                                                    .min_size(Vec2::new(84.0, 24.0)),
                                                )
                                                .clicked()
                                            {
                                                self.begin_sign_in(service);
                                            }
                                            ui.add_space(8.0);
                                        }
                                    }
                                    if !account.is_empty() && account != title {
                                        ui.label(
                                            RichText::new(account)
                                                .font(theme::regular(11.5))
                                                .color(p.text_muted),
                                        );
                                    }
                                },
                            );
                        });
                    });

                // Search field, inset like a grouped table cell.
                Frame::new()
                    .inner_margin(Margin::symmetric(16, 0))
                    .show(ui, |ui| {
                        Frame::new()
                            .fill(p.input_bg)
                            .corner_radius(10)
                            .inner_margin(Margin::symmetric(10, 7))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    // The magnifier is part of the field: a
                                    // click has to put the caret in it, like
                                    // clicking the icon in WhatsApp Web.
                                    let icon_rect = Rect::from_min_size(
                                        ui.cursor().min + Vec2::new(0.0, 1.0),
                                        Vec2::splat(14.0),
                                    );
                                    let icon = ui.interact(
                                        icon_rect,
                                        ui.id().with("search-icon"),
                                        Sense::click(),
                                    );
                                    if icon.hovered() {
                                        ui.ctx().set_cursor_icon(egui::CursorIcon::Text);
                                    }
                                    icons::paint(
                                        ui.painter(),
                                        icon_rect,
                                        Icon::Search,
                                        p.text_muted,
                                        1.5,
                                    );
                                    ui.add_space(18.0);
                                    let field = ui.add(
                                        egui::TextEdit::singleline(&mut self.search)
                                            .hint_text(
                                                RichText::new("Search")
                                                    .color(p.text_muted)
                                                    .font(theme::regular(13.5)),
                                            )
                                            .frame(egui::Frame::NONE)
                                            .desired_width(f32::INFINITY)
                                            .font(theme::regular(13.5)),
                                    );
                                    if icon.clicked() || std::mem::take(&mut self.search_focus) {
                                        field.request_focus();
                                    }
                                });
                            });
                    });
                // Filter pills replace WhatsApp/Telegram's tab strip. Discord
                // keeps its channel tree, so the pills stay hidden there.
                if matches!(
                    self.view,
                    View::Service(Service::WhatsApp | Service::Telegram)
                ) {
                    Frame::new()
                        .inner_margin(Margin::symmetric(16, 0))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                // WhatsApp Web prints the backlog on the
                                // filter itself ("Unread 99").
                                let unread_total: u32 =
                                    (if unified { &self.unified } else { &self.chats })
                                        .iter()
                                        .map(|c| c.unread)
                                        .sum();
                                let unread_label = if unread_total > 0 {
                                    format!("Unread {unread_total}")
                                } else {
                                    "Unread".to_string()
                                };
                                for (label, flag) in
                                    [("All".to_string(), false), (unread_label, true)]
                                {
                                    let active = self.unread_only == flag;
                                    let text = RichText::new(label)
                                        .font(theme::medium(12.0))
                                        .color(if active {
                                            if p.accent == Color32::from_rgb(0x00, 0xa8, 0x84) {
                                                Color32::from_rgb(0x00, 0x7a, 0x5f)
                                            } else {
                                                p.accent
                                            }
                                        } else {
                                            p.text_muted
                                        });
                                    let button = egui::Button::new(text)
                                        .fill(if active {
                                            p.accent.gamma_multiply(0.14)
                                        } else {
                                            Color32::TRANSPARENT
                                        })
                                        .corner_radius(9)
                                        .min_size(Vec2::new(58.0, 24.0));
                                    if ui.add(button).clicked() {
                                        self.unread_only = flag;
                                    }
                                }
                            });
                        });
                }
                ui.add_space(10.0);

                let list: &[Chat] = if unified { &self.unified } else { &self.chats };
                let query = self.search.trim().to_lowercase();
                let mut filtered: Vec<Chat> = list
                    .iter()
                    .filter(|c| {
                        let archived = self
                            .chat_flags
                            .get(&(c.account.0.clone(), c.id.clone()))
                            .is_some_and(|flags| flags.1);
                        if archived != self.show_archived {
                            return false;
                        }
                        if self.unread_only
                            && matches!(
                                self.view,
                                View::Service(Service::WhatsApp | Service::Telegram)
                            )
                        {
                            if c.unread == 0 {
                                return false;
                            }
                        }
                        query.is_empty()
                            || c.title.to_lowercase().contains(&query)
                            || c.preview.to_lowercase().contains(&query)
                    })
                    .cloned()
                    .collect();
                let pinned_flag = |chat: &Chat| {
                    self.chat_flags
                        .get(&(chat.account.0.clone(), chat.id.clone()))
                        .is_some_and(|flags| flags.0)
                };
                // Pinned conversations stay on top, like WhatsApp Web.
                filtered.sort_by(|a, b| {
                    pinned_flag(b)
                        .cmp(&pinned_flag(a))
                        .then_with(|| b.last_at.cmp(&a.last_at))
                });
                let archived_count = list
                    .iter()
                    .filter(|c| {
                        self.chat_flags
                            .get(&(c.account.0.clone(), c.id.clone()))
                            .is_some_and(|flags| flags.1)
                    })
                    .count();

                ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        if filtered.is_empty() {
                            ui.add_space(48.0);
                            ui.vertical_centered(|ui| {
                                icons::paint(
                                    ui.painter(),
                                    Rect::from_min_size(
                                        ui.cursor().min + Vec2::new(width / 2.0 - 16.0, 0.0),
                                        Vec2::splat(32.0),
                                    ),
                                    Icon::Bubbles,
                                    p.text_muted.gamma_multiply(0.6),
                                    1.6,
                                );
                                ui.add_space(40.0);
                                ui.label(
                                    RichText::new(if query.is_empty() {
                                        "No Conversations"
                                    } else {
                                        "No Results"
                                    })
                                    .font(theme::medium(13.5))
                                    .color(p.text_muted),
                                );
                            });
                        }
                        for (index, chat) in filtered.iter().enumerate() {
                            // Only the head of the list: asking for every
                            // stored chat at once (800+) earns a server
                            // rate-limit and the pictures stop arriving at
                            // all. Opening a chat asks for its own.
                            if index < 15 {
                                self.preload_avatar(chat);
                            }
                            self.resolve_title(chat);
                            let selected = self
                                .active_chat
                                .get(&chat.service)
                                .map(|(a, c)| a == &chat.account.0 && c == &chat.id)
                                .unwrap_or(false);
                            let time = format::chat_time(chat.last_at);
                            let pinned = self
                                .chat_flags
                                .get(&(chat.account.0.clone(), chat.id.clone()))
                                .is_some_and(|flags| flags.0);
                            let picture = self.avatar_texture(ui.ctx(), chat.avatar.as_deref()); // The list shows the same text as the bubbles: a
                            // mention reads as a name there too, and an unsent
                            // draft is tagged the way both web clients do.
                            let draft = self
                                .drafts
                                .get(&(chat.account.0.clone(), chat.id.clone()))
                                .map(|text| text.trim())
                                .filter(|text| !text.is_empty());
                            let display;
                            let chat = if chat.preview.contains('@') || draft.is_some() {
                                display = {
                                    let mut copy = chat.clone();
                                    if copy.preview.contains('@') {
                                        copy.preview =
                                            resolve_mentions(&copy.preview, &self.contact_names).0;
                                    }
                                    if let Some(text) = draft {
                                        let one_line = text.replace('\n', " ");
                                        copy.preview = format!("Draft: {one_line}");
                                        copy.preview_sender.clear();
                                    }
                                    copy
                                };
                                &display
                            } else {
                                chat
                            };
                            if widgets::chat_row(
                                ui,
                                chat,
                                &p,
                                selected,
                                unified,
                                &time,
                                picture.as_ref(),
                                pinned,
                            ) {
                                self.open_chat(chat.clone());
                            }
                        }
                        if query.is_empty() && (archived_count > 0 || self.show_archived) {
                            ui.add_space(4.0);
                            let label = if self.show_archived {
                                "Kembali ke obrolan".to_string()
                            } else {
                                format!("Diarsipkan ({archived_count})")
                            };
                            if ui
                                .add(
                                    egui::Button::new(
                                        RichText::new(label)
                                            .font(theme::medium(12.5))
                                            .color(p.accent),
                                    )
                                    .fill(Color32::TRANSPARENT),
                                )
                                .clicked()
                            {
                                self.show_archived = !self.show_archived;
                            }
                        }
                    });
            });
    }

    /// Right-hand group info, shaped like WhatsApp Web: big picture, name,
    /// member list and the media that was shared in the chat.
    fn chat_info(&mut self, ui: &mut egui::Ui, p: &theme::Palette, chat: &Chat) {
        ui.painter().vline(
            ui.max_rect().left() + HAIRLINE / 2.0,
            ui.max_rect().y_range(),
            Stroke::new(HAIRLINE, p.divider),
        );
        Frame::new()
            .inner_margin(Margin::symmetric(16, 12))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(26.0), Sense::click());
                    if resp.hovered() {
                        ui.painter().circle_filled(rect.center(), 13.0, p.hover);
                    }
                    if resp.clicked() {
                        self.info_open = false;
                    }
                    let ink = Stroke::new(1.4, p.text_muted);
                    let c = rect.center();
                    ui.painter()
                        .line_segment([c + Vec2::new(-5.0, -5.0), c + Vec2::new(5.0, 5.0)], ink);
                    ui.painter()
                        .line_segment([c + Vec2::new(5.0, -5.0), c + Vec2::new(-5.0, 5.0)], ink);
                    ui.add_space(6.0);
                    ui.label(
                        RichText::new(if chat.is_group {
                            "Info grup"
                        } else {
                            "Info kontak"
                        })
                        .font(theme::medium(15.0))
                        .color(p.text),
                    );
                });
            });

        let roster_key = format!("{}|{}", chat.account.0, chat.id);
        if chat.is_group
            && !self.members.contains_key(&roster_key)
            && self
                .member_requests
                .get(&roster_key)
                .is_none_or(|at| at.elapsed() > std::time::Duration::from_secs(60))
            && matches!(self.status_of(chat.service), ServiceStatus::Online)
            && let Some(adapter) = self.adapters.get(&chat.service).cloned()
        {
            self.member_requests
                .insert(roster_key.clone(), Instant::now());
            let tx = self.members_tx.clone();
            let key = roster_key.clone();
            let id = chat.id.clone();
            tokio::spawn(async move {
                let list = adapter.members(&id).await.unwrap_or_default();
                let _ = tx.send((key, list));
            });
        }
        // The roster knows numbers; the address book knows who that number
        // is. The pane shows the person, like the mention picker does.
        let mut members = self.members.get(&roster_key).cloned().unwrap_or_default();
        for (id, name) in members.iter_mut() {
            if let Some(known) = self.contact_names.get(&mention_key(id)) {
                *name = known.clone();
            }
        }
        let picture = self.avatar_texture(ui.ctx(), chat.avatar.as_deref());

        egui::ScrollArea::vertical().show(ui, |ui| {
            Frame::new()
                .inner_margin(Margin::symmetric(16, 8))
                .show(ui, |ui| {
                    ui.add_space(10.0);
                    ui.vertical_centered(|ui| {
                        widgets::avatar(ui, &chat.title, 128.0, &chat.id, picture.as_ref());
                        ui.add_space(10.0);
                        ui.label(
                            RichText::new(&chat.title)
                                .font(theme::semibold(18.0))
                                .color(p.text),
                        );
                        let sub = if chat.is_group {
                            if members.is_empty() {
                                "Grup".to_string()
                            } else {
                                format!("Grup · {} anggota", members.len())
                            }
                        } else {
                            "Kontak".to_string()
                        };
                        ui.label(
                            RichText::new(sub)
                                .font(theme::regular(12.5))
                                .color(p.accent),
                        );
                    });
                    ui.add_space(14.0);
                    ui.separator();
                    if chat.is_group && !members.is_empty() {
                        ui.add_space(8.0);
                        ui.label(
                            RichText::new(format!("{} anggota", members.len()))
                                .font(theme::semibold(13.0))
                                .color(p.text),
                        );
                        for (id, name) in members.iter().take(12) {
                            ui.horizontal(|ui| {
                                widgets::avatar(ui, name, 30.0, id, None);
                                ui.add_space(6.0);
                                ui.label(
                                    RichText::new(name).font(theme::regular(13.5)).color(p.text),
                                );
                            });
                            ui.add_space(4.0);
                        }
                        ui.add_space(6.0);
                        ui.separator();
                    }

                    let key = (chat.account.0.clone(), chat.id.clone());
                    let thumbs: Vec<(String, String)> = self
                        .messages
                        .get(&key)
                        .map(|list| {
                            list.iter()
                                .rev()
                                .filter_map(|msg| {
                                    let media = MediaRef::parse(msg.media.as_deref()?)?;
                                    let raw = media.thumb?;
                                    Some((format!("{}|{}", msg.id, raw.len()), raw))
                                })
                                .take(4)
                                .collect()
                        })
                        .unwrap_or_default();
                    if !thumbs.is_empty() {
                        ui.add_space(10.0);
                        ui.label(
                            RichText::new("Media, tautan, dan dokumen")
                                .font(theme::semibold(13.0))
                                .color(p.text),
                        );
                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            for (thumb_key, raw) in &thumbs {
                                if let Some(texture) = self.media_thumb(ui.ctx(), thumb_key, raw) {
                                    let (rect, _) =
                                        ui.allocate_exact_size(Vec2::splat(62.0), Sense::hover());
                                    egui::Image::new((texture.id(), Vec2::splat(62.0)))
                                        .corner_radius(6)
                                        .paint_at(ui, rect);
                                }
                            }
                        });
                    }

                    ui.add_space(14.0);
                    ui.separator();
                    ui.add_space(6.0);
                    let muted = chat.muted;
                    if ui
                        .button(if muted {
                            "Bunyikan notifikasi"
                        } else {
                            "Senyapkan notifikasi"
                        })
                        .clicked()
                    {
                        if let Ok(Some(mut row)) = self.store.chat(&chat.account.0, &chat.id) {
                            row.muted = !muted;
                            let _ = self.store.upsert_chat(&row);
                            self.refresh_lists();
                        }
                    }
                    ui.add_space(10.0);
                });
        });
    }

    fn conversation(&mut self, ui: &mut egui::Ui) {
        let service = self.active_service();
        let p = match service {
            Some(service) => theme::service_palette(self.mode, service),
            None => self.palette(),
        };
        let chat = self.active_chat_obj();

        egui::CentralPanel::default()
            .frame(Frame::new().fill(p.chat_bg))
            .show(ui, |ui| {
                let Some(service) = service else {
                    self.unified_detail(ui, &p);
                    return;
                };
                let Some(chat) = chat else {
                    self.empty_state(ui, service, &p);
                    return;
                };

                // Faint wallpaper under the transcript; header and info pane
                // paint opaque on top of it.
                widgets::wallpaper(ui, &p);
                ui.data_mut(|d| {
                    d.insert_temp(egui::Id::new("select-mode-active"), self.select_mode);
                });

                if self.info_open {
                    egui::Panel::right("chat-info")
                        .exact_size(300.0)
                        .resizable(false)
                        .frame(Frame::new().fill(p.panel))
                        .show(ui, |ui| self.chat_info(ui, &p, &chat));
                }

                // Header: avatar, name and presence, hover actions on the right.
                let header = Frame::new()
                    .fill(p.header)
                    .inner_margin(Margin::symmetric(16, 10))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            let picture = self.avatar_texture(ui.ctx(), chat.avatar.as_deref());
                            widgets::avatar(ui, &chat.title, 34.0, &chat.title, picture.as_ref());
                            ui.add_space(10.0);
                            ui.vertical(|ui| {
                                ui.label(
                                    RichText::new(&chat.title)
                                        .font(theme::semibold(14.5))
                                        .color(p.text),
                                );
                                let key = (chat.account.0.clone(), chat.id.clone());
                                // What the other side is doing wins over
                                // connection state: it is the freshest fact.
                                if let Some((name, recording, seen)) =
                                    self.typing.get(&key).cloned()
                                {
                                    if seen.elapsed() < Duration::from_secs(6) {
                                        let verb = if recording {
                                            "recording audio…"
                                        } else {
                                            "typing…"
                                        };
                                        let line = if chat.is_group {
                                            format!("{name} is {verb}")
                                        } else if recording {
                                            "recording audio…".to_string()
                                        } else {
                                            "typing…".to_string()
                                        };
                                        ui.label(
                                            RichText::new(line)
                                                .font(theme::regular(11.0))
                                                .color(p.accent),
                                        );
                                        return;
                                    }
                                    self.typing.remove(&key);
                                }
                                let presence = self.presence.get(&key).copied();
                                let sub = if let Some((online, last_seen)) = presence {
                                    if online {
                                        "Online".to_string()
                                    } else if let Some(at) = last_seen {
                                        format!("Last seen {}", format::last_seen(at))
                                    } else if chat.is_group {
                                        "Group".to_string()
                                    } else {
                                        "Offline".to_string()
                                    }
                                } else {
                                    match self.status_of(service) {
                                        ServiceStatus::Online => {
                                            if chat.is_group {
                                                "Group".to_string()
                                            } else {
                                                "Online".to_string()
                                            }
                                        }
                                        ServiceStatus::Connecting => "Connecting…".to_string(),
                                        ServiceStatus::NeedsAuth => "Sign in required".to_string(),
                                        ServiceStatus::Error => "Unavailable".to_string(),
                                        ServiceStatus::SignedOut => "Offline".to_string(),
                                    }
                                };
                                ui.label(
                                    RichText::new(sub)
                                        .font(theme::regular(11.0))
                                        .color(p.text_muted),
                                );
                            });
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    let more = icons::button(
                                        ui,
                                        Icon::Ellipsis,
                                        30.0,
                                        p.text_muted,
                                        p.hover,
                                    );
                                    more.clone().on_hover_text("Menu");
                                    // Voice calls exist for WhatsApp direct
                                    // chats only; groups and other services
                                    // keep the lightweight build.
                                    if !chat.is_group
                                        && service == Service::WhatsApp
                                        && self.call.is_none()
                                    {
                                        let phone = icons::button(
                                            ui,
                                            Icon::Phone,
                                            30.0,
                                            p.text_muted,
                                            p.hover,
                                        );
                                        phone.clone().on_hover_text("Panggilan suara");
                                        if phone.clicked()
                                            && let Some(adapter) =
                                                self.adapters.get(&service).cloned()
                                        {
                                            let target = chat.id.clone();
                                            let chat_id = chat.id.clone();
                                            let title = chat.title.clone();
                                            // Paint the overlay immediately;
                                            // the adapter replaces it with the
                                            // real call id once the offer is
                                            // sent.
                                            self.call = Some(CallUi {
                                                service,
                                                chat_id,
                                                call_id: String::new(),
                                                state: CallState::Ringing,
                                                title,
                                                connected: None,
                                            });
                                            tokio::spawn(async move {
                                                if let Err(err) = adapter.place_call(&target).await
                                                {
                                                    log::warn!("place call failed: {err:#}");
                                                }
                                            });
                                        }
                                    }
                                    let account = chat.account.0.clone();
                                    let chat_id = chat.id.clone();
                                    let is_group = chat.is_group;
                                    let muted = chat.muted;
                                    egui::Popup::menu(&more).show(|ui| {
                                        ui.set_min_width(170.0);
                                        let info_label =
                                            if is_group { "Info grup" } else { "Info kontak" };
                                        if ui.button(info_label).clicked() {
                                            self.info_open = true;
                                            ui.close();
                                        }
                                        if ui.button("Mark as read").clicked() {
                                            let _ = self.store.set_unread(&account, &chat_id, 0);
                                            if let Some(adapter) =
                                                self.adapters.get(&service).cloned()
                                            {
                                                let target = chat.target.clone();
                                                tokio::spawn(async move {
                                                    let _ = adapter.mark_read(&target).await;
                                                });
                                            }
                                            self.refresh_lists();
                                            ui.close();
                                        }
                                        if ui.button("Pilih pesan").clicked() {
                                            self.select_mode = true;
                                            self.selected.clear();
                                            ui.close();
                                        }
                                        let (pinned_now, archived_now) = self
                                            .chat_flags
                                            .get(&(account.clone(), chat_id.clone()))
                                            .copied()
                                            .unwrap_or((false, false));
                                        if ui
                                            .button(if pinned_now {
                                                "Lepas pin"
                                            } else {
                                                "Pin obrolan"
                                            })
                                            .clicked()
                                        {
                                            let _ = self.store.set_chat_flag(
                                                &account,
                                                &chat_id,
                                                Some(!pinned_now),
                                                None,
                                            );
                                            self.refresh_lists();
                                            ui.close();
                                        }
                                        if ui
                                            .button(if archived_now {
                                                "Keluarkan dari arsip"
                                            } else {
                                                "Arsipkan"
                                            })
                                            .clicked()
                                        {
                                            let _ = self.store.set_chat_flag(
                                                &account,
                                                &chat_id,
                                                None,
                                                Some(!archived_now),
                                            );
                                            self.refresh_lists();
                                            ui.close();
                                        }
                                        let mute_label = if muted {
                                            "Bunyikan notifikasi"
                                        } else {
                                            "Senyapkan notifikasi"
                                        };
                                        if ui.button(mute_label).clicked() {
                                            if let Ok(Some(mut row)) =
                                                self.store.chat(&account, &chat_id)
                                            {
                                                row.muted = !muted;
                                                let _ = self.store.upsert_chat(&row);
                                                self.refresh_lists();
                                            }
                                            ui.close();
                                        }
                                        if ui.button("Bersihkan pesan").clicked() {
                                            let _ = self.store.clear_chat(&account, &chat_id);
                                            self.messages.insert(
                                                (account.clone(), chat_id.clone()),
                                                Vec::new(),
                                            );
                                            self.history_oldest
                                                .remove(&(account.clone(), chat_id.clone()));
                                            self.history_done
                                                .insert((account.clone(), chat_id.clone()));
                                            self.refresh_lists();
                                            ui.close();
                                        }
                                        if ui.button("Hapus obrolan").clicked() {
                                            let _ = self.store.delete_chat(&account, &chat_id);
                                            let key = (account.clone(), chat_id.clone());
                                            self.messages.remove(&key);
                                            self.history_oldest.remove(&key);
                                            self.active_chat.remove(&service);
                                            self.sync_notify_open();
                                            self.info_open = false;
                                            self.refresh_lists();
                                            ui.close();
                                        }
                                        if ui.button("Tutup obrolan").clicked() {
                                            self.active_chat.remove(&service);
                                            self.sync_notify_open();
                                            self.info_open = false;
                                            ui.close();
                                        }
                                    });
                                    let find = icons::button(
                                        ui,
                                        Icon::Search,
                                        30.0,
                                        p.text_muted,
                                        p.hover,
                                    );
                                    find.clone().on_hover_text("Cari pesan");
                                    if find.clicked() {
                                        self.chat_search_open = !self.chat_search_open;
                                        if !self.chat_search_open {
                                            self.chat_search.clear();
                                            self.chat_search_hits.clear();
                                        }
                                    }
                                },
                            );
                        });
                    });
                let header_rect = header.response.rect;
                // WhatsApp Web's "Select messages" toolbar replaces the chat
                // header while the mode is on.
                if self.select_mode {
                    ui.painter()
                        .rect_filled(header_rect, CornerRadius::ZERO, p.header);
                    let account = chat.account.0.clone();
                    let chat_id = chat.id.clone();
                    let mut cancel = false;
                    let mut copy = false;
                    let mut delete = false;
                    egui::Area::new(egui::Id::new("select-bar"))
                        .order(egui::Order::Foreground)
                        .fixed_pos(header_rect.min)
                        .show(ui.ctx(), |ui| {
                            // Swallow clicks on the covered header so its own
                            // buttons (search, menu) do not fire underneath.
                            ui.interact(
                                header_rect,
                                ui.id().with("select-bar-block"),
                                Sense::click(),
                            );
                            Frame::new()
                                .fill(p.header)
                                .inner_margin(Margin::symmetric(16, 10))
                                .show(ui, |ui| {
                                    ui.set_width(header_rect.width() - 32.0);
                                    ui.horizontal(|ui| {
                                        if ui.button("✕").clicked() {
                                            cancel = true;
                                        }
                                        ui.label(
                                            RichText::new(format!(
                                                "{} dipilih",
                                                self.selected.len()
                                            ))
                                            .font(theme::semibold(14.0))
                                            .color(p.text),
                                        );
                                        ui.with_layout(
                                            egui::Layout::right_to_left(egui::Align::Center),
                                            |ui| {
                                                if ui
                                                    .add_enabled(
                                                        !self.selected.is_empty(),
                                                        egui::Button::new("Hapus"),
                                                    )
                                                    .clicked()
                                                {
                                                    delete = true;
                                                }
                                                if ui
                                                    .add_enabled(
                                                        !self.selected.is_empty(),
                                                        egui::Button::new("Salin"),
                                                    )
                                                    .clicked()
                                                {
                                                    copy = true;
                                                }
                                            },
                                        );
                                    });
                                });
                        });
                    if cancel {
                        self.select_mode = false;
                        self.selected.clear();
                    }
                    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                        self.select_mode = false;
                        self.selected.clear();
                    }
                    if copy {
                        let key = (account.clone(), chat_id.clone());
                        let text = self
                            .messages
                            .get(&key)
                            .map(|list| {
                                list.iter()
                                    .filter(|m| self.selected.contains(&m.id))
                                    .map(|m| m.body.as_str())
                                    .collect::<Vec<_>>()
                                    .join("\n")
                            })
                            .unwrap_or_default();
                        if !text.is_empty() {
                            ui.ctx().copy_text(text);
                        }
                        self.select_mode = false;
                        self.selected.clear();
                    }
                    if delete {
                        let key = (account.clone(), chat_id.clone());
                        for id in self.selected.iter() {
                            let _ = self.store.delete_message(&account, &chat_id, id);
                        }
                        if let Some(list) = self.messages.get_mut(&key) {
                            list.retain(|m| !self.selected.contains(&m.id));
                        }
                        self.selected.clear();
                        self.select_mode = false;
                        self.refresh_lists();
                    }
                }
                ui.painter().hline(
                    header_rect.x_range(),
                    header_rect.max.y - HAIRLINE / 2.0,
                    Stroke::new(HAIRLINE, p.divider),
                );

                // In-chat search, like the magnifier in WhatsApp Web's header.
                if self.chat_search_open {
                    let mut chosen: Option<Message> = None;
                    Frame::new()
                        .fill(p.header)
                        .inner_margin(Margin::symmetric(16, 8))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                let resp = ui.add(
                                    egui::TextEdit::singleline(&mut self.chat_search)
                                        .hint_text("Cari di obrolan ini")
                                        .desired_width(f32::INFINITY)
                                        .font(theme::regular(13.0)),
                                );
                                if resp.changed() {
                                    let query = self.chat_search.trim().to_string();
                                    self.chat_search_hits = if query.is_empty() {
                                        Vec::new()
                                    } else {
                                        self.store
                                            .search_messages(
                                                &chat.account.0,
                                                Some(&chat.id),
                                                &query,
                                                40,
                                            )
                                            .unwrap_or_default()
                                    };
                                }
                                let count = self.chat_search_hits.len();
                                ui.label(
                                    RichText::new(if count == 0 {
                                        String::new()
                                    } else {
                                        format!("{count} hasil")
                                    })
                                    .font(theme::regular(11.5))
                                    .color(p.text_muted),
                                );
                                if ui.button("Tutup").clicked() {
                                    self.chat_search_open = false;
                                    self.chat_search.clear();
                                    self.chat_search_hits.clear();
                                }
                            });
                            if !self.chat_search_hits.is_empty() {
                                ui.add_space(4.0);
                                egui::ScrollArea::vertical()
                                    .max_height(190.0)
                                    .show(ui, |ui| {
                                        for hit in &self.chat_search_hits {
                                            let body: String = hit.body.chars().take(90).collect();
                                            let line = format!("{}  ·  {}", body, hit.sender_name);
                                            if ui.selectable_label(false, line).clicked() {
                                                chosen = Some(hit.clone());
                                            }
                                        }
                                    });
                            }
                        });
                    if let Some(hit) = chosen {
                        self.open_message_at(&hit);
                    }
                    ui.painter().hline(
                        header_rect.x_range(),
                        ui.min_rect().top() + 1.0,
                        Stroke::new(HAIRLINE, p.divider),
                    );
                }

                // History arrives in bursts after pairing. A quiet pill says
                // "still loading" without blocking the window, so the UI keeps
                // repainting while the backlog lands in the database.
                if let Some((active, started)) = self.syncing {
                    if active == chat.service && started.elapsed().as_secs() < 8 {
                        // 12 fps is plenty for a spinner and keeps the sync
                        // burst from painting the whole window at 60.
                        ui.ctx().request_repaint_after(Duration::from_millis(80));
                        let area = ui.max_rect();
                        egui::Area::new(egui::Id::new("syncing-pill"))
                            .order(egui::Order::Foreground)
                            .pivot(egui::Align2::CENTER_TOP)
                            .fixed_pos(egui::pos2(area.center().x, header_rect.bottom() + 18.0))
                            .show(ui.ctx(), |ui| {
                                Frame::new()
                                    .fill(p.panel)
                                    .stroke(Stroke::new(HAIRLINE, p.divider))
                                    .corner_radius(16)
                                    .inner_margin(Margin::symmetric(14, 8))
                                    .show(ui, |ui| {
                                        ui.horizontal(|ui| {
                                            ui.add(
                                                egui::Spinner::new().size(14.0).color(p.text_muted),
                                            );
                                            ui.add_space(8.0);
                                            ui.label(
                                                RichText::new(format!(
                                                    "Syncing {} messages…",
                                                    active.display_name()
                                                ))
                                                .font(theme::regular(12.5))
                                                .color(p.text_muted),
                                            );
                                        });
                                    });
                            });
                    } else if active == chat.service {
                        self.syncing = None;
                    }
                }

                // Messages
                let key = (chat.account.0.clone(), chat.id.clone());
                self.ensure_messages(&chat);
                // Take the list for the frame instead of cloning every message
                // (a 200-row conversation copies hundreds of strings otherwise).
                let mut messages = self.messages.remove(&key).unwrap_or_default();
                // Debug hook: UNITED_FAKE_REPLY=1 makes the newest bubble quote
                // the one before it so the quoted layout can be screenshotted
                // without poking the live database.
                if std::env::var("UNITED_FAKE_REPLY").is_ok() && messages.len() >= 2 {
                    let target = messages[messages.len() - 2].id.clone();
                    if let Some(last) = messages.last_mut() {
                        if last.reply_to.is_none() {
                            last.reply_to = Some(target);
                        }
                    }
                }

                // Group participants get their own face beside the bubble. The
                // textures are resolved once per open instead of per frame.
                let group_pictures: HashMap<String, Option<egui::TextureHandle>> = if chat.is_group
                {
                    let mut map = HashMap::new();
                    for msg in messages.iter().filter(|m| !m.outgoing) {
                        if map.contains_key(&msg.sender_id) {
                            continue;
                        }
                        let path = self.sender_avatar_path(&chat.account.0, &msg.sender_id);
                        let texture = self.avatar_texture(ui.ctx(), path.as_deref());
                        map.insert(msg.sender_id.clone(), texture);
                    }
                    map
                } else {
                    HashMap::new()
                };

                // ---- @-mention picker -------------------------------------
                // The roster is fetched once per group; Discord has no member
                // call there, so whoever spoke in this chat stands in.
                let roster_key = format!("{}|{}", chat.account.0, chat.id);
                let roster_due = self
                    .member_requests
                    .get(&roster_key)
                    .map(|at| at.elapsed().as_secs() >= 5)
                    .unwrap_or(true);
                let roster_empty = self
                    .members
                    .get(&roster_key)
                    .is_none_or(|list| list.is_empty());
                if chat.is_group && roster_empty && roster_due {
                    self.member_requests
                        .insert(roster_key.clone(), Instant::now());
                    if let Some(adapter) = self.adapters.get(&service).cloned() {
                        let tx = self.members_tx.clone();
                        let chat_id = chat.id.clone();
                        let roster_key = roster_key.clone();
                        tokio::spawn(async move {
                            let list = adapter.members(&chat_id).await.unwrap_or_else(|err| {
                                log::debug!("member list failed: {err:#}");
                                Vec::new()
                            });
                            let _ = tx.send((roster_key, list));
                        });
                    }
                }
                let draft_now = self.drafts.get(&key).cloned().unwrap_or_default();
                let mention = chat
                    .is_group
                    .then(|| mention_query(&draft_now))
                    .flatten()
                    .filter(|_| self.mention_dismissed.as_deref() != Some(draft_now.as_str()));
                let candidates: Vec<(String, String)> = match &mention {
                    Some((_, _, query)) => {
                        let mut list = self.members.get(&roster_key).cloned().unwrap_or_default();
                        if list.is_empty() {
                            // No roster (offline, Discord, or a chat whose
                            // history carries no sender names): offer whoever
                            // spoke, minus the group's own row.
                            let mut seen = std::collections::HashSet::new();
                            for msg in messages.iter().filter(|m| !m.outgoing) {
                                if msg.sender_id.is_empty()
                                    || msg.sender_id == chat.id
                                    || msg.sender_name.trim().is_empty()
                                {
                                    continue;
                                }
                                if seen.insert(msg.sender_id.clone()) {
                                    list.push((msg.sender_id.clone(), msg.sender_name.clone()));
                                }
                            }
                        }
                        // A group roster knows numbers; the address book knows
                        // who that number is. The picker shows the person.
                        for (id, name) in list.iter_mut() {
                            if let Some(known) = self.contact_names.get(&mention_key(id)) {
                                *name = known.clone();
                            }
                        }
                        let needle = query.to_lowercase();
                        list.retain(|(id, name)| {
                            needle.is_empty()
                                || name.to_lowercase().contains(&needle)
                                || mention_key(id).contains(&needle)
                        });
                        list.truncate(8);
                        list
                    }
                    None => Vec::new(),
                };
                if let Some((start, end, _)) = &mention {
                    if !candidates.is_empty() {
                        if ui.input_mut(|i| {
                            i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown)
                        }) {
                            self.mention_selected = (self.mention_selected + 1) % candidates.len();
                        }
                        if ui
                            .input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp))
                        {
                            self.mention_selected =
                                (self.mention_selected + candidates.len() - 1) % candidates.len();
                        }
                        let picked = ui.input_mut(|i| {
                            i.consume_key(egui::Modifiers::NONE, egui::Key::Enter)
                                || i.consume_key(egui::Modifiers::NONE, egui::Key::Tab)
                        });
                        if picked {
                            let index = self.mention_selected.min(candidates.len() - 1);
                            let token = mention_token(service, &candidates[index]);
                            apply_mention(
                                self.drafts.entry(key.clone()).or_default(),
                                *start,
                                *end,
                                &token,
                            );
                            self.mention_dismissed = None;
                            self.mention_selected = 0;
                        }
                        if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
                        {
                            self.mention_dismissed = Some(draft_now.clone());
                        }
                    }
                }
                let show_mention = mention.is_some();

                // Composer first: a bottom panel sizes to its content, so a
                // wrapped draft grows the bar instead of being clipped by a
                // fixed message height.
                let mut typing_edge: Option<bool> = None;
                let composer_panel = egui::Panel::bottom("conversation-composer")
                    .resizable(false)
                    .frame(Frame::new().fill(p.header))
                    .show(ui, |ui| {
                        let full = ui.max_rect();
                        ui.painter().hline(
                            full.x_range(),
                            full.min.y + HAIRLINE / 2.0,
                            Stroke::new(HAIRLINE, p.divider),
                        );
                        Frame::new()
                            .inner_margin(Margin::symmetric(14, 8))
                            .show(ui, |ui| {
                                // Reply preview above the bar, like WhatsApp
                                // Web: the quote stays until it is sent, the
                                // chat changes, or the user dismisses it.
                                if let Some(reply) = self.reply_to.clone() {
                                    if reply.account.0 == key.0 && reply.chat_id == key.1 {
                                        let name = if reply.outgoing {
                                            "Anda".to_string()
                                        } else if reply.sender_name.trim().is_empty() {
                                            "Pesan".to_string()
                                        } else {
                                            reply.sender_name.clone()
                                        };
                                        let body = if reply.body.trim().is_empty() {
                                            reply
                                                .media
                                                .as_deref()
                                                .and_then(MediaRef::parse)
                                                .map(|m| m.preview())
                                                .unwrap_or_else(|| "Pesan".into())
                                        } else {
                                            widgets::display_text(&reply.body).replace('\n', " ")
                                        };
                                        Frame::new()
                                            .fill(p.bubble_in)
                                            .corner_radius(8)
                                            .inner_margin(Margin::symmetric(10, 6))
                                            .show(ui, |ui| {
                                                ui.horizontal(|ui| {
                                                    let (bar, _) = ui.allocate_exact_size(
                                                        Vec2::new(3.0, 30.0),
                                                        Sense::hover(),
                                                    );
                                                    ui.painter().rect_filled(bar, 2, p.accent);
                                                    ui.vertical(|ui| {
                                                        ui.label(
                                                            RichText::new(name)
                                                                .font(theme::semibold(12.0))
                                                                .color(p.accent),
                                                        );
                                                        ui.add(
                                                            egui::Label::new(
                                                                RichText::new(body)
                                                                    .font(theme::regular(12.5))
                                                                    .color(p.text_muted),
                                                            )
                                                            .truncate(),
                                                        );
                                                    });
                                                    ui.with_layout(
                                                        egui::Layout::right_to_left(
                                                            egui::Align::Center,
                                                        ),
                                                        |ui| {
                                                            let close = ui.small_button("✕");
                                                            if close.clicked() {
                                                                self.reply_to = None;
                                                            }
                                                        },
                                                    );
                                                });
                                            });
                                        ui.add_space(6.0);
                                    }
                                }
                                let draft = self.drafts.entry(key.clone()).or_default();
                                let before = draft.clone();
                                let placeholder = match service {
                                    Service::WhatsApp => "Type a message".to_string(),
                                    Service::Telegram => "Message".to_string(),
                                    // Discord DMs name the person; channels are
                                    // outside the adapter.
                                    Service::Discord => format!("Message @{}", chat.title),
                                };
                                let out = widgets::composer(ui, draft, &p, &placeholder);
                                if *draft != before {
                                    typing_edge = Some(!draft.trim().is_empty());
                                    let spec = format!("{}|{}|{}", service.id(), key.0, key.1);
                                    if draft.trim().is_empty() {
                                        self.settings.drafts.remove(&spec);
                                    } else {
                                        self.settings.drafts.insert(spec, draft.clone());
                                    }
                                    self.settings_dirty = true;
                                }
                                if out.action == Some(widgets::ComposerAction::Emoji) {
                                    self.emoji_open = !self.emoji_open;
                                }
                                if self.emoji_open {
                                    let mut picked: Option<&'static str> = None;
                                    let anchor = out.rect;
                                    egui::Area::new(egui::Id::new("emoji-picker"))
                                        .order(egui::Order::Foreground)
                                        .fixed_pos(egui::pos2(
                                            (anchor.right() - 316.0).max(anchor.left()),
                                            (anchor.top() - 206.0).max(8.0),
                                        ))
                                        .show(ui.ctx(), |ui| {
                                            Frame::new()
                                                .fill(p.panel)
                                                .stroke(Stroke::new(HAIRLINE, p.divider))
                                                .corner_radius(12)
                                                .inner_margin(Margin::symmetric(8, 8))
                                                .show(ui, |ui| {
                                                    ui.set_width(300.0);
                                                    egui::Grid::new("emoji-grid")
                                                        .spacing([2.0, 2.0])
                                                        .show(ui, |ui| {
                                                            for (i, emoji) in
                                                                EMOJI.iter().enumerate()
                                                            {
                                                                let (cell, resp) = ui
                                                                    .allocate_exact_size(
                                                                        Vec2::splat(32.0),
                                                                        Sense::click(),
                                                                    );
                                                                let bg = if resp.hovered() {
                                                                    ui.painter().rect_filled(
                                                                        cell,
                                                                        CornerRadius::same(7),
                                                                        p.hover,
                                                                    );
                                                                    p.hover
                                                                } else {
                                                                    p.panel
                                                                };
                                                                let galley =
                                                                    ui.painter().layout_no_wrap(
                                                                        (*emoji).to_string(),
                                                                        theme::regular(20.0),
                                                                        p.text,
                                                                    );
                                                                let at = cell.center()
                                                                    - galley.size() / 2.0;
                                                                ui.painter().galley(
                                                                    at,
                                                                    galley.clone(),
                                                                    p.text,
                                                                );
                                                                crate::emoji::paint(
                                                                    ui.painter(),
                                                                    at,
                                                                    &galley,
                                                                    Some(bg),
                                                                );
                                                                if resp.clicked() {
                                                                    picked = Some(emoji);
                                                                }
                                                                if (i + 1) % 8 == 0 {
                                                                    ui.end_row();
                                                                }
                                                            }
                                                        });
                                                });
                                        });
                                    if let Some(emoji) = picked {
                                        draft.push_str(emoji);
                                        self.emoji_open = false;
                                    }
                                    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                                        self.emoji_open = false;
                                    }
                                }
                                match out.action {
                                    Some(widgets::ComposerAction::Send) => self.send_current(),
                                    Some(widgets::ComposerAction::AttachMedia) => self.attach(true),
                                    Some(widgets::ComposerAction::AttachFile) => self.attach(false),
                                    _ => {}
                                }
                            });
                    });
                // Tell the other side we are typing, at most every few seconds,
                // and one "paused" when the draft empties. WhatsApp Web does
                // exactly this; anything chattier is a stanza per keystroke.
                if let Some(active) = typing_edge {
                    let now = Instant::now();
                    let send = match self.typing_sent.get(&key) {
                        Some((at, was)) => {
                            if *was != active {
                                true
                            } else {
                                active && at.elapsed() > Duration::from_secs(4)
                            }
                        }
                        None => active,
                    };
                    if send {
                        self.typing_sent.insert(key.clone(), (now, active));
                        if let Some(adapter) = self.adapters.get(&service).cloned() {
                            let target = chat.target.clone();
                            tokio::spawn(async move {
                                let _ = adapter.typing(&target, active).await;
                            });
                        }
                    }
                }
                // Ctrl/Cmd+V with an image on the clipboard uploads it as an
                // attachment. WhatsApp accepts it anywhere in the open chat,
                // not only while the composer owns focus.
                if ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::V)) {
                    // Never read the clipboard on the UI thread: an X11 owner
                    // that stops responding would freeze the window. Text
                    // paste stays with egui; this only adds images.
                    let tx = self.paste_tx.clone();
                    std::thread::spawn(move || {
                        if let Some(path) = clipboard_image() {
                            let _ = tx.send(path);
                        }
                    });
                }
                if show_mention {
                    let anchor = composer_panel.response.rect;
                    let mut clicked: Option<usize> = None;
                    egui::Area::new(egui::Id::new("mention-picker"))
                        .order(egui::Order::Foreground)
                        .pivot(egui::Align2::LEFT_BOTTOM)
                        .fixed_pos(egui::pos2(anchor.left() + 62.0, anchor.top() - 6.0))
                        .show(ui.ctx(), |ui| {
                            Frame::new()
                                .fill(p.panel)
                                .stroke(Stroke::new(HAIRLINE, p.divider))
                                .corner_radius(12)
                                .inner_margin(Margin::symmetric(6, 6))
                                .show(ui, |ui| {
                                    ui.set_width(252.0);
                                    if candidates.is_empty() {
                                        ui.label(
                                            RichText::new("Waiting for group members…")
                                                .font(theme::regular(12.0))
                                                .color(p.text_muted),
                                        );
                                    }
                                    egui::ScrollArea::vertical()
                                        .max_height(240.0)
                                        .show(ui, |ui| {
                                            for (index, (id, name)) in candidates.iter().enumerate()
                                            {
                                                let _ = id;
                                                let row = ui.selectable_label(
                                                    index == self.mention_selected,
                                                    RichText::new(name)
                                                        .font(theme::regular(13.0))
                                                        .color(p.text),
                                                );
                                                if row.clicked() {
                                                    clicked = Some(index);
                                                }
                                            }
                                        });
                                });
                        });
                    if let Some(index) = clicked {
                        if let Some((start, end, _)) = mention {
                            let token = mention_token(service, &candidates[index]);
                            apply_mention(
                                self.drafts.entry(key.clone()).or_default(),
                                start,
                                end,
                                &token,
                            );
                            self.mention_dismissed = None;
                            self.mention_selected = 0;
                        }
                    }
                }

                // Debug hook: UNITED_SCROLL_ANIM=1 sweeps the view so scroll
                // smoothness (and lazy media loading) can be measured without a
                // real wheel; it has to let go of the bottom to move at all.
                let sweep = std::env::var("UNITED_SCROLL_ANIM").is_ok();
                // Keep the request until the view actually reaches the end:
                // one scroll_to_cursor can run before the content height of a
                // freshly opened chat is known.
                let forced_jump = self.jump_bottom.as_ref() == Some(&key);
                if forced_jump {
                    self.pinned_bottom = true;
                }
                let mut area = ScrollArea::vertical()
                    .auto_shrink([false, false])
                    // Fresh salt: an offset saved before the jump fix could
                    // be `f32::MAX`, which poisons every later frame.
                    // Per conversation: one shared id meant every chat opened
                    // at the offset the previous one was left at.
                    .id_salt(("chat-scroll-v2", &key.0, &key.1));
                // Only a wheel over the transcript unpins it. The list panel
                // has its own scroll area, and egui's smoothed delta is global,
                // so an upward wheel there used to reopen this chat mid-page.
                let wheel_here = ui.rect_contains_pointer(ui.max_rect())
                    && ui.input(|i| i.smooth_scroll_delta.y > 0.0);
                if wheel_here {
                    self.pinned_bottom = false;
                }
                // Only ask for a jump while actually behind. The old code did
                // this every pinned frame, which fought the scroll area whenever
                // an image finished decoding (the "blinking" chat).
                let need_jump = self.pinned_bottom && (!self.at_bottom || forced_jump);
                // Open (and follow) at the newest message by seeding the offset
                // from the last painted frame. The scroll area clamps it.
                if need_jump
                    && !sweep
                    && let Some(bottom) = self.chat_bottom.get(&key).copied()
                {
                    area = area.vertical_scroll_offset(bottom);
                }
                if sweep {
                    let travel = (self.started.elapsed().as_secs_f32() * 500.0) % 6000.0;
                    area = area.vertical_scroll_offset(travel);
                    ui.ctx().request_repaint();
                }
                let mut pending_delete: Option<Message> = None;
                let mut pending_react: Option<(Message, String)> = None;
                let mut pending_revoke: Option<Message> = None;
                let listing = area.show(ui, |ui| {
                    ui.add_space(8.0);
                    Frame::new()
                        .inner_margin(Margin::symmetric(16, 0))
                        .show(ui, |ui| {
                            let flat = service == Service::Discord;
                            // `@<phone>` is how WhatsApp spells a mention in the
                            // body; bubbles show the contact name instead.
                            let mut mention_names: HashMap<String, String> = HashMap::new();
                            if chat.is_group {
                                for (id, name) in
                                    self.members.get(&roster_key).cloned().unwrap_or_default()
                                {
                                    if name.trim().is_empty() {
                                        continue;
                                    }
                                    // A roster often knows only the phone number;
                                    // a synced name beats it.
                                    let key = mention_key(&id);
                                    if self.contact_names.contains_key(&key) {
                                        continue;
                                    }
                                    mention_names.insert(key, name);
                                }
                                for (key, name) in &self.contact_names {
                                    mention_names.insert(key.clone(), name.clone());
                                }
                                for msg in messages.iter().filter(|m| !m.outgoing) {
                                    if msg.sender_name.trim().is_empty() {
                                        continue;
                                    }
                                    mention_names
                                        .entry(mention_key(&msg.sender_id))
                                        .or_insert_with(|| msg.sender_name.clone());
                                }
                            }
                            // Replies resolve their quoted row by id once, not
                            // for every bubble.
                            let by_id: HashMap<&str, &Message> =
                                messages.iter().map(|m| (m.id.as_str(), m)).collect();
                            // A quote can point at a row older than the 200 loaded
                            // here; pull just that row from disk once per frame.
                            let mut older_quotes: HashMap<String, Message> = HashMap::new();
                            for m in &messages {
                                if let Some(id) = m.reply_to.as_deref() {
                                    if by_id.contains_key(id) || older_quotes.contains_key(id) {
                                        continue;
                                    }
                                    let cache_key = format!("{}|{}|{id}", m.account.0, m.chat_id);
                                    if !self.quote_cache.contains_key(&cache_key) {
                                        let found = self
                                            .store
                                            .message(&m.account.0, &m.chat_id, id)
                                            .ok()
                                            .flatten();
                                        self.quote_cache.insert(cache_key.clone(), found);
                                    }
                                    if let Some(Some(q)) = self.quote_cache.get(&cache_key) {
                                        older_quotes.insert(id.to_string(), q.clone());
                                    }
                                }
                            }
                            // Debug hook: UNITED_SELECT=1 highlights the first
                            // words of the newest bubble for a screenshot.
                            if std::env::var("UNITED_SELECT").is_ok()
                                && let Some(last) = messages.last()
                            {
                                let id = last.id.as_str();
                                ui.data_mut(|d| {
                                    d.insert_temp(
                                        egui::Id::new("bubble-text-sel-active"),
                                        last.id.clone(),
                                    );
                                    d.insert_temp(
                                        egui::Id::new(("bubble-text-sel", id)),
                                        (0usize, 6usize),
                                    );
                                });
                            }
                            let mut last_day = String::new();
                            let mut prev_sender = String::new();
                            let older_anchor = self.older_anchor.take();
                            let unread_marker = self
                                .unread_divider
                                .take()
                                .filter(|(a, c, _, _)| a == &key.0 && c == &key.1);
                            let mut unread_drawn = unread_marker.is_none();
                            // Every loaded bubble is laid out: the page is small
                            // and an estimate for offscreen rows made the chat
                            // flicker while it corrected itself.
                            for msg in &messages {
                                if !unread_drawn
                                    && let Some((_, _, at, count)) = &unread_marker
                                    && msg.at >= *at
                                {
                                    ui.add_space(8.0);
                                    ui.horizontal(|ui| {
                                        let label = format!("{count} pesan belum dibaca");
                                        let font = theme::medium(10.5);
                                        let label_w = ui
                                            .painter()
                                            .layout_no_wrap(label.clone(), font.clone(), p.accent)
                                            .size()
                                            .x;
                                        let pad = ((ui.available_width() - label_w - 20.0) / 2.0)
                                            .max(0.0);
                                        ui.add_space(pad);
                                        Frame::new()
                                            .fill(p.input_bg)
                                            .corner_radius(9)
                                            .inner_margin(Margin::symmetric(10, 3))
                                            .show(ui, |ui| {
                                                ui.label(
                                                    RichText::new(label).font(font).color(p.accent),
                                                );
                                            });
                                    });
                                    ui.add_space(4.0);
                                    unread_drawn = true;
                                }
                                let day = format::day_label(msg.at);
                                if day != last_day {
                                    ui.add_space(8.0);
                                    let font = theme::medium(10.5);
                                    let label_w = ui
                                        .painter()
                                        .layout_no_wrap(day.clone(), font.clone(), p.text_muted)
                                        .size()
                                        .x;
                                    ui.horizontal(|ui| {
                                        let pad = ((ui.available_width() - label_w - 20.0) / 2.0)
                                            .max(0.0);
                                        ui.add_space(pad);
                                        Frame::new()
                                            .fill(p.input_bg)
                                            .corner_radius(9)
                                            .inner_margin(Margin::symmetric(10, 3))
                                            .show(ui, |ui| {
                                                ui.label(
                                                    RichText::new(&day)
                                                        .font(font)
                                                        .color(p.text_muted),
                                                );
                                            });
                                    });
                                    ui.add_space(4.0);
                                    last_day = day;
                                }
                                // A group row can arrive without a name (history
                                // sync); the roster knows who that id is.
                                // WhatsApp shows the address-book name above the
                                // push name, even when the stored row only carries
                                // the number or the sender's own label.
                                let known = if chat.is_group && !msg.outgoing {
                                    mention_names
                                        .get(&mention_key(&msg.sender_id))
                                        .filter(|name| {
                                            !name.trim().is_empty()
                                                && name.as_str() != msg.sender_name
                                        })
                                        .cloned()
                                } else {
                                    None
                                };
                                let patched;
                                let msg = if let Some(name) = known {
                                    patched = {
                                        let mut copy = msg.clone();
                                        copy.sender_name = name;
                                        copy
                                    };
                                    &patched
                                } else {
                                    msg
                                };
                                if flat {
                                    let show_header = prev_sender != msg.sender_id
                                        || messages
                                            .iter()
                                            .take_while(|m| m.id != msg.id)
                                            .last()
                                            .map(|m| msg.at - m.at > 300)
                                            .unwrap_or(true);
                                    let shown = resolve_discord_mentions(&msg.body, &mention_names);
                                    let row =
                                        widgets::flat_message(ui, msg, &shown, &p, show_header);
                                    if older_anchor.as_deref() == Some(msg.id.as_str()) {
                                        ui.scroll_to_rect(row.rect, Some(egui::Align::TOP));
                                    }
                                } else {
                                    let media = self.media_of(msg);
                                    let local = self.media_local(msg);
                                    let (shown, mention_spans) =
                                        if service == Service::WhatsApp && chat.is_group {
                                            resolve_mentions(&msg.body, &mention_names)
                                        } else {
                                            (msg.body.clone(), Vec::new())
                                        };
                                    let image_texture = media.as_ref().and_then(|m| {
                                        let decodable = m.is_image() || m.is_sticker();
                                        decodable
                                            .then(|| {
                                                local.as_deref().and_then(|path| {
                                                    self.media_texture(ui.ctx(), path)
                                                })
                                            })
                                            .flatten()
                                    });
                                    let thumb_texture = media
                                        .as_ref()
                                        .and_then(|m| {
                                            m.thumb.as_deref().map(|raw| {
                                                let key = format!("{}|{}", msg.id, raw.len());
                                                self.media_thumb(ui.ctx(), &key, raw)
                                            })
                                        })
                                        .flatten();
                                    let open_path = local.clone();
                                    let quoted = msg
                                        .reply_to
                                        .as_deref()
                                        .and_then(|id| {
                                            by_id.get(id).copied().or_else(|| older_quotes.get(id))
                                        })
                                        .map(|q| widgets::Quoted {
                                            name: if q.outgoing {
                                                "Anda"
                                            } else if q.sender_name.trim().is_empty() {
                                                "Pesan"
                                            } else {
                                                q.sender_name.as_str()
                                            },
                                            body: q.body.as_str(),
                                        });
                                    let bubble_media =
                                        media.as_ref().map(|m| widgets::BubbleMedia {
                                            texture: image_texture.as_ref(),
                                            thumb: thumb_texture.as_ref(),
                                            path: open_path.clone().unwrap_or_default(),
                                            label: m.preview(),
                                            caption: Some(msg.body.clone()).filter(|body| {
                                                // Placeholders carry a leading emoji; strip it
                                                // so "📄 invoice.pdf" is not repeated as caption.
                                                let clean: String = body
                                                    .trim_start_matches(|c: char| !c.is_ascii())
                                                    .trim()
                                                    .to_string();
                                                let clean = if clean.is_empty() {
                                                    body.trim().to_string()
                                                } else {
                                                    clean
                                                };
                                                !clean.is_empty() && clean != m.preview()
                                            }),
                                            sticker: m.is_sticker(),
                                            play: m.is_video(),
                                            aspect: m.width.zip(m.height),
                                            picture: m.is_image() || m.is_video() || m.is_sticker(),
                                        });
                                    let resp = if msg.outgoing {
                                        ui.with_layout(
                                            egui::Layout::right_to_left(egui::Align::Min),
                                            |ui| {
                                                Frame::new()
                                                    .inner_margin(Margin::ZERO)
                                                    .show(ui, |ui| {
                                                        widgets::bubble(
                                                            ui,
                                                            msg,
                                                            &shown,
                                                            &p,
                                                            false,
                                                            bubble_media.clone(),
                                                            &mention_spans,
                                                            quoted,
                                                        )
                                                    })
                                                    .inner
                                            },
                                        )
                                        .inner
                                    } else {
                                        let show_author =
                                            chat.is_group && prev_sender != msg.sender_id;
                                        let picture = group_pictures
                                            .get(&msg.sender_id)
                                            .and_then(|t| t.as_ref());
                                        ui.with_layout(
                                            egui::Layout::left_to_right(egui::Align::Min),
                                            |ui| {
                                                if chat.is_group {
                                                    ui.add_space(2.0);
                                                    if show_author {
                                                        widgets::avatar(
                                                            ui,
                                                            &msg.sender_name,
                                                            28.0,
                                                            &msg.sender_id,
                                                            picture,
                                                        );
                                                    } else {
                                                        // Keep later bubbles of the run aligned
                                                        // under the first one.
                                                        ui.add_space(28.0);
                                                    }
                                                    ui.add_space(6.0);
                                                }
                                                widgets::bubble(
                                                    ui,
                                                    msg,
                                                    &shown,
                                                    &p,
                                                    show_author,
                                                    bubble_media,
                                                    &mention_spans,
                                                    quoted,
                                                )
                                            },
                                        )
                                        .inner
                                    };
                                    // Right-click is the desktop equivalent of
                                    // WhatsApp Web's bubble menu.
                                    if older_anchor.as_deref() == Some(msg.id.as_str()) {
                                        ui.scroll_to_rect(resp.rect, Some(egui::Align::TOP));
                                    }
                                    egui::Popup::context_menu(&resp).show(|ui| {
                                        ui.set_min_width(150.0);
                                        ui.horizontal(|ui| {
                                            for emoji in ["👍", "❤️", "😂", "😮", "😢", "🙏"]
                                            {
                                                let button = egui::Button::new(
                                                    RichText::new(emoji).size(18.0),
                                                )
                                                .frame(false)
                                                .min_size(egui::vec2(26.0, 26.0));
                                                if ui.add(button).clicked() {
                                                    pending_react =
                                                        Some((msg.clone(), emoji.to_string()));
                                                    ui.close();
                                                }
                                            }
                                        });
                                        ui.separator();
                                        if ui.button("Balas").clicked() {
                                            self.reply_to = Some(msg.clone());
                                            ui.close();
                                        }
                                        if !msg.body.trim().is_empty()
                                            && ui.button("Salin").clicked()
                                        {
                                            ui.ctx().copy_text(msg.body.clone());
                                            ui.close();
                                        }
                                        let image = media.as_ref().is_some_and(|m| m.is_image());
                                        if image
                                            && let Some(path) = open_path.as_deref()
                                            && ui.button("Salin gambar").clicked()
                                        {
                                            self.copy_image(path);
                                            ui.close();
                                        }
                                        if let Some(path) = open_path.as_deref()
                                            && ui.button("Simpan sebagai…").clicked()
                                        {
                                            let name = media
                                                .as_ref()
                                                .and_then(|m| m.name.clone())
                                                .or_else(|| {
                                                    std::path::Path::new(path)
                                                        .file_name()
                                                        .and_then(|n| n.to_str())
                                                        .map(str::to_owned)
                                                })
                                                .unwrap_or_default();
                                            self.save_media_as(path, &name);
                                            ui.close();
                                        }
                                        if ui.button("Pilih pesan").clicked() {
                                            self.select_mode = true;
                                            self.selected.clear();
                                            ui.close();
                                        }
                                        if msg.outgoing
                                            && !msg.body.trim().is_empty()
                                            && ui.button("Edit").clicked()
                                        {
                                            self.edit_target =
                                                Some((msg.clone(), msg.body.clone()));
                                            ui.close();
                                        }
                                        if ui.button("Hapus untuk saya").clicked() {
                                            pending_delete = Some(msg.clone());
                                            ui.close();
                                        }
                                        if msg.outgoing
                                            && ui.button("Hapus untuk semua orang").clicked()
                                        {
                                            pending_revoke = Some(msg.clone());
                                            ui.close();
                                        }
                                    });
                                    if self.select_mode {
                                        let on = self.selected.contains(&msg.id);
                                        let center = resp.rect.left_top() + egui::vec2(13.0, 13.0);
                                        let check =
                                            Rect::from_center_size(center, egui::Vec2::splat(18.0));
                                        ui.painter().circle_filled(
                                            center,
                                            9.0,
                                            if on {
                                                p.accent
                                            } else {
                                                Color32::from_black_alpha(120)
                                            },
                                        );
                                        ui.painter().circle_stroke(
                                            center,
                                            9.0,
                                            Stroke::new(1.2, Color32::WHITE),
                                        );
                                        if on {
                                            let tick = Stroke::new(2.0, Color32::WHITE);
                                            ui.painter().line_segment(
                                                [
                                                    check.center() + egui::vec2(-4.0, 0.0),
                                                    check.center() + egui::vec2(-1.0, 3.5),
                                                ],
                                                tick,
                                            );
                                            ui.painter().line_segment(
                                                [
                                                    check.center() + egui::vec2(-1.0, 3.5),
                                                    check.center() + egui::vec2(4.5, -3.5),
                                                ],
                                                tick,
                                            );
                                            ui.painter().rect_stroke(
                                                resp.rect.expand(1.5),
                                                CornerRadius::same(12),
                                                Stroke::new(1.5, p.accent),
                                                egui::StrokeKind::Outside,
                                            );
                                        }
                                        if resp.clicked() && !self.selected.remove(&msg.id) {
                                            self.selected.insert(msg.id.clone());
                                        }
                                    } else if resp.clicked() {
                                        let image = media.as_ref().is_some_and(|m| m.is_image());
                                        if image {
                                            if let (Some(texture), Some(path)) =
                                                (image_texture.as_ref(), open_path.as_deref())
                                            {
                                                self.media_view =
                                                    Some((texture.clone(), path.to_owned()));
                                                self.media_zoom = 1.0;
                                                self.media_pan = egui::Vec2::ZERO;
                                            }
                                        } else if let Some(path) = open_path.as_deref() {
                                            widgets::open_url(&format!("file://{path}"));
                                        }
                                    }
                                    // Older pages fetch their attachments only once
                                    // they are actually on screen, so opening a long
                                    // chat does not start a hundred downloads.
                                    if media.as_ref().is_some_and(|m| m.local().is_none())
                                        && ui.is_rect_visible(resp.rect)
                                    {
                                        self.request_media(msg);
                                    }
                                    // Reaction chips sit just under the bubble,
                                    // grouped by emoji with a count.
                                    if let Some(list) =
                                        self.reactions.get(&key).and_then(|map| map.get(&msg.id))
                                        && !list.is_empty()
                                    {
                                        let mut counts: Vec<(&str, usize)> = Vec::new();
                                        for (_, emoji) in list {
                                            match counts
                                                .iter_mut()
                                                .find(|(seen, _)| *seen == emoji.as_str())
                                            {
                                                Some((_, count)) => *count += 1,
                                                None => counts.push((emoji.as_str(), 1)),
                                            }
                                        }
                                        let layout = if msg.outgoing {
                                            egui::Layout::right_to_left(egui::Align::Min)
                                        } else {
                                            egui::Layout::left_to_right(egui::Align::Min)
                                        };
                                        ui.with_layout(layout, |ui| {
                                            for (emoji, count) in counts {
                                                let label = if count > 1 {
                                                    format!("{emoji} {count}")
                                                } else {
                                                    emoji.to_string()
                                                };
                                                Frame::new()
                                                    .fill(p.panel)
                                                    .stroke(Stroke::new(HAIRLINE, p.divider))
                                                    .corner_radius(9)
                                                    .inner_margin(Margin::symmetric(7, 2))
                                                    .show(ui, |ui| {
                                                        ui.label(
                                                            RichText::new(label)
                                                                .size(11.5)
                                                                .color(p.text_muted),
                                                        );
                                                    });
                                            }
                                        });
                                    }
                                    ui.add_space(2.0);
                                }
                                prev_sender = msg.sender_id.clone();
                            }
                        });
                    ui.add_space(8.0);
                });

                if let Some(dead) = pending_delete {
                    let _ = self
                        .store
                        .delete_message(&dead.account.0, &dead.chat_id, &dead.id);
                    let _ = self.store.repair_previews();
                    messages.retain(|m| m.id != dead.id);
                }
                if let Some((msg, emoji)) = pending_react {
                    if let Some(adapter) = self.adapters.get(&msg.service).cloned() {
                        let target = chat.target.clone();
                        tokio::spawn(async move {
                            if let Err(err) = adapter.react(&target, &msg, &emoji).await {
                                log::warn!("react failed: {err:#}");
                            }
                        });
                    }
                }
                if let Some(msg) = pending_revoke {
                    if let Some(adapter) = self.adapters.get(&msg.service).cloned() {
                        let target = chat.target.clone();
                        tokio::spawn(async move {
                            if let Err(err) = adapter.revoke_message(&target, &msg).await {
                                log::warn!("revoke failed: {err:#}");
                            }
                        });
                    }
                }
                self.messages.insert(key.clone(), messages);
                self.chat_bottom.insert(
                    key.clone(),
                    (listing.content_size.y - listing.inner_rect.height()).max(0.0),
                );
                // Scrolled up through history? Offer the way back, like a chat
                // client's floating down-arrow at the bottom right.
                let view = listing.inner_rect;
                let behind = listing.content_size.y - (listing.state.offset.y + view.height());
                self.at_bottom = behind <= 1.0;
                // Dragging or wheeling back to the newest message re-pins the
                // view, so later arrivals keep following it.
                if self.at_bottom {
                    self.pinned_bottom = true;
                    if forced_jump {
                        self.jump_bottom = None;
                    }
                }
                if behind > 240.0 && !self.pinned_bottom {
                    let center = egui::pos2(view.center().x, view.max.y - 26.0);
                    let button = ui.interact(
                        Rect::from_center_size(center, Vec2::splat(38.0)),
                        ui.id().with(("jump-bottom", &key.0, &key.1)),
                        Sense::click(),
                    );
                    let bg = if button.hovered() { p.hover } else { p.panel };
                    ui.painter().circle_filled(center, 19.0, bg);
                    ui.painter()
                        .circle_stroke(center, 19.0, Stroke::new(HAIRLINE, p.divider));
                    let ink = p.text_muted;
                    ui.painter().line_segment(
                        [
                            egui::pos2(center.x - 5.0, center.y - 3.0),
                            egui::pos2(center.x, center.y + 3.0),
                        ],
                        Stroke::new(1.6, ink),
                    );
                    ui.painter().line_segment(
                        [
                            egui::pos2(center.x, center.y + 3.0),
                            egui::pos2(center.x + 5.0, center.y - 3.0),
                        ],
                        Stroke::new(1.6, ink),
                    );
                    if button.clicked() {
                        self.jump_bottom = Some(key.clone());
                        ui.ctx().request_repaint();
                    }
                }

                // Scrolled to the very top? Page further back through the
                // protocol. WhatsApp in particular only pushes history once, so
                // older messages have to be asked for.
                let at_top = listing.state.offset.y <= 1.0;
                let scrolled_up = ui.input(|i| i.smooth_scroll_delta.y > 0.0);
                if at_top
                    && scrolled_up
                    && self
                        .older_request
                        .get(&key)
                        .map(|at| at.elapsed() > Duration::from_secs(2))
                        .unwrap_or(true)
                {
                    self.older_request.insert(key.clone(), Instant::now());
                    // Prefer the local database: the protocol only keeps a
                    // recent window and each request costs a round-trip.
                    if !self.load_older_local(&chat) {
                        if let Some(adapter) = self.adapters.get(&service).cloned() {
                            let target = chat.target.clone();
                            tokio::spawn(async move {
                                let _ = adapter.load_history(&target, 100).await;
                            });
                        }
                    }
                }
            });
    }

    fn empty_state(&mut self, ui: &mut egui::Ui, service: Service, p: &Palette) {
        ui.vertical_centered(|ui| {
            // Keep the paragraph readable on wide windows instead of letting a
            // single line stretch across the whole pane.
            ui.set_max_width(440.0);
            ui.add_space(ui.available_height() * 0.22);
            let (rect, _) = ui.allocate_exact_size(Vec2::splat(64.0), Sense::hover());
            match widgets::brand_icon(service) {
                Some(src) => {
                    egui::Image::new(src)
                        .fit_to_exact_size(Vec2::splat(56.0))
                        .paint_at(ui, Rect::from_center_size(rect.center(), Vec2::splat(56.0)));
                }
                None => icons::paint(
                    ui.painter(),
                    Rect::from_center_size(rect.center(), Vec2::splat(64.0)),
                    Icon::Bubbles,
                    p.text_muted.gamma_multiply(0.55),
                    2.4,
                ),
            }
            ui.add_space(14.0);
            ui.label(
                RichText::new(service.display_name())
                    .font(theme::semibold(19.0))
                    .color(p.text),
            );
            ui.add_space(6.0);
            let (status, detail) = self
                .statuses
                .get(&service)
                .cloned()
                .unwrap_or((ServiceStatus::SignedOut, None));
            let text = match status {
                ServiceStatus::Online => "Choose a conversation to start reading.".to_string(),
                ServiceStatus::Connecting => "Connecting to the service…".to_string(),
                ServiceStatus::NeedsAuth => "Sign in to start syncing.".to_string(),
                ServiceStatus::Error => detail.unwrap_or_else(|| "Connection error".into()),
                ServiceStatus::SignedOut => "Not signed in yet.".to_string(),
            };
            ui.label(
                RichText::new(text)
                    .font(theme::regular(13.0))
                    .color(p.text_muted),
            );
            // A connected service must not keep offering "Sign In": the empty
            // pane means "pick a chat", not "you are logged out".
            let connected = matches!(status, ServiceStatus::Online | ServiceStatus::Connecting);
            if self.adapters.contains_key(&service) && !connected {
                ui.add_space(18.0);
                if ui
                    .add(
                        egui::Button::new(
                            RichText::new("Sign In")
                                .font(theme::medium(13.5))
                                .color(Color32::WHITE),
                        )
                        .fill(p.accent)
                        .corner_radius(10)
                        .min_size(Vec2::new(120.0, 32.0)),
                    )
                    .clicked()
                {
                    self.begin_sign_in(service);
                }
            }
        });
    }

    fn unified_detail(&mut self, ui: &mut egui::Ui, p: &Palette) {
        ui.vertical_centered(|ui| {
            ui.add_space(ui.available_height() * 0.3);
            let (rect, _) = ui.allocate_exact_size(Vec2::splat(56.0), Sense::hover());
            icons::paint(
                ui.painter(),
                Rect::from_center_size(rect.center(), Vec2::splat(56.0)),
                Icon::Bubbles,
                p.accent,
                2.2,
            );
            ui.add_space(12.0);
            ui.label(
                RichText::new("All Messages")
                    .font(theme::semibold(20.0))
                    .color(p.text),
            );
            ui.add_space(6.0);
            let total: u32 = self.unified.iter().map(|c| c.unread).sum();
            ui.label(
                RichText::new(format!(
                    "{} conversations · {} unread",
                    self.unified.len(),
                    total
                ))
                .font(theme::regular(13.0))
                .color(p.text_muted),
            );
            ui.add_space(4.0);
            ui.label(
                RichText::new("Messages arrive straight from each protocol adapter.")
                    .font(theme::regular(12.0))
                    .color(p.text_muted.gamma_multiply(0.9)),
            );
        });
    }

    fn settings_view(&mut self, ui: &mut egui::Ui) {
        let p = self.palette();
        egui::CentralPanel::default()
            .frame(Frame::new().fill(p.input_bg))
            .show(ui, |ui| {
                let mut area = ScrollArea::vertical().auto_shrink([false, false]);
                // Debug hook only: pinning the offset unconditionally would
                // fight the user's wheel on every frame.
                if let Some(scroll) = std::env::var("UNITED_SCROLL")
                    .ok()
                    .and_then(|value| value.parse::<f32>().ok())
                {
                    area = area.vertical_scroll_offset(scroll);
                }
                area.show(ui, |ui| {
                        ui.add_space(22.0);
                        ui.horizontal(|ui| {
                            ui.add_space(24.0);
                            ui.vertical(|ui| {
                                ui.set_width(560.0);
                                ui.label(
                                    RichText::new("Settings")
                                        .font(theme::semibold(26.0))
                                        .color(p.text),
                                );

                                widgets::section(ui, "Appearance", &p);
                                widgets::card(ui, &p, |ui| {
                                    widgets::row(
                                        ui,
                                        &p,
                                        "Theme",
                                        Some("System follows the desktop appearance"),
                                        |ui| {
                                            let current = match self.settings.theme {
                                                united_core::settings::Theme::System => 0,
                                                united_core::settings::Theme::Light => 1,
                                                united_core::settings::Theme::Dark => 2,
                                            };
                                            if let Some(picked) = widgets::segmented(
                                                ui,
                                                &p,
                                                current,
                                                &["System", "Light", "Dark"],
                                            ) {
                                                let value = match picked {
                                                    1 => united_core::settings::Theme::Light,
                                                    2 => united_core::settings::Theme::Dark,
                                                    _ => united_core::settings::Theme::System,
                                                };
                                                self.settings.theme = value;
                                                self.mode = match value {
                                                    united_core::settings::Theme::Light => {
                                                        Mode::Light
                                                    }
                                                    united_core::settings::Theme::Dark => Mode::Dark,
                                                    united_core::settings::Theme::System => {
                                                        detect_system_mode(ui.ctx())
                                                    }
                                                };
                                                apply_mode(ui.ctx(), self.mode);
                                                self.settings.save();
                                            }
                                        },
                                    );
                                });

                                widgets::section(ui, "Window", &p);
                                widgets::card(ui, &p, |ui| {
                                    widgets::row(
                                        ui,
                                        &p,
                                        "Keep running after close",
                                        Some("Minimizes instead of quitting"),
                                        |ui| {
                                            let mut value = self.settings.minimize_on_close;
                                            if widgets::switch(ui, &mut value, &p).clicked() {
                                                self.settings.minimize_on_close = value;
                                                self.settings.save();
                                            }
                                        },
                                    );
                                    widgets::row(ui, &p, "Desktop notifications", None, |ui| {
                                        let mut value = self.settings.desktop_notifications;
                                        if widgets::switch(ui, &mut value, &p).clicked() {
                                            self.settings.desktop_notifications = value;
                                            self.notify_enabled.store(value, Ordering::Relaxed);
                                            self.settings.save();
                                        }
                                    });
                                    widgets::row(ui, &p, "Launch at login", None, |ui| {
                                        let mut value = self.settings.launch_at_startup;
                                        if widgets::switch(ui, &mut value, &p).clicked() {
                                            self.settings.launch_at_startup = value;
                                            AppSettings::apply_autostart(value);
                                            self.settings.save();
                                        }
                                    });
                                });

                                widgets::section(ui, "Services", &p);
                                widgets::card(ui, &p, |ui| {
                                    let mut unlink: Option<Service> = None;
                                    for service in Service::ALL {
                                        let before = self.settings.is_enabled(service.id());
                                        let mut on = before;
                                        let (status, _) = self
                                            .statuses
                                            .get(&service)
                                            .cloned()
                                            .unwrap_or((ServiceStatus::SignedOut, None));
                                        let detail = match status {
                                            ServiceStatus::Online => "Connected".to_string(),
                                            ServiceStatus::Connecting => "Connecting…".to_string(),
                                            ServiceStatus::NeedsAuth => {
                                                "Sign in required".to_string()
                                            }
                                            ServiceStatus::Error => "Error".to_string(),
                                            ServiceStatus::SignedOut => "Signed out".to_string(),
                                        };
                                        widgets::row(
                                            ui,
                                            &p,
                                            service.display_name(),
                                            Some(&detail),
                                            |ui| {
                                                let available = self.adapters.contains_key(&service)
                                                    && !matches!(
                                                        status,
                                                        ServiceStatus::Online
                                                            | ServiceStatus::Connecting
                                                    );
                                                if available
                                                    && ui
                                                        .add(
                                                            egui::Button::new(
                                                                RichText::new("Sign in")
                                                                    .font(theme::medium(11.5))
                                                                    .color(Color32::WHITE),
                                                            )
                                                            .fill(p.accent)
                                                            .corner_radius(8)
                                                            .min_size(Vec2::new(64.0, 24.0)),
                                                        )
                                                        .clicked()
                                                {
                                                    self.begin_sign_in(service);
                                                }
                                                if matches!(
                                                    status,
                                                    ServiceStatus::Online
                                                        | ServiceStatus::Connecting
                                                ) {
                                                    if ui
                                                        .add(
                                                            egui::Button::new(
                                                                RichText::new("Disconnect")
                                                                    .font(theme::medium(11.5))
                                                                    .color(p.text),
                                                            )
                                                            .fill(p.input_bg)
                                                            .corner_radius(8)
                                                            .min_size(Vec2::new(84.0, 24.0)),
                                                        )
                                                        .clicked()
                                                    {
                                                        unlink = Some(service);
                                                    }
                                                }
                                                ui.add_space(8.0);
                                                widgets::switch(ui, &mut on, &p);
                                            },
                                        );
                                        if on != before {
                                            if on {
                                                self.settings
                                                    .enabled_services
                                                    .push(service.id().into());
                                            } else {
                                                self.settings
                                                    .enabled_services
                                                    .retain(|s| s != service.id());
                                            }
                                            self.settings.save();
                                            self.refresh_lists();
                                        }
                                    }
                                    if let Some(service) = unlink {
                                        self.disconnect(service);
                                    }
                                });

                                widgets::section(ui, "Sign-in guide", &p);
                                widgets::card(ui, &p, |ui| {
                                    let guides: [(&str, &str, &str); 3] = [
                                        (
                                            "WhatsApp",
                                            "Phone › WhatsApp › Settings › Linked devices › Link a device, then scan the code in this window. Codes live ~2 minutes; \"Show a new code\" opens a fresh one. Missing old chats? Unlink this device on the phone and scan again: WhatsApp only sends the full history once per link. Runs locally over the multi-device protocol — no browser is involved.",
                                            "https://faq.whatsapp.com/1317564962313205",
                                        ),
                                        (
                                            "Telegram",
                                            "Create an app at my.telegram.org › API development tools, then paste api_id and api_hash here. Telegram sends the login code inside the app afterwards.",
                                            "https://my.telegram.org/apps",
                                        ),
                                        (
                                            "Discord",
                                            "Create an application, open Bot and reset the token. Discord has no official user-account API; a user token can get the account banned.",
                                            "https://discord.com/developers/applications",
                                        ),
                                    ];
                                    for (title, body, url) in guides {
                                        ui.horizontal(|ui| {
                                            ui.add_space(16.0);
                                            ui.vertical(|ui| {
                                                ui.set_width(ui.available_width() - 16.0);
                                                ui.add_space(10.0);
                                                ui.label(
                                                    RichText::new(title)
                                                        .font(theme::medium(13.0))
                                                        .color(p.text),
                                                );
                                                ui.add_space(2.0);
                                                ui.label(
                                                    RichText::new(body)
                                                        .font(theme::regular(11.5))
                                                        .color(p.text_muted),
                                                );
                                                ui.add_space(2.0);
                                                widgets::link(ui, "Official page", url, &p);
                                                ui.add_space(10.0);
                                            });
                                        });
                                    }
                                });

                                widgets::section(ui, "About", &p);
                                widgets::card(ui, &p, |ui| {
                                    let version = format!(
                                        "United {} · {}",
                                        env!("CARGO_PKG_VERSION"),
                                        format::os_name()
                                    );
                                    widgets::row(ui, &p, "Version", Some(&version), |_ui| {});
                                    widgets::row(
                                        ui,
                                        &p,
                                        "Engine",
                                        Some("Rust + egui · no Chromium, no WebView, no embedded browser"),
                                        |_ui| {},
                                    );
                                    widgets::row(
                                        ui,
                                        &p,
                                        "Data",
                                        Some(&united_core::paths::data_dir().display().to_string()),
                                        |_ui| {},
                                    );
                                });

                                ui.add_space(28.0);
                            });
                        });
                    });
            });
    }

    fn auth_modal(&mut self, ctx: &egui::Context) {
        // Debug hook: UNITED_NO_PROMPT=1 hides the sign-in sheet so the chat
        // behind it can be screenshotted.
        if std::env::var("UNITED_NO_PROMPT").is_ok() {
            return;
        }
        let Some((service, prompt)) = self.prompt.clone() else {
            return;
        };
        // Only interrupt the conversation list of the service being signed in.
        // Settings and other services get the rail badge instead: a sheet that
        // covers the pane also swallows its mouse wheel.
        match self.view {
            View::Service(active) if active == service => {}
            _ => return,
        }
        let p = self.palette();
        let mut open = true;
        egui::Window::new(format!("{} sign in", service.display_name()))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .frame(
                Frame::new()
                    .fill(p.panel)
                    .corner_radius(16)
                    .inner_margin(Margin::same(22))
                    .stroke(Stroke::new(1.0, p.divider)),
            )
            .show(ctx, |ui| {
                ui.set_width(320.0);
                match &prompt {
                    AuthPrompt::Qr { payload } if payload.is_empty() => {
                        let unlinked = self
                            .statuses
                            .get(&service)
                            .map(|(status, detail)| {
                                *status == ServiceStatus::SignedOut
                                    && detail.as_deref() == Some("unlinked from phone")
                            })
                            .unwrap_or(false);
                        let (title, body) = if unlinked {
                            (
                                "This phone unlinked United",
                                "Your chats stay on this computer. Show a fresh code and \
                                 scan it from WhatsApp › Linked devices › Link a device.",
                            )
                        } else {
                            (
                                "This code expired",
                                "WhatsApp only keeps a link code alive for about two \
                                 minutes.",
                            )
                        };
                        ui.vertical_centered(|ui| {
                            ui.add_space(12.0);
                            ui.label(
                                RichText::new(title)
                                    .font(theme::semibold(15.0))
                                    .color(p.text),
                            );
                            ui.add_space(6.0);
                            ui.label(
                                RichText::new(body)
                                    .font(theme::regular(12.0))
                                    .color(p.text_muted),
                            );
                            ui.add_space(14.0);
                            if primary_button(ui, &p, "Show a new code") {
                                self.submit_auth();
                            }
                            ui.add_space(4.0);
                        });
                    }
                    AuthPrompt::Qr { .. } => {
                        ui.vertical_centered(|ui| {
                            if let Some((_, texture)) = &self.qr {
                                let size = texture.size_vec2();
                                ui.add(
                                    egui::Image::new((texture.id(), size))
                                        .corner_radius(6),
                                );
                            } else {
                                ui.add_space(90.0);
                                ui.label(
                                    RichText::new("Waiting for the QR code…")
                                        .color(p.text_muted),
                                );
                                ui.add_space(90.0);
                            }
                            ui.add_space(8.0);
                            ui.label(
                                RichText::new("WhatsApp › Linked devices › Link a device")
                                    .size(12.0)
                                    .color(p.text_muted),
                            );
                        });
                    }
                    AuthPrompt::Credentials => {
                        ui.label(
                            RichText::new("Telegram API credentials")
                                .font(theme::semibold(14.0))
                                .color(p.text),
                        );
                        ui.add_space(4.0);
                        ui.label(
                            RichText::new(
                                "Telegram blocks third-party clients without an application \
                                 id. Create one at my.telegram.org › API development tools, \
                                 then paste the pair here.",
                            )
                            .font(theme::regular(12.0))
                            .color(p.text_muted),
                        );
                        ui.add_space(10.0);
                        ui.label(RichText::new("api_id").font(theme::regular(12.5)).color(p.text));
                        ui.add(
                            egui::TextEdit::singleline(&mut self.prompt_buf)
                                .hint_text("1234567")
                                .desired_width(f32::INFINITY)
                                .font(theme::regular(13.5)),
                        );
                        ui.add_space(6.0);
                        ui.label(RichText::new("api_hash").font(theme::regular(12.5)).color(p.text));
                        ui.add(
                            egui::TextEdit::singleline(&mut self.prompt_buf2)
                                .hint_text("32 hex characters")
                                .desired_width(f32::INFINITY)
                                .font(theme::regular(13.5)),
                        );
                        ui.add_space(10.0);
                        widgets::link(ui, "Open my.telegram.org", "https://my.telegram.org/apps", &p);
                        ui.add_space(10.0);
                        if primary_button(ui, &p, "Continue") {
                            self.submit_auth();
                        }
                    }
                    AuthPrompt::Phone => {
                        ui.label(RichText::new("Phone number").color(p.text));
                        ui.add(
                            egui::TextEdit::singleline(&mut self.prompt_buf)
                                .hint_text("+62…")
                                .desired_width(f32::INFINITY),
                        );
                        ui.add_space(10.0);
                        if primary_button(ui, &p, "Continue") {
                            self.submit_auth();
                        }
                    }
                    AuthPrompt::Code => {
                        ui.label(RichText::new("Login code").color(p.text));
                        ui.add(
                            egui::TextEdit::singleline(&mut self.prompt_buf)
                                .hint_text("12345")
                                .desired_width(f32::INFINITY),
                        );
                        ui.add_space(6.0);
                        ui.label(
                            RichText::new("Sent inside Telegram, not by SMS.")
                                .size(11.5)
                                .color(p.text_muted),
                        );
                        ui.add_space(10.0);
                        if primary_button(ui, &p, "Verify") {
                            self.submit_auth();
                        }
                    }
                    AuthPrompt::Password => {
                        ui.label(RichText::new("Two-step password").color(p.text));
                        ui.add(
                            egui::TextEdit::singleline(&mut self.prompt_buf)
                                .password(true)
                                .desired_width(f32::INFINITY),
                        );
                        ui.add_space(10.0);
                        if primary_button(ui, &p, "Unlock") {
                            self.submit_auth();
                        }
                    }
                    AuthPrompt::Token => {
                        ui.label(RichText::new("Bot token").color(p.text));
                        ui.add_space(2.0);
                        Frame::new()
                            .fill(p.input_bg)
                            .corner_radius(10)
                            .inner_margin(Margin::symmetric(10, 7))
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.add(
                                    egui::TextEdit::singleline(&mut self.prompt_buf)
                                        .password(true)
                                        .hint_text("paste bot token")
                                        .frame(egui::Frame::NONE)
                                        .desired_width(f32::INFINITY),
                                );
                            });
                        ui.add_space(10.0);
                        ui.label(
                            RichText::new("Create the bot first")
                                .font(theme::semibold(12.5))
                                .color(p.text),
                        );
                        ui.add_space(4.0);
                        for (step, text) in [
                            (
                                "1",
                                "Open the Developer Portal and press New Application.",
                            ),
                            (
                                "2",
                                "In the app, open Bot \u{203a} Reset Token and copy the token.",
                            ),
                            (
                                "3",
                                "At the bottom of OAuth2, tick bot scope, then invite the bot \
                                 to your server.",
                            ),
                            (
                                "4",
                                "Paste the token above. United reads servers and channels \
                                 the bot can already see.",
                            ),
                        ] {
                            ui.horizontal_wrapped(|ui| {
                                ui.spacing_mut().item_spacing.x = 6.0;
                                ui.label(
                                    RichText::new(step)
                                        .font(theme::semibold(11.5))
                                        .color(p.accent),
                                );
                                ui.label(RichText::new(text).size(11.5).color(p.text_muted));
                            });
                        }
                        ui.add_space(6.0);
                        widgets::link(
                            ui,
                            "Open Discord Developer Portal",
                            "https://discord.com/developers/applications",
                            &p,
                        );
                        widgets::link(
                            ui,
                            "Bot authorization guide",
                            "https://discord.com/developers/docs/topics/oauth2#bot-authorization-flow",
                            &p,
                        );
                        ui.add_space(10.0);
                        if primary_button(ui, &p, "Connect") {
                            self.submit_auth();
                        }
                    }
                }
            });
        if !open {
            self.prompt = None;
            self.qr = None;
        }
    }

    /// Full-window voice call screen: ringing copy, answer/decline while
    /// incoming, duration and hangup once media is up.
    fn call_overlay(&mut self, ctx: &egui::Context) {
        let Some(call) = self.call.as_ref() else {
            return;
        };
        let CallUi {
            service,
            chat_id,
            call_id,
            state,
            title,
            connected,
            ..
        } = call;
        let (service, chat_id, call_id, state, title, connected) = (
            *service,
            chat_id.clone(),
            call_id.clone(),
            *state,
            title.clone(),
            *connected,
        );
        let avatar = self
            .chats
            .iter()
            .find(|chat| chat.id == chat_id)
            .and_then(|chat| chat.avatar.clone());
        let picture = self.avatar_texture(ctx, avatar.as_deref());
        let screen = ctx.content_rect();
        let mut action = 0u8;
        let duration = connected.map(|at| at.elapsed().as_secs()).unwrap_or(0);
        egui::Area::new(egui::Id::new("call-overlay"))
            .order(egui::Order::Foreground)
            .fixed_pos(screen.min)
            .show(ctx, |ui| {
                ui.allocate_rect(screen, Sense::click());
                ui.painter().rect_filled(
                    screen,
                    CornerRadius::ZERO,
                    Color32::from_rgba_unmultiplied(11, 20, 26, 244),
                );
                let card = Rect::from_center_size(
                    screen.center(),
                    egui::vec2(
                        320.0,
                        if state == CallState::Incoming {
                            330.0
                        } else {
                            300.0
                        },
                    ),
                );
                ui.scope_builder(egui::UiBuilder::new().max_rect(card), |ui| {
                    ui.painter().rect_filled(
                        card,
                        CornerRadius::same(18),
                        Color32::from_rgb(17, 27, 34),
                    );
                    ui.painter().rect_stroke(
                        card,
                        CornerRadius::same(18),
                        Stroke::new(1.0, Color32::from_white_alpha(18)),
                        egui::StrokeKind::Inside,
                    );
                    ui.vertical_centered(|ui| {
                        ui.add_space(18.0);
                        widgets::avatar(ui, &title, 104.0, &title, picture.as_ref());
                        ui.add_space(16.0);
                        ui.label(
                            RichText::new(&title)
                                .font(theme::semibold(22.0))
                                .color(Color32::WHITE),
                        );
                        ui.add_space(4.0);
                        let status = match state {
                            CallState::Incoming => "Panggilan suara masuk".to_string(),
                            CallState::Ringing => "Memanggil…".to_string(),
                            CallState::Connecting => "Menghubungkan…".to_string(),
                            CallState::Active => {
                                format!("{:02}:{:02}", duration / 60, duration % 60)
                            }
                            CallState::Ended => String::new(),
                        };
                        ui.label(
                            RichText::new(status)
                                .font(theme::regular(13.5))
                                .color(Color32::from_white_alpha(190)),
                        );
                        ui.add_space(28.0);
                        ui.horizontal(|ui| {
                            let gap = 34.0;
                            let incoming = state == CallState::Incoming;
                            if incoming {
                                let decline = egui::Button::new(
                                    RichText::new("Tolak")
                                        .font(theme::semibold(14.0))
                                        .color(Color32::WHITE),
                                )
                                .fill(Color32::from_rgb(0xd9, 0x3f, 0x3f))
                                .min_size(egui::vec2(104.0, 40.0))
                                .corner_radius(CornerRadius::same(20));
                                if ui.add(decline).clicked() {
                                    action = 2;
                                }
                                let accept = egui::Button::new(
                                    RichText::new("Terima")
                                        .font(theme::semibold(14.0))
                                        .color(Color32::from_rgb(11, 20, 26)),
                                )
                                .fill(Color32::from_rgb(0x25, 0xd3, 0x66))
                                .min_size(egui::vec2(104.0, 40.0))
                                .corner_radius(CornerRadius::same(20));
                                if ui.add(accept).clicked() {
                                    action = 1;
                                }
                            } else {
                                let end = egui::Button::new(
                                    RichText::new("Akhiri")
                                        .font(theme::semibold(14.0))
                                        .color(Color32::WHITE),
                                )
                                .fill(Color32::from_rgb(0xd9, 0x3f, 0x3f))
                                .min_size(egui::vec2(120.0, 40.0))
                                .corner_radius(CornerRadius::same(20));
                                if ui.add(end).clicked() {
                                    action = 3;
                                }
                            }
                            ui.add_space(gap);
                        });
                    });
                });
            });
        if matches!(
            state,
            CallState::Ringing | CallState::Connecting | CallState::Active
        ) {
            ctx.request_repaint_after(Duration::from_secs(1));
        }
        if action == 0 {
            return;
        }
        let Some(adapter) = self.adapters.get(&service).cloned() else {
            return;
        };
        if action == 3 {
            if call_id.is_empty() {
                self.call = None;
                return;
            }
            let id = call_id.clone();
            tokio::spawn(async move {
                let _ = adapter.end_call(&id).await;
            });
            return;
        }
        let id = call_id.clone();
        tokio::spawn(async move {
            let result = if action == 1 {
                adapter.answer_call(&id).await
            } else {
                adapter.reject_call(&id).await
            };
            if let Err(err) = result {
                log::warn!("call action failed: {err:#}");
            }
        });
    }

    /// Small modal for editing one outgoing message, like WhatsApp's edit box.
    fn edit_dialog(&mut self, ctx: &egui::Context) {
        let Some((message, mut text)) = self.edit_target.clone() else {
            return;
        };
        let mut open = true;
        let mut commit = false;
        let mut cancel = false;
        egui::Window::new("Edit pesan")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.set_width(360.0);
                ui.add(
                    egui::TextEdit::multiline(&mut text)
                        .desired_rows(3)
                        .desired_width(f32::INFINITY),
                );
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Batal").clicked() {
                        cancel = true;
                    }
                    let save = egui::Button::new(
                        RichText::new("Simpan").color(Color32::from_rgb(11, 20, 26)),
                    )
                    .fill(Color32::from_rgb(0x25, 0xd3, 0x66));
                    if ui.add(save).clicked() {
                        commit = true;
                    }
                });
            });
        let body = text.trim().to_string();
        if commit && !body.is_empty() && body != message.body {
            if let Some(adapter) = self.adapters.get(&message.service).cloned() {
                let target = self
                    .store
                    .chat(&message.account.0, &message.chat_id)
                    .ok()
                    .flatten()
                    .map(|chat| chat.target)
                    .unwrap_or_else(|| message.chat_id.clone());
                let message = message.clone();
                tokio::spawn(async move {
                    if let Err(err) = adapter.edit_text(&target, &message, &body).await {
                        log::warn!("edit failed: {err:#}");
                    }
                });
            }
            self.edit_target = None;
        } else if commit || cancel || !open {
            self.edit_target = None;
        } else {
            self.edit_target = Some((message, text));
        }
    }

    fn toast(&mut self, ctx: &egui::Context) {
        let Some((text, at)) = self.toast.clone() else {
            return;
        };
        if at.elapsed() > Duration::from_secs(4) {
            self.toast = None;
            return;
        }
        egui::Area::new("toast".into())
            .anchor(egui::Align2::CENTER_BOTTOM, Vec2::new(0.0, -30.0))
            .show(ctx, |ui| {
                Frame::new()
                    .fill(Color32::from_rgb(0x32, 0x32, 0x32))
                    .corner_radius(CornerRadius::same(8))
                    .inner_margin(Margin::symmetric(14, 8))
                    .show(ui, |ui| {
                        ui.label(RichText::new(text).color(Color32::WHITE).size(13.0));
                    });
            });
        ctx.request_repaint_after(Duration::from_millis(200));
    }
}

/// Filled accent button used as the modal's single primary action.
/// Digits of a participant id, which is how WhatsApp spells mentions.
fn mention_key(id: &str) -> String {
    id.split('@')
        .next()
        .unwrap_or(id)
        .chars()
        .filter(char::is_ascii_digit)
        .collect()
}

impl UnitedApp {
    /// Display name for an id, falling back to the raw number when the address
    /// book has never heard of it.
    fn name_of(&self, _account: &str, id: &str) -> Option<String> {
        self.contact_names.get(&mention_key(id)).cloned()
    }
}

/// Body token inserted for a picked member. WhatsApp recognises `@<phone>`;
/// other services only ever see plain text, so the name is friendlier there.
fn mention_token(service: Service, candidate: &(String, String)) -> String {
    match service {
        // WhatsApp matches the body token against the JID it is sent with.
        Service::WhatsApp => {
            let key = mention_key(&candidate.0);
            if !key.is_empty() {
                return key;
            }
        }
        // Discord renders `<@id>` as the member's name and notifies them.
        Service::Discord => {
            let id = candidate.0.trim();
            if !id.is_empty() && id.chars().all(|c| c.is_ascii_digit()) {
                return format!("<@{id}>");
            }
        }
        _ => {}
    }
    candidate.1.clone()
}

/// Discord spells a mention `<@id>` (or `<@!id>`); show the member's name.
fn resolve_discord_mentions(body: &str, names: &HashMap<String, String>) -> String {
    if !body.contains("<@") {
        return body.to_string();
    }
    let mut out = String::with_capacity(body.len());
    let mut rest = body;
    while let Some(at) = rest.find("<@") {
        out.push_str(&rest[..at]);
        let after = &rest[at + 2..];
        let after = after.strip_prefix('!').unwrap_or(after);
        let digits: String = after.chars().take_while(char::is_ascii_digit).collect();
        if !digits.is_empty() && after[digits.len()..].starts_with('>') {
            let name = names
                .get(&digits)
                .cloned()
                .unwrap_or_else(|| digits.clone());
            out.push('@');
            out.push_str(&name);
            rest = &after[digits.len() + 1..];
        } else {
            out.push_str("<@");
            rest = after;
        }
    }
    out.push_str(rest);
    out
}

/// Replace every `@<phone>` with `@<name>` for display, using the roster and
/// whoever spoke in this chat. Returns the new body plus the byte ranges of
/// the resulting `@name` tags so the bubble can paint them green.
fn resolve_mentions(
    body: &str,
    names: &HashMap<String, String>,
) -> (String, Vec<std::ops::Range<usize>>) {
    let mut spans = Vec::new();
    if names.is_empty() {
        return (body.to_string(), spans);
    }
    let mut out = String::with_capacity(body.len());
    let bytes = body.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'@' {
            let start = i + 1;
            let mut end = start;
            while end < bytes.len() && bytes[end].is_ascii_digit() {
                end += 1;
            }
            if end > start {
                if let Some(name) = names.get(&body[start..end]) {
                    let tag = out.len();
                    out.push('@');
                    out.push_str(name);
                    spans.push(tag..out.len());
                    i = end;
                    continue;
                }
            }
        }
        let ch = body[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    (out, spans)
}

/// PNG path of the clipboard image, if the user just copied a screenshot.
fn clipboard_image() -> Option<std::path::PathBuf> {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_millis();
    let path = std::env::temp_dir().join(format!("united-paste-{stamp}.png"));
    if let Ok(mut clipboard) = arboard::Clipboard::new()
        && let Ok(image) = clipboard.get_image()
        && let Some(buffer) = image::RgbaImage::from_raw(
            image.width as u32,
            image.height as u32,
            image.bytes.into_owned(),
        )
        && buffer.save(&path).is_ok()
    {
        return Some(path);
    }
    // GNOME does not expose the data-control protocol, so arboard falls back
    // to X11 and sometimes sees an empty selection. xclip speaks to the same
    // XWayland bridge and rescues screenshots copied from native apps.
    let output = std::process::Command::new("xclip")
        .args(["-selection", "clipboard", "-t", "image/png", "-o"])
        .output()
        .ok()?;
    if !output.status.success() || output.stdout.is_empty() {
        return None;
    }
    image::load_from_memory(&output.stdout)
        .ok()?
        .save(&path)
        .ok()?;
    Some(path)
}

/// `@` query at the end of the draft: byte range of "@name" plus the typed
/// text. `None` once whitespace follows, which means the mention is done.
fn mention_query(draft: &str) -> Option<(usize, usize, String)> {
    let at = draft.rfind('@')?;
    if at > 0 && !draft[..at].chars().next_back()?.is_whitespace() {
        return None;
    }
    let query = &draft[at + 1..];
    if query.len() > 40 || query.contains(char::is_whitespace) {
        return None;
    }
    Some((at, draft.len(), query.to_string()))
}

fn apply_mention(draft: &mut String, start: usize, end: usize, name: &str) {
    draft.replace_range(start..end, &format!("@{name} "));
}

fn message_key(message: &Message) -> String {
    format!("{}|{}|{}", message.account.0, message.chat_id, message.id)
}

fn primary_button(ui: &mut egui::Ui, p: &Palette, label: &str) -> bool {
    ui.add(
        egui::Button::new(
            RichText::new(label)
                .font(theme::medium(13.5))
                .color(Color32::WHITE),
        )
        .fill(p.accent)
        .corner_radius(9)
        .min_size(Vec2::new(110.0, 32.0)),
    )
    .clicked()
}

fn detect_system_mode(_ctx: &egui::Context) -> Mode {
    match std::env::var("UNITED_THEME").as_deref() {
        Ok("light") => Mode::Light,
        Ok("dark") => Mode::Dark,
        _ => {
            if std::env::var("GTK_THEME")
                .map(|v| v.contains("dark"))
                .unwrap_or(false)
            {
                Mode::Dark
            } else {
                Mode::Dark
            }
        }
    }
}

/// WhatsApp stores a per-session device suffix in JIDs (`628…:12@s.whatsapp.net`).
/// The same person appears without it in history and in avatar events, so both
/// sides normalise before comparing.
fn normalize_sender(raw: &str) -> String {
    match raw.split_once('@') {
        Some((user, server)) => {
            format!("{}@{}", user.split(':').next().unwrap_or(user), server)
        }
        None => raw.to_string(),
    }
}

pub fn apply_mode(ctx: &egui::Context, mode: Mode) {
    theme::apply_mode(ctx, mode);
}

impl eframe::App for UnitedApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let focused = ctx.input(|i| i.focused);
        if focused != self.window_focused {
            log::info!("window focused: {focused}");
        }
        self.window_focused = focused;
        self.notify_focus.store(focused, Ordering::Relaxed);
        self.frames = self.frames.wrapping_add(1);
        if std::env::var("UNITED_FPS").is_ok() {
            let elapsed = self.fps_mark.elapsed().as_secs_f32();
            if elapsed >= 2.0 {
                log::warn!(
                    "frames: {:.1} fps · chrome {:.1}ms rail {:.1}ms main {:.1}ms",
                    self.frames as f32 / elapsed,
                    self.prof[0] / self.frames as f64,
                    self.prof[1] / self.frames as f64,
                    self.prof[2] / self.frames as f64
                );
                self.frames = 0;
                self.prof = [0.0; 4];
                self.fps_mark = Instant::now();
            }
        }
        self.drain_events(&ctx);
        if self
            .contacts_dirty
            .is_some_and(|at| at.elapsed() >= Duration::from_millis(750))
        {
            self.contacts_dirty = None;
            for account in ["whatsapp:me", "telegram:me", "discord:me"] {
                let _ = self.store.repair_contact_names(account);
                let _ = self.store.backfill_names(account);
            }
            self.refresh_lists();
        }

        let t0 = Instant::now();
        self.chrome(ui);
        let t1 = Instant::now();
        let no_service = matches!(self.view, View::Settings);
        self.rail(ui);
        let t2 = Instant::now();
        if !no_service {
            self.chat_list(ui);
            if matches!(self.view, View::Service(Service::WhatsApp)) {
                match self.wa_section {
                    WaSection::Chats => self.conversation(ui),
                    WaSection::Business | WaSection::Profile => self.wa_page(ui),
                    _ => self.wa_detail(ui),
                }
            } else if matches!(self.view, View::Service(Service::Telegram)) {
                if matches!(self.tg_section, TgSection::Chats) {
                    self.conversation(ui);
                } else {
                    self.tg_detail(ui);
                }
            } else if matches!(self.view, View::Service(Service::Discord)) {
                if matches!(self.dc_section, DcSection::Chat) {
                    self.conversation(ui);
                } else {
                    self.dc_page(ui);
                }
            } else {
                self.conversation(ui);
            }
        } else {
            self.settings_view(ui);
        }
        let t3 = Instant::now();
        let prof = std::env::var("UNITED_PROF").is_ok();
        if prof {
            self.prof[0] += (t1 - t0).as_secs_f64() * 1000.0;
            self.prof[1] += (t2 - t1).as_secs_f64() * 1000.0;
            self.prof[2] += (t3 - t2).as_secs_f64() * 1000.0;
        }
        // A lost texture is rebuilt from the payload, so a resize or a fresh
        // pairing keeps the code on screen.
        if let Some((_, AuthPrompt::Qr { payload })) = &self.prompt {
            if self.qr.is_none() && !payload.is_empty() {
                self.qr = qr::qr_texture(&ctx, payload, "wa-qr").map(|t| (payload.clone(), t));
            }
        }
        self.auth_modal(&ctx);
        self.toast(&ctx);
        self.edit_dialog(&ctx);
        match self.wa_modal {
            Some(WaModal::Media) if matches!(self.view, View::Service(Service::WhatsApp)) => {
                self.wa_media_sheet(&ctx)
            }
            Some(WaModal::Advertise) if matches!(self.view, View::Service(Service::WhatsApp)) => {
                self.wa_advertise_sheet(&ctx)
            }
            Some(_) => self.wa_modal = None,
            None => {}
        }
        self.call_overlay(&ctx);
        if let Some((texture, path)) = self.media_view.clone() {
            let screen = ctx.content_rect();
            let mut close = false;
            let mut copy = false;
            let mut save = false;
            let mut zoom = self.media_zoom;
            let mut pan = self.media_pan;
            egui::Area::new(egui::Id::new("media-view"))
                .order(egui::Order::Foreground)
                .fixed_pos(screen.min)
                .show(&ctx, |ui| {
                    let backdrop = ui.allocate_rect(screen, Sense::click_and_drag());
                    ui.painter().rect_filled(
                        screen,
                        CornerRadius::ZERO,
                        Color32::from_black_alpha(232),
                    );
                    let size = texture.size_vec2();
                    let fit = ((screen.width() - 80.0) / size.x)
                        .min((screen.height() - 80.0) / size.y)
                        .min(1.0);
                    let wheel = ui.input(|i| i.smooth_scroll_delta.y);
                    if backdrop.hovered() && wheel != 0.0 {
                        zoom = (zoom * (1.0 + wheel * 0.002)).clamp(1.0, 8.0);
                        if zoom <= 1.0 {
                            pan = egui::Vec2::ZERO;
                        }
                    }
                    if backdrop.dragged() && zoom > 1.0 {
                        pan += backdrop.drag_delta();
                    }
                    let shown = size * fit * zoom;
                    let limit = ((shown - screen.size()) * 0.5).max(egui::Vec2::ZERO)
                        + egui::Vec2::splat(60.0);
                    pan = egui::vec2(
                        pan.x.clamp(-limit.x, limit.x),
                        pan.y.clamp(-limit.y, limit.y),
                    );
                    let image_rect = Rect::from_center_size(screen.center() + pan, shown);
                    egui::Image::new((texture.id(), shown)).paint_at(ui, image_rect);
                    // Toolbar: same actions as WhatsApp Web's viewer.
                    let mut x = screen.right() - 14.0;
                    for (label, id) in [("Tutup", 0u8), ("Simpan", 1), ("Salin", 2)] {
                        let width = ui
                            .painter()
                            .layout_no_wrap(label.to_owned(), theme::medium(12.5), Color32::WHITE)
                            .size()
                            .x
                            + 24.0;
                        let rect = Rect::from_min_size(
                            egui::pos2(x - width, screen.top() + 14.0),
                            egui::vec2(width, 30.0),
                        );
                        x = rect.left() - 8.0;
                        ui.painter().rect_filled(
                            rect,
                            CornerRadius::same(15),
                            Color32::from_black_alpha(140),
                        );
                        ui.painter().text(
                            rect.center(),
                            egui::Align2::CENTER_CENTER,
                            label,
                            theme::medium(12.5),
                            Color32::WHITE,
                        );
                        let hit =
                            ui.interact(rect, ui.id().with(("media-view-btn", id)), Sense::click());
                        if hit.hovered() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                        if hit.clicked() {
                            match label {
                                "Tutup" => close = true,
                                "Salin" => copy = true,
                                _ => save = true,
                            }
                        }
                    }
                    ui.painter().text(
                        egui::pos2(20.0, screen.bottom() - 18.0),
                        egui::Align2::LEFT_CENTER,
                        if zoom > 1.0 {
                            "Scroll untuk zoom · Seret untuk geser · Esc menutup"
                        } else {
                            "Scroll untuk zoom · Klik latar menutup"
                        },
                        theme::regular(11.5),
                        Color32::from_white_alpha(170),
                    );
                    if backdrop.clicked()
                        && !image_rect
                            .contains(backdrop.interact_pointer_pos().unwrap_or(screen.min))
                    {
                        close = true;
                    }
                    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                        close = true;
                    }
                });
            self.media_zoom = zoom;
            self.media_pan = pan;
            if copy {
                self.copy_image(&path);
            }
            if save {
                let name = std::path::Path::new(&path)
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("united-media");
                self.save_media_as(&path, name);
            }
            if close {
                self.media_view = None;
                self.media_zoom = 1.0;
                self.media_pan = egui::Vec2::ZERO;
            }
        }

        let unread: u32 = self.unified.iter().map(|c| c.unread).sum();
        let title = if unread > 0 {
            format!("({unread}) United")
        } else {
            "United".to_string()
        };
        if title != self.last_title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.last_title = title;
        }

        // Ctrl+Q always quits, even when the window is set to minimize.
        if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Q)) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if ctx.input(|i| i.viewport().close_requested()) && self.settings.minimize_on_close {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
        }

        // Debug hook: start minimized so notification banners can be checked
        // without touching the window.
        if self.frames == 1 && std::env::var("UNITED_START_MIN").is_ok() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
        }
        // Debug hook: UNITED_TEST_NOTIFY=1 fires one desktop notification so
        // the D-Bus path can be checked without waiting for a real message.
        // A number > 0 delays it by that many seconds, which lets another
        // window take focus first.
        if self.frames == 1
            && let Ok(spec) = std::env::var("UNITED_TEST_NOTIFY")
        {
            match spec.parse::<u64>() {
                Ok(delay) if delay > 0 => {
                    std::thread::spawn(move || {
                        std::thread::sleep(Duration::from_secs(delay));
                        notify_banner("United", "Notifikasi tes");
                    });
                }
                _ => self.notify_desktop("United", "Notifikasi tes"),
            }
        }
        // Debug hook: UNITED_CALL_TEST=incoming paints the call screen so its
        // layout can be reviewed without a second phone.
        if self.frames == 1 && std::env::var("UNITED_CALL_TEST").is_ok() {
            self.call = Some(CallUi {
                service: Service::WhatsApp,
                chat_id: String::new(),
                call_id: "test".into(),
                state: CallState::Incoming,
                title: "Panggilan Uji".into(),
                connected: None,
            });
        }

        // Debug hook: UNITED_SCREENSHOT=/path.png dumps one frame for UI review.
        if let Ok(path) = std::env::var("UNITED_SCREENSHOT") {
            // Let the UI settle (and any async prompt arrive) before capturing:
            // UNITED_SHOT_DELAY=seconds overrides the default.
            ctx.request_repaint_after(Duration::from_millis(150));
            let delay = std::env::var("UNITED_SHOT_DELAY")
                .ok()
                .and_then(|v| v.parse::<u64>().ok())
                .unwrap_or(10);
            // Ask again every frame until the compositor answers: a single
            // request can be dropped before the surface is ready, which used
            // to leave no PNG at all.
            if !self.shot_sent && self.started.elapsed().as_secs() >= delay {
                ctx.request_repaint();
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
            }
            let shot = ctx.input(|i| {
                i.events.iter().find_map(|e| match e {
                    egui::Event::Screenshot { image, .. } => Some(image.clone()),
                    _ => None,
                })
            });
            if let Some(image) = shot {
                self.shot_sent = true;
                let [w, h] = image.size;
                let mut rgba = Vec::with_capacity(w * h * 4);
                for px in &image.pixels {
                    rgba.extend_from_slice(&[px.r(), px.g(), px.b(), 255]);
                }
                if let Some(buf) = image::RgbaImage::from_raw(w as u32, h as u32, rgba) {
                    let _ = buf.save(&path);
                }
            }
        }

        if self.settings_dirty && self.last_save.elapsed() > Duration::from_secs(5) {
            self.settings.save();
            self.settings_dirty = false;
            self.last_save = Instant::now();
        }
        // Only animations need a clock: an expired typing notice, a toast
        // fading out, the sync pill, a decode still in flight. Everything else
        // waits for an event to wake the window.
        if !self.typing.is_empty()
            || self.toast.is_some()
            || self.syncing.is_some()
            || !self.decoding.is_empty()
        {
            ctx.request_repaint_after(Duration::from_millis(400));
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.settings.save();
    }
}

/// Fire one desktop banner on a worker thread. D-Bus calls must never stall a
/// frame, and a missing notification daemon must not break the app.
fn notify_banner(title: &str, body: &str) {
    log::info!("desktop notification: {title}");
    let title = title.to_string();
    let body = body.to_string();
    std::thread::spawn(move || {
        match notify_rust::Notification::new()
            .appname("United")
            .icon("united")
            .summary(&title)
            .body(&body)
            .timeout(notify_rust::Timeout::Milliseconds(8000))
            .show()
        {
            // Keep the D-Bus connection alive while the banner is on screen.
            // Dropping the handle immediately makes GNOME close the
            // notification in ~40 ms (NotificationClosed reason=2).
            Ok(handle) => {
                std::thread::sleep(Duration::from_secs(8));
                drop(handle);
            }
            Err(err) => log::warn!("desktop notification failed: {err}"),
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn device_suffix_is_not_a_person() {
        assert_eq!(
            normalize_sender("628123:12@s.whatsapp.net"),
            "628123@s.whatsapp.net"
        );
        assert_eq!(normalize_sender("17158511816899@lid"), "17158511816899@lid");
        assert_eq!(normalize_sender("whatsapp:me"), "whatsapp:me");
    }

    #[test]
    fn mention_query_tracks_the_last_at_word() {
        assert_eq!(mention_query("@"), Some((0, 1, String::new())));
        assert_eq!(mention_query("halo @sa"), Some((5, 8, "sa".into())));
        assert_eq!(mention_query("halo @sa "), None);
        assert_eq!(mention_query("email a@b"), None);
        assert_eq!(mention_query("tanpa tag"), None);

        let mut draft = "halo @sa".to_string();
        apply_mention(&mut draft, 5, 8, "Sari");
        assert_eq!(draft, "halo @Sari ");
    }

    #[test]
    fn whatsapp_mentions_use_phone_tokens_and_show_names() {
        let candidate = ("628123@s.whatsapp.net".to_string(), "Sari".to_string());
        assert_eq!(mention_token(Service::WhatsApp, &candidate), "628123");
        assert_eq!(mention_token(Service::Telegram, &candidate), "Sari");
        let mut names = HashMap::new();
        names.insert("628123".to_string(), "Sari".to_string());
        assert_eq!(
            resolve_mentions("halo @628123, cek ya", &names).0,
            "halo @Sari, cek ya"
        );
        assert_eq!(resolve_mentions("tanpa tag", &names).0, "tanpa tag");
        // The tag's range is what the bubble paints green.
        assert_eq!(resolve_mentions("halo @628123", &names).1, vec![5..10]);
    }

    #[test]
    fn discord_mentions_become_names() {
        let candidate = ("424242".to_string(), "budi".to_string());
        assert_eq!(mention_token(Service::Discord, &candidate), "<@424242>");
        let mut names = HashMap::new();
        names.insert("424242".to_string(), "Budi".to_string());
        assert_eq!(
            resolve_discord_mentions("hai <@424242> dan <@!424242>", &names),
            "hai @Budi dan @Budi"
        );
        assert_eq!(
            resolve_discord_mentions("<@999> apa kabar", &names),
            "@999 apa kabar"
        );
        assert_eq!(resolve_discord_mentions("tanpa tag", &names), "tanpa tag");
    }
}
