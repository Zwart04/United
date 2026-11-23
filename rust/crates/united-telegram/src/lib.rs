//! Telegram MTProto client built on `grammers`. No web client, no browser.
//!
//! Telegram requires application credentials (`api_id` / `api_hash`) issued at
//! <https://my.telegram.org>. Set `UNITED_TELEGRAM_API_ID` and
//! `UNITED_TELEGRAM_API_HASH`, or write
//! `~/.local/share/united/sessions/telegram/credentials.json`.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result, anyhow};
use async_trait::async_trait;
use grammers_client::SignInError;
use grammers_client::client::{LoginToken, PasswordToken};
use grammers_client::message::InputMessage;
use grammers_client::peer::Peer;
mod session;

use grammers_client::session::updates::UpdatesLike;
use grammers_client::update::Update;
use grammers_client::{Client, SenderPool};
use grammers_session::types::PeerRef;
use session::FileSession;
use tokio::sync::watch;
use united_core::events::{AuthPrompt, CoreEvent, ServiceStatus};
use united_core::model::{AccountId, Chat, MediaRef, Message, MessageStatus, Service};
use united_core::{Adapter, AuthInput, EventBus};

struct Credentials {
    api_id: i32,
    api_hash: String,
}

pub struct TelegramAdapter {
    bus: Mutex<Option<EventBus>>,
    client: Mutex<Option<Client>>,
    updates: Mutex<Option<tokio::sync::mpsc::Receiver<UpdatesLike>>>,
    login: Mutex<Option<(LoginToken, String)>>,
    password: Mutex<Option<PasswordToken>>,
    targets: Arc<Mutex<HashMap<String, PeerRef>>>,
    /// Oldest message id already paged back to, per chat. Telegram ids grow
    /// monotonically, so paging from the newest page keeps the cache gap-free
    /// after the app has been closed for a while.
    anchors: Mutex<HashMap<String, i32>>,
    account: Arc<Mutex<String>>,
    label: Arc<Mutex<String>>,
    prompt: Mutex<Option<AuthPrompt>>,
    shutdown: watch::Sender<bool>,
    tasks: Mutex<Vec<tokio::task::JoinHandle<()>>>,
}

impl TelegramAdapter {
    pub fn new() -> Self {
        Self {
            bus: Mutex::new(None),
            client: Mutex::new(None),
            updates: Mutex::new(None),
            login: Mutex::new(None),
            password: Mutex::new(None),
            targets: Arc::new(Mutex::new(HashMap::new())),
            anchors: Mutex::new(HashMap::new()),
            account: Arc::new(Mutex::new("telegram:me".into())),
            label: Arc::new(Mutex::new("Telegram".into())),
            prompt: Mutex::new(None),
            shutdown: watch::channel(false).0,
            tasks: Mutex::new(Vec::new()),
        }
    }

    fn credentials() -> Option<Credentials> {
        if let (Ok(id), Ok(hash)) = (
            std::env::var("UNITED_TELEGRAM_API_ID"),
            std::env::var("UNITED_TELEGRAM_API_HASH"),
        ) {
            if let Ok(api_id) = id.trim().parse::<i32>() {
                return Some(Credentials {
                    api_id,
                    api_hash: hash.trim().to_string(),
                });
            }
        }
        let path = united_core::paths::service_dir("telegram").join("credentials.json");
        let raw = std::fs::read_to_string(path).ok()?;
        let value: serde_json::Value = serde_json::from_str(&raw).ok()?;
        Some(Credentials {
            api_id: value["api_id"].as_i64()? as i32,
            api_hash: value["api_hash"].as_str()?.to_string(),
        })
    }

    fn publish(&self, event: CoreEvent) {
        if let Some(bus) = self.bus.lock().unwrap().clone() {
            bus.publish(event);
        }
    }

    fn set_status(&self, status: ServiceStatus, detail: Option<String>) {
        self.publish(CoreEvent::Status {
            account: self.account(),
            service: Service::Telegram,
            status,
            detail,
        });
    }

    fn set_prompt(&self, prompt: AuthPrompt) {
        *self.prompt.lock().unwrap() = Some(prompt.clone());
        self.publish(CoreEvent::Auth {
            service: Service::Telegram,
            prompt,
        });
    }

    async fn ensure_client(&self) -> Result<Client> {
        if let Some(client) = self.client.lock().unwrap().clone() {
            return Ok(client);
        }
        let creds = Self::credentials()
            .ok_or_else(|| anyhow!("Telegram needs api_id/api_hash from my.telegram.org"))?;
        let path = united_core::paths::service_dir("telegram").join("session.json");
        let session = Arc::new(FileSession::open(&path));
        let SenderPool {
            runner,
            handle,
            updates,
        } = SenderPool::new(Arc::clone(&session), creds.api_id);
        let client = Client::new(handle);
        tokio::spawn(runner.run());
        *self.updates.lock().unwrap() = Some(updates);
        *self.client.lock().unwrap() = Some(client.clone());
        Ok(client)
    }

    fn start_updates(&self, client: Client) {
        let Some(bus) = self.bus.lock().unwrap().clone() else {
            return;
        };
        let Some(updates) = self.updates.lock().unwrap().take() else {
            return;
        };
        let account = self.account.clone();
        let targets = self.targets.clone();
        let mut shutdown = self.shutdown.subscribe();
        let handle = tokio::spawn(async move {
            let mut stream = match client.stream_updates(updates, Default::default()).await {
                Ok(stream) => stream,
                Err(err) => {
                    log::warn!("telegram updates: {err}");
                    return;
                }
            };
            loop {
                tokio::select! {
                    _ = shutdown.changed() => return,
                    update = stream.next() => {
                        let Ok(Update::NewMessage(update)) = update else {
                            if let Err(err) = update {
                                log::warn!("telegram update stream: {err}");
                                return;
                            }
                            continue;
                        };
                        let msg = update.into_inner();
                        let chat_id = msg.peer_id().to_string();
                        if let Ok(Some(peer_ref)) = msg.peer_ref().await {
                            targets.lock().unwrap().insert(chat_id.clone(), peer_ref);
                        }
                        let peer = msg.peer();
                        let title = peer.and_then(|p| p.name()).unwrap_or("Telegram").to_string();
                        let is_group = matches!(peer, Some(Peer::Group(_)) | Some(Peer::Channel(_)));
                        let sender = msg.sender().and_then(|p| p.name()).unwrap_or(&title).to_string();
                        let at = msg.date().as_second();
                        let account_id = AccountId(account.lock().unwrap().clone());
                        let preview_sender = if is_group {
                            if msg.outgoing() {
                                "Anda".to_string()
                            } else {
                                sender.clone()
                            }
                        } else {
                            String::new()
                        };
                        let media = media_ref(&msg);
                        let body = {
                            let text = msg.text();
                            if text.is_empty() {
                                media.as_ref().map(MediaRef::preview).unwrap_or_default()
                            } else {
                                text.to_string()
                            }
                        };
                        let message = Message {
                            id: msg.id().to_string(),
                            account: account_id.clone(),
                            chat_id: chat_id.clone(),
                            service: Service::Telegram,
                            sender_id: msg.sender_id().map(|id| id.to_string()).unwrap_or_default(),
                            sender_name: sender,
                            body: body.clone(),
                            at,
                            outgoing: msg.outgoing(),
                            status: MessageStatus::Sent,
                            media: media.map(|m| m.encode()),
                            reply_to: None,
                        };
                        bus.publish(CoreEvent::Message { message, live: true });
                        bus.publish(CoreEvent::ChatUpsert { chat: Chat {
                            id: chat_id.clone(),
                            account: account_id,
                            service: Service::Telegram,
                            title,
                            is_group,
                            avatar: None,
                            unread: 0,
                            muted: false,
                            last_at: at,
                            preview: body,
                            preview_sender,
                            target: chat_id,
                        }});
                        let _ = stream.sync_update_state().await;
                    }
                }
            }
        });
        self.tasks.lock().unwrap().push(handle);
    }

    async fn after_login(&self, client: Client) -> Result<()> {
        let me = client.get_me().await.context("telegram get_me")?;
        let id = me.id().bot_api_dialog_id().unwrap_or_default();
        *self.account.lock().unwrap() = AccountId::new(Service::Telegram, &id.to_string()).0;
        let display = [me.first_name(), me.last_name()]
            .into_iter()
            .flatten()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        if !display.is_empty() {
            *self.label.lock().unwrap() = display;
        } else if let Some(username) = me.username() {
            *self.label.lock().unwrap() = format!("@{username}");
        }
        *self.prompt.lock().unwrap() = None;
        self.set_status(ServiceStatus::Online, Some("MTProto connected".into()));
        self.sync_dialogs(client.clone()).await;
        self.start_updates(client);
        Ok(())
    }

    async fn sync_dialogs(&self, client: Client) {
        let Some(bus) = self.bus.lock().unwrap().clone() else {
            return;
        };
        let account = self.account.lock().unwrap().clone();
        let mut dialogs = client.iter_dialogs();
        let mut chats = Vec::new();
        let mut avatars = Vec::new();
        let mut count = 0;
        loop {
            match dialogs.next().await {
                Ok(Some(dialog)) => {
                    let id = dialog.peer_id().to_string();
                    self.targets
                        .lock()
                        .unwrap()
                        .insert(id.clone(), dialog.peer_ref());
                    let title = dialog.peer.name().unwrap_or("Telegram").to_string();
                    let last_at = dialog
                        .last_message
                        .as_ref()
                        .map(|m| m.date().as_second())
                        .unwrap_or(0);
                    let preview = dialog
                        .last_message
                        .as_ref()
                        .map(|m| m.text().to_string())
                        .unwrap_or_default();
                    chats.push(Chat {
                        id: id.clone(),
                        account: AccountId(account.clone()),
                        service: Service::Telegram,
                        title,
                        is_group: matches!(dialog.peer, Peer::Group(_) | Peer::Channel(_)),
                        avatar: None,
                        unread: 0,
                        muted: false,
                        last_at,
                        preview,
                        preview_sender: String::new(),
                        target: id.clone(),
                    });
                    // Profile photos are downloaded after the list is shown so
                    // a slow photo never delays the first paint.
                    if chats.len() <= 40 {
                        avatars.push((dialog.peer_ref(), id));
                    }
                    count += 1;
                    if count >= 200 {
                        break;
                    }
                }
                Ok(None) => break,
                Err(err) => {
                    log::warn!("telegram dialogs: {err}");
                    break;
                }
            }
        }
        bus.publish(CoreEvent::Chats {
            service: Service::Telegram,
            chats,
        });

        for (peer, chat_id) in avatars {
            let bus = bus.clone();
            let account = account.clone();
            let client = client.clone();
            tokio::spawn(async move {
                if let Some(path) = fetch_avatar(&client, peer, &chat_id).await {
                    bus.publish(CoreEvent::Avatar {
                        account,
                        service: Service::Telegram,
                        chat_id,
                        path,
                    });
                }
            });
        }
    }

    fn peer_for(&self, chat_id: &str) -> Result<PeerRef> {
        self.targets
            .lock()
            .unwrap()
            .get(chat_id)
            .copied()
            .ok_or_else(|| anyhow!("unknown Telegram peer"))
    }
}

impl Default for TelegramAdapter {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Adapter for TelegramAdapter {
    fn service(&self) -> Service {
        Service::Telegram
    }

    fn account(&self) -> String {
        self.account.lock().unwrap().clone()
    }

    fn label(&self) -> String {
        self.label.lock().unwrap().clone()
    }

    fn prompt(&self) -> Option<AuthPrompt> {
        self.prompt.lock().unwrap().clone()
    }

    fn sign_in_prompt(&self) -> Option<AuthPrompt> {
        Some(if Self::credentials().is_none() {
            AuthPrompt::Credentials
        } else {
            AuthPrompt::Phone
        })
    }

    async fn start(&self, bus: EventBus) -> Result<()> {
        *self.bus.lock().unwrap() = Some(bus);
        if Self::credentials().is_none() {
            self.set_status(
                ServiceStatus::NeedsAuth,
                Some("Telegram needs the api_id/api_hash from my.telegram.org".into()),
            );
            // Telegram rejects every login without an application id, so ask
            // for it before showing the phone step.
            self.set_prompt(AuthPrompt::Credentials);
            return Ok(());
        }
        let client = self.ensure_client().await?;
        match client.is_authorized().await {
            Ok(true) => {
                self.set_status(ServiceStatus::Connecting, None);
                self.after_login(client).await?;
            }
            Ok(false) => {
                self.set_status(
                    ServiceStatus::NeedsAuth,
                    Some("phone login required".into()),
                );
                self.set_prompt(AuthPrompt::Phone);
            }
            Err(err) => {
                self.set_status(ServiceStatus::Error, Some(err.to_string()));
            }
        }
        Ok(())
    }

    async fn submit(&self, input: AuthInput) -> Result<()> {
        match input {
            AuthInput::ApiCredentials { api_id, api_hash } => {
                let hash = api_hash.trim().to_string();
                if api_id <= 0 {
                    return Err(anyhow!("api_id must be a positive number"));
                }
                if hash.len() < 32 || !hash.chars().all(|c| c.is_ascii_hexdigit()) {
                    return Err(anyhow!("api_hash must be the 32-character hex string"));
                }
                let dir = united_core::paths::service_dir("telegram");
                std::fs::create_dir_all(&dir).context("create telegram session dir")?;
                let path = dir.join("credentials.json");
                let body = serde_json::json!({ "api_id": api_id, "api_hash": hash });
                std::fs::write(&path, body.to_string()).context("write telegram credentials")?;
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
                }
                self.set_status(
                    ServiceStatus::NeedsAuth,
                    Some("phone login required".into()),
                );
                self.set_prompt(AuthPrompt::Phone);
            }
            AuthInput::Phone(phone) => {
                let Some(creds) = Self::credentials() else {
                    // Telegram rejects the login before the code is even sent;
                    // send the user back to the credentials step instead of
                    // failing with a dead end.
                    self.set_prompt(AuthPrompt::Credentials);
                    return Err(anyhow!(
                        "Telegram needs api_id/api_hash from my.telegram.org first"
                    ));
                };
                let client = self.ensure_client().await?;
                let token = client
                    .request_login_code(&phone, &creds.api_hash)
                    .await
                    .map_err(|e| anyhow!("request_login_code: {e}"))?;
                *self.login.lock().unwrap() = Some((token, phone));
                self.set_prompt(AuthPrompt::Code);
            }
            AuthInput::Code(code) => {
                let Some((token, phone)) = self.login.lock().unwrap().take() else {
                    return Ok(());
                };
                let client = self.ensure_client().await?;
                match client.sign_in(&token, &code).await {
                    Ok(_user) => {
                        self.after_login(client).await?;
                    }
                    Err(SignInError::PasswordRequired(password)) => {
                        *self.password.lock().unwrap() = Some(password);
                        *self.login.lock().unwrap() = Some((token, phone));
                        self.set_prompt(AuthPrompt::Password);
                    }
                    Err(err) => {
                        *self.login.lock().unwrap() = Some((token, phone));
                        return Err(anyhow!("telegram sign_in: {err}"));
                    }
                }
            }
            AuthInput::Password(password) => {
                let Some(token) = self.password.lock().unwrap().take() else {
                    return Ok(());
                };
                let client = self.ensure_client().await?;
                client
                    .check_password(token, &password)
                    .await
                    .map_err(|e| anyhow!("telegram password: {e}"))?;
                self.after_login(client).await?;
            }
            _ => {}
        }
        Ok(())
    }

    async fn send_text(
        &self,
        chat_id: &str,
        text: &str,
        _reply_to: Option<&Message>,
    ) -> Result<Message> {
        let client = self
            .client
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| anyhow!("telegram not connected"))?;
        let peer = self.peer_for(chat_id)?;
        let sent = client
            .send_message(peer, text)
            .await
            .map_err(|e| anyhow!("telegram send: {e}"))?;
        let message = Message {
            id: sent.id().to_string(),
            account: AccountId(self.account()),
            chat_id: chat_id.to_string(),
            service: Service::Telegram,
            sender_id: String::new(),
            sender_name: self.label(),
            body: sent.text().to_string(),
            at: sent.date().as_second(),
            outgoing: true,
            status: MessageStatus::Sent,
            media: None,
            reply_to: None,
        };
        self.publish(CoreEvent::Message {
            message: message.clone(),
            live: true,
        });
        Ok(message)
    }

    async fn send_file(
        &self,
        chat_id: &str,
        path: &std::path::Path,
        caption: &str,
    ) -> Result<Option<Message>> {
        let client = self
            .client
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| anyhow!("telegram not connected"))?;
        let peer = self.peer_for(chat_id)?;
        let uploaded = client
            .upload_file(path)
            .await
            .map_err(|e| anyhow!("telegram upload: {e}"))?;
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("file")
            .to_string();
        let mime = guess_mime(path);
        let picture = mime.starts_with("image/");
        let mut input = InputMessage::new();
        if !caption.trim().is_empty() {
            input = input.text(caption.trim());
        }
        input = if picture {
            input.photo(uploaded)
        } else {
            input.document(uploaded)
        };
        let sent = client
            .send_message(peer, input)
            .await
            .map_err(|e| anyhow!("telegram send file: {e}"))?;
        let media = MediaRef {
            kind: if picture { "image" } else { "document" }.into(),
            path: Some(path.display().to_string()),
            name: Some(name),
            mime: Some(mime.to_string()),
            ..MediaRef::default()
        };
        let message = Message {
            id: sent.id().to_string(),
            account: AccountId(self.account()),
            chat_id: chat_id.to_string(),
            service: Service::Telegram,
            sender_id: String::new(),
            sender_name: self.label(),
            body: sent.text().to_string(),
            at: sent.date().as_second(),
            outgoing: true,
            status: MessageStatus::Sent,
            media: Some(media.encode()),
            reply_to: None,
        };
        united_core::paths::prune_media(
            &united_core::paths::service_dir("telegram").join("media"),
            united_core::paths::MEDIA_BUDGET,
        );
        self.publish(CoreEvent::Message {
            message: message.clone(),
            live: true,
        });
        Ok(Some(message))
    }

    async fn download_media(&self, message: &Message) -> Result<Option<MediaRef>> {
        let Some(raw) = message.media.as_deref() else {
            return Ok(None);
        };
        let Some(media) = MediaRef::parse(raw) else {
            return Ok(None);
        };
        if media.local().is_some() {
            return Ok(Some(media));
        }
        let client = self
            .client
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| anyhow!("telegram not connected"))?;
        let peer = self.peer_for(&message.chat_id)?;
        let id = message
            .id
            .parse::<i32>()
            .map_err(|_| anyhow!("bad telegram message id"))?;
        // Telegram has no CDN links: the message object carries the file
        // reference, so re-fetch that one id and download from it.
        let found = client
            .get_messages_by_id(peer, &[id])
            .await
            .map_err(|e| anyhow!("telegram media lookup: {e}"))?
            .into_iter()
            .next()
            .flatten();
        let Some(msg) = found else {
            return Ok(None);
        };
        let dir = united_core::paths::service_dir("telegram").join("media");
        std::fs::create_dir_all(&dir)?;
        let name = format!("{}_{}", message.id, media.file_name());
        let safe: String = name
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '.' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        let path = dir.join(safe);
        let media_kind = msg.media();
        let Some(downloadable) = media_kind else {
            return Ok(None);
        };
        client
            .download_media(&downloadable, &path)
            .await
            .map_err(|e| anyhow!("telegram download: {e}"))?;
        let mut done = media;
        done.path = Some(path.display().to_string());
        united_core::paths::prune_media(&dir, united_core::paths::MEDIA_BUDGET);
        // Videos and documents have no decodable frame of their own: keep the
        // small Telegram thumbnail beside the file so the tile paints instantly.
        if done.thumb.is_none() {
            if let Some(grammers_client::media::Media::Document(doc)) = msg.media() {
                let thumbs = doc.thumbs();
                if let Some(thumb) = thumbs.into_iter().max_by_key(|t| t.size()) {
                    let thumb_path = dir.join(format!("{}.thumb.jpg", message.id));
                    if client.download_media(&thumb, &thumb_path).await.is_ok() {
                        done.thumb = Some(thumb_path.display().to_string());
                    }
                }
            }
        }
        self.publish(CoreEvent::MediaReady {
            account: message.account.0.clone(),
            chat_id: message.chat_id.clone(),
            message_id: message.id.clone(),
            media: done.encode(),
        });
        Ok(Some(done))
    }

    async fn load_history(&self, chat_id: &str, limit: usize) -> Result<()> {
        // Startup restores the last chat before MTProto has connected and the
        // dialog list has populated the peer map. Wait for both instead of
        // failing the one history fetch the open chat will ever ask for.
        let mut waited = 0u32;
        let (client, peer) = loop {
            let client = self.client.lock().unwrap().clone();
            let peer = self.targets.lock().unwrap().get(chat_id).copied();
            if let (Some(client), Some(peer)) = (client, peer) {
                break (client, peer);
            }
            if waited >= 40 {
                return Err(anyhow!("telegram not ready for history"));
            }
            waited += 1;
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        };
        // Page from the newest message first. Until a chat has been paged in
        // this process the store may end days behind (the app only receives
        // live updates while it runs), and anchoring on its oldest row would
        // fetch even older messages while the newest ones never arrive.
        let anchor = self.anchors.lock().unwrap().get(chat_id).copied();
        let mut iterator = client.iter_messages(peer);
        if let Some(id) = anchor {
            iterator = iterator.offset_id(id);
        }
        let mut iter = iterator.limit(limit.min(100));
        let mut oldest: Option<i32> = None;
        while let Some(msg) = iter.next().await.map_err(|e| anyhow!("history: {e}"))? {
            let id = msg.id();
            oldest = Some(oldest.map_or(id, |min| min.min(id)));
            let media = media_ref(&msg);
            let body = {
                let text = msg.text();
                if text.is_empty() {
                    media.as_ref().map(MediaRef::preview).unwrap_or_default()
                } else {
                    text.to_string()
                }
            };
            let message = Message {
                id: msg.id().to_string(),
                account: AccountId(self.account()),
                chat_id: chat_id.to_string(),
                service: Service::Telegram,
                sender_id: msg.sender_id().map(|id| id.to_string()).unwrap_or_default(),
                sender_name: msg
                    .sender()
                    .and_then(|p| p.name())
                    .unwrap_or("Telegram")
                    .to_string(),
                body,
                at: msg.date().as_second(),
                outgoing: msg.outgoing(),
                status: MessageStatus::Sent,
                media: media.map(|m| m.encode()),
                reply_to: None,
            };
            self.publish(CoreEvent::Message {
                message,
                live: false,
            });
        }
        if let Some(id) = oldest {
            self.anchors.lock().unwrap().insert(chat_id.to_string(), id);
        }
        Ok(())
    }

    async fn members(&self, chat_id: &str) -> Result<Vec<(String, String)>> {
        let client = self
            .client
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| anyhow!("telegram not connected"))?;
        let peer = self.peer_for(chat_id)?;
        let mut members = Vec::new();
        let mut iter = client.iter_participants(peer);
        while let Some(participant) = iter.next().await? {
            let id = participant.user.id().to_string();
            let name = participant
                .user
                .first_name()
                .map(str::to_string)
                .unwrap_or_else(|| id.clone());
            members.push((id, name));
            if members.len() >= 200 {
                break;
            }
        }
        Ok(members)
    }

    async fn logout(&self) -> Result<()> {
        let client = self.client.lock().unwrap().clone();
        if let Some(client) = client {
            client
                .sign_out()
                .await
                .map_err(|e| anyhow!("telegram sign out: {e}"))?;
        }
        let path = united_core::paths::service_dir("telegram").join("session.json");
        let _ = std::fs::remove_file(path);
        self.set_status(ServiceStatus::SignedOut, Some("signed out".into()));
        Ok(())
    }

    async fn mark_read(&self, _chat_id: &str) -> Result<()> {
        Ok(())
    }

    async fn shutdown(&self) {
        let _ = self.shutdown.send(true);
        for task in self.tasks.lock().unwrap().drain(..) {
            task.abort();
        }
    }
}

/// Content type from the extension; Telegram uses it to decide previews.
fn guess_mime(path: &std::path::Path) -> &'static str {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    match ext.as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "mp4" => "video/mp4",
        "mov" => "video/quicktime",
        "mkv" => "video/x-matroska",
        "mp3" => "audio/mpeg",
        "ogg" | "opus" => "audio/ogg",
        "wav" => "audio/wav",
        "pdf" => "application/pdf",
        "zip" => "application/zip",
        _ => "application/octet-stream",
    }
}

/// Descriptor for the attachment on one Telegram message. Telegram downloads
/// need the live message object, so only metadata is stored here; the bytes
/// arrive when [`TelegramAdapter::download_media`] re-fetches the id.
fn media_ref(msg: &grammers_client::message::Message) -> Option<MediaRef> {
    use grammers_client::media::Media;
    match msg.media()? {
        Media::Photo(_) => Some(MediaRef {
            kind: "image".into(),
            mime: Some("image/jpeg".into()),
            ..MediaRef::default()
        }),
        Media::Sticker(_) => Some(MediaRef {
            kind: "sticker".into(),
            ..MediaRef::default()
        }),
        Media::Document(doc) => {
            let mime = doc.mime_type().unwrap_or_default().to_string();
            let kind = if mime.starts_with("image/") {
                "image"
            } else if mime.starts_with("video/") {
                "video"
            } else if mime.starts_with("audio/") {
                "audio"
            } else {
                "document"
            };
            Some(MediaRef {
                kind: kind.into(),
                name: doc.name().map(str::to_string),
                mime: (!mime.is_empty()).then_some(mime),
                len: doc.size().map(|size| size as u64),
                ..MediaRef::default()
            })
        }
        _ => None,
    }
}

/// Save the newest profile photo of a peer under `sessions/telegram/avatars`.
async fn fetch_avatar(client: &Client, peer: PeerRef, chat_id: &str) -> Option<String> {
    let mut photos = client.iter_profile_photos(peer);
    let photo = photos.next().await.ok().flatten()?;
    let dir = united_core::paths::service_dir("telegram").join("avatars");
    std::fs::create_dir_all(&dir).ok()?;
    let name: String = chat_id
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    let path = dir.join(format!("{name}.jpg"));
    client.download_media(&photo, &path).await.ok()?;
    Some(path.display().to_string())
}
