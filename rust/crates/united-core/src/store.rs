use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;

use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, params};

use crate::model::{AccountId, Chat, MediaRef, Message, MessageStatus, Service, now_secs};

/// Person key used by `contacts`: JID user part without a device suffix.
fn contact_key(id: &str) -> &str {
    let user = id.split('@').next().unwrap_or(id);
    user.split([':', '-']).next().unwrap_or(user)
}

/// Whether a stored title/name carries an actual name rather than a number
/// or a masked `+62∙∙∙∙∙∙∙∙∙NN` placeholder.
fn has_letters(text: &str) -> bool {
    text.chars().any(char::is_alphabetic)
}

/// SQLite-backed cache of chats and messages. The protocol remains the source
/// of truth; this store keeps the window instant on launch and feeds the
/// unified inbox.
pub struct Store {
    conn: Mutex<Connection>,
}

impl Store {
    /// Process-wide store. The UI and every adapter share one connection
    /// instead of opening a fresh `united.db` for each history page or media
    /// lookup, which was both slow and prone to lock contention.
    pub fn shared() -> Result<std::sync::Arc<Store>> {
        use std::sync::{Arc, OnceLock};
        static SHARED: OnceLock<std::result::Result<Arc<Store>, String>> = OnceLock::new();
        SHARED
            .get_or_init(|| {
                Store::open(crate::paths::db_path())
                    .map(Arc::new)
                    .map_err(|err| format!("{err:#}"))
            })
            .clone()
            .map_err(anyhow::Error::msg)
    }

    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let conn = Connection::open(path).context("open united.db")?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        // The adapters open a second connection for maintenance; give it time
        // instead of failing with "database is locked".
        conn.busy_timeout(std::time::Duration::from_secs(2))?;
        conn.execute_batch(SCHEMA)?;
        migrate(&conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn upsert_chat(&self, chat: &Chat) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO chats (account_id, id, service, title, is_group, avatar, unread,
                                muted, last_at, preview, preview_sender, target)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)
             ON CONFLICT(account_id, id) DO UPDATE SET
                -- A history chunk that carries no name or messages must not
                -- replace a contact name or the last preview already stored.
                title=CASE
                    WHEN excluded.title IN (
                        substr(excluded.id, 1, instr(excluded.id, '@') - 1),
                        'Group ' || substr(excluded.id, 1, instr(excluded.id, '@') - 1))
                     AND chats.title NOT IN (
                        substr(chats.id, 1, instr(chats.id, '@') - 1),
                        'Group ' || substr(chats.id, 1, instr(chats.id, '@') - 1))
                    THEN chats.title
                    ELSE excluded.title END,
                is_group=excluded.is_group,
                avatar=COALESCE(excluded.avatar, chats.avatar),
                unread=MAX(chats.unread, excluded.unread),
                muted=MAX(chats.muted, excluded.muted),
                last_at=MAX(chats.last_at, excluded.last_at),
                -- A history chunk (on-demand paging, app-state resync) carries
                -- an old preview. Without the timestamp guard it replaced the
                -- newest one while `last_at` kept the new time, so the list
                -- showed a stale message under a fresh clock.
                preview=CASE WHEN excluded.last_at >= chats.last_at
                              AND excluded.preview <> ''
                             THEN excluded.preview ELSE chats.preview END,
                preview_sender=CASE WHEN excluded.last_at >= chats.last_at
                              AND excluded.preview <> ''
                             THEN excluded.preview_sender ELSE chats.preview_sender END,
                target=excluded.target",
            params![
                chat.account.0,
                chat.id,
                chat.service.id(),
                chat.title,
                chat.is_group as i32,
                chat.avatar,
                chat.unread,
                chat.muted as i32,
                chat.last_at,
                chat.preview,
                chat.preview_sender,
                chat.target,
            ],
        )?;
        Ok(())
    }

    /// Re-derive the chat list row from the newest stored message. History
    /// chunks, local deletes and edits can leave the preview or clock stale;
    /// the messages table is the source of truth, so compare against it.
    pub fn repair_previews(&self) -> Result<usize> {
        let conn = self.conn.lock().unwrap();
        let changed = conn.execute(
            "UPDATE chats SET
                 preview = (
                     SELECT m.body FROM messages m
                     WHERE m.account_id = chats.account_id AND m.chat_id = chats.id
                       AND m.body <> ''
                     ORDER BY m.at DESC, m.rowid DESC LIMIT 1
                 ),
                 preview_sender = CASE WHEN chats.is_group = 1 THEN (
                     SELECT CASE WHEN m.outgoing THEN 'Anda' ELSE m.sender_name END
                     FROM messages m
                     WHERE m.account_id = chats.account_id AND m.chat_id = chats.id
                       AND m.body <> ''
                     ORDER BY m.at DESC, m.rowid DESC LIMIT 1
                 ) ELSE '' END,
                 last_at = (
                     SELECT MAX(m.at) FROM messages m
                     WHERE m.account_id = chats.account_id AND m.chat_id = chats.id
                 )
             WHERE EXISTS (
                 SELECT 1 FROM messages m
                 WHERE m.account_id = chats.account_id AND m.chat_id = chats.id
                   AND m.body <> ''
             )
             AND (
                 preview <> (
                     SELECT m.body FROM messages m
                     WHERE m.account_id = chats.account_id AND m.chat_id = chats.id
                       AND m.body <> ''
                     ORDER BY m.at DESC, m.rowid DESC LIMIT 1
                 )
                 OR preview_sender <> CASE WHEN chats.is_group = 1 THEN (
                     SELECT CASE WHEN m.outgoing THEN 'Anda' ELSE m.sender_name END
                     FROM messages m
                     WHERE m.account_id = chats.account_id AND m.chat_id = chats.id
                       AND m.body <> ''
                     ORDER BY m.at DESC, m.rowid DESC LIMIT 1
                 ) ELSE '' END
                 OR last_at <> (
                     SELECT MAX(m.at) FROM messages m
                     WHERE m.account_id = chats.account_id AND m.chat_id = chats.id
                 )
             )",
            [],
        )?;
        Ok(changed)
    }

    pub fn upsert_chats(&self, chats: &[Chat]) -> Result<()> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO chats (account_id, id, service, title, is_group, avatar, unread,
                                    muted, last_at, preview, preview_sender, target)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)
                 ON CONFLICT(account_id, id) DO UPDATE SET
                    title=CASE
                        WHEN excluded.title IN (
                            substr(excluded.id, 1, instr(excluded.id, '@') - 1),
                            'Group ' || substr(excluded.id, 1, instr(excluded.id, '@') - 1))
                         AND chats.title NOT IN (
                            substr(chats.id, 1, instr(chats.id, '@') - 1),
                            'Group ' || substr(chats.id, 1, instr(chats.id, '@') - 1))
                        THEN chats.title
                        ELSE excluded.title END,
                    is_group=excluded.is_group,
                    avatar=COALESCE(excluded.avatar, chats.avatar),
                    unread=MAX(chats.unread, excluded.unread),
                muted=MAX(chats.muted, excluded.muted),
                    last_at=MAX(chats.last_at, excluded.last_at),
                    preview=CASE WHEN excluded.last_at >= chats.last_at
                                  AND excluded.preview <> ''
                                 THEN excluded.preview ELSE chats.preview END,
                    preview_sender=CASE WHEN excluded.last_at >= chats.last_at
                                  AND excluded.preview <> ''
                                 THEN excluded.preview_sender ELSE chats.preview_sender END,
                    target=excluded.target",
            )?;
            for chat in chats {
                stmt.execute(params![
                    chat.account.0,
                    chat.id,
                    chat.service.id(),
                    chat.title,
                    chat.is_group as i32,
                    chat.avatar,
                    chat.unread,
                    chat.muted as i32,
                    chat.last_at,
                    chat.preview,
                    chat.preview_sender,
                    chat.target,
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Insert a message. Returns false when it was already stored (history
    /// replay, reconnect overlap).
    pub fn put_message(&self, m: &Message) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        let changed = conn.execute(
            "INSERT OR IGNORE INTO messages
                (account_id, chat_id, id, service, sender_id, sender_name, body, at,
                 outgoing, status, media, reply_to)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
            params![
                m.account.0,
                m.chat_id,
                m.id,
                m.service.id(),
                m.sender_id,
                m.sender_name,
                m.body,
                m.at,
                m.outgoing as i32,
                status_str(m.status),
                m.media,
                m.reply_to,
            ],
        )?;
        Ok(changed > 0)
    }

    /// Insert many messages in one transaction. History sync arrives in
    /// thousands; a commit per row would freeze the UI thread.
    pub fn put_messages(&self, messages: &[Message]) -> Result<Vec<bool>> {
        if messages.is_empty() {
            return Ok(Vec::new());
        }
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        let mut inserted = Vec::with_capacity(messages.len());
        for m in messages {
            let changed = tx.execute(
                "INSERT OR IGNORE INTO messages
                    (account_id, chat_id, id, service, sender_id, sender_name, body, at,
                     outgoing, status, media, reply_to)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
                params![
                    m.account.0,
                    m.chat_id,
                    m.id,
                    m.service.id(),
                    m.sender_id,
                    m.sender_name,
                    m.body,
                    m.at,
                    m.outgoing as i32,
                    status_str(m.status),
                    m.media,
                    m.reply_to,
                ],
            )?;
            if changed == 0 {
                // Rows stored before captions were read carry the generic
                // "Photo"/"Video" label. A protocol re-delivery of the same id
                // now upgrades the body instead of being dropped as a dup.
                tx.execute(
                    "UPDATE messages SET body=?4
                     WHERE account_id=?1 AND chat_id=?2 AND id=?3
                       AND body IN ('Photo', 'Video', 'Sticker', 'Document')
                       AND body <> ?4",
                    params![m.account.0, m.chat_id, m.id, m.body],
                )?;
            }
            inserted.push(changed > 0);
        }
        tx.commit()?;
        Ok(inserted)
    }

    /// Upsert address-book names. One transaction per sync batch.
    pub fn put_contacts(&self, account: &str, rows: &[(String, String)]) -> Result<()> {
        if rows.is_empty() {
            return Ok(());
        }
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        for (id, name) in rows {
            if id.is_empty() || name.trim().is_empty() {
                continue;
            }
            tx.execute(
                "INSERT INTO contacts (account_id, id, name) VALUES (?1,?2,?3)
                 ON CONFLICT(account_id, id) DO UPDATE SET name=excluded.name",
                params![account, id, name],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Pull address-book names into rows written before the contact arrived
    /// (or before its delta). The phone's name wins over the sender's own
    /// push name, and over a masked `+62∙∙∙∙∙∙∙∙∙NN` placeholder. Group
    /// subjects are left alone.
    pub fn repair_contact_names(&self, account: &str) -> Result<()> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        tx.execute(
            "UPDATE chats SET title = c.name
             FROM contacts c
             WHERE chats.account_id = ?1
               AND chats.is_group = 0
               AND c.account_id = chats.account_id
               AND c.id = substr(chats.id, 1, instr(chats.id, '@') - 1)
               AND c.name GLOB '*[^0-9]*'
               AND chats.title <> c.name",
            params![account],
        )?;
        // Only placeholders/empty author names are cheap enough to rewrite
        // per delta; real push names are left to the one-time backfill.
        tx.execute(
            "UPDATE messages SET sender_name = c.name
             FROM contacts c
             WHERE messages.account_id = ?1
               AND messages.outgoing = 0
               AND messages.sender_name NOT GLOB '*[^0-9]*'
               AND c.account_id = messages.account_id
               AND c.id = substr(messages.sender_id, 1, instr(messages.sender_id, '@') - 1)
               AND c.name GLOB '*[^0-9]*'",
            params![account],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Fill placeholder names from evidence already on disk: push names seen
    /// in other chats, the last author of each direct chat, and finally the
    /// phone number itself. Runs once per launch; deltas are cheap.
    pub fn backfill_names(&self, account: &str) -> Result<()> {
        let mut conn = self.conn.lock().unwrap();
        let mut names: HashMap<String, String> = HashMap::new();
        {
            let mut stmt = conn.prepare("SELECT id, name FROM contacts WHERE account_id = ?1")?;
            let rows = stmt.query_map(params![account], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?;
            for row in rows {
                let (id, name) = row?;
                if has_letters(&name) {
                    names.insert(contact_key(&id).to_owned(), name);
                }
            }
        }
        {
            let mut stmt = conn.prepare(
                "SELECT sender_id, sender_name FROM messages
                 WHERE account_id = ?1 AND outgoing = 0 AND sender_name <> ''
                 ORDER BY at DESC, rowid DESC",
            )?;
            let rows = stmt.query_map(params![account], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?;
            for row in rows {
                let (id, name) = row?;
                if has_letters(&name) {
                    names.entry(contact_key(&id).to_owned()).or_insert(name);
                }
            }
        }
        // A direct chat is often addressed by the peer's LID while history
        // sync stored the author under another JID. The last incoming author
        // name is the best label for that row.
        let mut chat_names: HashMap<String, String> = HashMap::new();
        {
            let mut stmt = conn.prepare(
                "SELECT chat_id, sender_name FROM messages
                 WHERE account_id = ?1 AND outgoing = 0 AND sender_name <> ''
                 ORDER BY at DESC, rowid DESC",
            )?;
            let rows = stmt.query_map(params![account], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?;
            for row in rows {
                let (chat, name) = row?;
                if has_letters(&name) {
                    chat_names.entry(chat).or_insert(name);
                }
            }
        }
        let chats = {
            let mut stmt =
                conn.prepare("SELECT id, title, is_group FROM chats WHERE account_id = ?1")?;
            let rows = stmt.query_map(params![account], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            })?;
            rows.filter_map(Result::ok).collect::<Vec<_>>()
        };
        let messages = {
            let mut stmt = conn.prepare(
                "SELECT chat_id, id, sender_id, sender_name FROM messages
                 WHERE account_id = ?1 AND outgoing = 0",
            )?;
            let rows = stmt.query_map(params![account], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            })?;
            rows.filter_map(Result::ok).collect::<Vec<_>>()
        };
        let tx = conn.transaction()?;
        for (id, title, is_group) in chats {
            if is_group != 0 || has_letters(&title) {
                continue;
            }
            let key = contact_key(&id);
            let known = names.get(key).or_else(|| chat_names.get(&id)).cloned();
            let next = match known {
                Some(name) => name,
                None if id.ends_with("@s.whatsapp.net") => {
                    let digits: String = key.chars().filter(char::is_ascii_digit).collect();
                    if digits.is_empty() {
                        continue;
                    }
                    format!("+{digits}")
                }
                None => continue,
            };
            if next != title {
                tx.execute(
                    "UPDATE chats SET title = ?2 WHERE account_id = ?1 AND id = ?3",
                    params![account, next, id],
                )?;
            }
        }
        for (chat, id, sender, name) in messages {
            if has_letters(&name) {
                continue;
            }
            let Some(known) = names.get(contact_key(&sender)) else {
                continue;
            };
            if *known != name {
                tx.execute(
                    "UPDATE messages SET sender_name = ?2
                     WHERE account_id = ?1 AND chat_id = ?3 AND id = ?4",
                    params![account, known, chat, id],
                )?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Every known name for one account, keyed by id.
    pub fn contacts(&self, account: &str) -> Result<HashMap<String, String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT id, name FROM contacts WHERE account_id = ?1")?;
        let rows = stmt.query_map(params![account], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        let mut map = HashMap::new();
        for row in rows {
            let (id, name) = row?;
            map.insert(id, name);
        }
        Ok(map)
    }

    /// Newest incoming messages of one chat as `(id, sender)`. Read receipts
    /// are sent for exactly these, grouped by whoever wrote them.
    pub fn recent_incoming(
        &self,
        account: &str,
        chat_id: &str,
        limit: usize,
    ) -> Result<Vec<(String, String)>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, sender_id FROM messages
             WHERE account_id = ?1 AND chat_id = ?2 AND outgoing = 0
             ORDER BY at DESC, rowid DESC LIMIT ?3",
        )?;
        let rows = stmt.query_map(params![account, chat_id, limit as i64], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    /// Point media rows at files that are already in `dir`.
    ///
    /// Downloads used to be written to disk without the path reaching the row,
    /// so a restart re-fetched media the phone had long dropped (answered
    /// `NotFound`). One pass over the directory fixes the backlog.
    pub fn link_local_media(&self, dir: &Path) -> Result<usize> {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return Ok(0);
        };
        let files: HashMap<String, String> = entries
            .flatten()
            .filter_map(|entry| {
                let path = entry.path();
                let stem = path.file_stem()?.to_str()?.to_string();
                Some((stem, path.display().to_string()))
            })
            .collect();
        if files.is_empty() {
            return Ok(0);
        }
        let pending: Vec<(String, String)> = {
            let conn = self.conn.lock().unwrap();
            let mut stmt =
                conn.prepare("SELECT id, media FROM messages WHERE media IS NOT NULL")?;
            let rows = stmt.query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?;
            rows.filter_map(|row| {
                let (id, raw) = row.ok()?;
                let media = MediaRef::parse(&raw)?;
                if media.local().is_some() {
                    return None;
                }
                let file = files.get(&id)?.clone();
                let mut done = media;
                done.path = Some(file);
                Some((id, done.encode()))
            })
            .collect()
        };
        if pending.is_empty() {
            return Ok(0);
        }
        let conn = self.conn.lock().unwrap();
        for (id, media) in &pending {
            conn.execute(
                "UPDATE messages SET media=?2 WHERE id=?1 AND media IS NOT NULL",
                params![id, media],
            )?;
        }
        Ok(pending.len())
    }

    /// Attach the downloaded file path to one stored message.
    pub fn set_message_media(
        &self,
        account: &str,
        chat_id: &str,
        id: &str,
        media: &str,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let changed = conn.execute(
            "UPDATE messages SET media=?4 WHERE account_id=?1 AND chat_id=?2 AND id=?3",
            params![account, chat_id, id, media],
        )?;
        if changed == 0 {
            log::debug!("media path not stored: no row for {chat_id}/{id}");
        }
        Ok(())
    }

    pub fn update_status(
        &self,
        account: &str,
        chat_id: &str,
        id: &str,
        status: MessageStatus,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE messages SET status=?4 WHERE account_id=?1 AND chat_id=?2 AND id=?3",
            params![account, chat_id, id, status_str(status)],
        )?;
        Ok(())
    }

    /// Replace a message's text after the sender edited it.
    pub fn set_message_body(
        &self,
        account: &str,
        chat_id: &str,
        id: &str,
        body: &str,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE messages SET body=?4 WHERE account_id=?1 AND chat_id=?2 AND id=?3",
            params![account, chat_id, id, body],
        )?;
        Ok(())
    }

    /// Blank a revoked message while keeping the row, the way WhatsApp shows
    /// "Pesan ini telah dihapus" in place.
    pub fn scrub_message(&self, account: &str, chat_id: &str, id: &str, body: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE messages SET body=?4, media=NULL WHERE account_id=?1 AND chat_id=?2 AND id=?3",
            params![account, chat_id, id, body],
        )?;
        Ok(())
    }

    /// Record one sender's reaction. An empty `emoji` removes it, which is how
    /// the protocol expresses "reaction taken back".
    pub fn set_reaction(
        &self,
        account: &str,
        chat_id: &str,
        message_id: &str,
        sender_id: &str,
        emoji: &str,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        if emoji.is_empty() {
            conn.execute(
                "DELETE FROM reactions
                  WHERE account_id=?1 AND chat_id=?2 AND message_id=?3 AND sender_id=?4",
                params![account, chat_id, message_id, sender_id],
            )?;
        } else {
            conn.execute(
                "INSERT INTO reactions(account_id, chat_id, message_id, sender_id, emoji, at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(account_id, chat_id, message_id, sender_id)
                 DO UPDATE SET emoji=excluded.emoji, at=excluded.at",
                params![account, chat_id, message_id, sender_id, emoji, now_secs()],
            )?;
        }
        Ok(())
    }

    /// Every reaction in one conversation, keyed by message id. Each value is
    /// `(sender id, emoji)` so the UI can count and label them.
    pub fn reactions(
        &self,
        account: &str,
        chat_id: &str,
    ) -> Result<HashMap<String, Vec<(String, String)>>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT message_id, sender_id, emoji FROM reactions
              WHERE account_id=?1 AND chat_id=?2 ORDER BY at",
        )?;
        let rows = stmt.query_map(params![account, chat_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;
        let mut out: HashMap<String, Vec<(String, String)>> = HashMap::new();
        for row in rows {
            let (message_id, sender_id, emoji) = row?;
            out.entry(message_id).or_default().push((sender_id, emoji));
        }
        Ok(out)
    }

    /// Drop one message locally ("Delete for me"). Protocol-level revoke is
    /// adapter work; this only hides the row on this device.
    pub fn delete_message(&self, account: &str, chat_id: &str, id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM messages WHERE account_id=?1 AND chat_id=?2 AND id=?3",
            params![account, chat_id, id],
        )?;
        Ok(())
    }

    /// Newest first. `service = None` returns every account (unified inbox).
    /// Remove a chat and its messages (used to drop the "Message yourself"
    /// thread, which is not an inbox conversation).
    pub fn delete_chat(&self, account: &str, chat_id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM messages WHERE account_id = ?1 AND chat_id = ?2",
            params![account, chat_id],
        )?;
        conn.execute(
            "DELETE FROM chats WHERE account_id = ?1 AND id = ?2",
            params![account, chat_id],
        )?;
        Ok(())
    }

    /// Move everything stored under `from` onto `to` (WhatsApp's LID/phone
    /// aliases point at the same conversation).
    pub fn reassign_chat(&self, account: &str, from: &str, to: &str) -> Result<()> {
        if from == to {
            return Ok(());
        }
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE OR IGNORE messages SET chat_id = ?3
             WHERE account_id = ?1 AND chat_id = ?2",
            params![account, from, to],
        )?;
        conn.execute(
            "DELETE FROM messages WHERE account_id = ?1 AND chat_id = ?2",
            params![account, from],
        )?;
        let target_exists = conn
            .query_row(
                "SELECT 1 FROM chats WHERE account_id = ?1 AND id = ?2",
                params![account, to],
                |_| Ok(()),
            )
            .optional()?
            .is_some();
        if target_exists {
            conn.execute(
                "DELETE FROM chats WHERE account_id = ?1 AND id = ?2",
                params![account, from],
            )?;
        } else {
            // A chat titled after its old alias ("77305149939757") gets the new
            // number; real contact names are left alone.
            let old_user = from.split('@').next().unwrap_or(from);
            let new_user = to.split('@').next().unwrap_or(to);
            conn.execute(
                "UPDATE chats SET id = ?3, target = ?3,
                        title = CASE WHEN title = ?4 THEN ?5 ELSE title END
                 WHERE account_id = ?1 AND id = ?2",
                params![account, from, to, old_user, new_user],
            )?;
        }
        Ok(())
    }

    /// Oldest stored message of a chat, used as the anchor for on-demand
    /// history requests (the phone pages backwards from a known message).
    pub fn oldest_message(
        &self,
        account: &str,
        chat_id: &str,
    ) -> Result<Option<(String, bool, i64)>> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT id, outgoing, at FROM messages
             WHERE account_id = ?1 AND chat_id = ?2
             ORDER BY at ASC LIMIT 1",
            params![account, chat_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)? != 0,
                    row.get::<_, i64>(2)?,
                ))
            },
        )
        .optional()
        .map_err(Into::into)
    }

    /// Point a chat at a freshly downloaded profile picture.
    pub fn set_chat_avatar(&self, account: &str, id: &str, path: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE chats SET avatar = ?3 WHERE account_id = ?1 AND id = ?2",
            params![account, id, path],
        )?;
        Ok(())
    }

    /// Cache one participant's profile picture for group message rows.
    pub fn set_avatar(&self, account: &str, jid: &str, path: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO avatars (account_id, jid, path) VALUES (?1, ?2, ?3)
             ON CONFLICT(account_id, jid) DO UPDATE SET path = excluded.path",
            params![account, jid, path],
        )?;
        Ok(())
    }

    pub fn avatar(&self, account: &str, jid: &str) -> Result<Option<String>> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT path FROM avatars WHERE account_id = ?1 AND jid = ?2",
            params![account, jid],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(Into::into)
    }

    /// Distinct non-empty sender JIDs seen in one chat, newest first.
    pub fn sender_ids(&self, account: &str, chat_id: &str, limit: usize) -> Result<Vec<String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT DISTINCT sender_id FROM messages
             WHERE account_id = ?1 AND chat_id = ?2 AND sender_id <> ''
             ORDER BY at DESC LIMIT ?3",
        )?;
        let rows = stmt.query_map(params![account, chat_id, limit as i64], |row| {
            row.get::<_, String>(0)
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn chats(&self, service: Option<Service>, limit: usize) -> Result<Vec<Chat>> {
        let conn = self.conn.lock().unwrap();
        let mut out = Vec::new();
        match service {
            Some(s) => {
                let mut stmt = conn.prepare(
                    "SELECT account_id, id, service, title, is_group, avatar, unread, muted,
                            last_at, preview, preview_sender, target
                     FROM chats WHERE service=?1
                       AND NOT (is_group=0 AND title NOT GLOB '*[^0-9+∙ ]*'
                                AND NOT EXISTS (SELECT 1 FROM messages m
                                                WHERE m.account_id=chats.account_id
                                                  AND m.chat_id=chats.id))
                     ORDER BY last_at DESC LIMIT ?2",
                )?;
                let rows = stmt.query_map(params![s.id(), limit as i64], row_to_chat)?;
                for r in rows {
                    out.push(r?);
                }
            }
            None => {
                let mut stmt = conn.prepare(
                    "SELECT account_id, id, service, title, is_group, avatar, unread, muted,
                            last_at, preview, preview_sender, target
                     FROM chats
                     WHERE NOT (is_group=0 AND title NOT GLOB '*[^0-9+∙ ]*'
                                AND NOT EXISTS (SELECT 1 FROM messages m
                                                WHERE m.account_id=chats.account_id
                                                  AND m.chat_id=chats.id))
                     ORDER BY last_at DESC LIMIT ?1",
                )?;
                let rows = stmt.query_map(params![limit as i64], row_to_chat)?;
                for r in rows {
                    out.push(r?);
                }
            }
        }
        Ok(out)
    }

    pub fn chat(&self, account: &str, id: &str) -> Result<Option<Chat>> {
        let conn = self.conn.lock().unwrap();
        Ok(conn
            .query_row(
                "SELECT account_id, id, service, title, is_group, avatar, unread, muted,
                        last_at, preview, preview_sender, target
                 FROM chats WHERE account_id=?1 AND id=?2",
                params![account, id],
                row_to_chat,
            )
            .optional()?)
    }

    /// Oldest first, so the UI can append at the bottom.
    pub fn messages(&self, account: &str, chat_id: &str, limit: usize) -> Result<Vec<Message>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT account_id, chat_id, id, service, sender_id, sender_name, body, at,
                    outgoing, status, media, reply_to
             FROM messages WHERE account_id=?1 AND chat_id=?2
             ORDER BY at DESC, rowid DESC LIMIT ?3",
        )?;
        let rows = stmt.query_map(params![account, chat_id, limit as i64], row_to_message)?;
        let mut out: Vec<Message> = Vec::new();
        for r in rows {
            out.push(r?);
        }
        out.reverse();
        Ok(out)
    }

    /// One older page, oldest first. `before` is the timestamp of the oldest
    /// loaded row; `None` returns the newest page. Lets scrolling walk the
    /// local cache without touching the network.
    pub fn messages_page(
        &self,
        account: &str,
        chat_id: &str,
        before: Option<i64>,
        limit: usize,
    ) -> Result<Vec<Message>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT account_id, chat_id, id, service, sender_id, sender_name, body, at,
                    outgoing, status, media, reply_to
             FROM messages
             WHERE account_id=?1 AND chat_id=?2 AND (?3 IS NULL OR at < ?3)
             ORDER BY at DESC, rowid DESC LIMIT ?4",
        )?;
        let rows = stmt.query_map(
            params![account, chat_id, before, limit as i64],
            row_to_message,
        )?;
        let mut out: Vec<Message> = rows.filter_map(|r| r.ok()).collect();
        out.reverse();
        Ok(out)
    }

    /// Full-text-ish search over message bodies, newest first. `chat_id` limits
    /// the search to the open conversation; `None` searches every chat.
    pub fn search_messages(
        &self,
        account: &str,
        chat_id: Option<&str>,
        query: &str,
        limit: usize,
    ) -> Result<Vec<Message>> {
        let conn = self.conn.lock().unwrap();
        let pattern = format!("%{}%", query.replace('%', ""));
        let mut stmt = conn.prepare(
            "SELECT account_id, chat_id, id, service, sender_id, sender_name, body, at,
                    outgoing, status, media, reply_to
             FROM messages
             WHERE account_id=?1 AND (?2 IS NULL OR chat_id=?2)
               AND body LIKE ?3
             ORDER BY at DESC, rowid DESC LIMIT ?4",
        )?;
        let rows = stmt.query_map(
            params![account, chat_id, pattern, limit as i64],
            row_to_message,
        )?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    /// Empty one conversation but keep its row in the inbox.
    pub fn clear_chat(&self, account: &str, chat_id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM messages WHERE account_id=?1 AND chat_id=?2",
            params![account, chat_id],
        )?;
        conn.execute(
            "UPDATE chats SET preview='', unread=0 WHERE account_id=?1 AND id=?2",
            params![account, chat_id],
        )?;
        Ok(())
    }

    /// Pinned/archived flags per chat. Kept in a side table so the `Chat`
    /// model (and every adapter's literal) stays unchanged.
    pub fn chat_flags(&self) -> Result<HashMap<(String, String), (bool, bool)>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT account_id, id, pinned, archived FROM chat_flags")?;
        let rows = stmt.query_map([], |row| {
            Ok((
                (row.get::<_, String>(0)?, row.get::<_, String>(1)?),
                (row.get::<_, i32>(2)? != 0, row.get::<_, i32>(3)? != 0),
            ))
        })?;
        let mut out = HashMap::new();
        for row in rows {
            let (key, flags) = row?;
            out.insert(key, flags);
        }
        Ok(out)
    }

    pub fn set_chat_flag(
        &self,
        account: &str,
        chat_id: &str,
        pinned: Option<bool>,
        archived: Option<bool>,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO chat_flags (account_id, id, pinned, archived)
             VALUES (?1, ?2, COALESCE(?3, 0), COALESCE(?4, 0))
             ON CONFLICT(account_id, id) DO UPDATE SET
                pinned=CASE WHEN ?3 IS NULL THEN pinned ELSE ?3 END,
                archived=CASE WHEN ?4 IS NULL THEN archived ELSE ?4 END",
            params![
                account,
                chat_id,
                pinned.map(|v| v as i32),
                archived.map(|v| v as i32)
            ],
        )?;
        Ok(())
    }

    /// One stored message, used to resolve a quote whose target has already
    /// scrolled past the loaded window.
    pub fn message(&self, account: &str, chat_id: &str, id: &str) -> Result<Option<Message>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT account_id, chat_id, id, service, sender_id, sender_name, body, at,
                    outgoing, status, media, reply_to
             FROM messages WHERE account_id=?1 AND chat_id=?2 AND id=?3",
        )?;
        let mut rows = stmt.query_map(params![account, chat_id, id], row_to_message)?;
        Ok(rows.next().transpose()?)
    }

    pub fn set_unread(&self, account: &str, chat_id: &str, unread: u32) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE chats SET unread=?3 WHERE account_id=?1 AND id=?2",
            params![account, chat_id, unread],
        )?;
        Ok(())
    }

    pub fn unread_total(&self) -> Result<u32> {
        let conn = self.conn.lock().unwrap();
        let total: i64 = conn.query_row("SELECT COALESCE(SUM(unread),0) FROM chats", [], |r| {
            r.get(0)
        })?;
        Ok(total.max(0) as u32)
    }

    pub fn unread_by_service(&self) -> Result<Vec<(String, u32)>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt =
            conn.prepare("SELECT service, COALESCE(SUM(unread),0) FROM chats GROUP BY service")?;
        let rows = stmt.query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? as u32))
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn search_chats(&self, query: &str, limit: usize) -> Result<Vec<Chat>> {
        let conn = self.conn.lock().unwrap();
        let pattern = format!("%{}%", query.replace('%', ""));
        let mut stmt = conn.prepare(
            "SELECT account_id, id, service, title, is_group, avatar, unread, muted,
                    last_at, preview, preview_sender, target
             FROM chats WHERE title LIKE ?1 ORDER BY last_at DESC LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![pattern, limit as i64], row_to_chat)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    /// Remove every row belonging to a service (sign-out / unpair).
    pub fn clear_service(&self, service: Service) -> Result<()> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        tx.execute(
            "DELETE FROM messages WHERE service=?1",
            params![service.id()],
        )?;
        tx.execute("DELETE FROM chats WHERE service=?1", params![service.id()])?;
        tx.commit()?;
        Ok(())
    }
}

fn status_str(status: MessageStatus) -> &'static str {
    match status {
        MessageStatus::Sent => "sent",
        MessageStatus::Pending => "pending",
        MessageStatus::Delivered => "delivered",
        MessageStatus::Read => "read",
        MessageStatus::Failed => "failed",
    }
}

fn status_from(value: String) -> MessageStatus {
    match value.as_str() {
        "pending" => MessageStatus::Pending,
        "delivered" => MessageStatus::Delivered,
        "read" => MessageStatus::Read,
        "failed" => MessageStatus::Failed,
        _ => MessageStatus::Sent,
    }
}

fn row_to_chat(row: &rusqlite::Row<'_>) -> rusqlite::Result<Chat> {
    let account: String = row.get(0)?;
    let service: String = row.get(2)?;
    Ok(Chat {
        account: AccountId(account),
        id: row.get(1)?,
        service: Service::from_id(&service).unwrap_or(Service::WhatsApp),
        title: row.get(3)?,
        is_group: row.get::<_, i32>(4)? != 0,
        avatar: row.get(5)?,
        unread: row.get(6)?,
        muted: row.get::<_, i32>(7)? != 0,
        last_at: row.get(8)?,
        preview: row.get(9)?,
        preview_sender: row.get(10)?,
        target: row.get(11)?,
    })
}

fn row_to_message(row: &rusqlite::Row<'_>) -> rusqlite::Result<Message> {
    let account: String = row.get(0)?;
    let service: String = row.get(3)?;
    let status: String = row.get(9)?;
    Ok(Message {
        account: AccountId(account),
        chat_id: row.get(1)?,
        id: row.get(2)?,
        service: Service::from_id(&service).unwrap_or(Service::WhatsApp),
        sender_id: row.get(4)?,
        sender_name: row.get(5)?,
        body: row.get(6)?,
        at: row.get(7)?,
        outgoing: row.get::<_, i32>(8)? != 0,
        status: status_from(status),
        media: row.get(10)?,
        reply_to: row.get(11)?,
    })
}

/// One-time data fixes, gated by `PRAGMA user_version`.
fn migrate(conn: &Connection) -> Result<()> {
    let version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if version >= 9 {
        return Ok(());
    }
    // The column is referenced by every migration below, and an existing table
    // does not get it from the CREATE IF NOT EXISTS above.
    let has_column: i64 = conn.query_row(
        "SELECT COUNT(*) FROM pragma_table_info('chats') WHERE name = 'preview_sender'",
        [],
        |row| row.get(0),
    )?;
    if has_column == 0 {
        conn.execute(
            "ALTER TABLE chats ADD COLUMN preview_sender TEXT NOT NULL DEFAULT ''",
            [],
        )?;
    }
    if version < 2 {
        // v2: WhatsApp used to name its account after the paired phone number,
        // which orphaned every chat and message written before pairing finished.
        // The account id is `whatsapp:me` again, so fold the numbered rows back in.
        conn.execute_batch(
            r#"
        BEGIN;
        INSERT OR REPLACE INTO chats (account_id, id, service, title, is_group, avatar,
                                       unread, muted, last_at, preview, preview_sender, target)
            SELECT 'whatsapp:me', id, service, title, is_group, avatar,
                   unread, muted, last_at, preview, preview_sender, target
            FROM chats
            WHERE account_id LIKE 'whatsapp:%' AND account_id <> 'whatsapp:me';
        DELETE FROM chats
            WHERE account_id LIKE 'whatsapp:%' AND account_id <> 'whatsapp:me';
        INSERT OR IGNORE INTO messages (account_id, chat_id, id, service, sender_id,
                                        sender_name, body, at, outgoing, status, media, reply_to)
            SELECT 'whatsapp:me', chat_id, id, service, sender_id,
                   sender_name, body, at, outgoing, status, media, reply_to
            FROM messages
            WHERE account_id LIKE 'whatsapp:%' AND account_id <> 'whatsapp:me';
        DELETE FROM messages
            WHERE account_id LIKE 'whatsapp:%' AND account_id <> 'whatsapp:me';
        PRAGMA user_version = 2;
        COMMIT;
        "#,
        )?;
    }
    if version < 3 {
        // v3: chat rows written by history sync could carry an empty preview
        // (last message was a reaction or protocol message), which made the
        // list say "No messages yet" for a chat full of messages.
        conn.execute_batch(
            r#"
            BEGIN;
            UPDATE chats
               SET preview = COALESCE((SELECT m.body FROM messages m
                     WHERE m.account_id = chats.account_id
                       AND m.chat_id = chats.id
                       AND m.body <> ''
                     ORDER BY m.at DESC, m.rowid DESC LIMIT 1), preview),
                   last_at = COALESCE((SELECT MAX(m.at) FROM messages m
                     WHERE m.account_id = chats.account_id
                       AND m.chat_id = chats.id), last_at)
             WHERE preview = '' OR last_at = 0;
            PRAGMA user_version = 3;
            COMMIT;
            "#,
        )?;
    }
    if version < 4 {
        // v4: WhatsApp channels and status broadcasts were stored as chats by
        // older builds; they are not conversations.
        conn.execute_batch(
            r#"
            BEGIN;
            DELETE FROM messages
             WHERE chat_id LIKE '%@newsletter' OR chat_id LIKE '%@broadcast';
            DELETE FROM chats
             WHERE id LIKE '%@newsletter' OR id LIKE '%@broadcast';
            PRAGMA user_version = 4;
            COMMIT;
            "#,
        )?;
    }
    if version < 5 {
        // v5: WhatsApp history sync created rows for chats whose only synced
        // messages were reactions (stored with an empty body), leaving an
        // inbox row that said "No messages yet". Empty threads are not useful.
        conn.execute_batch(
            r#"
            BEGIN;
            DELETE FROM chats
             WHERE service = 'whatsapp'
               AND preview = ''
               AND NOT EXISTS (SELECT 1 FROM messages m
                                WHERE m.account_id = chats.account_id
                                  AND m.chat_id = chats.id);
            PRAGMA user_version = 5;
            COMMIT;
            "#,
        )?;
    }
    if version < 6 {
        // v6: reactions reached the message table with an empty body (the old
        // handler stored them). They render as blank bubbles and blank the
        // chat preview, so drop them and then any thread left empty.
        conn.execute_batch(
            r#"
            BEGIN;
            DELETE FROM messages WHERE service = 'whatsapp' AND body = '';
            DELETE FROM chats
             WHERE service = 'whatsapp'
               AND NOT EXISTS (SELECT 1 FROM messages m
                                WHERE m.account_id = chats.account_id
                                  AND m.chat_id = chats.id);
            PRAGMA user_version = 6;
            COMMIT;
            "#,
        )?;
    }
    if version < 7 {
        // v7: direct chats were titled with the bare phone number even when the
        // messages carried a push name. Use the newest non-empty push name.
        conn.execute_batch(
            r#"
            BEGIN;
            UPDATE chats
               SET title = COALESCE((SELECT m.sender_name FROM messages m
                     WHERE m.account_id = chats.account_id
                       AND m.chat_id = chats.id
                       AND m.sender_name <> ''
                     ORDER BY m.at DESC LIMIT 1), title)
             WHERE service = 'whatsapp'
               AND is_group = 0
               AND title = substr(id, 1, instr(id, '@') - 1);
            PRAGMA user_version = 7;
            COMMIT;
            "#,
        )?;
    }
    if version < 8 {
        // v8: a nameless history chunk could blank a contact name and preview
        // that were already known. Rebuild both from the stored messages.
        conn.execute_batch(
            r#"
            BEGIN;
            UPDATE chats
               SET title = COALESCE((SELECT m.sender_name FROM messages m
                     WHERE m.account_id = chats.account_id
                       AND m.chat_id = chats.id
                       AND m.sender_name <> ''
                     ORDER BY m.at DESC LIMIT 1), title)
             WHERE service = 'whatsapp' AND is_group = 0
               AND title = substr(id, 1, instr(id, '@') - 1);
            UPDATE chats
               SET preview = COALESCE((SELECT m.body FROM messages m
                     WHERE m.account_id = chats.account_id
                       AND m.chat_id = chats.id
                       AND m.body <> ''
                     ORDER BY m.at DESC, m.rowid DESC LIMIT 1), preview)
             WHERE preview = '';
            UPDATE chats
               SET last_at = COALESCE((SELECT MAX(m.at) FROM messages m
                     WHERE m.account_id = chats.account_id
                       AND m.chat_id = chats.id), last_at)
             WHERE last_at = 0;
            PRAGMA user_version = 8;
            COMMIT;
            "#,
        )?;
    }
    if version < 9 {
        // v9: group rows show "Sender: text" like WhatsApp Web, so the author
        // of the preview is carried next to it instead of being looked up for
        // every row on every frame.
        conn.execute_batch(
            r#"
            BEGIN;
            UPDATE chats
               SET preview_sender = COALESCE((SELECT CASE WHEN m.outgoing THEN 'Anda'
                                                          ELSE m.sender_name END
                     FROM messages m
                     WHERE m.account_id = chats.account_id
                       AND m.chat_id = chats.id
                       AND m.body <> ''
                     ORDER BY m.at DESC, m.rowid DESC LIMIT 1), '')
             WHERE is_group = 1 AND preview_sender = '';
            PRAGMA user_version = 9;
            COMMIT;
            "#,
        )?;
    }
    Ok(())
}

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS chats (
    account_id TEXT NOT NULL,
    id         TEXT NOT NULL,
    service    TEXT NOT NULL,
    title      TEXT NOT NULL,
    is_group   INTEGER NOT NULL DEFAULT 0,
    avatar     TEXT,
    unread     INTEGER NOT NULL DEFAULT 0,
    muted      INTEGER NOT NULL DEFAULT 0,
    last_at    INTEGER NOT NULL DEFAULT 0,
    preview    TEXT NOT NULL DEFAULT '',
    preview_sender TEXT NOT NULL DEFAULT '',
    target     TEXT NOT NULL DEFAULT '',
    PRIMARY KEY (account_id, id)
);
CREATE INDEX IF NOT EXISTS chats_by_time ON chats(last_at DESC);
CREATE INDEX IF NOT EXISTS chats_by_service ON chats(service, last_at DESC);

-- Local-only inbox flags. Kept apart from `chats` so protocol upserts from
-- history sync can never overwrite what the user chose.
CREATE TABLE IF NOT EXISTS chat_flags (
    account_id TEXT NOT NULL,
    id         TEXT NOT NULL,
    pinned     INTEGER NOT NULL DEFAULT 0,
    archived   INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (account_id, id)
);


CREATE TABLE IF NOT EXISTS messages (
    account_id  TEXT NOT NULL,
    chat_id     TEXT NOT NULL,
    id          TEXT NOT NULL,
    service     TEXT NOT NULL,
    sender_id   TEXT NOT NULL DEFAULT '',
    sender_name TEXT NOT NULL DEFAULT '',
    body        TEXT NOT NULL DEFAULT '',
    at          INTEGER NOT NULL DEFAULT 0,
    outgoing    INTEGER NOT NULL DEFAULT 0,
    status      TEXT NOT NULL DEFAULT 'sent',
    media       TEXT,
    reply_to    TEXT,
    PRIMARY KEY (account_id, chat_id, id)
);
CREATE INDEX IF NOT EXISTS messages_by_chat ON messages(account_id, chat_id, at DESC);

-- Emoji reactions, one per sender per message. Kept out of `messages` so a
-- protocol edit never rewrites the transcript row and history stays small.
CREATE TABLE IF NOT EXISTS reactions (
    account_id TEXT NOT NULL,
    chat_id    TEXT NOT NULL,
    message_id TEXT NOT NULL,
    sender_id  TEXT NOT NULL,
    emoji      TEXT NOT NULL,
    at         INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (account_id, chat_id, message_id, sender_id)
);

-- Address-book and push names, keyed by the id WhatsApp addresses.
CREATE TABLE IF NOT EXISTS contacts (
    account_id TEXT NOT NULL,
    id         TEXT NOT NULL,
    name       TEXT NOT NULL,
    PRIMARY KEY (account_id, id)
);

-- Profile pictures of individual group participants, keyed by their own JID.
CREATE TABLE IF NOT EXISTS avatars (
    account_id TEXT NOT NULL,
    jid        TEXT NOT NULL,
    path       TEXT NOT NULL,
    PRIMARY KEY (account_id, jid)
);
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reactions_upsert_and_clear() {
        let store = Store::in_memory().unwrap();
        let account = "whatsapp:me";
        let chat = "628123@s.whatsapp.net";
        let id = "ABC";
        store
            .set_reaction(account, chat, id, "6281@s.whatsapp.net", "👍")
            .unwrap();
        store
            .set_reaction(account, chat, id, "6281@s.whatsapp.net", "❤️")
            .unwrap();
        store
            .set_reaction(account, chat, id, "6282@s.whatsapp.net", "👍")
            .unwrap();
        let list = store.reactions(account, chat).unwrap();
        let list = list.get(id).unwrap();
        assert_eq!(list.len(), 2);
        assert!(
            list.iter()
                .any(|(sender, emoji)| sender.starts_with("6281") && emoji == "❤️")
        );
        store
            .set_reaction(account, chat, id, "6281@s.whatsapp.net", "")
            .unwrap();
        assert_eq!(
            store
                .reactions(account, chat)
                .unwrap()
                .get(id)
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn group_avatar_roundtrip() {
        let store = Store::in_memory().unwrap();
        let jid = "628123@s.whatsapp.net";
        assert_eq!(store.avatar("whatsapp:me", jid).unwrap(), None);
        store.set_avatar("whatsapp:me", jid, "/tmp/a.jpg").unwrap();
        store.set_avatar("whatsapp:me", jid, "/tmp/b.jpg").unwrap();
        assert_eq!(
            store.avatar("whatsapp:me", jid).unwrap().as_deref(),
            Some("/tmp/b.jpg")
        );
    }

    #[test]
    fn address_book_repairs_numeric_titles_and_senders() {
        let store = Store::in_memory().unwrap();
        let chat = Chat {
            id: "628123@s.whatsapp.net".into(),
            account: AccountId("whatsapp:me".into()),
            service: Service::WhatsApp,
            title: "628123".into(),
            is_group: false,
            avatar: None,
            unread: 0,
            muted: false,
            last_at: 1,
            preview: String::new(),
            preview_sender: String::new(),
            target: "628123@s.whatsapp.net".into(),
        };
        store.upsert_chat(&chat).unwrap();
        let message = Message {
            id: "a".into(),
            account: AccountId("whatsapp:me".into()),
            chat_id: chat.id.clone(),
            service: Service::WhatsApp,
            sender_id: "628123@s.whatsapp.net".into(),
            sender_name: "628123".into(),
            body: "halo".into(),
            at: 1,
            outgoing: false,
            status: crate::model::MessageStatus::Sent,
            media: None,
            reply_to: None,
        };
        store.put_messages(&[message]).unwrap();
        store
            .put_contacts("whatsapp:me", &[("628123".into(), "Dewi".into())])
            .unwrap();
        store.repair_contact_names("whatsapp:me").unwrap();
        let chat = store
            .chat("whatsapp:me", "628123@s.whatsapp.net")
            .unwrap()
            .unwrap();
        assert_eq!(chat.title, "Dewi");
        let stored = store
            .messages("whatsapp:me", "628123@s.whatsapp.net", 10)
            .unwrap();
        assert_eq!(stored[0].sender_name, "Dewi");
    }

    #[test]
    fn empty_unnamed_chats_stay_out_of_inbox() {
        let store = Store::in_memory().unwrap();
        let chat = |id: &str, title: &str, at: i64| Chat {
            id: id.into(),
            account: AccountId("whatsapp:me".into()),
            service: Service::WhatsApp,
            title: title.into(),
            is_group: false,
            avatar: None,
            unread: 0,
            muted: false,
            last_at: at,
            preview: String::new(),
            preview_sender: String::new(),
            target: id.into(),
        };
        store
            .upsert_chats(&[
                chat("111@s.whatsapp.net", "111", 3),
                chat("222@lid", "+62∙28", 2),
                chat("333@s.whatsapp.net", "Ibu", 1),
            ])
            .unwrap();
        let ids: Vec<String> = store
            .chats(Some(Service::WhatsApp), 100)
            .unwrap()
            .into_iter()
            .map(|chat| chat.id)
            .collect();
        assert_eq!(ids, ["333@s.whatsapp.net"]);
    }

    #[test]
    fn message_batch_inserts_each_row_once() {
        let store = Store::in_memory().unwrap();
        let message = |id: &str| Message {
            id: id.into(),
            account: AccountId("whatsapp:me".into()),
            chat_id: "628123@s.whatsapp.net".into(),
            service: Service::WhatsApp,
            sender_id: String::new(),
            sender_name: "Dewi".into(),
            body: format!("halo {id}"),
            at: 1_700_000_000,
            outgoing: false,
            status: crate::model::MessageStatus::Sent,
            media: None,
            reply_to: None,
        };
        let batch = vec![message("a"), message("b")];
        assert_eq!(store.put_messages(&batch).unwrap(), vec![true, true]);
        assert_eq!(store.put_messages(&batch).unwrap(), vec![false, false]);
        assert_eq!(
            store
                .messages("whatsapp:me", "628123@s.whatsapp.net", 10)
                .unwrap()
                .len(),
            2
        );
    }

    #[test]
    fn redelivered_media_message_gains_its_caption() {
        let store = Store::in_memory().unwrap();
        let mut message = Message {
            id: "A1".into(),
            account: AccountId("whatsapp:me".into()),
            chat_id: "120@g.us".into(),
            service: Service::WhatsApp,
            sender_id: String::new(),
            sender_name: "Dewi".into(),
            body: "Photo".into(),
            at: 1_700_000_000,
            outgoing: false,
            status: MessageStatus::Sent,
            media: None,
            reply_to: None,
        };
        assert_eq!(store.put_messages(&[message.clone()]).unwrap(), vec![true]);
        message.body = "laporan harian".into();
        assert_eq!(store.put_messages(&[message]).unwrap(), vec![false]);
        let stored = store.messages("whatsapp:me", "120@g.us", 10).unwrap();
        assert_eq!(stored[0].body, "laporan harian");
    }

    #[test]
    fn old_history_preview_does_not_replace_the_newest() {
        let store = Store::in_memory().unwrap();
        let chat = |preview: &str, at: i64| Chat {
            id: "120@g.us".into(),
            account: AccountId("whatsapp:me".into()),
            service: Service::WhatsApp,
            title: "Grup".into(),
            is_group: true,
            avatar: None,
            unread: 0,
            muted: false,
            last_at: at,
            preview: preview.into(),
            preview_sender: String::new(),
            target: "120@g.us".into(),
        };
        store.upsert_chat(&chat("terbaru", 200)).unwrap();
        // An on-demand history chunk arrives with a preview from months ago.
        store.upsert_chat(&chat("lama", 100)).unwrap();
        let stored = store.chat("whatsapp:me", "120@g.us").unwrap().unwrap();
        assert_eq!(stored.preview, "terbaru");
        assert_eq!(stored.last_at, 200);
    }

    #[test]
    fn repair_rederives_group_preview_and_clock() {
        let store = Store::in_memory().unwrap();
        let chat = Chat {
            id: "120@g.us".into(),
            account: AccountId("whatsapp:me".into()),
            service: Service::WhatsApp,
            title: "Grup".into(),
            is_group: true,
            avatar: None,
            unread: 2,
            muted: true,
            last_at: 500,
            preview: "lama".into(),
            preview_sender: String::new(),
            target: "120@g.us".into(),
        };
        store.upsert_chat(&chat).unwrap();
        let message = |id: &str, body: &str, at: i64, outgoing: bool| Message {
            id: id.into(),
            account: AccountId("whatsapp:me".into()),
            chat_id: "120@g.us".into(),
            service: Service::WhatsApp,
            sender_id: "6281@s.whatsapp.net".into(),
            sender_name: "Dewi".into(),
            body: body.into(),
            at,
            outgoing,
            status: crate::model::MessageStatus::Sent,
            media: None,
            reply_to: None,
        };
        assert_eq!(
            store
                .put_messages(&[
                    message("a", "lama", 100, false),
                    message("b", "baru", 200, true)
                ])
                .unwrap(),
            vec![true, true]
        );
        assert_eq!(store.repair_previews().unwrap(), 1);
        let fixed = store.chat("whatsapp:me", "120@g.us").unwrap().unwrap();
        assert_eq!(fixed.preview, "baru");
        assert_eq!(fixed.preview_sender, "Anda");
        assert_eq!(fixed.last_at, 200);

        // Adapter resyncs carry unread=0 and muted=false; they must not wipe
        // state the user set or the live counter.
        let mut resync = chat;
        resync.unread = 0;
        resync.muted = false;
        resync.last_at = 100;
        store.upsert_chat(&resync).unwrap();
        let kept = store.chat("whatsapp:me", "120@g.us").unwrap().unwrap();
        assert_eq!(kept.unread, 2);
        assert!(kept.muted);
    }

    #[test]
    fn link_local_media_adopts_files_on_disk() {
        let store = Store::in_memory().unwrap();
        let dir = std::env::temp_dir().join(format!("united-link-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("AABB.jpg");
        std::fs::write(&file, b"jpg").unwrap();
        let media = MediaRef {
            kind: "image".into(),
            direct: Some("/v/t62/x".into()),
            ..MediaRef::default()
        }
        .encode();
        store
            .put_message(&Message {
                id: "AABB".into(),
                account: AccountId("whatsapp:me".into()),
                chat_id: "628123@s.whatsapp.net".into(),
                service: Service::WhatsApp,
                sender_id: String::new(),
                sender_name: "Dewi".into(),
                body: "Photo".into(),
                at: 1_700_000_000,
                outgoing: false,
                status: MessageStatus::Sent,
                media: Some(media),
                reply_to: None,
            })
            .unwrap();
        assert_eq!(store.link_local_media(&dir).unwrap(), 1);
        // Idempotent: the row now points at the file.
        assert_eq!(store.link_local_media(&dir).unwrap(), 0);
        let media = store
            .messages("whatsapp:me", "628123@s.whatsapp.net", 10)
            .unwrap()
            .remove(0)
            .media
            .unwrap();
        let media = MediaRef::parse(&media).unwrap();
        assert_eq!(media.local(), Some(file.to_str().unwrap()));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
