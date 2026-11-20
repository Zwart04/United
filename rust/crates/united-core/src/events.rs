use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use crate::model::{Chat, Message, MessageStatus, Service};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ServiceStatus {
    /// Adapter loaded but not paired/signed in.
    SignedOut,
    /// Waiting for the user: QR scan, phone code, password, or token.
    NeedsAuth,
    Connecting,
    Online,
    Error,
}

#[derive(Clone, Debug)]
pub enum AuthPrompt {
    /// WhatsApp QR payload to render.
    Qr {
        payload: String,
    },
    /// Telegram sign-in steps.
    Phone,
    Code,
    Password,
    /// Discord: paste the token issued by the web client.
    Token,
    /// Telegram: ask for the app credentials Telegram requires before any
    /// phone login can be attempted.
    Credentials,
}

/// Everything an adapter reports to the shell.
#[derive(Clone, Debug)]
pub enum CoreEvent {
    Status {
        account: String,
        service: Service,
        status: ServiceStatus,
        detail: Option<String>,
    },
    Auth {
        service: Service,
        prompt: AuthPrompt,
    },
    Chats {
        service: Service,
        chats: Vec<Chat>,
    },
    ChatUpsert {
        chat: Chat,
    },
    Message {
        message: Message,
        /// True when this message arrived while the app was running (drives
        /// notifications and the unified inbox), false during history sync.
        live: bool,
    },
    Read {
        account: String,
        chat_id: String,
    },
    /// Profile picture downloaded to `path` (local file, never a remote URL).
    Avatar {
        account: String,
        service: Service,
        chat_id: String,
        path: String,
    },
    /// Profile picture of one group participant, stored per sender JID. Group
    /// messages show the author's face, not just the chat photo.
    SenderAvatar {
        account: String,
        service: Service,
        jid: String,
        path: String,
    },
    /// Address-book names synced from the phone, as `(identity, name)` pairs.
    /// WhatsApp sends these over app-state sync; without them group messages
    /// show raw ids instead of the name the sender chose.
    Contacts {
        account: String,
        rows: Vec<(String, String)>,
    },
    /// Attachment finished downloading; `media` is the updated [`MediaRef`]
    /// descriptor with a local path.
    MediaReady {
        account: String,
        chat_id: String,
        message_id: String,
        media: String,
    },
    /// One or more of our messages changed state because the recipient's device
    /// reported it (WhatsApp delivery and read receipts).
    Receipt {
        account: String,
        chat_id: String,
        ids: Vec<String>,
        status: MessageStatus,
    },
    /// Someone in a chat started or stopped typing (or recording audio).
    Typing {
        account: String,
        chat_id: String,
        /// The person, as the id the service addresses them by.
        who: String,
        recording: bool,
        active: bool,
    },
    /// A contact came online or went away, with when they were last seen.
    Presence {
        account: String,
        /// Chat or person the status belongs to, service-native.
        chat_id: String,
        online: bool,
        last_seen: Option<i64>,
    },
    /// Voice call state for the open chat. One call at a time; the UI paints
    /// the incoming banner or the in-call bar from this.
    Call {
        account: String,
        chat_id: String,
        call_id: String,
        state: CallState,
    },
    /// One reaction changed. `emoji` empty removes the sender's reaction.
    Reaction {
        account: String,
        chat_id: String,
        message_id: String,
        sender_id: String,
        emoji: String,
    },
    /// The sender edited a message's text.
    MessageEdit {
        account: String,
        chat_id: String,
        message_id: String,
        body: String,
    },
    /// The sender deleted a message for everyone.
    MessageRevoke {
        account: String,
        chat_id: String,
        message_id: String,
    },
}

/// Lifecycle of a one-to-one voice call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CallState {
    /// The phone is ringing us and the user has not answered.
    Incoming,
    /// We dialed and the peer has not answered.
    Ringing,
    /// Answered, media still negotiating.
    Connecting,
    /// Media is connected.
    Active,
    /// Hung up, rejected, missed, or failed.
    Ended,
}

/// Fan-out bus: every adapter publishes, the UI subscribes.
///
/// A small ring buffer keeps the most recent events so the window — created
/// after the adapters start, and therefore after the first history batch — can
/// replay what it missed instead of losing the conversation list.
#[derive(Clone)]
pub struct EventBus {
    tx: tokio::sync::broadcast::Sender<CoreEvent>,
    recent: Arc<Mutex<VecDeque<CoreEvent>>>,
}

impl EventBus {
    /// Large enough for a cold-start history drain, small enough to stay cheap.
    const REPLAY_CAP: usize = 4096;

    pub fn new() -> Self {
        let (tx, _) = tokio::sync::broadcast::channel(2048);
        Self {
            tx,
            recent: Arc::new(Mutex::new(VecDeque::with_capacity(256))),
        }
    }

    pub fn publish(&self, event: CoreEvent) {
        {
            let mut recent = self.recent.lock().unwrap();
            if recent.len() == Self::REPLAY_CAP {
                recent.pop_front();
            }
            recent.push_back(event.clone());
        }
        let _ = self.tx.send(event);
    }

    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<CoreEvent> {
        self.tx.subscribe()
    }

    /// Everything published so far, oldest first. Used once at startup.
    pub fn replay(&self) -> Vec<CoreEvent> {
        self.recent.lock().unwrap().iter().cloned().collect()
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}
