//! Data of the reading pane (stage 3): message header and locations, previews, prefetch
//! candidates and settings, the remote-content allowlist. Synchronous functions over a
//! connection; `body_api` runs them on the writer thread or a reader.

use std::collections::HashMap;

use mail_model::{
    AccountId, Address, MailboxId, MailboxRole, MessageId, PrefetchPeriod, PrefetchSettings, Uid,
    UidValidity,
};
use rusqlite::types::Type;
use rusqlite::{Connection, OptionalExtension, params};

use crate::StoreError;
use crate::accounts::{self, account_id_at, conversion_error};
use crate::settings::now_millis;

/// Longest stored preview, in characters.
pub const MAX_PREVIEW_CHARS: usize = 256;

/// Most prefetch candidates one call returns.
pub const MAX_PREFETCH_BATCH: u32 = 1_000;

/// Setting keys (global scope) of [`PrefetchSettings`]. Device class: not to be registered for
/// the settings sync (stage 11).
pub(crate) const PREFETCH_PERIOD_KEY: &str = "prefetch.period";
pub(crate) const PREFETCH_SIZE_KEY: &str = "prefetch.maxMessageSize";

pub(crate) const MESSAGE_SUMMARY: &str = "
    SELECT (SELECT a.uuid FROM account a WHERE a.id = m.account_id), m.message_id_hdr, m.date,
           m.subject, m.from_name, m.from_addr, m.to_addrs, m.cc_addrs, m.size, m.preview,
           m.has_attachments
    FROM message m WHERE m.id = ?1";

pub(crate) const MESSAGE_LOCATIONS: &str = "
    SELECT l.uid, b.id, b.name, b.uidvalidity, b.role, b.selectable
    FROM message_location l CROSS JOIN mailbox b ON b.id = l.mailbox_id
    WHERE l.message_id = ?1 ORDER BY l.mailbox_id";

pub(crate) const SET_PREVIEW: &str = "UPDATE message SET preview = ?2 WHERE id = ?1";

pub(crate) const PREFETCH_MAILBOXES: &str =
    "SELECT id, role FROM mailbox WHERE account_id = ?1 AND selectable = 1";

/// Messages of one mailbox in the period without a stored body, newest first.
pub(crate) const PREFETCH_IN_MAILBOX: &str = "
    SELECT l.message_id, l.uid, m.size
    FROM message_location l CROSS JOIN message m ON m.id = l.message_id
    WHERE l.mailbox_id = ?1 AND l.message_id >= ?2 AND l.message_id < ?3
      AND NOT EXISTS (SELECT 1 FROM message_body b WHERE b.message_id = l.message_id)
    ORDER BY l.message_id DESC LIMIT ?4";

/// The same for flagged messages only. `INDEXED BY`: without statistics the planner takes the
/// primary key and reads every location of the period instead of the few flagged ones.
pub(crate) const PREFETCH_FLAGGED_IN_MAILBOX: &str = "
    SELECT l.message_id, l.uid, m.size
    FROM message_location l INDEXED BY loc_flagged CROSS JOIN message m ON m.id = l.message_id
    WHERE l.mailbox_id = ?1 AND l.flagged = 1 AND l.message_id >= ?2 AND l.message_id < ?3
      AND NOT EXISTS (SELECT 1 FROM message_body b WHERE b.message_id = l.message_id)
    ORDER BY l.message_id DESC LIMIT ?4";

pub(crate) const SENDER_ALLOWED: &str =
    "SELECT 1 FROM remote_content_sender WHERE account_uuid = ?1 AND sender = ?2";

pub(crate) const ALLOW_SENDER: &str = "
    INSERT INTO remote_content_sender (account_uuid, sender, created_at) VALUES (?1, ?2, ?3)
    ON CONFLICT (account_uuid, sender) DO NOTHING";

pub(crate) const REVOKE_SENDER: &str =
    "DELETE FROM remote_content_sender WHERE account_uuid = ?1 AND sender = ?2";

pub(crate) const SENDERS: &str =
    "SELECT sender FROM remote_content_sender WHERE account_uuid = ?1 ORDER BY sender";

/// Header data of a message as stored with the list (see [`crate::Store::message_summary`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredMessage {
    pub id: MessageId,
    pub account_id: AccountId,
    /// `Message-ID` without angle brackets.
    pub message_id_header: Option<String>,
    /// Sort date, Unix seconds.
    pub date: i64,
    pub subject: String,
    /// First `From` address.
    pub from: Option<Address>,
    pub to: Vec<Address>,
    pub cc: Vec<Address>,
    /// RFC822.SIZE.
    pub size: u32,
    pub preview: Option<String>,
    pub has_attachments: bool,
}

/// Where a message lies, with what the engine needs to fetch it from there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageLocation {
    pub mailbox_id: MailboxId,
    /// Wire name, for `MailBackend::open_mailbox`.
    pub mailbox_name: String,
    pub role: MailboxRole,
    pub selectable: bool,
    /// Generation of `uid`; compare with the UIDVALIDITY the server reports when opening.
    pub uid_validity: Option<UidValidity>,
    pub uid: Uid,
}

/// A message to prefetch (see [`crate::Store::prefetch_candidates`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrefetchCandidate {
    pub message_id: MessageId,
    pub mailbox_id: MailboxId,
    pub uid: Uid,
    /// RFC822.SIZE, to decide between a whole fetch and the text parts.
    pub size: u32,
}

pub(crate) fn summary(
    conn: &Connection,
    message: MessageId,
) -> Result<Option<StoredMessage>, StoreError> {
    let row = conn
        .prepare_cached(MESSAGE_SUMMARY)?
        .query_row([message.0], |r| {
            let from_name: Option<String> = r.get(4)?;
            let from_addr: Option<String> = r.get(5)?;
            Ok(StoredMessage {
                id: message,
                account_id: account_id_at(r, 0)?,
                message_id_header: r.get(1)?,
                date: r.get(2)?,
                subject: r.get(3)?,
                from: from_addr.map(|email| Address {
                    name: from_name,
                    email,
                }),
                to: addresses_at(r, 6)?,
                cc: addresses_at(r, 7)?,
                size: r.get(8)?,
                preview: r.get(9)?,
                has_attachments: r.get(10)?,
            })
        })
        .optional()?;
    Ok(row)
}

fn addresses_at(r: &rusqlite::Row<'_>, idx: usize) -> rusqlite::Result<Vec<Address>> {
    let Some(json) = r.get::<_, Option<String>>(idx)? else {
        return Ok(Vec::new());
    };
    serde_json::from_str(&json).map_err(|err| conversion_error(idx, Type::Text, err.to_string()))
}

pub(crate) fn locations(
    conn: &Connection,
    message: MessageId,
) -> Result<Vec<MessageLocation>, StoreError> {
    let rows = conn
        .prepare_cached(MESSAGE_LOCATIONS)?
        .query_map([message.0], |r| {
            let role: Option<String> = r.get(4)?;
            let uid_validity: Option<u32> = r.get(3)?;
            Ok(MessageLocation {
                uid: r.get(0)?,
                mailbox_id: MailboxId(r.get(1)?),
                mailbox_name: r.get(2)?,
                uid_validity: uid_validity.map(UidValidity),
                role: MailboxRole::from_db(role.as_deref()),
                selectable: r.get(5)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// `text` cut to [`MAX_PREVIEW_CHARS`] characters, white space collapsed; `None` if empty.
pub(crate) fn clean_preview(text: &str) -> Option<String> {
    let mut out = String::new();
    let mut chars = 0;
    for word in text.split_whitespace() {
        let word: String = word.chars().filter(|c| !c.is_control()).collect();
        if word.is_empty() {
            continue;
        }
        let needed = word.chars().count() + usize::from(!out.is_empty());
        if chars + needed > MAX_PREVIEW_CHARS {
            if !out.is_empty() {
                out.push(' ');
                chars += 1;
            }
            out.extend(word.chars().take(MAX_PREVIEW_CHARS.saturating_sub(chars)));
            break;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(&word);
        chars += needed;
    }
    let out = out.trim_end().to_owned();
    (!out.is_empty()).then_some(out)
}

/// Writes the previews; unknown messages are skipped. Returns how many were written.
pub(crate) fn set_previews(
    conn: &Connection,
    previews: &[(MessageId, String)],
) -> Result<usize, StoreError> {
    let mut stmt = conn.prepare_cached(SET_PREVIEW)?;
    let mut written = 0;
    for (message, text) in previews {
        written += stmt
            .execute(params![message.0, clean_preview(text)])
            .map_err(StoreError::sqlite("set preview"))?;
    }
    Ok(written)
}

/// See [`crate::Store::prefetch_candidates`].
pub(crate) fn prefetch_candidates(
    conn: &Connection,
    account: AccountId,
    since: Option<i64>,
    before: Option<MessageId>,
    limit: u32,
) -> Result<Vec<PrefetchCandidate>, StoreError> {
    if limit == 0 || limit > MAX_PREFETCH_BATCH {
        return Err(StoreError::InvalidArgument(format!(
            "prefetch batch {limit} is outside 1..={MAX_PREFETCH_BATCH}"
        )));
    }
    let account_row = accounts::row_id(conn, account)?;
    let mut mailboxes = conn
        .prepare_cached(PREFETCH_MAILBOXES)?
        .query_map([account_row], |r| {
            let role: Option<String> = r.get(1)?;
            Ok((MailboxId(r.get(0)?), MailboxRole::from_db(role.as_deref())))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    // Inbox, Sent and Drafts first: a message found there keeps that location.
    let whole = |role: MailboxRole| {
        matches!(
            role,
            MailboxRole::Inbox | MailboxRole::Sent | MailboxRole::Drafts
        )
    };
    mailboxes.sort_by_key(|(id, role)| (!whole(*role), *id));
    let low = since.unwrap_or(0).max(0);
    let high = before.map_or(i64::MAX, |m| m.0);
    let mut found: HashMap<i64, PrefetchCandidate> = HashMap::new();
    for (mailbox, role) in mailboxes {
        let sql = if whole(role) {
            PREFETCH_IN_MAILBOX
        } else {
            PREFETCH_FLAGGED_IN_MAILBOX
        };
        let rows = conn
            .prepare_cached(sql)?
            .query_map(params![mailbox.0, low, high, i64::from(limit)], |r| {
                Ok(PrefetchCandidate {
                    message_id: MessageId(r.get(0)?),
                    mailbox_id: mailbox,
                    uid: r.get(1)?,
                    size: r.get(2)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        for candidate in rows {
            found.entry(candidate.message_id.0).or_insert(candidate);
        }
    }
    let mut candidates: Vec<PrefetchCandidate> = found.into_values().collect();
    candidates.sort_by_key(|c| std::cmp::Reverse(c.message_id));
    candidates.truncate(usize::try_from(limit).unwrap_or(usize::MAX));
    Ok(candidates)
}

/// The stored prefetch settings; defaults for missing or unreadable values (logged).
pub(crate) fn prefetch_settings(conn: &Connection) -> Result<PrefetchSettings, StoreError> {
    let mut settings = PrefetchSettings::default();
    let mut stmt =
        conn.prepare_cached("SELECT key, value FROM setting WHERE scope = ?1 AND key IN (?2, ?3)")?;
    let rows = stmt
        .query_map(
            params![crate::GLOBAL_SCOPE, PREFETCH_PERIOD_KEY, PREFETCH_SIZE_KEY],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
        )?
        .collect::<Result<Vec<_>, _>>()?;
    for (key, value) in rows {
        let ok = match key.as_str() {
            PREFETCH_PERIOD_KEY => serde_json::from_str::<PrefetchPeriod>(&value)
                .map(|p| settings.period = p)
                .is_ok(),
            PREFETCH_SIZE_KEY => serde_json::from_str::<u32>(&value)
                .ok()
                .filter(|&size| size <= PrefetchSettings::MAX_MESSAGE_SIZE)
                .map(|size| settings.max_message_size = size)
                .is_some(),
            _ => true,
        };
        if !ok {
            tracing::warn!(%key, %value, "unreadable prefetch setting; using the default");
        }
    }
    Ok(settings)
}

/// The setting rows of `settings` (key, JSON value).
pub(crate) fn prefetch_entries(
    settings: PrefetchSettings,
) -> Result<Vec<(String, String)>, StoreError> {
    let period = serde_json::to_string(&settings.period)
        .map_err(|err| StoreError::InvalidArgument(format!("prefetch period: {err}")))?;
    Ok(vec![
        (PREFETCH_PERIOD_KEY.to_owned(), period),
        (
            PREFETCH_SIZE_KEY.to_owned(),
            settings.max_message_size.to_string(),
        ),
    ])
}

pub(crate) fn sender_allowed(
    conn: &Connection,
    account: AccountId,
    sender: &str,
) -> Result<bool, StoreError> {
    Ok(conn
        .prepare_cached(SENDER_ALLOWED)?
        .query_row(params![account.to_string(), sender], |_| Ok(()))
        .optional()?
        .is_some())
}

pub(crate) fn allow_sender(
    conn: &Connection,
    account: AccountId,
    sender: &str,
) -> Result<bool, StoreError> {
    accounts::row_id(conn, account)?;
    let added = conn
        .prepare_cached(ALLOW_SENDER)?
        .execute(params![account.to_string(), sender, now_millis()])
        .map_err(StoreError::sqlite("allow sender"))?;
    Ok(added > 0)
}

pub(crate) fn revoke_sender(
    conn: &Connection,
    account: AccountId,
    sender: &str,
) -> Result<bool, StoreError> {
    let removed = conn
        .prepare_cached(REVOKE_SENDER)?
        .execute(params![account.to_string(), sender])
        .map_err(StoreError::sqlite("revoke sender"))?;
    Ok(removed > 0)
}

pub(crate) fn senders(conn: &Connection, account: AccountId) -> Result<Vec<String>, StoreError> {
    let rows = conn
        .prepare_cached(SENDERS)?
        .query_map([account.to_string()], |r| r.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn previews_are_collapsed_and_cut() {
        assert_eq!(
            clean_preview("  Hallo\r\n\r\n  Welt\t!  ").as_deref(),
            Some("Hallo Welt !")
        );
        assert_eq!(clean_preview(" \n\t "), None);
        assert_eq!(clean_preview("a\u{0}b \u{7} c").as_deref(), Some("ab c"));
        let long = "Wort ".repeat(100);
        let cut = clean_preview(&long).expect("preview");
        assert!(cut.chars().count() <= MAX_PREVIEW_CHARS, "{}", cut.len());
        assert!(cut.starts_with("Wort Wort"));
        let one_word = "ä".repeat(400);
        assert_eq!(
            clean_preview(&one_word).map(|p| p.chars().count()),
            Some(MAX_PREVIEW_CHARS)
        );
    }
}
