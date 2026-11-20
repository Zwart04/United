use std::fmt;

use serde::{Deserialize, Serialize};

/// A messenger United can host natively.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Service {
    WhatsApp,
    Telegram,
    Discord,
}

impl Service {
    pub const ALL: [Service; 3] = [Service::WhatsApp, Service::Telegram, Service::Discord];

    pub fn id(self) -> &'static str {
        match self {
            Service::WhatsApp => "whatsapp",
            Service::Telegram => "telegram",
            Service::Discord => "discord",
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Service::WhatsApp => "WhatsApp",
            Service::Telegram => "Telegram",
            Service::Discord => "Discord",
        }
    }

    pub fn from_id(id: &str) -> Option<Service> {
        Service::ALL.into_iter().find(|s| s.id() == id)
    }
}

impl fmt::Display for Service {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.display_name())
    }
}

/// Stable identifier for one signed-in account, e.g. `telegram:812345`.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AccountId(pub String);

impl AccountId {
    pub fn new(service: Service, native_id: &str) -> Self {
        AccountId(format!("{}:{}", service.id(), native_id))
    }

    pub fn service(&self) -> Option<Service> {
        Service::from_id(self.0.split(':').next().unwrap_or_default())
    }
}

impl fmt::Display for AccountId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum MessageStatus {
    #[default]
    Sent,
    Pending,
    Delivered,
    Read,
    Failed,
}

/// One conversation, keyed by its service-native id inside an account.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Chat {
    pub id: String,
    pub account: AccountId,
    pub service: Service,
    pub title: String,
    pub is_group: bool,
    pub avatar: Option<String>,
    pub unread: u32,
    pub muted: bool,
    /// Unix seconds of the last message (or chat creation when empty).
    pub last_at: i64,
    pub preview: String,
    /// Who wrote the preview, for group chats (WhatsApp Web prefixes
    /// "Sender: text"). Empty for direct chats and own-message history where
    /// the sender is not known.
    #[serde(default)]
    pub preview_sender: String,
    /// Service-specific routing key used by the adapter (JID, peer id, channel id).
    pub target: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Message {
    pub id: String,
    pub account: AccountId,
    pub chat_id: String,
    pub service: Service,
    pub sender_id: String,
    pub sender_name: String,
    pub body: String,
    pub at: i64,
    pub outgoing: bool,
    pub status: MessageStatus,
    /// Optional media descriptor: JSON-encoded [`MediaRef`]. Local paths are
    /// filled in after the file is downloaded, so the UI never talks to a CDN.
    pub media: Option<String>,
    /// Quoted message id when this is a reply.
    pub reply_to: Option<String>,
}

/// One attachment carried by a message: photo, video, voice note, sticker or
/// document. `path` is the local copy the shell renders; the CDN fields let the
/// adapter fetch it lazily when the conversation is opened for the first time.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct MediaRef {
    /// `image`, `video`, `audio`, `sticker` or `document`.
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub direct: Option<String>,
    /// Hex-encoded 32-byte media key (WhatsApp).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enc: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub len: Option<u64>,
    /// Low-resolution preview: base64 JPEG/PNG bytes from the protocol, or a
    /// local thumbnail path. Lets videos and stickers paint before the full
    /// file is downloaded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thumb: Option<String>,
    /// Pixel dimensions from the protocol. The bubble reserves the real
    /// aspect ratio before the file lands, so downloading a photo or video
    /// no longer makes the transcript jump from a 62 px tile to full height.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
}

impl MediaRef {
    pub fn parse(raw: &str) -> Option<Self> {
        serde_json::from_str(raw).ok()
    }

    pub fn encode(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    /// Local copy, when it has already been downloaded.
    pub fn local(&self) -> Option<&str> {
        self.path
            .as_deref()
            .filter(|p| std::path::Path::new(p).is_file())
    }

    /// One-line label for the chat-list preview and message fallbacks.
    pub fn preview(&self) -> String {
        match self.kind.as_str() {
            "image" => "Photo".to_string(),
            "video" => "Video".to_string(),
            "audio" => "Voice message".to_string(),
            "sticker" => "Sticker".to_string(),
            _ => match self.name.as_deref() {
                Some(name) if !name.is_empty() => name.to_string(),
                _ => "Document".to_string(),
            },
        }
    }

    /// Display name for a document tile.
    pub fn file_name(&self) -> String {
        if let Some(name) = self.name.as_deref().filter(|n| !n.is_empty()) {
            return name.to_string();
        }
        self.path
            .as_deref()
            .and_then(|p| p.rsplit('/').next())
            .unwrap_or("Document")
            .to_string()
    }

    pub fn is_image(&self) -> bool {
        self.kind == "image"
    }

    pub fn is_sticker(&self) -> bool {
        self.kind == "sticker"
    }

    pub fn is_video(&self) -> bool {
        self.kind == "video"
    }

    pub fn is_audio(&self) -> bool {
        self.kind == "audio"
    }
}

impl Chat {
    pub fn unread_badge(&self) -> Option<String> {
        match self.unread {
            0 => None,
            1..=99 => Some(self.unread.to_string()),
            _ => Some("99+".to_string()),
        }
    }
}

/// Current wall clock in Unix seconds.
pub fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
