//! WhatsApp client over the multi-device protocol (`whatsapp-rust`).
//!
//! Pairing is QR-based, exactly like WhatsApp Web, but the protocol is spoken
//! directly: no Chromium, no DOM scraping. Sessions live in a SQLite device
//! store under `~/.local/share/united/sessions/whatsapp/`.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use anyhow::{Result, anyhow};
use async_trait::async_trait;
use base64::Engine as _;
use tokio::sync::watch;
use united_core::events::{AuthPrompt, CallState, CoreEvent, ServiceStatus};
use united_core::model::{AccountId, Chat, MediaRef, Message, MessageStatus, Service, now_secs};
use united_core::{Adapter, AuthInput, EventBus};
use whatsapp_rust::Jid;
use whatsapp_rust::prelude::*;
use whatsapp_rust::store::SqliteStore;
use whatsapp_rust::voip::{CallEvent, CallHandle};
use whatsapp_rust::wacore::history_sync::HistorySyncStream;
use whatsapp_rust::wacore::iq::contacts::{ProfilePictureSpec, ProfilePictureType};
use whatsapp_rust::wacore::types::call::{CallAction, IncomingCall};
use whatsapp_rust::wacore::types::presence::{ChatActivity, PresenceStatus, ReceiptType};
use whatsapp_rust::wacore_binary::JidExt as _;

mod voice;

type TargetMap = Arc<Mutex<HashMap<String, Jid>>>;

pub struct WhatsAppAdapter {
    bus: Mutex<Option<EventBus>>,
    client: Mutex<Option<Arc<Client>>>,
    handle: Mutex<Option<BotHandle>>,
    prompt: Arc<Mutex<Option<AuthPrompt>>>,
    targets: TargetMap,
    account: Arc<Mutex<String>>,
    label: Arc<Mutex<String>>,
    shutdown: watch::Sender<bool>,
    /// Chats whose profile picture was already requested (one shot each).
    avatars: Arc<Mutex<HashSet<String>>>,
    /// Groups whose subject was already resolved (one shot each).
    group_titles: Arc<Mutex<HashSet<String>>>,
    /// The phone unlinked this device; the next pairing must start clean.
    logged_out: Arc<std::sync::atomic::AtomicBool>,
    /// Ringing offers by call id, so Terima/Tolak can reach the protocol.
    incoming_calls: Arc<Mutex<HashMap<String, IncomingCall>>>,
    /// Live call handles by call id, for hangup and end detection.
    live_calls: Arc<Mutex<HashMap<String, Arc<CallHandle>>>>,
    /// Downloads are heavy (photos, clips); a burst of visible bubbles must
    /// not open a hundred sockets at once.
    downloads: Arc<tokio::sync::Semaphore>,
}

impl WhatsAppAdapter {
    pub fn new() -> Self {
        Self {
            bus: Mutex::new(None),
            client: Mutex::new(None),
            handle: Mutex::new(None),
            prompt: Arc::new(Mutex::new(None)),
            targets: Arc::new(Mutex::new(HashMap::new())),
            account: Arc::new(Mutex::new("whatsapp:me".into())),
            label: Arc::new(Mutex::new("WhatsApp".into())),
            shutdown: watch::channel(false).0,
            avatars: Arc::new(Mutex::new(HashSet::new())),
            group_titles: Arc::new(Mutex::new(HashSet::new())),
            logged_out: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            incoming_calls: Arc::new(Mutex::new(HashMap::new())),
            live_calls: Arc::new(Mutex::new(HashMap::new())),
            downloads: Arc::new(tokio::sync::Semaphore::new(4)),
        }
    }

    fn publish(&self, event: CoreEvent) {
        if let Some(bus) = self.bus.lock().unwrap().clone() {
            bus.publish(event);
        }
    }

    fn set_status(&self, status: ServiceStatus, detail: Option<String>) {
        self.publish(CoreEvent::Status {
            account: self.account(),
            service: Service::WhatsApp,
            status,
            detail,
        });
    }

    fn publish_call(&self, chat_id: &str, call_id: &str, state: CallState) {
        self.publish(CoreEvent::Call {
            account: self.account(),
            chat_id: chat_id.to_string(),
            call_id: call_id.to_string(),
            state,
        });
    }

    /// Keep a live handle and follow it until the media task ends, publishing
    /// Connected/Ended so the UI bar matches reality even when the peer hangs
    /// up first.
    fn track_call(&self, chat_id: String, handle: CallHandle) {
        let call_id = handle.call_id().to_string();
        let handle = Arc::new(handle);
        self.live_calls
            .lock()
            .unwrap()
            .insert(call_id.clone(), handle.clone());
        let bus = self.bus.lock().unwrap().clone();
        let account = self.account();
        let live = self.live_calls.clone();
        tokio::spawn(async move {
            let mut events = handle.take_events();
            if let Some(events) = events.as_mut() {
                while let Ok(event) = events.recv().await {
                    match event {
                        CallEvent::MediaConnected => {
                            if let Some(bus) = &bus {
                                bus.publish(CoreEvent::Call {
                                    account: account.clone(),
                                    chat_id: chat_id.clone(),
                                    call_id: call_id.clone(),
                                    state: CallState::Active,
                                });
                            }
                        }
                        CallEvent::Closed(_) => break,
                        _ => {}
                    }
                }
            }
            handle.wait_ended().await;
            live.lock().unwrap().remove(&call_id);
            if let Some(bus) = &bus {
                bus.publish(CoreEvent::Call {
                    account,
                    chat_id,
                    call_id,
                    state: CallState::Ended,
                });
            }
        });
    }

    /// Build (or rebuild) the pairing/runtime client. Called on launch and when
    /// the user asks for a fresh QR code after the previous refs expired.
    async fn spawn_bot(&self, bus: EventBus) -> Result<()> {
        if let Some(handle) = self.handle.lock().unwrap().take() {
            handle.abort();
        }
        let db = united_core::paths::service_dir("whatsapp").join("device.db");
        // Bound the attachment cache once per launch instead of letting a
        // year of clips grow to gigabytes.
        united_core::paths::prune_media(
            &united_core::paths::service_dir("whatsapp").join("media"),
            united_core::paths::MEDIA_BUDGET,
        );
        let db_url = db.to_string_lossy().into_owned();
        let store = SqliteStore::open(&db_url)
            .await
            .map_err(|e| anyhow!("whatsapp store: {e}"))?;

        let qr_bus = bus.clone();
        let connected_bus = bus.clone();
        let message_bus = bus.clone();
        let logout_bus = bus.clone();
        let logout_flag = self.logged_out.clone();
        let logout_prompt = self.prompt.clone();
        let expired_bus = bus.clone();
        let expired_prompt = self.prompt.clone();
        let avatars = self.avatars.clone();
        let group_titles = self.group_titles.clone();
        let targets = self.targets.clone();
        let account = self.account.clone();
        let history_bus = bus.clone();
        let history_account = account.clone();
        let history_targets = targets.clone();
        let history_channels = group_titles.clone();
        let contact_bus = bus.clone();
        let contact_account = account.clone();
        let receipt_bus = bus.clone();
        let receipt_account = account.clone();
        let receipt_targets = targets.clone();
        let typing_bus = bus.clone();
        let typing_account = account.clone();
        let typing_targets = targets.clone();
        let presence_bus = bus.clone();
        let presence_account = account.clone();
        let presence_targets = targets.clone();
        let call_bus = bus.clone();
        let call_account = account.clone();
        let call_targets = targets.clone();
        let call_incoming = self.incoming_calls.clone();
        let call_live = self.live_calls.clone();
        let connected_account = self.account.clone();
        let connected_label = self.label.clone();
        let prompt_clear = self.prompt.clone();

        let builder = Bot::builder()
            .with_backend(store)
            .on_qr_code(move |code, _timeout| {
                let bus = qr_bus.clone();
                async move {
                    bus.publish(CoreEvent::Auth {
                        service: Service::WhatsApp,
                        prompt: AuthPrompt::Qr { payload: code },
                    });
                    // The socket is up but the account is not linked yet.
                    bus.publish(CoreEvent::Status {
                        account: "whatsapp:me".into(),
                        service: Service::WhatsApp,
                        status: ServiceStatus::NeedsAuth,
                        detail: Some("scan the code with your phone".into()),
                    });
                }
            })
            .on_event(move |event, _client| {
                let bus = expired_bus.clone();
                let prompt = expired_prompt.clone();
                async move {
                    // Six QR refs are issued over ~2 minutes; when they run out the
                    // socket closes. Tell the shell so it can offer a fresh code
                    // instead of leaving a dead QR on screen.
                    if matches!(&*event, Event::PairingQrCodesExhausted(_)) {
                        let expired = AuthPrompt::Qr {
                            payload: String::new(),
                        };
                        *prompt.lock().unwrap() = Some(expired.clone());
                        bus.publish(CoreEvent::Auth {
                            service: Service::WhatsApp,
                            prompt: expired,
                        });
                        bus.publish(CoreEvent::Status {
                            account: "whatsapp:me".into(),
                            service: Service::WhatsApp,
                            status: ServiceStatus::NeedsAuth,
                            detail: Some("pairing code expired".into()),
                        });
                    }
                }
            })
            .on_connected(move |client| {
                let bus = connected_bus.clone();
                let account = connected_account.clone();
                let label = connected_label.clone();
                let prompt = prompt_clear.clone();
                async move {
                    let merge_client = client.clone();
                    let merge_bus = bus.clone();
                    let merge_account = account.clone();
                    tokio::spawn(async move {
                        let account = merge_account.lock().unwrap().clone();
                        reconcile_lid_chats(&merge_client, &account).await;
                        drop_self_chat(&merge_client, &account);
                        // Replaying the regular app-state collection turns the
                        // phone's address book into ContactUpdate events.
                        // Once per launch: the old "only if empty" guard kept
                        // masked/placeholder names forever.
                        static CONTACTS_SYNCED: std::sync::atomic::AtomicBool =
                            std::sync::atomic::AtomicBool::new(false);
                        if !CONTACTS_SYNCED.swap(true, std::sync::atomic::Ordering::Relaxed) {
                            match merge_client
                                .resync_app_state(
                                    [
                                        whatsapp_rust::WAPatchName::Regular,
                                        whatsapp_rust::WAPatchName::CriticalUnblockLow,
                                    ],
                                    whatsapp_rust::AppStateResyncMode::Snapshot,
                                )
                                .await
                            {
                                Ok(report) => {
                                    log::info!("whatsapp contact snapshot: {:?}", report.synced)
                                }
                                Err(err) => log::warn!("contact snapshot failed: {err}"),
                            }
                        }
                        refresh_group_titles(&merge_client, &merge_bus).await;
                        // Reload the list now that aliased rows are merged.
                        merge_bus.publish(CoreEvent::Status {
                            account,
                            service: Service::WhatsApp,
                            status: ServiceStatus::Online,
                            detail: None,
                        });
                    });
                    let jid = client.pn();
                    if let Some(jid) = jid {
                        // The account id stays `whatsapp:me` for the lifetime of
                        // the install: renaming it on every pairing orphaned all
                        // the chats and messages written before the rename. The
                        // phone number is display state, so it lives in `label`.
                        *label.lock().unwrap() = format!("+{}", jid.user);
                        // The profile rail shows the account photo. It is
                        // published under the pseudo-JID "me" so the shell does
                        // not need to know the phone-number JID.
                        let avatar_client = client.clone();
                        let avatar_bus = bus.clone();
                        tokio::spawn(async move {
                            match fetch_avatar(&avatar_client, &jid).await {
                                Ok(Some(path)) => avatar_bus.publish(CoreEvent::SenderAvatar {
                                    account: "whatsapp:me".into(),
                                    service: Service::WhatsApp,
                                    jid: "me".into(),
                                    path,
                                }),
                                Ok(None) => {}
                                Err(err) => log::debug!("own avatar failed: {err:#}"),
                            }
                        });
                    }
                    *prompt.lock().unwrap() = None;
                    bus.publish(CoreEvent::Status {
                        account: account.lock().unwrap().clone(),
                        service: Service::WhatsApp,
                        status: ServiceStatus::Online,
                        detail: Some("linked device".into()),
                    });
                }
            })
            .on_logged_out(move |_info| {
                let bus = logout_bus.clone();
                let logged_out = logout_flag.clone();
                let prompt = logout_prompt.clone();
                async move {
                    logged_out.store(true, std::sync::atomic::Ordering::SeqCst);
                    // Hand the shell an empty pairing prompt so "Show a new
                    // code" is one click away instead of a dead conversation.
                    let expired = AuthPrompt::Qr {
                        payload: String::new(),
                    };
                    *prompt.lock().unwrap() = Some(expired.clone());
                    bus.publish(CoreEvent::Auth {
                        service: Service::WhatsApp,
                        prompt: expired,
                    });
                    bus.publish(CoreEvent::Status {
                        account: "whatsapp:me".into(),
                        service: Service::WhatsApp,
                        status: ServiceStatus::SignedOut,
                        detail: Some("unlinked from phone".into()),
                    });
                }
            })
            .on_message(move |ctx| {
                let bus = message_bus.clone();
                let targets = targets.clone();
                let account = account.clone();
                let avatars = avatars.clone();
                let group_titles = group_titles.clone();
                async move {
                    let raw_jid = ctx.info.source.chat.clone();
                    // Status broadcasts are not conversations. Channels
                    // (newsletters) are, but live in their own tab.
                    if raw_jid.server.as_str() == "broadcast" {
                        return;
                    }
                    // WhatsApp addresses chats by LID on newer accounts while
                    // history sync uses the phone number. Collapse both onto
                    // the PN so one conversation never becomes two rows.
                    let chat_jid = canonical_jid(&ctx.client, raw_jid.clone()).await;
                    // "Message yourself" is not an inbox conversation; it only
                    // makes the app look like it is texting your own number.
                    if ctx.client.pn().map(|jid| jid.to_non_ad()).as_ref() == Some(&chat_jid)
                        || ctx.client.lid().map(|jid| jid.to_non_ad()).as_ref() == Some(&chat_jid)
                    {
                        return;
                    }
                    let chat_id = chat_jid.to_string();
                    targets
                        .lock()
                        .unwrap()
                        .insert(raw_jid.to_string(), raw_jid);
                    targets.lock().unwrap().insert(chat_id.clone(), chat_jid.clone());
                    let at = ctx.info.timestamp.timestamp_millis() / 1000;
                    let is_group = ctx.info.source.is_group;
                    let title = if is_group {
                        format!("Group {}", chat_jid.user)
                    } else {
                        ctx.info.push_name.to_string()
                    };
                    let title = if title.trim().is_empty() {
                        chat_jid.user.to_string()
                    } else {
                        title
                    };
                    // WhatsApp only knows a contact's address-book name if the
                    // phone has it saved; fall back to the profile push name
                    // instead of showing a bare number.
                    let title = if !is_group
                        && title == chat_jid.user.as_str()
                        && !ctx.info.push_name.is_empty()
                    {
                        ctx.info.push_name.to_string()
                    } else {
                        title
                    };
                    let media = media_ref(&ctx.message);
                    let body = message_body(&ctx.message, media.as_ref());
                    // Group rows show "Sender: text"; keep the author beside
                    // the preview so the list never looks it up per frame.
                    let preview_sender = if is_group {
                        if ctx.info.source.is_from_me {
                            "Anda".to_string()
                        } else {
                            ctx.info.push_name.to_string()
                        }
                    } else {
                        String::new()
                    };
                    let account_id = AccountId(account.lock().unwrap().clone());
                    // Device suffixes (`:12@s.whatsapp.net`) belong to one
                    // session, not to the person: collapse them so the sender
                    // avatar and colour stay stable.
                    let sender_jid = ctx.info.source.sender.to_non_ad();
                    let sender_id = sender_jid.to_string();
                    let outgoing = ctx.info.source.is_from_me;
                    // Reactions and protocol envelopes change an existing row
                    // instead of appending a message.
                    if let Some(reaction) = ctx.message.reaction_message.as_option() {
                        if let Some(key) = reaction.key.as_option()
                            && let Some(id) = key.id.as_deref().filter(|id| !id.is_empty())
                        {
                            bus.publish(CoreEvent::Reaction {
                                account: account_id.0.clone(),
                                chat_id: chat_id.clone(),
                                message_id: id.to_string(),
                                sender_id: sender_id.clone(),
                                emoji: reaction.text.clone().unwrap_or_default(),
                            });
                        }
                        return;
                    }
                    if let Some(protocol) = ctx.message.protocol_message.as_option() {
                        use whatsapp_rust::waproto::whatsapp::message::protocol_message::Type;
                        let target = protocol
                            .key
                            .as_option()
                            .and_then(|key| key.id.clone())
                            .filter(|id| !id.is_empty());
                        match protocol.r#type {
                            Some(Type::REVOKE) => {
                                if let Some(id) = target {
                                    bus.publish(CoreEvent::MessageRevoke {
                                        account: account_id.0.clone(),
                                        chat_id: chat_id.clone(),
                                        message_id: id,
                                    });
                                }
                                return;
                            }
                            Some(Type::MESSAGE_EDIT) => {
                                let body = protocol
                                    .edited_message
                                    .as_option()
                                    .map(|inner| message_body(inner, None))
                                    .unwrap_or_default();
                                if let Some(id) = target
                                    && !body.trim().is_empty()
                                {
                                    bus.publish(CoreEvent::MessageEdit {
                                        account: account_id.0.clone(),
                                        chat_id: chat_id.clone(),
                                        message_id: id,
                                        body,
                                    });
                                }
                                return;
                            }
                            _ => {}
                        }
                    }
                    let message = Message {
                        id: ctx.info.id.to_string(),
                        account: account_id.clone(),
                        chat_id: chat_id.clone(),
                        service: Service::WhatsApp,
                        sender_id: sender_id.clone(),
                        sender_name: ctx.info.push_name.to_string(),
                        body: body.clone(),
                        at,
                        outgoing,
                        status: MessageStatus::Sent,
                        media: media.as_ref().map(MediaRef::encode),
                        reply_to: ctx
                            .message
                            .context_info()
                            .and_then(|info| info.stanza_id.clone()),
                    };
                    // Reactions and protocol messages carry neither text nor an
                    // attachment: storing them would blank the chat preview.
                    if body.is_empty() && media.is_none() {
                        return;
                    }
                    // Offline-sync messages are history, not new activity: they
                    // must not raise unread counts or fire notifications.
                    bus.publish(CoreEvent::Message {
                        message,
                        live: !ctx.info.is_offline,
                    });
                    // Attachments are fetched after the row is on screen so a
                    // slow CDN never blocks incoming messages.
                    if let Some(pending) = media.clone() {
                        if pending.local().is_none() && pending.direct.is_some() {
                            let client = ctx.client.clone();
                            let bus = bus.clone();
                            let account = account_id.0.clone();
                            let chat_id = chat_id.clone();
                            let id = ctx.info.id.to_string();
                            tokio::spawn(async move {
                                if let Ok(done) = download_media(&client, &pending, &id).await {
                                    bus.publish(CoreEvent::MediaReady {
                                        account,
                                        chat_id,
                                        message_id: id,
                                        media: done.encode(),
                                    });
                                }
                            });
                        }
                    }
                    // Profile picture: the library exposes the IQ spec but no
                    // download helper, so fetch it here and keep the bytes on
                    // disk — the UI never talks to a remote host.
                    {
                        let client = ctx.client.clone();
                        let seen = avatars.clone();
                        let bus = bus.clone();
                        let account = account_id.clone();
                        let chat_id = chat_id.clone();
                        let jid = chat_jid.clone();
                        tokio::spawn(async move {
                            if !seen.lock().unwrap().insert(format!("chat:{chat_id}")) {
                                return;
                            }
                            if let Ok(Some(path)) = fetch_avatar(&client, &jid).await {
                                bus.publish(CoreEvent::Avatar {
                                    account: account.0,
                                    service: Service::WhatsApp,
                                    chat_id,
                                    path,
                                });
                            }
                        });
                    }
                    // A group row shows who spoke: fetch the author's own photo
                    // once, keyed by sender JID so it is shared across groups.
                    if is_group && !outgoing {
                        let client = ctx.client.clone();
                        let seen = avatars.clone();
                        let bus = bus.clone();
                        let account = account_id.0.clone();
                        let jid = sender_jid.clone();
                        tokio::spawn(async move {
                            let key = format!("sender:{}", jid);
                            if !seen.lock().unwrap().insert(key) {
                                return;
                            }
                            if let Ok(Some(path)) = fetch_avatar(&client, &jid).await {
                                bus.publish(CoreEvent::SenderAvatar {
                                    account,
                                    service: Service::WhatsApp,
                                    jid: jid.to_string(),
                                    path,
                                });
                            }
                        });
                    }
                    if is_group {
                        // Groups are titled by their subject, which history sync
                        // does not always carry; resolve it once per chat.
                        let client = ctx.client.clone();
                        let bus = bus.clone();
                        let account = account_id.clone();
                        let chat_id = chat_id.clone();
                        let jid = chat_jid.clone();
                        let seen = group_titles.clone();
                        let preview = body.clone();
                        let preview_sender = preview_sender.clone();
                        tokio::spawn(async move {
                            if !seen.lock().unwrap().insert(chat_id.clone()) {
                                return;
                            }
                            let Ok(meta) = client.groups().fetch_metadata(&jid).await else {
                                return;
                            };
                            let Some(subject) = meta.subject.filter(|s| !s.trim().is_empty())
                            else {
                                return;
                            };
                            bus.publish(CoreEvent::ChatUpsert { chat: Chat {
                                id: chat_id.clone(),
                                account,
                                service: Service::WhatsApp,
                                title: subject,
                                is_group: true,
                                avatar: None,
                                unread: 0,
                                muted: false,
                                last_at: at,
                                preview,
                                preview_sender,
                                target: chat_id,
                            }});
                        });
                    }
                    if chat_jid.server.as_str() == "newsletter" {
                        // A channel's own name and picture only come from its
                        // metadata; without this the row shows the numeric JID.
                        let client = ctx.client.clone();
                        let bus = bus.clone();
                        let account = account_id.clone();
                        let chat_id = chat_id.clone();
                        let jid = chat_jid.clone();
                        let seen = group_titles.clone();
                        let preview = body.clone();
                        let preview_sender = preview_sender.clone();
                        tokio::spawn(async move {
                            if !seen.lock().unwrap().insert(format!("newsletter:{chat_id}")) {
                                return;
                            }
                            let Some((title, avatar)) = fetch_newsletter(&client, &jid).await
                            else {
                                return;
                            };
                            bus.publish(CoreEvent::ChatUpsert { chat: Chat {
                                id: chat_id.clone(),
                                account,
                                service: Service::WhatsApp,
                                title,
                                is_group: false,
                                avatar,
                                unread: 0,
                                muted: false,
                                last_at: at,
                                preview,
                                preview_sender,
                                target: chat_id,
                            }});
                        });
                    }
                    bus.publish(CoreEvent::ChatUpsert { chat: Chat {
                        id: chat_id.clone(),
                        account: account_id,
                        service: Service::WhatsApp,
                        title,
                        is_group,
                        avatar: None,
                        unread: 0,
                        muted: false,
                        last_at: at,
                        preview: body.clone(),
                        preview_sender,
                        target: chat_id,
                    }});
                }
            })
            .on_event_for(&[EventKind::HistorySync], move |event, client| {
                let bus = history_bus.clone();
                let account = history_account.clone();
                let targets = history_targets.clone();
                let channels = history_channels.clone();
                async move {
                    let Event::HistorySync(chunk) = &*event else {
                        return;
                    };
                    let own = client.pn().map(|jid| jid.to_non_ad());
                    let own_lid = client.lid().map(|jid| jid.to_non_ad());
                    let account_id = account.lock().unwrap().clone();
                    // ponytail: the inflate runs inline (tens of ms per chunk,
                    // this handler owns its own task). Move to spawn_blocking if
                    // history sync ever competes with live traffic.
                    let mut stream = HistorySyncStream::new(
                        chunk.compressed_bytes(),
                        chunk.decompressed_size() as u64,
                    );
                    let sync_type = chunk.sync_type();
                    let mut conversations = 0usize;
                    let mut stored = 0usize;
                    loop {
                        let conversation = match stream.next_conversation() {
                            Ok(Some(conversation)) => conversation,
                            Ok(None) => break,
                            Err(err) => {
                                log::warn!("whatsapp history chunk stopped: {err}");
                                break;
                            }
                        };
                        let Some((chat, messages)) = history_chat(&conversation, &account_id)
                        else {
                            continue;
                        };
                        let own_id = own.as_ref().map(|jid| jid.to_string());
                        let own_lid_id = own_lid.as_ref().map(|jid| jid.to_string());
                        if own_id.as_deref() == Some(chat.id.as_str())
                            || own_lid_id.as_deref() == Some(chat.id.as_str())
                        {
                            continue;
                        }
                        conversations += 1;
                        stored += messages.len();
                        if let Ok(jid) = chat.target.parse::<Jid>() {
                            targets.lock().unwrap().insert(chat.id.clone(), jid);
                        }
                        bus.publish(CoreEvent::ChatUpsert { chat });
                        for message in messages {
                            // live=false: history must not raise unread counts
                            // or fire notifications.
                            bus.publish(CoreEvent::Message {
                                message,
                                live: false,
                            });
                        }
                    }
                    log::info!(
                        "whatsapp history sync type={sync_type} conversations={conversations} messages={stored}"
                    );
                    refresh_channels(&client, &bus, &channels).await;
                }
            })
            // Names come from the phone's address book, or from the name the
            // sender chose. Without them a group shows ids and mention tokens
            // cannot be turned back into a person.
            .on_event_for(&[EventKind::ContactUpdate], move |event, _client| {
                let bus = contact_bus.clone();
                let account = contact_account.clone();
                async move {
                    let Event::ContactUpdate(update) = &*event else {
                        return;
                    };
                    let action = &update.action;
                    let name = action
                        .full_name
                        .as_deref()
                        .map(str::trim)
                        .filter(|name| !name.is_empty())
                        .or_else(|| {
                            action
                                .first_name
                                .as_deref()
                                .map(str::trim)
                                .filter(|name| !name.is_empty())
                        })
                        .map(str::to_string);
                    let Some(name) = name else {
                        return;
                    };
                    let mut rows = Vec::new();
                    let mut push = |jid: &str| {
                        let user = jid.split('@').next().unwrap_or(jid);
                        if !user.is_empty() && !rows.iter().any(|(id, _)| id == user) {
                            rows.push((user.to_string(), name.clone()));
                        }
                    };
                    push(&update.jid.to_non_ad().to_string());
                    for alias in [action.lid_jid.as_deref(), action.pn_jid.as_deref()]
                        .into_iter()
                        .flatten()
                    {
                        if !alias.is_empty() {
                            push(alias);
                        }
                    }
                    if rows.is_empty() {
                        return;
                    }
                    bus.publish(CoreEvent::Contacts {
                        account: account.lock().unwrap().clone(),
                        rows,
                    });
                }
            })
            // The recipient's device reported that our message arrived or was
            // read. One receipt can cover a batch, and its chat may be named by
            // the LID alias, so the stored key is looked up first.
            .on_event_for(&[EventKind::Receipt], move |event, _client| {
                let bus = receipt_bus.clone();
                let account = receipt_account.clone();
                let targets = receipt_targets.clone();
                async move {
                    let Event::Receipt(receipt) = &*event else {
                        return;
                    };
                    let status = match receipt.r#type {
                        ReceiptType::Read | ReceiptType::Played | ReceiptType::PlayedSelf => {
                            MessageStatus::Read
                        }
                        ReceiptType::Delivered | ReceiptType::Inactive => {
                            MessageStatus::Delivered
                        }
                        ReceiptType::ServerError => MessageStatus::Failed,
                        _ => return,
                    };
                    let ids: Vec<String> = receipt
                        .message_ids
                        .iter()
                        .map(|id| id.as_str().to_string())
                        .collect();
                    if ids.is_empty() {
                        return;
                    }
                    bus.publish(CoreEvent::Receipt {
                        account: account.lock().unwrap().clone(),
                        chat_id: chat_key(&targets, &receipt.source.chat),
                        ids,
                        status,
                    });
                }
            })
            // Typing and recording states, so the header can say what the other
            // side is doing. WhatsApp never reports our own state back to us.
            .on_event_for(&[EventKind::ChatPresence], move |event, _client| {
                let bus = typing_bus.clone();
                let account = typing_account.clone();
                let targets = typing_targets.clone();
                async move {
                    let Event::ChatPresence(update) = &*event else {
                        return;
                    };
                    if update.source.is_from_me {
                        return;
                    }
                    let (active, recording) = match update.state {
                        ChatActivity::Typing => (true, false),
                        ChatActivity::RecordingAudio => (true, true),
                        _ => (false, false),
                    };
                    bus.publish(CoreEvent::Typing {
                        account: account.lock().unwrap().clone(),
                        chat_id: chat_key(&targets, &update.source.chat),
                        who: update.source.sender.to_non_ad().to_string(),
                        recording,
                        active,
                    });
                }
            })
            // Availability of the person we are talking to, once we subscribed.
            .on_event_for(&[EventKind::Presence], move |event, _client| {
                let bus = presence_bus.clone();
                let account = presence_account.clone();
                let targets = presence_targets.clone();
                async move {
                    let Event::Presence(update) = &*event else {
                        return;
                    };
                    bus.publish(CoreEvent::Presence {
                        account: account.lock().unwrap().clone(),
                        chat_id: chat_key(&targets, &update.from),
                        online: matches!(update.status, PresenceStatus::Available),
                        last_seen: update.last_seen.map(|at| at.timestamp()),
                    });
                }
            })
            // Voice call signaling: ring the UI on an offer, clear it when the
            // peer hangs up or another of our devices answers.
            .on_event_for(&[EventKind::IncomingCall], move |event, _client| {
                let bus = call_bus.clone();
                let account = call_account.clone();
                let targets = call_targets.clone();
                let incoming = call_incoming.clone();
                let live = call_live.clone();
                async move {
                    let Event::IncomingCall(call) = &*event else {
                        return;
                    };
                    let call_id = call.action.call_id().to_string();
                    let chat_id = chat_key(&targets, &call.from);
                    let state = match &call.action {
                        CallAction::Offer { .. } => {
                            incoming.lock().unwrap().insert(call_id.clone(), (**call).clone());
                            CallState::Incoming
                        }
                        CallAction::Terminate { .. } | CallAction::Reject { .. } => {
                            incoming.lock().unwrap().remove(&call_id);
                            live.lock().unwrap().remove(&call_id);
                            CallState::Ended
                        }
                        _ => return,
                    };
                    bus.publish(CoreEvent::Call {
                        account: account.lock().unwrap().clone(),
                        chat_id,
                        call_id,
                        state,
                    });
                }
            });

        let bot = builder
            .build()
            .await
            .map_err(|e| anyhow!("whatsapp build: {e}"))?;
        let handle = bot.spawn();
        *self.client.lock().unwrap() = Some(handle.client());
        *self.handle.lock().unwrap() = Some(handle);
        self.set_status(ServiceStatus::Connecting, None);
        Ok(())
    }
}

impl Default for WhatsAppAdapter {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Adapter for WhatsAppAdapter {
    fn service(&self) -> Service {
        Service::WhatsApp
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
        // The empty payload means "no live code yet"; the sheet then offers
        // "Show a new code", which starts a fresh pairing socket.
        Some(AuthPrompt::Qr {
            payload: String::new(),
        })
    }

    async fn start(&self, bus: EventBus) -> Result<()> {
        *self.bus.lock().unwrap() = Some(bus.clone());
        self.spawn_bot(bus).await
    }

    async fn submit(&self, input: AuthInput) -> Result<()> {
        match input {
            // WhatsApp rotates six QR refs in about two minutes, then closes
            // the socket. "Show a new code" reopens it without a restart.
            AuthInput::Refresh => {
                let bus = self
                    .bus
                    .lock()
                    .unwrap()
                    .clone()
                    .ok_or_else(|| anyhow!("whatsapp adapter not started"))?;
                *self.prompt.lock().unwrap() = None;
                // Stop the old socket before deleting its SQLite session, then
                // start from an empty device: the phone treats this as a new
                // link and replays its recent history.
                if let Some(handle) = self.handle.lock().unwrap().take() {
                    handle.abort();
                }
                self.logged_out
                    .store(false, std::sync::atomic::Ordering::SeqCst);
                reset_session();
                self.spawn_bot(bus).await
            }
            _ => Ok(()),
        }
    }

    /// Legacy group ids (`<creator>-<timestamp>@g.us`) arrive through history
    /// sync without a subject, so the list keeps the `Group 62812…` fallback
    /// until the server is asked.
    async fn chat_title(&self, chat_id: &str) -> Result<Option<String>> {
        let Some(client) = self.client.lock().unwrap().clone() else {
            return Ok(None);
        };
        let jid = match self.targets.lock().unwrap().get(chat_id).cloned() {
            Some(jid) => jid,
            None => match chat_id.parse::<Jid>() {
                Ok(jid) => jid,
                Err(_) => return Ok(None),
            },
        };
        if !jid.is_group() {
            return Ok(None);
        }
        let meta = client.groups().fetch_metadata(&jid).await?;
        Ok(meta.subject.filter(|s| !s.trim().is_empty()))
    }

    /// Fetch the chat's own picture. Only fetched lazily for messages and
    /// history senders before, so contacts that never spoke were left with
    /// initials.
    async fn load_avatar(&self, chat_id: &str) -> Result<()> {
        let Some(client) = self.client.lock().unwrap().clone() else {
            return Ok(());
        };
        let jid = match self.targets.lock().unwrap().get(chat_id).cloned() {
            Some(jid) => jid,
            None => match chat_id.parse::<Jid>() {
                Ok(jid) => jid,
                Err(_) => return Ok(()),
            },
        };
        // Same keyspace as the live-message path, so a chat that already has
        // a request in flight is not asked twice.
        if !self
            .avatars
            .lock()
            .unwrap()
            .insert(format!("chat:{chat_id}"))
        {
            return Ok(());
        }
        match fetch_avatar(&client, &jid).await {
            Ok(Some(path)) => self.publish(CoreEvent::Avatar {
                account: self.account(),
                service: Service::WhatsApp,
                chat_id: chat_id.to_string(),
                path,
            }),
            Ok(None) => log::debug!("whatsapp: no profile picture for {chat_id}"),
            // Retry on the next open: the picture IQ often fails while the
            // socket is still warming up after a reconnect.
            Err(err) => {
                self.avatars
                    .lock()
                    .unwrap()
                    .remove(&format!("chat:{chat_id}"));
                log::debug!("whatsapp: avatar for {chat_id} failed: {err:#}");
            }
        }
        Ok(())
    }

    /// Page backwards through the conversation on the primary phone. WhatsApp
    /// only pushes history sync once per linked device, so older messages have
    /// to be requested on demand.
    async fn load_history(&self, chat_id: &str, limit: usize) -> Result<()> {
        let client = self
            .client
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| anyhow!("whatsapp not connected"))?;
        let jid = match self.targets.lock().unwrap().get(chat_id).cloned() {
            Some(jid) => jid,
            None => chat_id
                .parse::<Jid>()
                .map_err(|_| anyhow!("unknown WhatsApp chat {chat_id}"))?,
        };
        let anchor = united_core::Store::shared()
            .ok()
            .and_then(|store| {
                store
                    .oldest_message(&self.account(), chat_id)
                    .ok()
                    .flatten()
            })
            .unwrap_or_else(|| (String::new(), false, now_secs()));
        let (oldest_id, oldest_outgoing, oldest_at) = anchor;
        client
            .fetch_message_history(
                &jid,
                &oldest_id,
                oldest_outgoing,
                oldest_at.saturating_mul(1000),
                limit.min(100) as i32,
            )
            .await
            .map_err(|e| anyhow!("history request: {e}"))?;
        // Group rows show the author's own picture. Ask for a bounded set of the
        // senders this chat revealed, once per process and build.
        if let Some(bus) = self.bus.lock().unwrap().clone() {
            let senders = united_core::Store::shared()
                .ok()
                .and_then(|store| store.sender_ids(&self.account(), chat_id, 12).ok())
                .unwrap_or_default();
            for raw in senders {
                let Ok(sender) = raw.parse::<Jid>() else {
                    continue;
                };
                let sender = sender.to_non_ad();
                let key = format!("sender:{sender}");
                if !self.avatars.lock().unwrap().insert(key) {
                    continue;
                }
                let client = client.clone();
                let bus = bus.clone();
                let account = self.account();
                tokio::spawn(async move {
                    if let Ok(Some(path)) = fetch_avatar(&client, &sender).await {
                        bus.publish(CoreEvent::SenderAvatar {
                            account,
                            service: Service::WhatsApp,
                            jid: sender.to_string(),
                            path,
                        });
                    }
                });
            }
        }
        Ok(())
    }

    async fn send_text(
        &self,
        chat_id: &str,
        text: &str,
        reply_to: Option<&Message>,
    ) -> Result<Message> {
        let client = self
            .client
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| anyhow!("whatsapp not connected"))?;
        let jid = self
            .targets
            .lock()
            .unwrap()
            .get(chat_id)
            .cloned()
            .ok_or_else(|| anyhow!("unknown WhatsApp chat"))?;
        // @<digits> tokens become real mentions: WhatsApp Web says who was
        // mentioned in `contextInfo.mentionedJid`, alongside the text token.
        let mentions = mention_tokens(text);
        // A reply carries the quoted message in `contextInfo`, exactly like
        // WhatsApp Web. The sender is the quote's author, not the chat.
        let mut context: Option<whatsapp_rust::waproto::whatsapp::ContextInfo> = None;
        if let Some(quoted) = reply_to {
            use whatsapp_rust::wacore::proto_helpers::build_quote_context;
            use whatsapp_rust::waproto::whatsapp as wa;
            let sender = if quoted.outgoing {
                client
                    .pn()
                    .map(|j| j.to_non_ad().to_string())
                    .unwrap_or_else(|| jid.to_non_ad().to_string())
            } else if quoted.sender_id.is_empty() {
                jid.to_non_ad().to_string()
            } else {
                quoted.sender_id.clone()
            };
            let text = if quoted.body.trim().is_empty() {
                "Pesan"
            } else {
                quoted.body.as_str()
            };
            context = Some(build_quote_context(
                quoted.id.clone(),
                sender,
                &wa::Message::text(text.to_string()),
            ));
        }
        if !mentions.is_empty() && jid.to_string().ends_with("@g.us") {
            if let Ok(mut meta) = client.groups().fetch_metadata(&jid).await {
                client
                    .groups()
                    .resolve_participant_addresses(&mut meta)
                    .await;
                let mut jids = Vec::new();
                for token in &mentions {
                    for member in &meta.participants {
                        let phone = member.phone_number.as_ref().map(|j| j.user.to_string());
                        if phone.as_deref() == Some(token.as_str())
                            || member.jid.user.to_string() == *token
                        {
                            jids.push(member.jid.to_string());
                            break;
                        }
                    }
                }
                if !jids.is_empty() {
                    let mut info = context.take().unwrap_or_default();
                    info.mentioned_jid = jids;
                    context = Some(info);
                }
            }
        }
        let payload = match context {
            Some(info) => {
                use whatsapp_rust::wacore::proto_helpers::MessageBuilderExt;
                use whatsapp_rust::waproto::whatsapp as wa;
                wa::Message::text_with_context(text.to_string(), info)
            }
            None => {
                use whatsapp_rust::wacore::proto_helpers::MessageBuilderExt;
                use whatsapp_rust::waproto::whatsapp as wa;
                wa::Message::text(text.to_string())
            }
        };
        let result = client
            .send_message(jid, payload)
            .await
            .map_err(|e| anyhow!("whatsapp send: {e}"))?;
        let message = Message {
            id: result.message_id.to_string(),
            account: AccountId(self.account()),
            chat_id: chat_id.to_string(),
            service: Service::WhatsApp,
            sender_id: String::new(),
            sender_name: self.label(),
            body: text.to_string(),
            at: now_secs(),
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
        let Some(client) = self.client.lock().unwrap().clone() else {
            return Ok(None);
        };
        let _slot = self.downloads.acquire().await?;
        let done = match fetch_media(&client, message, &media).await {
            Some(done) => done,
            None => return Ok(None),
        };
        if done.path.is_some() {
            self.publish(CoreEvent::MediaReady {
                account: message.account.0.clone(),
                chat_id: message.chat_id.clone(),
                message_id: message.id.clone(),
                media: done.encode(),
            });
            Ok(Some(done))
        } else {
            Ok(None)
        }
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
            .ok_or_else(|| anyhow!("whatsapp not connected"))?;
        let jid = self
            .targets
            .lock()
            .unwrap()
            .get(chat_id)
            .cloned()
            .ok_or_else(|| anyhow!("unknown WhatsApp chat"))?;
        let data = std::fs::read(path).map_err(|e| anyhow!("read attachment: {e}"))?;
        // ponytail: 64 MB ceiling, enough for photos, short clips and PDFs.
        // Raise it when the adapter learns resumable up/downloads.
        if data.len() > 64 * 1024 * 1024 {
            return Err(anyhow!("attachment is larger than 64 MB"));
        }
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("file")
            .to_string();
        let mime = guess_mime(path);
        let kind = if mime.starts_with("image/") {
            "image"
        } else if mime.starts_with("video/") {
            "video"
        } else {
            "document"
        };
        let upload = client
            .upload(data, media_type(kind), whatsapp_rust::UploadOptions::new())
            .await
            .map_err(|e| anyhow!("whatsapp upload: {e:#}"))?;
        let caption = caption.trim();
        let caption = (!caption.is_empty()).then(|| caption.to_string());
        let proto = match kind {
            "image" => {
                let mut options = whatsapp_rust::media::ImageOptions::default().with_mimetype(mime);
                if let Some(caption) = caption.clone() {
                    options = options.with_caption(caption);
                }
                whatsapp_rust::media::image_message(upload, options)
            }
            "video" => {
                let mut options = whatsapp_rust::media::VideoOptions::default().with_mimetype(mime);
                if let Some(caption) = caption.clone() {
                    options = options.with_caption(caption);
                }
                whatsapp_rust::media::video_message(upload, options)
            }
            _ => {
                let mut options = whatsapp_rust::media::DocumentOptions::default()
                    .with_mimetype(mime)
                    .with_file_name(name.clone());
                if let Some(caption) = caption.clone() {
                    options = options.with_caption(caption);
                }
                whatsapp_rust::media::document_message(upload, options)
            }
        };
        let result = client
            .send_message(jid, proto)
            .await
            .map_err(|e| anyhow!("whatsapp send file: {e}"))?;
        let local = MediaRef {
            kind: kind.into(),
            path: Some(path.display().to_string()),
            name: Some(name),
            mime: Some(mime.to_string()),
            ..MediaRef::default()
        };
        let message = Message {
            id: result.message_id.to_string(),
            account: AccountId(self.account()),
            chat_id: chat_id.to_string(),
            service: Service::WhatsApp,
            sender_id: String::new(),
            sender_name: self.label(),
            body: caption.clone().unwrap_or_else(|| local.preview()),
            at: now_secs(),
            outgoing: true,
            status: MessageStatus::Sent,
            media: Some(local.encode()),
            reply_to: None,
        };
        self.publish(CoreEvent::Message {
            message: message.clone(),
            live: true,
        });
        Ok(Some(message))
    }

    async fn members(&self, chat_id: &str) -> Result<Vec<(String, String)>> {
        let client = self
            .client
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| anyhow!("whatsapp not connected"))?;
        let jid = match self.targets.lock().unwrap().get(chat_id).cloned() {
            Some(jid) => jid,
            None => chat_id
                .parse::<Jid>()
                .map_err(|_| anyhow!("unknown WhatsApp chat {chat_id}"))?,
        };
        let meta = client.groups().fetch_metadata(&jid).await?;
        // The picker offers other people, not yourself.
        let own = [
            client.pn().map(|j| j.to_non_ad()),
            client.lid().map(|j| j.to_non_ad()),
        ];
        let is_self = |jid: &Jid| own.iter().flatten().any(|me| me == &jid.to_non_ad());
        // Use the participant's own address, lid or phone. WhatsApp matches a
        // body token against the `mentionedJid` it is sent with, so the exact
        // id in the group is what both the token and the JID must name.
        Ok(meta
            .participants
            .into_iter()
            .filter(|member| !is_self(&member.jid))
            .map(|member| {
                let name = member
                    .details
                    .as_ref()
                    .and_then(|d| d.display_name.as_ref().map(|n| n.to_string()))
                    .or_else(|| member.username.as_ref().map(|u| u.to_string()))
                    .or_else(|| member.phone_number.as_ref().map(|j| j.user.to_string()))
                    .filter(|name| !name.trim().is_empty())
                    .unwrap_or_else(|| member.jid.user.to_string());
                (member.jid.user.to_string(), name)
            })
            .collect())
    }

    async fn logout(&self) -> Result<()> {
        if let Some(handle) = self.handle.lock().unwrap().take() {
            handle.abort();
        }
        let client = self.client.lock().unwrap().take();
        if let Some(client) = client {
            // Best effort: asks the phone to drop this companion. Local files
            // are deleted either way so a fresh QR can pair again.
            let _ = client.logout().await;
        }
        let dir = united_core::paths::service_dir("whatsapp");
        for name in ["device.db", "device.db-shm", "device.db-wal"] {
            let _ = std::fs::remove_file(dir.join(name));
        }
        self.logged_out
            .store(true, std::sync::atomic::Ordering::SeqCst);
        self.set_status(ServiceStatus::SignedOut, Some("unlinked from phone".into()));
        Ok(())
    }

    /// Send read receipts for the recent incoming messages of a chat, the way
    /// WhatsApp Web turns "I opened this" into blue ticks on the other phone.
    async fn mark_read(&self, chat_id: &str) -> Result<()> {
        let client = match self.client.lock().unwrap().clone() {
            Some(client) => client,
            None => return Ok(()),
        };
        let jid = match self.targets.lock().unwrap().get(chat_id).cloned() {
            Some(jid) => jid,
            None => match chat_id.parse::<Jid>() {
                Ok(jid) => jid,
                Err(_) => return Ok(()),
            },
        };
        let rows = united_core::Store::shared()
            .ok()
            .and_then(|store| store.recent_incoming(&self.account(), chat_id, 50).ok())
            .unwrap_or_default();
        // A group receipt names the message's author; a direct chat names the
        // chat itself, which the protocol wants as `None`.
        let mut by_sender: HashMap<String, Vec<String>> = HashMap::new();
        for (id, sender) in rows {
            let key = if sender == chat_id || sender.is_empty() {
                String::new()
            } else {
                sender
            };
            by_sender.entry(key).or_default().push(id);
        }
        for (sender, ids) in by_sender {
            let sender_jid = sender.parse::<Jid>().ok();
            let ids: Vec<&str> = ids.iter().map(String::as_str).collect();
            if let Err(err) = client.mark_as_read(&jid, sender_jid.as_ref(), &ids).await {
                log::debug!("read receipt failed for {chat_id}: {err}");
            }
        }
        Ok(())
    }

    async fn typing(&self, chat_id: &str, state: bool) -> Result<()> {
        let client = match self.client.lock().unwrap().clone() {
            Some(client) => client,
            None => return Ok(()),
        };
        let jid = match self.targets.lock().unwrap().get(chat_id).cloned() {
            Some(jid) => jid,
            None => return Ok(()),
        };
        let result = if state {
            client.chatstate().send_composing(&jid).await
        } else {
            client.chatstate().send_paused(&jid).await
        };
        if let Err(err) = result {
            log::debug!("chat state failed for {chat_id}: {err}");
        }
        Ok(())
    }

    async fn place_call(&self, chat_id: &str) -> Result<()> {
        let Some(client) = self.client.lock().unwrap().clone() else {
            return Err(anyhow!("WhatsApp is not connected"));
        };
        let Some(jid) = self.targets.lock().unwrap().get(chat_id).cloned() else {
            return Err(anyhow!("unknown chat"));
        };
        let (mic, speaker) = voice::open()?;
        match client.voip().call(&jid).audio(mic, speaker).start().await {
            Ok(handle) => {
                let call_id = handle.call_id().to_string();
                self.publish_call(chat_id, &call_id, CallState::Ringing);
                self.track_call(chat_id.to_string(), handle);
                Ok(())
            }
            Err(err) => {
                self.publish_call(chat_id, "", CallState::Ended);
                Err(anyhow!("call: {err}"))
            }
        }
    }

    async fn answer_call(&self, call_id: &str) -> Result<()> {
        let Some(call) = self.incoming_calls.lock().unwrap().remove(call_id) else {
            return Ok(());
        };
        let chat_id = chat_key(&self.targets, &call.from);
        let Some(client) = self.client.lock().unwrap().clone() else {
            self.publish_call(&chat_id, call_id, CallState::Ended);
            return Ok(());
        };
        self.publish_call(&chat_id, call_id, CallState::Connecting);
        let (mic, speaker) = match voice::open() {
            Ok(audio) => audio,
            Err(err) => {
                self.publish_call(&chat_id, call_id, CallState::Ended);
                return Err(err);
            }
        };
        match client
            .voip()
            .accept(&call)
            .audio(mic, speaker)
            .start()
            .await
        {
            Ok(handle) => {
                self.track_call(chat_id, handle);
                Ok(())
            }
            Err(err) => {
                self.publish_call(&chat_id, call_id, CallState::Ended);
                Err(anyhow!("answer: {err}"))
            }
        }
    }

    async fn reject_call(&self, call_id: &str) -> Result<()> {
        let Some(call) = self.incoming_calls.lock().unwrap().remove(call_id) else {
            return Ok(());
        };
        let chat_id = chat_key(&self.targets, &call.from);
        let client = self.client.lock().unwrap().clone();
        if let Some(client) = client {
            let _ = client.voip().reject(&call).await;
        }
        self.publish_call(&chat_id, call_id, CallState::Ended);
        Ok(())
    }

    async fn end_call(&self, call_id: &str) -> Result<()> {
        let handle = self.live_calls.lock().unwrap().remove(call_id);
        if let Some(handle) = handle {
            let _ = handle.terminate().await;
        }
        Ok(())
    }

    async fn react(&self, chat_id: &str, message: &Message, emoji: &str) -> Result<()> {
        let client = self
            .client
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| anyhow!("whatsapp not connected"))?;
        let jid = self
            .targets
            .lock()
            .unwrap()
            .get(chat_id)
            .cloned()
            .ok_or_else(|| anyhow!("unknown WhatsApp chat"))?;
        use whatsapp_rust::waproto::whatsapp as wa;
        let mut key = wa::MessageKey::default();
        key.remote_jid = Some(jid.to_string());
        key.from_me = Some(message.outgoing);
        key.id = Some(message.id.clone());
        // Groups scope a reaction to the original author; a DM has one.
        key.participant = (jid.is_group() && !message.outgoing && !message.sender_id.is_empty())
            .then(|| message.sender_id.clone());
        client
            .send_reaction_raw(jid, key, emoji)
            .await
            .map_err(|e| anyhow!("whatsapp reaction: {e}"))?;
        let me = client
            .pn()
            .map(|jid| jid.to_non_ad().to_string())
            .unwrap_or_default();
        self.publish(CoreEvent::Reaction {
            account: self.account(),
            chat_id: chat_id.to_string(),
            message_id: message.id.clone(),
            sender_id: me,
            emoji: emoji.to_string(),
        });
        Ok(())
    }

    async fn edit_text(&self, chat_id: &str, message: &Message, body: &str) -> Result<()> {
        let client = self
            .client
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| anyhow!("whatsapp not connected"))?;
        let jid = self
            .targets
            .lock()
            .unwrap()
            .get(chat_id)
            .cloned()
            .ok_or_else(|| anyhow!("unknown WhatsApp chat"))?;
        use whatsapp_rust::waproto::whatsapp as wa;
        let mut content = wa::Message::default();
        content.conversation = Some(body.to_string());
        client
            .edit_message_raw(
                jid,
                message.id.clone(),
                content,
                whatsapp_rust::EditOptions::default(),
            )
            .await
            .map_err(|e| anyhow!("whatsapp edit: {e}"))?;
        self.publish(CoreEvent::MessageEdit {
            account: self.account(),
            chat_id: chat_id.to_string(),
            message_id: message.id.clone(),
            body: body.to_string(),
        });
        Ok(())
    }

    async fn revoke_message(&self, chat_id: &str, message: &Message) -> Result<()> {
        let client = self
            .client
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| anyhow!("whatsapp not connected"))?;
        let jid = self
            .targets
            .lock()
            .unwrap()
            .get(chat_id)
            .cloned()
            .ok_or_else(|| anyhow!("unknown WhatsApp chat"))?;
        client
            .revoke_message_raw(jid, message.id.clone(), whatsapp_rust::RevokeType::Sender)
            .await
            .map_err(|e| anyhow!("whatsapp revoke: {e}"))?;
        self.publish(CoreEvent::MessageRevoke {
            account: self.account(),
            chat_id: chat_id.to_string(),
            message_id: message.id.clone(),
        });
        Ok(())
    }

    async fn watch_presence(&self, chat_id: &str) -> Result<()> {
        let Some(client) = self.client.lock().unwrap().clone() else {
            return Ok(());
        };
        let jid = match self.targets.lock().unwrap().get(chat_id).cloned() {
            Some(jid) => jid,
            None => return Ok(()),
        };
        // Groups have no single availability to follow.
        if jid.server.as_str() == "g.us" {
            return Ok(());
        }
        if let Err(err) = client.presence().subscribe(&jid).await {
            log::debug!("presence subscribe failed for {chat_id}: {err}");
        }
        Ok(())
    }

    async fn shutdown(&self) {
        let _ = self.shutdown.send(true);
        if let Some(handle) = self.handle.lock().unwrap().take() {
            handle.abort();
        }
    }
}

/// Our stored chat id for a JID. Receipts, typing states and presence updates
/// may arrive under the LID alias while the conversation is filed under the
/// phone number, so an already-known target is preferred.
fn chat_key(targets: &TargetMap, jid: &Jid) -> String {
    let plain = jid.to_non_ad().to_string();
    let map = targets.lock().unwrap();
    map.iter()
        .find(|(_, known)| known.to_non_ad().to_string() == plain)
        .map(|(key, _)| key.clone())
        .unwrap_or(plain)
}

/// Collapse a WhatsApp device suffix before it reaches the database. Two
/// sessions of one phone must not look like two different people.
fn normalize_jid(raw: &str) -> String {
    raw.parse::<Jid>()
        .map(|jid| jid.to_non_ad().to_string())
        .unwrap_or_else(|_| raw.to_string())
}

/// Turn one history-sync conversation into a chat plus its most recent
/// messages. Returns `None` for conversations this shell cannot render
/// (status broadcasts). Channels are kept: they have their own tab.
fn history_chat(
    conversation: &whatsapp_rust::waproto::whatsapp::Conversation,
    account: &str,
) -> Option<(Chat, Vec<Message>)> {
    let mut target = conversation.id.clone();
    // Some accounts are addressed by LID; history carries the phone-number
    // alias beside it. Filing both under the PN keeps one conversation.
    if target.ends_with("@lid")
        && let Some(pn) = conversation.pn_jid.as_deref().filter(|pn| !pn.is_empty())
    {
        target = pn.to_string();
    }
    if target.is_empty() || target.ends_with("@broadcast") {
        return None;
    }
    let is_group = target.ends_with("@g.us");
    let user = target.split('@').next().unwrap_or(&target).to_string();
    let title = conversation
        .display_name
        .clone()
        .or_else(|| conversation.name.clone())
        .filter(|name| !name.trim().is_empty())
        // Push names in the payload are the next best label for a direct chat
        // the phone has not saved as a contact.
        .or_else(|| {
            (!is_group)
                .then(|| {
                    conversation
                        .messages
                        .iter()
                        .rev()
                        .find_map(|entry| entry.message.as_option()?.push_name.clone())
                })
                .flatten()
                .filter(|name| !name.trim().is_empty())
        })
        .unwrap_or_else(|| {
            if is_group {
                format!("Group {user}")
            } else {
                user.clone()
            }
        });

    let mut messages: Vec<Message> = conversation
        .messages
        .iter()
        .filter_map(|entry| {
            let info = entry.message.as_option()?;
            let key = info.key.as_option()?;
            let content = info.message.as_option()?;
            let media = media_ref(content);
            let body = content
                .text_content()
                .map(str::to_string)
                .filter(|text| !text.is_empty())
                .or_else(|| media.as_ref().map(MediaRef::preview))
                .unwrap_or_default();
            if body.is_empty() && media.is_none() {
                return None;
            }
            let outgoing = key.from_me.unwrap_or(false);
            let remote = key.remote_jid.clone().unwrap_or_else(|| target.clone());
            // Group history keeps the author on the record, not always on the
            // key; without it every row would be filed as written by the group
            // itself and the chat shows no sender names or mentions.
            let author = key
                .participant
                .clone()
                .filter(|p| !p.is_empty())
                .or_else(|| info.participant.clone().filter(|p| !p.is_empty()))
                .unwrap_or_else(|| {
                    if outgoing {
                        account.to_string()
                    } else {
                        remote
                    }
                });
            Some(Message {
                id: key.id.clone()?,
                account: AccountId(account.to_string()),
                chat_id: target.clone(),
                service: Service::WhatsApp,
                sender_id: normalize_jid(&author),
                sender_name: info.push_name.clone().unwrap_or_default(),
                body,
                at: info.message_timestamp.unwrap_or(0) as i64,
                outgoing,
                // The phone already knows whether it was delivered or read;
                // keep that so ticks are right from the first paint.
                status: match info.status.map(|s| s as i32) {
                    Some(3) => MessageStatus::Delivered,
                    Some(4) | Some(5) => MessageStatus::Read,
                    _ => MessageStatus::Sent,
                },
                media: media.as_ref().map(MediaRef::encode),
                reply_to: content
                    .context_info()
                    .and_then(|info| info.stanza_id.clone()),
            })
        })
        .collect();
    // Chunks arrive newest-first in some builds; the UI wants oldest-first.
    messages.sort_by_key(|m| m.at);
    // ponytail: bound what one conversation can inject at once. The UI only
    // reads the newest 200 anyway; raise this when it gets paging.
    let drop = messages.len().saturating_sub(200);
    messages.drain(..drop);

    let last_at = messages
        .last()
        .map(|m| m.at)
        .or(conversation.conversation_timestamp.map(|t| t as i64))
        .unwrap_or(0);
    let preview = messages.last().map(|m| m.body.clone()).unwrap_or_default();
    let preview_sender = if is_group {
        messages
            .last()
            .map(|m| {
                if m.outgoing {
                    "Anda".to_string()
                } else {
                    m.sender_name.clone()
                }
            })
            .unwrap_or_default()
    } else {
        String::new()
    };

    Some((
        Chat {
            id: target.clone(),
            account: AccountId(account.to_string()),
            service: Service::WhatsApp,
            title,
            is_group,
            avatar: None,
            unread: 0,
            muted: false,
            last_at,
            preview,
            preview_sender,
            target,
        },
        messages,
    ))
}

/// Pull the CDN coordinates out of one message so the shell can render the
/// placeholder now and download the bytes later, when the chat is opened.
/// `@1234567890` tokens in an outgoing body.
fn mention_tokens(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'@' {
            let start = i + 1;
            let mut end = start;
            while end < bytes.len() && bytes[end].is_ascii_digit() {
                end += 1;
            }
            if end > start {
                tokens.push(text[start..end].to_string());
                i = end;
                continue;
            }
        }
        i += 1;
    }
    tokens
}

fn media_ref(message: &whatsapp_rust::waproto::whatsapp::Message) -> Option<MediaRef> {
    let hex = |bytes: &Option<Vec<u8>>| bytes.as_deref().map(hex_encode);
    // Embedded previews are tiny JPEG/PNG blobs; carry them as base64 so the
    // UI can paint a video frame or sticker before the CDN download finishes.
    let thumb = |bytes: &Option<Vec<u8>>| {
        bytes
            .as_deref()
            .map(|data| base64::engine::general_purpose::STANDARD.encode(data))
    };
    if let Some(image) = message.image_message.as_option() {
        return Some(MediaRef {
            kind: "image".into(),
            url: image.url.clone(),
            direct: image.direct_path.clone(),
            key: hex(&image.media_key),
            sha: hex(&image.file_sha256),
            enc: hex(&image.file_enc_sha256),
            len: image.file_length,
            mime: image.mimetype.clone(),
            thumb: thumb(&image.jpeg_thumbnail),
            width: image.width,
            height: image.height,
            ..MediaRef::default()
        });
    }
    if let Some(video) = message.video_message.as_option() {
        return Some(MediaRef {
            kind: "video".into(),
            url: video.url.clone(),
            direct: video.direct_path.clone(),
            key: hex(&video.media_key),
            sha: hex(&video.file_sha256),
            enc: hex(&video.file_enc_sha256),
            len: video.file_length,
            mime: video.mimetype.clone(),
            thumb: thumb(&video.jpeg_thumbnail),
            width: video.width,
            height: video.height,
            ..MediaRef::default()
        });
    }
    if let Some(audio) = message.audio_message.as_option() {
        return Some(MediaRef {
            kind: "audio".into(),
            url: audio.url.clone(),
            direct: audio.direct_path.clone(),
            key: hex(&audio.media_key),
            sha: hex(&audio.file_sha256),
            enc: hex(&audio.file_enc_sha256),
            len: audio.file_length,
            mime: audio.mimetype.clone(),
            ..MediaRef::default()
        });
    }
    if let Some(sticker) = message.sticker_message.as_option() {
        return Some(MediaRef {
            kind: "sticker".into(),
            url: sticker.url.clone(),
            direct: sticker.direct_path.clone(),
            key: hex(&sticker.media_key),
            sha: hex(&sticker.file_sha256),
            enc: hex(&sticker.file_enc_sha256),
            len: sticker.file_length,
            mime: sticker.mimetype.clone(),
            thumb: thumb(&sticker.png_thumbnail),
            width: sticker.width,
            height: sticker.height,
            ..MediaRef::default()
        });
    }
    if let Some(document) = message.document_message.as_option() {
        return Some(MediaRef {
            kind: "document".into(),
            url: document.url.clone(),
            direct: document.direct_path.clone(),
            key: hex(&document.media_key),
            sha: hex(&document.file_sha256),
            enc: hex(&document.file_enc_sha256),
            len: document.file_length,
            mime: document.mimetype.clone(),
            name: document.file_name.clone(),
            ..MediaRef::default()
        });
    }
    None
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

fn hex_decode(text: &str) -> Option<Vec<u8>> {
    if text.len() % 2 != 0 {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).ok())
        .collect()
}

/// A `Downloadable` whose URL is the signed one from the message itself, so
/// the download skips the media-connection dance entirely.
struct SignedMedia<'a> {
    url: &'a str,
    key: Option<Vec<u8>>,
    sha: Vec<u8>,
    enc: Option<Vec<u8>>,
    len: u64,
    kind: whatsapp_rust::download::MediaType,
}

impl whatsapp_rust::download::Downloadable for SignedMedia<'_> {
    fn direct_path(&self) -> Option<&str> {
        None
    }
    fn media_key(&self) -> Option<&[u8]> {
        self.key.as_deref()
    }
    fn file_enc_sha256(&self) -> Option<&[u8]> {
        self.enc.as_deref()
    }
    fn file_sha256(&self) -> Option<&[u8]> {
        Some(&self.sha)
    }
    fn file_length(&self) -> Option<u64> {
        Some(self.len)
    }
    fn app_info(&self) -> whatsapp_rust::download::MediaType {
        self.kind
    }
    fn static_url(&self) -> Option<&str> {
        Some(self.url)
    }
}

fn media_type(kind: &str) -> whatsapp_rust::download::MediaType {
    use whatsapp_rust::download::MediaType;
    match kind {
        "image" => MediaType::Image,
        "video" => MediaType::Video,
        "audio" => MediaType::Audio,
        "sticker" => MediaType::Sticker,
        _ => MediaType::Document,
    }
}

fn media_extension(media: &MediaRef) -> &'static str {
    let mime = media.mime.as_deref().unwrap_or_default();
    if mime.contains("jpeg") || mime.contains("jpg") {
        "jpg"
    } else if mime.contains("png") {
        "png"
    } else if mime.contains("webp") {
        "webp"
    } else if mime.contains("gif") {
        "gif"
    } else if mime.contains("mp4") {
        "mp4"
    } else if mime.contains("pdf") {
        "pdf"
    } else if mime.contains("ogg") || mime.contains("opus") {
        "ogg"
    } else if mime.contains("mpeg") || mime.contains("mp3") {
        "mp3"
    } else {
        match media.kind.as_str() {
            "image" | "sticker" => "jpg",
            "video" => "mp4",
            "audio" => "ogg",
            _ => "bin",
        }
    }
}

/// Download one attachment into `sessions/whatsapp/media` and return the
/// descriptor with `path` filled in.
/// Download one attachment, waiting out a reconnect and asking the phone to
/// re-publish media whose link expired. WhatsApp Web retries the same way; the
/// first attempt often lands while the socket is still handshaking, which is
/// why a plain one-shot fetch left most photos as empty tiles.
async fn fetch_media(client: &Client, message: &Message, media: &MediaRef) -> Option<MediaRef> {
    let mut current = media.clone();
    for attempt in 0..2 {
        if !client.is_session_ready() {
            // Bounded: a phone that stays offline must not hold a slot (and
            // the visible tile) forever.
            let waited = tokio::time::timeout(
                std::time::Duration::from_secs(60),
                client.wait_until_reachable(),
            )
            .await;
            if waited.is_err() {
                log::debug!("media {}: still offline, giving up for now", message.id);
                return None;
            }
        }
        match download_media(client, &current, &message.id).await {
            Ok(done) => return Some(done),
            Err(err) => {
                log::debug!(
                    "media download failed for {} (attempt {}): {err:#}",
                    message.id,
                    attempt + 1
                );
                // Expired links are the other common failure: the CDN rejects
                // a stale signature and only the phone can mint a new one.
                // The old signed `url` has to go too, or the retry would use
                // the same expired token.
                if attempt == 0
                    && let Some(path) = reupload_media(client, message, &current).await
                {
                    current.direct = Some(path);
                    current.url = None;
                    continue;
                }
                // Nothing left to try: the server answered `NotFound`, or the
                // route keeps failing. Holding the slot would starve others.
                return None;
            }
        }
    }
    log::warn!("media {} could not be downloaded", message.id);
    None
}

/// Ask the phone to re-publish expired media and return its fresh path.
/// WhatsApp Web sends the same server-error receipt when a CDN URL 404s.
async fn reupload_media(client: &Client, message: &Message, media: &MediaRef) -> Option<String> {
    let key = media.key.as_deref().and_then(hex_decode)?;
    let key: [u8; 32] = key.as_slice().try_into().ok()?;
    let chat = message.chat_id.parse::<Jid>().ok()?;
    let sender = message.sender_id.parse::<Jid>().ok();
    let target = whatsapp_rust::MessageRef::new(
        &chat,
        whatsapp_rust::MessageId::new(&message.id).ok()?,
        sender.as_ref(),
        message.outgoing,
    )
    .ok()?;
    let request = whatsapp_rust::MediaReuploadRequest {
        target,
        media_key: &key,
    };
    match client.media_reupload().request(&request).await {
        Ok(whatsapp_rust::MediaRetryResult::Success { direct_path }) => Some(direct_path),
        Ok(other) => {
            log::debug!("media reupload for {}: {other:?}", message.id);
            None
        }
        Err(err) => {
            log::debug!("media reupload failed for {}: {err}", message.id);
            None
        }
    }
}

async fn download_media(client: &Client, media: &MediaRef, stem: &str) -> Result<MediaRef> {
    if media.local().is_some() {
        return Ok(media.clone());
    }
    let Some(direct) = media.direct.clone().filter(|d| !d.is_empty()) else {
        return Ok(media.clone());
    };
    // The URL WhatsApp puts on the message is already signed: it carries the
    // auth token the CDN checks. Rebuilding it from the bare `direct_path`
    // (what `DownloadParams` does) loses that token, so the CDN answers with
    // 403 or an error page that fails the hash check.
    let signed = media
        .url
        .as_deref()
        .filter(|url| url.starts_with("https://"))
        .and_then(|url| {
            let sha = media.sha.as_deref().and_then(hex_decode)?;
            let key = media.key.as_deref().and_then(hex_decode);
            let enc = media.enc.as_deref().and_then(hex_decode);
            Some(SignedMedia {
                url,
                key,
                sha,
                enc,
                len: media.len.unwrap_or(0),
                kind: media_type(&media.kind),
            })
        });
    let bytes = if let Some(signed) = signed {
        client
            .download(&signed, Default::default())
            .await
            .map_err(|e| anyhow!("media download: {e}"))?
    } else if let (Some(key), Some(sha), Some(enc)) = (
        media.key.as_deref().and_then(hex_decode),
        media.sha.as_deref().and_then(hex_decode),
        media.enc.as_deref().and_then(hex_decode),
    ) {
        let params = whatsapp_rust::download::DownloadParams::encrypted(
            direct,
            &key,
            &sha,
            &enc,
            media.len.unwrap_or(0),
            media_type(&media.kind),
        )
        .map_err(|e| anyhow!("media params: {e:?}"))?;
        client
            .download(&params, Default::default())
            .await
            .map_err(|e| anyhow!("media download: {e}"))?
    } else if let Some(sha) = media.sha.as_deref().and_then(hex_decode) {
        let params = whatsapp_rust::download::DownloadParams::plaintext(
            direct,
            &sha,
            media.len.unwrap_or(0),
            media_type(&media.kind),
        )
        .map_err(|e| anyhow!("media params: {e:?}"))?;
        client
            .download(&params, Default::default())
            .await
            .map_err(|e| anyhow!("media download: {e}"))?
    } else {
        return Ok(media.clone());
    };
    let dir = united_core::paths::service_dir("whatsapp").join("media");
    std::fs::create_dir_all(&dir)?;
    let safe: String = stem
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    let path = dir.join(format!("{safe}.{}", media_extension(media)));
    std::fs::write(&path, &bytes)?;
    united_core::paths::prune_media(&dir, united_core::paths::MEDIA_BUDGET);
    let mut done = media.clone();
    done.path = Some(path.display().to_string());
    Ok(done)
}

/// A tiny extension table for outgoing files; WhatsApp reads the MIME type,
/// the name is only metadata.
fn guess_mime(path: &std::path::Path) -> &'static str {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "mp4" => "video/mp4",
        "mov" => "video/quicktime",
        "mp3" => "audio/mpeg",
        "ogg" | "opus" => "audio/ogg; codecs=opus",
        "pdf" => "application/pdf",
        "zip" => "application/zip",
        "txt" => "text/plain",
        _ => "application/octet-stream",
    }
}

/// Text the bubble shows: plain/extended text first, then a media caption,
/// then the generic `Photo`/`Video` label. A photo or document keeps its
/// caption outside `text_content`, which used to drop the whole paragraph.
fn message_body(
    message: &whatsapp_rust::waproto::whatsapp::Message,
    media: Option<&MediaRef>,
) -> String {
    message
        .text_content()
        .map(|s| s.to_string())
        .filter(|text| !text.is_empty())
        .or_else(|| {
            message
                .get_caption()
                .filter(|text| !text.trim().is_empty())
                .map(|s| s.to_string())
        })
        .or_else(|| media.map(|m| m.preview()))
        .unwrap_or_default()
}

/// Fetch the preview profile picture and cache it under `sessions/whatsapp/avatars`.
///
/// Newer chats are addressed by LID, and the trusted-contact token is stored
/// under the LID, so the first attempt uses the JID as given. Only when that
/// yields nothing is the phone number tried: translating first (the old
/// behaviour) threw the token away and earned a 401.
async fn fetch_avatar(client: &Client, jid: &Jid) -> Result<Option<String>> {
    match fetch_picture(client, jid).await {
        Ok(Some(path)) => return Ok(Some(path)),
        Ok(None) => {}
        Err(err) if !jid.is_lid() => return Err(err),
        Err(err) => log::debug!("whatsapp: avatar for lid {jid} failed: {err:#}"),
    }
    if !jid.is_lid() {
        return Ok(None);
    }
    let Ok(Some(entry)) = client.get_lid_pn_entry(jid).await else {
        return Ok(None);
    };
    fetch_picture(client, &Jid::pn(entry.phone_number.to_string())).await
}

async fn lookup_picture(
    client: &Client,
    jid: &Jid,
    picture_type: ProfilePictureType,
    tc_token: &Option<Vec<u8>>,
) -> Result<Option<whatsapp_rust::wacore::iq::contacts::ProfilePicture>> {
    let lookup = client
        .execute(ProfilePictureSpec {
            jid: jid.clone(),
            picture_type,
            tc_token: tc_token.clone(),
            existing_id: None,
            common_gid: None,
            invite: None,
            persona_id: None,
            is_community: false,
            timeout: Some(std::time::Duration::from_secs(10)),
        })
        .await
        .map_err(|e| anyhow!("profile picture iq: {e}"))?;
    Ok(lookup.into_found())
}

async fn fetch_picture(client: &Client, jid: &Jid) -> Result<Option<String>> {
    // Contacts that do not have us in their address book only serve the
    // picture to a trusted-contact token (the one the phone sends with its
    // messages). Without it the server answers 401 not-authorized.
    let tc_token = client
        .tc_token()
        .get(&jid.user)
        .await
        .ok()
        .flatten()
        .map(|entry| entry.token);
    let picture = match lookup_picture(client, jid, ProfilePictureType::Preview, &tc_token).await? {
        Some(picture) => picture,
        // Group and business avatars are sometimes only served in full size.
        None => match lookup_picture(client, jid, ProfilePictureType::Full, &tc_token).await? {
            Some(picture) => picture,
            None => return Ok(None),
        },
    };
    download_avatar(jid, &picture.url).await
}

/// Cache one avatar bitmap under the JID's own file name and hand back its
/// path. Sharing this keeps newsletter pictures in the same cache the UI
/// already reads for contacts.
async fn download_avatar(jid: &Jid, url: &str) -> Result<Option<String>> {
    let response = reqwest::get(url)
        .await
        .map_err(|e| anyhow!("avatar download: {e}"))?;
    if !response.status().is_success() {
        return Ok(None);
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|e| anyhow!("avatar body: {e}"))?;
    let dir = united_core::paths::service_dir("whatsapp").join("avatars");
    std::fs::create_dir_all(&dir)?;
    let name: String = jid
        .to_string()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    let path = dir.join(format!("{name}.jpg"));
    std::fs::write(&path, &bytes)?;
    Ok(Some(path.display().to_string()))
}

/// Channels are newsletters: history sync carries their JID but no display
/// name, so the Channels tab used to show the raw JID. Fetch the metadata the
/// same way the web client does and store the real title plus picture.
async fn fetch_newsletter(client: &Client, jid: &Jid) -> Option<(String, Option<String>)> {
    let meta = client.newsletter().get_metadata(jid).await.ok()?;
    let name = meta.name.trim().to_string();
    if name.is_empty() {
        return None;
    }
    let avatar = match meta.picture_url.as_deref() {
        Some(url) => download_avatar(jid, url).await.ok().flatten(),
        None => None,
    };
    Some((name, avatar))
}

/// Channels are newsletters: history sync carries their JID but no display
/// name, so the Channels tab used to show the raw JID. One `list_subscribed`
/// call carries every name and picture; patch the rows that already exist.
async fn refresh_channels(client: &Client, bus: &EventBus, seen: &Arc<Mutex<HashSet<String>>>) {
    if !seen.lock().unwrap().insert("channels:list".into()) {
        return;
    }
    let Ok(store) = united_core::Store::shared() else {
        return;
    };
    let Ok(items) = client.newsletter().list_subscribed().await else {
        log::warn!("whatsapp: newsletter list failed");
        return;
    };
    for meta in items {
        let title = meta.name.trim().to_string();
        if title.is_empty() {
            continue;
        }
        let id = meta.jid.to_string();
        let Ok(Some(mut chat)) = store.chat("whatsapp:me", &id) else {
            continue;
        };
        if chat.title == title && chat.avatar.is_some() {
            continue;
        }
        chat.title = title;
        if let Some(url) = meta.picture_url.as_deref()
            && let Ok(Some(path)) = download_avatar(&meta.jid, url).await
        {
            chat.avatar = Some(path);
        }
        bus.publish(CoreEvent::ChatUpsert { chat });
    }
}

/// Newer WhatsApp accounts address a chat by LID (`…@lid`); history sync and
/// outgoing sends use the phone number. Resolve to the PN when possible.
async fn canonical_jid(client: &Client, jid: Jid) -> Jid {
    if !jid.is_lid() {
        return jid;
    }
    match client.get_lid_pn_entry(&jid).await {
        Ok(Some(entry)) => Jid::pn(entry.phone_number.to_string()),
        _ => jid,
    }
}

/// One-shot cleanup: chats stored under a LID alias are folded into the phone
/// number row so the same person stops showing up twice.
async fn reconcile_lid_chats(client: &Client, account: &str) {
    let Ok(store) = united_core::Store::shared() else {
        return;
    };
    let Ok(chats) = store.chats(Some(Service::WhatsApp), 500) else {
        return;
    };
    for chat in chats {
        let Ok(jid) = chat.id.parse::<Jid>() else {
            continue;
        };
        if !jid.is_lid() {
            continue;
        }
        let Ok(Some(entry)) = client.get_lid_pn_entry(&jid).await else {
            continue;
        };
        let pn = Jid::pn(entry.phone_number.to_string());
        if pn.to_string() == chat.id {
            continue;
        }
        match store.reassign_chat(account, &chat.id, &pn.to_string()) {
            Ok(()) => log::info!("whatsapp: merged chat {} into {}", chat.id, pn),
            Err(err) => log::warn!("whatsapp: merge {} failed: {err}", chat.id),
        }
    }
}

/// Groups fall back to "Group <id>" until their subject is known; history sync
/// does not always carry it, so ask once per stored group at startup.
async fn refresh_group_titles(client: &Client, bus: &EventBus) {
    let Ok(store) = united_core::Store::shared() else {
        return;
    };
    let Ok(chats) = store.chats(Some(Service::WhatsApp), 500) else {
        return;
    };
    for chat in chats {
        if !chat.is_group {
            continue;
        }
        let Ok(jid) = chat.id.parse::<Jid>() else {
            continue;
        };
        let Ok(meta) = client.groups().fetch_metadata(&jid).await else {
            continue;
        };
        let Some(subject) = meta.subject.filter(|s| !s.trim().is_empty()) else {
            continue;
        };
        if subject == chat.title {
            continue;
        }
        log::info!("whatsapp: group {} renamed to {}", chat.id, subject);
        bus.publish(CoreEvent::ChatUpsert {
            chat: Chat {
                title: subject,
                ..chat
            },
        });
    }
}

/// "Message yourself" belongs on the phone, not in a unified inbox: leaving it
/// there reads as the app texting your own number.
fn drop_self_chat(client: &Client, account: &str) {
    let Ok(store) = united_core::Store::shared() else {
        return;
    };
    for jid in [client.pn(), client.lid()]
        .into_iter()
        .flatten()
        .map(|jid| jid.to_non_ad())
    {
        let id = jid.to_string();
        if store.delete_chat(account, &id).is_ok() {
            log::info!("whatsapp: dropped self chat {id}");
        }
    }
}

/// Removes the pairing database (and its WAL side files) left by an unlinked
/// device. Profile pictures are user data and stay.
fn reset_session() {
    let dir = united_core::paths::service_dir("whatsapp");
    for name in ["device.db", "device.db-wal", "device.db-shm"] {
        let _ = std::fs::remove_file(dir.join(name));
    }
    log::info!("whatsapp: cleared unlinked device session");
}

#[cfg(test)]
mod tests {
    use super::*;
    use whatsapp_rust::waproto::whatsapp as wa;

    fn image_message(caption: Option<&str>) -> wa::Message {
        let mut image = wa::message::ImageMessage::default();
        image.caption = caption.map(|text| text.to_string());
        let mut message = wa::Message::default();
        message.image_message = Some(image).into();
        message
    }

    #[test]
    fn image_caption_is_the_bubble_text() {
        let media = MediaRef {
            kind: "image".into(),
            ..MediaRef::default()
        };
        assert_eq!(
            message_body(&image_message(Some("Laporan Iklan Harian")), Some(&media)),
            "Laporan Iklan Harian"
        );
    }

    #[test]
    fn media_without_caption_keeps_the_generic_label() {
        let media = MediaRef {
            kind: "image".into(),
            ..MediaRef::default()
        };
        assert_eq!(
            message_body(&image_message(None), Some(&media)),
            media.preview()
        );
    }
}
