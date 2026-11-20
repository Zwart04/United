//! Local-time formatting. `chrono` handles the platform difference between
//! `localtime_r` (Unix) and the Windows CRT, so the same code builds everywhere.

use chrono::{Datelike, Local, TimeZone};

fn local(ts: i64) -> chrono::DateTime<Local> {
    Local
        .timestamp_opt(ts, 0)
        .single()
        .unwrap_or_else(Local::now)
}

/// `14:05`
pub fn clock(ts: i64) -> String {
    local(ts).format("%H:%M").to_string()
}

/// Chat-list timestamp: time today, `Yest.`, weekday this week, else date.
pub fn chat_time(ts: i64) -> String {
    if ts <= 0 {
        return String::new();
    }
    let t = local(ts);
    let today = Local::now().date_naive();
    let day = t.date_naive();
    if day == today {
        return clock(ts);
    }
    let delta = (today - day).num_days();
    if delta == 1 {
        return "Yest.".into();
    }
    if (1..7).contains(&delta) {
        const NAMES: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
        return NAMES[t.weekday().num_days_from_sunday() as usize].into();
    }
    t.format("%d/%m/%y").to_string()
}

/// Presence line, WhatsApp style: `today at 14:05`, `yesterday at 09:12`,
/// else the date.
pub fn last_seen(ts: i64) -> String {
    if ts <= 0 {
        return "recently".into();
    }
    let t = local(ts);
    let today = Local::now().date_naive();
    let day = t.date_naive();
    let delta = (today - day).num_days();
    let at = t.format("%H:%M").to_string();
    if delta <= 0 {
        format!("today at {at}")
    } else if delta == 1 {
        format!("yesterday at {at}")
    } else {
        t.format("%d/%m/%y").to_string()
    }
}

/// Sticky day separator text inside a conversation.
pub fn day_label(ts: i64) -> String {
    let t = local(ts);
    let today = Local::now().date_naive();
    let day = t.date_naive();
    if day == today {
        return "TODAY".into();
    }
    if (today - day).num_days() == 1 {
        return "YESTERDAY".into();
    }
    let date = t.format("%-d %B").to_string().to_uppercase();
    if day.year() == today.year() {
        date
    } else {
        format!("{date} {}", day.year())
    }
}

/// Small helper for the About page.
pub fn os_name() -> String {
    format!("{} {}", std::env::consts::OS, std::env::consts::ARCH)
}

/// Two-letter initials for the avatar fallback.
pub fn initials(name: &str) -> String {
    let mut out = String::new();
    for word in name.split_whitespace().take(2) {
        if let Some(c) = word.chars().next() {
            out.extend(c.to_uppercase());
        }
    }
    if out.is_empty() {
        out.push('?');
    }
    out
}

/// Deterministic avatar tint from a name, like the web clients' generated avatars.
pub fn avatar_color(seed: &str) -> egui::Color32 {
    let mut hash: u32 = 2166136261;
    for b in seed.bytes() {
        hash ^= b as u32;
        hash = hash.wrapping_mul(16777619);
    }
    let palette: [egui::Color32; 8] = [
        egui::Color32::from_rgb(0x53, 0xb7, 0xde),
        egui::Color32::from_rgb(0x8e, 0x8b, 0xe8),
        egui::Color32::from_rgb(0xe8, 0x83, 0x8b),
        egui::Color32::from_rgb(0x7b, 0xc8, 0x62),
        egui::Color32::from_rgb(0xe0, 0x9f, 0x4a),
        egui::Color32::from_rgb(0x6f, 0xbf, 0xa8),
        egui::Color32::from_rgb(0xc6, 0x7b, 0xd8),
        egui::Color32::from_rgb(0x49, 0xa1, 0x86),
    ];
    palette[(hash as usize) % palette.len()]
}
