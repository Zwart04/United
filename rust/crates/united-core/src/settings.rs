use serde::{Deserialize, Serialize};

use crate::paths;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    System,
    Light,
    Dark,
}

impl Default for Theme {
    fn default() -> Self {
        Theme::System
    }
}

/// Persisted app settings. No credentials: sessions live in each adapter's
/// own store under `sessions/`.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct AppSettings {
    pub theme: Theme,
    /// Sidebar order of enabled services.
    pub enabled_services: Vec<String>,
    pub last_service: Option<String>,
    /// Last opened conversation as `[service, account, chat_id]`, restored on
    /// launch like every desktop messenger.
    pub last_chat: Option<[String; 3]>,
    /// Unsent composer text per `service|account|chat`, so a restart does not
    /// throw away a half-written reply.
    pub drafts: std::collections::HashMap<String, String>,
    /// Close button behaviour. Off by default: a window with no visible tray
    /// icon must not be able to trap the process.
    pub minimize_on_close: bool,
    pub launch_at_startup: bool,
    pub desktop_notifications: bool,
    pub window: Option<[f32; 4]>,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            theme: Theme::default(),
            enabled_services: crate::Service::ALL
                .iter()
                .map(|s| s.id().to_string())
                .collect(),
            last_service: None,
            last_chat: None,
            drafts: std::collections::HashMap::new(),
            minimize_on_close: false,
            launch_at_startup: false,
            desktop_notifications: true,
            window: None,
        }
    }
}

impl AppSettings {
    pub fn load() -> Self {
        let path = paths::settings_path();
        std::fs::read_to_string(&path)
            .ok()
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let path = paths::settings_path();
        if let Ok(raw) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(path, raw);
        }
    }

    /// Create or remove the XDG autostart entry for the current executable.
    pub fn apply_autostart(on: bool) {
        let Some(dirs) = directories::BaseDirs::new() else {
            return;
        };
        let dir = dirs.config_dir().join("autostart");
        let file = dir.join("united.desktop");
        if !on {
            let _ = std::fs::remove_file(file);
            return;
        }
        let exe = std::env::current_exe()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| "united".into());
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(
            file,
            format!(
                "[Desktop Entry]\nType=Application\nName=United\nExec={exe}\nX-GNOME-Autostart-enabled=true\n"
            ),
        );
    }

    pub fn is_enabled(&self, service: &str) -> bool {
        self.enabled_services.iter().any(|s| s == service)
    }
}
