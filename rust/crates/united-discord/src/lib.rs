//! Discord account client: REST + Gateway over WebSocket, no browser engine.
//!
//! Uses the same private endpoints the official web client calls, so the
//! unified inbox receives real MESSAGE_CREATE events with channel ids, author
//! ids and timestamps.

use std::sync::{Arc, Mutex};

use anyhow::{Context, Result, anyhow};
use async_trait::async_trait;
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::sync::watch;
use tokio_tungstenite::tungstenite::Message as WsMessage;
use united_core::events::{AuthPrompt, CoreEvent, ServiceStatus};
use united_core::model::{AccountId, Chat, MediaRef, Message, MessageStatus, Service, now_secs};
use united_core::{Adapter, AuthInput, EventBus};

const API: &str = "https://discord.com/api/v9";

pub struct DiscordAdapter {
    client: reqwest::Client,
    bus: Mutex<Option<EventBus>>,
    token: Mutex<Option<String>>,
    prompt: Mutex<Option<AuthPrompt>>,
    account: Arc<Mutex<String>>,
    me_name: Arc<Mutex<String>>,
    shutdown: watch::Sender<bool>,
    task: Mutex<Option<tokio::task::JoinHandle<()>>>,
}

impl DiscordAdapter {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::builder()
                .user_agent("Mozilla/5.0 (X11; Linux x86_64) United/0.3")
                .build()
                .unwrap_or_else(|_| reqwest::Client::new()),
            bus: Mutex::new(None),
            token: Mutex::new(None),
            prompt: Mutex::new(None),
            account: Arc::new(Mutex::new("discord:me".into())),
            me_name: Arc::new(Mutex::new("Discord".into())),
            shutdown: watch::channel(false).0,
            task: Mutex::new(None),
        }
    }

    fn token_path() -> std::path::PathBuf {
        united_core::paths::service_dir("discord").join("token")
    }

    fn load_token() -> Option<String> {
        std::fs::read_to_string(Self::token_path())
            .ok()
            .map(|s| s.trim().trim_start_matches("Bot ").to_string())
            .filter(|s| !s.is_empty())
    }

    fn save_token(token: &str) -> Result<()> {
        let path = Self::token_path();
        std::fs::write(&path, token)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
        }
        Ok(())
    }

    fn publish(&self, event: CoreEvent) {
        if let Some(bus) = self.bus.lock().unwrap().clone() {
            bus.publish(event);
        }
    }

    fn set_status(&self, status: ServiceStatus, detail: Option<String>) {
        self.publish(CoreEvent::Status {
            account: self.account(),
            service: Service::Discord,
            status,
            detail,
        });
    }

    fn stop_task(&self) {
        let _ = self.shutdown.send(true);
        if let Some(handle) = self.task.lock().unwrap().take() {
            handle.abort();
        }
    }

    async fn rest_me(client: &reqwest::Client, token: &str) -> Result<Value> {
        let resp = client
            .get(format!("{API}/users/@me"))
            .header("Authorization", format!("Bot {token}"))
            .send()
            .await
            .context("discord /users/@me")?;
        if !resp.status().is_success() {
            return Err(anyhow!("token rejected ({})", resp.status()));
        }
        Ok(resp.json().await?)
    }

    async fn fetch_channels(client: &reqwest::Client, token: &str) -> Result<Vec<Value>> {
        let resp = client
            .get(format!("{API}/users/@me/channels"))
            .header("Authorization", format!("Bot {token}"))
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(anyhow!("channels: {}", resp.status()));
        }
        let value: Value = resp.json().await?;
        Ok(value.as_array().cloned().unwrap_or_default())
    }

    async fn fetch_messages(
        client: &reqwest::Client,
        token: &str,
        channel: &str,
        limit: usize,
        before: Option<&str>,
    ) -> Result<Vec<Value>> {
        let mut query = vec![("limit", limit.to_string())];
        if let Some(before) = before {
            query.push(("before", before.to_string()));
        }
        let resp = client
            .get(format!("{API}/channels/{channel}/messages"))
            .query(&query)
            .header("Authorization", format!("Bot {token}"))
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(anyhow!("messages: {}", resp.status()));
        }
        let value: Value = resp.json().await?;
        Ok(value.as_array().cloned().unwrap_or_default())
    }
}

impl Default for DiscordAdapter {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Adapter for DiscordAdapter {
    fn service(&self) -> Service {
        Service::Discord
    }

    fn account(&self) -> String {
        self.account.lock().unwrap().clone()
    }

    fn label(&self) -> String {
        self.me_name.lock().unwrap().clone()
    }

    fn sign_in_prompt(&self) -> Option<AuthPrompt> {
        Some(AuthPrompt::Token)
    }

    fn prompt(&self) -> Option<AuthPrompt> {
        self.prompt.lock().unwrap().clone()
    }

    async fn start(&self, bus: EventBus) -> Result<()> {
        *self.bus.lock().unwrap() = Some(bus.clone());
        if let Some(token) = Self::load_token() {
            *self.token.lock().unwrap() = Some(token.clone());
            self.set_status(ServiceStatus::Connecting, None);
            self.spawn_gateway(token);
        } else {
            *self.prompt.lock().unwrap() = Some(AuthPrompt::Token);
            self.set_status(ServiceStatus::NeedsAuth, Some("token required".into()));
            bus.publish(CoreEvent::Auth {
                service: Service::Discord,
                prompt: AuthPrompt::Token,
            });
        }
        Ok(())
    }

    async fn submit(&self, input: AuthInput) -> Result<()> {
        let AuthInput::Token(token) = input else {
            return Ok(());
        };
        let token = token.trim().trim_start_matches("Bot ").to_string();
        if token.is_empty() {
            return Ok(());
        }
        let me = Self::rest_me(&self.client, &token).await?;
        let id = me["id"].as_str().unwrap_or("me");
        let name = me["global_name"]
            .as_str()
            .or_else(|| me["username"].as_str())
            .unwrap_or("Discord")
            .to_string();
        *self.account.lock().unwrap() = AccountId::new(Service::Discord, id).0;
        *self.me_name.lock().unwrap() = name;
        Self::save_token(&token)?;
        *self.token.lock().unwrap() = Some(token.clone());
        *self.prompt.lock().unwrap() = None;
        self.stop_task();
        self.set_status(ServiceStatus::Connecting, None);
        self.spawn_gateway(token);
        Ok(())
    }

    async fn send_text(
        &self,
        chat_id: &str,
        text: &str,
        _reply_to: Option<&Message>,
    ) -> Result<Message> {
        let token = self
            .token
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| anyhow!("not signed in"))?;
        let resp = self
            .client
            .post(format!("{API}/channels/{chat_id}/messages"))
            .header("Authorization", format!("Bot {token}"))
            .json(&json!({ "content": text, "tts": false }))
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(anyhow!("send failed: {}", resp.status()));
        }
        let value: Value = resp.json().await?;
        let message = self.message_from_json(chat_id, &value, true);
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
        let token = self
            .token
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| anyhow!("not signed in"))?;
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("file")
            .to_string();
        let bytes = tokio::fs::read(path)
            .await
            .map_err(|e| anyhow!("read attachment: {e}"))?;
        // Discord sends attachments as multipart with the message metadata in
        // `payload_json`; the file field index matches the attachment id.
        let payload = json!({
            "content": caption.trim(),
            "attachments": [{ "id": 0, "filename": name }]
        });
        let form = reqwest::multipart::Form::new()
            .text("payload_json", payload.to_string())
            .part(
                "files[0]",
                reqwest::multipart::Part::bytes(bytes).file_name(name),
            );
        let resp = self
            .client
            .post(format!("{API}/channels/{chat_id}/messages"))
            .header("Authorization", format!("Bot {token}"))
            .multipart(form)
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(anyhow!("send file failed: {}", resp.status()));
        }
        let value: Value = resp.json().await?;
        let mut message = self.message_from_json(chat_id, &value, true);
        // The bytes are already local; skip a redundant CDN round-trip.
        if let Some(raw) = message.media.as_deref()
            && let Some(mut media) = MediaRef::parse(raw)
        {
            media.path = Some(path.display().to_string());
            message.media = Some(media.encode());
        }
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
        let Some(url) = media.url.clone().filter(|u| !u.is_empty()) else {
            return Ok(None);
        };
        let bytes = self.client.get(url).send().await?.bytes().await?;
        let dir = united_core::paths::service_dir("discord").join("media");
        std::fs::create_dir_all(&dir)?;
        let ext = media
            .name
            .as_deref()
            .and_then(|n| n.rsplit_once('.'))
            .map(|(_, ext)| ext)
            .filter(|ext| ext.len() <= 5)
            .unwrap_or("bin");
        let safe: String = message
            .id
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
            .collect();
        let path = dir.join(format!("{safe}.{ext}"));
        std::fs::write(&path, &bytes)?;
        united_core::paths::prune_media(&dir, united_core::paths::MEDIA_BUDGET);
        let mut done = media.clone();
        done.path = Some(path.display().to_string());
        self.publish(CoreEvent::MediaReady {
            account: message.account.0.clone(),
            chat_id: message.chat_id.clone(),
            message_id: message.id.clone(),
            media: done.encode(),
        });
        Ok(Some(done))
    }

    async fn logout(&self) -> Result<()> {
        self.stop_task();
        *self.token.lock().unwrap() = None;
        let _ = std::fs::remove_file(Self::token_path());
        self.set_status(ServiceStatus::SignedOut, Some("signed out".into()));
        Ok(())
    }

    async fn load_history(&self, chat_id: &str, limit: usize) -> Result<()> {
        let token = self
            .token
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| anyhow!("not signed in"))?;
        // Page backwards from the oldest stored message.
        let anchor = united_core::Store::shared()
            .ok()
            .and_then(|store| {
                store
                    .oldest_message(&self.account(), chat_id)
                    .ok()
                    .flatten()
            })
            .map(|(id, _, _)| id);
        let raw = Self::fetch_messages(
            &self.client,
            &token,
            chat_id,
            limit.min(100),
            anchor.as_deref(),
        )
        .await?;
        for value in raw.iter().rev() {
            let message = self.message_from_json(chat_id, value, false);
            self.publish(CoreEvent::Message {
                message,
                live: false,
            });
        }
        Ok(())
    }

    async fn mark_read(&self, _chat_id: &str) -> Result<()> {
        Ok(())
    }

    async fn shutdown(&self) {
        self.stop_task();
    }
}

impl DiscordAdapter {
    fn spawn_gateway(&self, token: String) {
        let Some(bus) = self.bus.lock().unwrap().clone() else {
            return;
        };
        let client = self.client.clone();
        let account = self.account.clone();
        let me_name = self.me_name.clone();
        let _ = self.shutdown.send(false);
        let shutdown = self.shutdown.subscribe();
        let handle = tokio::spawn(async move {
            gateway_loop(client, token, bus, account, me_name, shutdown).await;
        });
        *self.task.lock().unwrap() = Some(handle);
    }

    fn message_from_json(&self, chat_id: &str, value: &Value, outgoing_hint: bool) -> Message {
        let author = &value["author"];
        let author_id = author["id"].as_str().unwrap_or_default();
        let account = self.account.lock().unwrap().clone();
        let my_id = account
            .strip_prefix("discord:")
            .unwrap_or_default()
            .to_string();
        let outgoing = outgoing_hint || author_id == my_id;
        Message {
            id: value["id"].as_str().unwrap_or_default().to_string(),
            account: AccountId(account),
            chat_id: chat_id.to_string(),
            service: Service::Discord,
            sender_id: author_id.to_string(),
            sender_name: author["global_name"]
                .as_str()
                .or_else(|| author["username"].as_str())
                .unwrap_or("Discord")
                .to_string(),
            body: {
                let text = value["content"].as_str().unwrap_or_default().to_string();
                if text.is_empty() {
                    media_from_discord(value)
                        .map(|m| m.preview())
                        .unwrap_or_default()
                } else {
                    text
                }
            },
            at: value["timestamp"]
                .as_str()
                .map(parse_timestamp)
                .unwrap_or_else(now_secs),
            outgoing,
            status: MessageStatus::Sent,
            media: media_from_discord(value).map(|m| m.encode()),
            reply_to: None,
        }
    }
}

/// CDN link + filename of the first attachment on a Discord message. The bytes
/// are fetched later by [`DiscordAdapter::download_media`].
fn media_from_discord(value: &Value) -> Option<MediaRef> {
    let attachment = value["attachments"].as_array()?.first()?;
    let mime = attachment["content_type"]
        .as_str()
        .unwrap_or_default()
        .to_string();
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
        name: attachment["filename"].as_str().map(str::to_string),
        mime: (!mime.is_empty()).then_some(mime),
        url: attachment["url"].as_str().map(str::to_string),
        len: attachment["size"].as_u64(),
        ..MediaRef::default()
    })
}

async fn gateway_loop(
    client: reqwest::Client,
    token: String,
    bus: EventBus,
    account: std::sync::Arc<Mutex<String>>,
    me_name: std::sync::Arc<Mutex<String>>,
    mut shutdown: watch::Receiver<bool>,
) {
    let mut backoff = 2u64;
    loop {
        if *shutdown.borrow() {
            return;
        }
        match gateway_once(&client, &token, &bus, &account, &me_name, &mut shutdown).await {
            Ok(()) => backoff = 2,
            Err(err) => {
                log::warn!("discord gateway: {err:#}");
                bus.publish(CoreEvent::Status {
                    account: account.lock().unwrap().clone(),
                    service: Service::Discord,
                    status: ServiceStatus::Error,
                    detail: Some(err.to_string()),
                });
            }
        }
        tokio::select! {
            _ = shutdown.changed() => return,
            _ = tokio::time::sleep(std::time::Duration::from_secs(backoff)) => {}
        }
        backoff = (backoff * 2).min(60);
    }
}

async fn gateway_once(
    client: &reqwest::Client,
    token: &str,
    bus: &EventBus,
    account: &std::sync::Arc<Mutex<String>>,
    me_name: &std::sync::Arc<Mutex<String>>,
    shutdown: &mut watch::Receiver<bool>,
) -> Result<()> {
    let info: Value = client
        .get(format!("{API}/gateway"))
        .send()
        .await?
        .json()
        .await?;
    let base = info["url"].as_str().unwrap_or("wss://gateway.discord.gg");
    let url = format!("{base}/?v=9&encoding=json");
    let (socket, _) = tokio_tungstenite::connect_async(&url).await?;
    let (mut write, mut read) = socket.split();

    let mut seq: Option<i64> = None;
    let mut heartbeat = tokio::time::interval(std::time::Duration::from_secs(41));

    loop {
        tokio::select! {
            _ = shutdown.changed() => {
                let _ = write.close().await;
                return Ok(());
            }
            _ = heartbeat.tick() => {
                let payload = json!({"op": 1, "d": seq});
                write.send(WsMessage::Text(payload.to_string().into())).await?;
            }
            frame = read.next() => {
                let Some(frame) = frame else { return Ok(()) };
                let frame = frame?;
                let text = match frame {
                    WsMessage::Text(t) => t.to_string(),
                    WsMessage::Binary(b) => String::from_utf8_lossy(&b).to_string(),
                    WsMessage::Close(_) => return Ok(()),
                    _ => continue,
                };
                let value: Value = match serde_json::from_str(&text) {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                if let Some(s) = value["s"].as_i64() {
                    seq = Some(s);
                }
                match value["op"].as_i64().unwrap_or(-1) {
                    10 => {
                        let interval = value["d"]["heartbeat_interval"].as_u64().unwrap_or(41250);
                        heartbeat = tokio::time::interval(std::time::Duration::from_millis(interval));
                        let identify = json!({
                            "op": 2,
                            "d": {
                                "token": token,
                                "capabilities": 16381,
                                "compress": false,
                                "properties": {
                                    "os": "Linux",
                                    "browser": "Chrome",
                                    "device": ""
                                },
                                "presence": {"status": "online", "since": 0, "activities": [], "afk": false}
                            }
                        });
                        write.send(WsMessage::Text(identify.to_string().into())).await?;
                    }
                    11 => {}
                    7 => return Ok(()),
                    9 => return Err(anyhow!("invalid session")),
                    0 => {
                        let event = value["t"].as_str().unwrap_or_default();
                        let data = &value["d"];
                        match event {
                            "READY" => {
                                let user = &data["user"];
                                if let Some(id) = user["id"].as_str() {
                                    *account.lock().unwrap() = AccountId::new(Service::Discord, id).0;
                                }
                                if let Some(name) = user["global_name"].as_str().or_else(|| user["username"].as_str()) {
                                    *me_name.lock().unwrap() = name.to_string();
                                }
                                bus.publish(CoreEvent::Status {
                                    account: account.lock().unwrap().clone(),
                                    service: Service::Discord,
                                    status: ServiceStatus::Online,
                                    detail: Some("gateway connected".into()),
                                });
                                let mut chats = sync_channels(client, token, account, data).await;
                                for (chat, mentions) in chats.iter_mut() {
                                    chat.unread = *mentions;
                                }
                                let list: Vec<Chat> = chats.into_iter().map(|(c, _)| c).collect();
                                bus.publish(CoreEvent::Chats { service: Service::Discord, chats: list.clone() });
                                // Warm the store with recent history, slowly enough to
                                // stay inside the global rate limit.
                                for chat in list.iter().take(20) {
                                    if *shutdown.borrow() { break; }
                                    if let Ok(raw) = DiscordAdapter::fetch_messages(client, token, &chat.id, 30, None).await {
                                        for value in raw.iter().rev() {
                                            let author = &value["author"];
                                            let author_id = author["id"].as_str().unwrap_or_default();
                                            let message = Message {
                                                id: value["id"].as_str().unwrap_or_default().to_string(),
                                                account: AccountId(account.lock().unwrap().clone()),
                                                chat_id: chat.id.clone(),
                                                service: Service::Discord,
                                                sender_id: author_id.to_string(),
                                                sender_name: author["global_name"].as_str()
                                                    .or_else(|| author["username"].as_str())
                                                    .unwrap_or("Discord").to_string(),
                                                body: {
                                                    let text = value["content"].as_str().unwrap_or_default();
                                                    if text.is_empty() {
                                                        media_from_discord(value).map(|m| m.preview()).unwrap_or_default()
                                                    } else {
                                                        text.to_string()
                                                    }
                                                },
                                                at: value["timestamp"].as_str().map(parse_timestamp).unwrap_or_else(now_secs),
                                                outgoing: {
                                                    let me = account.lock().unwrap().clone();
                                                    author_id == me.strip_prefix("discord:").unwrap_or_default()
                                                },
                                                status: MessageStatus::Sent,
                                                media: media_from_discord(value).map(|m| m.encode()),
                                                reply_to: None,
                                            };
                                            bus.publish(CoreEvent::Message { message, live: false });
                                        }
                                    }
                                    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
                                }
                            }
                            "MESSAGE_CREATE" => {
                                let channel = data["channel_id"].as_str().unwrap_or_default();
                                let author = &data["author"];
                                let author_id = author["id"].as_str().unwrap_or_default();
                                let my_id = account.lock().unwrap().clone();
                                let my_id = my_id.strip_prefix("discord:").unwrap_or_default().to_string();
                                let message = Message {
                                    id: data["id"].as_str().unwrap_or_default().to_string(),
                                    account: AccountId(account.lock().unwrap().clone()),
                                    chat_id: channel.to_string(),
                                    service: Service::Discord,
                                    sender_id: author_id.to_string(),
                                    sender_name: author["global_name"].as_str()
                                        .or_else(|| author["username"].as_str())
                                        .unwrap_or("Discord").to_string(),
                                    body: {
                                        let text = data["content"].as_str().unwrap_or_default();
                                        if text.is_empty() {
                                            media_from_discord(&data).map(|m| m.preview()).unwrap_or_default()
                                        } else {
                                            text.to_string()
                                        }
                                    },
                                    at: data["timestamp"].as_str().map(parse_timestamp).unwrap_or_else(now_secs),
                                    outgoing: author_id == my_id,
                                    status: MessageStatus::Sent,
                                    media: media_from_discord(&data).map(|m| m.encode()),
                                    reply_to: None,
                                };
                                bus.publish(CoreEvent::Message { message, live: true });
                            }
                            "CHANNEL_CREATE" | "CHANNEL_UPDATE" => {
                                if let Some(chat) = channel_to_chat(data, account) {
                                    bus.publish(CoreEvent::ChatUpsert { chat });
                                }
                            }
                            _ => {}
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}

async fn sync_channels(
    client: &reqwest::Client,
    token: &str,
    account: &std::sync::Arc<Mutex<String>>,
    ready: &Value,
) -> Vec<(Chat, u32)> {
    let mentions: std::collections::HashMap<String, u32> = ready["read_state"]
        .as_array()
        .map(|rows| {
            rows.iter()
                .filter_map(|r| {
                    Some((
                        r["id"].as_str()?.to_string(),
                        r["mention_count"].as_u64().unwrap_or(0) as u32,
                    ))
                })
                .collect()
        })
        .unwrap_or_default();

    let Ok(channels) = DiscordAdapter::fetch_channels(client, token).await else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for value in channels {
        let Some(chat) = channel_to_chat(&value, account) else {
            continue;
        };
        let count = mentions.get(&chat.id).copied().unwrap_or(0);
        out.push((chat, count));
    }
    out.sort_by(|a, b| b.0.last_at.cmp(&a.0.last_at));
    out
}

fn channel_to_chat(value: &Value, account: &std::sync::Arc<Mutex<String>>) -> Option<Chat> {
    let id = value["id"].as_str()?.to_string();
    let kind = value["type"].as_u64().unwrap_or(1);
    // 1 = DM, 3 = group DM. Servers (guild channels) are out of scope for the
    // inbox and need a separate channel tree.
    if kind != 1 && kind != 3 {
        return None;
    }
    let recipients = value["recipients"].as_array().cloned().unwrap_or_default();
    let title = if kind == 3 {
        value["name"]
            .as_str()
            .map(|s| s.to_string())
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| {
                recipients
                    .iter()
                    .filter_map(|r| r["global_name"].as_str().or_else(|| r["username"].as_str()))
                    .collect::<Vec<_>>()
                    .join(", ")
            })
    } else {
        recipients
            .first()
            .and_then(|r| r["global_name"].as_str().or_else(|| r["username"].as_str()))
            .unwrap_or("Direct message")
            .to_string()
    };
    let last_at = value["last_message_id"]
        .as_str()
        .and_then(snowflake_to_secs)
        .unwrap_or(0);
    Some(Chat {
        id: id.clone(),
        account: AccountId(account.lock().unwrap().clone()),
        service: Service::Discord,
        title: if title.is_empty() {
            "Direct message".into()
        } else {
            title
        },
        is_group: kind == 3,
        avatar: None,
        unread: 0,
        muted: false,
        last_at,
        preview: String::new(),
        preview_sender: String::new(),
        target: id,
    })
}

fn snowflake_to_secs(id: &str) -> Option<i64> {
    let value: u64 = id.parse().ok()?;
    Some(((value >> 22) + 1_420_070_400_000) as i64 / 1000)
}

/// Discord sends ISO-8601 UTC (`2026-11-14T10:11:12.123000+00:00`).
fn parse_timestamp(text: &str) -> i64 {
    let bytes = text.as_bytes();
    if bytes.len() < 19 {
        return now_secs();
    }
    let num = |range: std::ops::Range<usize>| -> i64 {
        text.get(range)
            .and_then(|s| s.parse::<i64>().ok())
            .unwrap_or(0)
    };
    let year = num(0..4);
    let month = num(5..7);
    let day = num(8..10);
    let hour = num(11..13);
    let minute = num(14..16);
    let second = num(17..19);
    let days = days_from_civil(year, month.clamp(1, 12), day.clamp(1, 31));
    days * 86_400 + hour * 3600 + minute * 60 + second
}

/// Howard Hinnant's civil-from-days inverse.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamp_roundtrip() {
        assert_eq!(parse_timestamp("1970-01-01T00:00:00.000000+00:00"), 0);
        assert_eq!(
            parse_timestamp("2026-11-14T10:11:12.000000+00:00"),
            1_794_651_072
        );
    }

    #[test]
    fn snowflake_is_plausible() {
        // A 2026 snowflake sits around 1.4e18.
        let secs = snowflake_to_secs("1400000000000000000").unwrap();
        assert!(secs > 1_700_000_000 && secs < 1_900_000_000);
    }
}
