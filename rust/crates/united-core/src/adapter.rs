use anyhow::Result;
use async_trait::async_trait;

use crate::events::{AuthPrompt, CoreEvent, EventBus};
use crate::model::{MediaRef, Message, Service};

/// What the user typed into an auth prompt.
#[derive(Clone, Debug)]
pub enum AuthInput {
    Phone(String),
    Code(String),
    Password(String),
    Token(String),
    /// Telegram: application credentials issued at <https://my.telegram.org>.
    ApiCredentials {
        api_id: i32,
        api_hash: String,
    },
    /// Start a fresh pairing window (WhatsApp QR codes expire after ~2 minutes).
    Refresh,
    /// Dismiss the current prompt.
    Cancel,
}

/// A native protocol client. One instance per signed-in account.
#[async_trait]
pub trait Adapter: Send + Sync {
    fn service(&self) -> Service;

    fn account(&self) -> String;

    /// Connect (or restore a saved session) and start publishing events.
    async fn start(&self, bus: EventBus) -> Result<()>;

    /// Answer the active auth prompt.
    async fn submit(&self, input: AuthInput) -> Result<()>;

    /// Current prompt, if the adapter is waiting on the user.
    fn prompt(&self) -> Option<AuthPrompt> {
        None
    }

    /// Unlink the account: revoke the session, delete local credentials. The
    /// shell clears cached chats afterwards.
    async fn logout(&self) -> Result<()> {
        Ok(())
    }

    /// First step the shell should open when the user taps "Sign In". Adapters
    /// that need configuration before a phone/token prompt (Telegram) override
    /// this so the button never leads to a guaranteed failure.
    fn sign_in_prompt(&self) -> Option<AuthPrompt> {
        None
    }

    /// Send one text message. `reply_to` quotes another message when the
    /// protocol supports it (WhatsApp); the others ignore it for now.
    async fn send_text(
        &self,
        chat_id: &str,
        text: &str,
        reply_to: Option<&Message>,
    ) -> Result<Message>;

    /// Download one attachment and hand back its local [`MediaRef`]. Adapters
    /// that cannot fetch media keep the default (nothing to render).
    async fn download_media(&self, _message: &Message) -> Result<Option<MediaRef>> {
        Ok(None)
    }

    /// Upload and send a file. Returns `None` when the protocol has no uploader.
    async fn send_file(
        &self,
        _chat_id: &str,
        _path: &std::path::Path,
        _caption: &str,
    ) -> Result<Option<Message>> {
        Ok(None)
    }

    /// Fetch older messages for a chat (history sync is protocol-specific).
    async fn load_history(&self, chat_id: &str, limit: usize) -> Result<()> {
        let _ = (chat_id, limit);
        Ok(())
    }

    /// Canonical group subject, for a chat stored under a placeholder name
    /// like `Group 62812…-1618833664`. `None` when the protocol has no better
    /// name (or the chat is not a group).
    async fn chat_title(&self, _chat_id: &str) -> Result<Option<String>> {
        Ok(None)
    }

    /// Fetch and publish the chat's profile picture as a `CoreEvent::Avatar`.
    /// Called for chats in the list and on open: history and live traffic both
    /// skip chats that never sent a message this session.
    async fn load_avatar(&self, _chat_id: &str) -> Result<()> {
        Ok(())
    }

    /// People who can be @-mentioned in this chat, as `(id, display name)`.
    /// Empty for direct chats and services without a roster.
    async fn members(&self, _chat_id: &str) -> Result<Vec<(String, String)>> {
        Ok(Vec::new())
    }

    async fn mark_read(&self, chat_id: &str) -> Result<()>;

    /// Tell the service the user is typing in `chat_id` (`false` = stopped).
    async fn typing(&self, _chat_id: &str, _state: bool) -> Result<()> {
        Ok(())
    }

    /// Place a voice call. Adapters without calls keep the default.
    async fn place_call(&self, _chat_id: &str) -> Result<()> {
        Ok(())
    }

    /// Answer a ringing call.
    async fn answer_call(&self, _call_id: &str) -> Result<()> {
        Ok(())
    }

    /// Decline a ringing call.
    async fn reject_call(&self, _call_id: &str) -> Result<()> {
        Ok(())
    }

    /// Hang up an active or outgoing call.
    async fn end_call(&self, _call_id: &str) -> Result<()> {
        Ok(())
    }

    /// Add or remove the user's reaction (`""` removes it).
    async fn react(&self, _chat_id: &str, _message: &Message, _emoji: &str) -> Result<()> {
        Ok(())
    }

    /// Replace the text of one of our messages.
    async fn edit_text(&self, _chat_id: &str, _message: &Message, _body: &str) -> Result<()> {
        Ok(())
    }

    /// Delete a message for every participant.
    async fn revoke_message(&self, _chat_id: &str, _message: &Message) -> Result<()> {
        Ok(())
    }

    /// Watch whether the other side is online. Called once when a chat opens.
    async fn watch_presence(&self, _chat_id: &str) -> Result<()> {
        Ok(())
    }

    /// Flush state and stop background tasks.
    async fn shutdown(&self);

    /// Human-readable account label for the sidebar (phone, @handle, ...).
    fn label(&self) -> String;

    /// Publish a status event; helper for adapters.
    fn status(&self, bus: &EventBus, status: crate::events::ServiceStatus, detail: Option<String>) {
        bus.publish(CoreEvent::Status {
            account: self.account(),
            service: self.service(),
            status,
            detail,
        });
    }
}
