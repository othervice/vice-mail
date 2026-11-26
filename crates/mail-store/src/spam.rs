//! Stage 22 (decision 0077): the learning spam filter's data (migration 0031), class Device.
//!
//! The store keeps counts and records; tokenising and scoring are `mail_content`'s, deciding is
//! the engine's. A token is its 64-bit hash, stored as a signed integer. Learning a message
//! records the exact tokens it taught, so unlearning (moved from spam to ham, "Undo") takes back
//! precisely those, whatever the message's body looks like by then.

use mail_model::{
    AccountId, Address, BatchId, MailboxId, MailboxRole, MessageId, SpamFilterMark, SpamVerdict,
};
use rusqlite::{Connection, OptionalExtension, params};

use crate::accounts::account_id_at;
use crate::{Store, StoreError};

/// The model is pruned (tokens seen in one mail only) once it holds more tokens than this.
pub const MAX_SPAM_TOKENS_STORED: u32 = 400_000;

/// Most references of a new message checked for a reply to the user's own mail.
const MAX_REFERENCES_CHECKED: usize = 10;

/// The class a message was learned as.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SpamClass {
    Spam,
    Ham,
}

impl SpamClass {
    fn as_str(self) -> &'static str {
        match self {
            Self::Spam => "spam",
            Self::Ham => "ham",
        }
    }

    fn parse(text: &str) -> Option<Self> {
        match text {
            "spam" => Some(Self::Spam),
            "ham" => Some(Self::Ham),
            _ => None,
        }
    }

    /// `(spam, ham)` deltas of one mail of this class.
    fn deltas(self, sign: i64) -> (i64, i64) {
        match self {
            Self::Spam => (sign, 0),
            Self::Ham => (0, sign),
        }
    }
}

/// What the model holds.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SpamModelSize {
    /// Mails learned as spam and as ham.
    pub spam: u32,
    pub ham: u32,
    /// Distinct tokens.
    pub tokens: u32,
}

/// Learned counts as `(spam, ham)`: of all mails (`totals`) and of each token asked for
/// (`tokens`, in the order asked; zero for a token never learned).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SpamLookup {
    pub totals: (u32, u32),
    pub tokens: Vec<(u32, u32)>,
}

/// What learning one message changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Learned {
    /// The class it had been learned as before (unlearned first).
    pub previous: Option<SpamClass>,
}

/// What the store knows about a new message's sender, for the rules that keep mail from ever
/// counting as spam (the engine compares the addresses).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SpamSenderFacts {
    pub account: Option<AccountId>,
    /// The first `From` address as written.
    pub from: Option<String>,
    /// The normalised sender address (set when the message was categorised).
    pub sender_key: Option<String>,
    /// The sender, domain or list is marked important (sender rules).
    pub important: bool,
    /// The sender is a contact the user wrote to, named, noted, merged or put in a group.
    pub known_contact: bool,
    /// Senders of the messages this one replies to or refers to (same account).
    pub referenced_senders: Vec<String>,
}

/// The stored header data of a message (for tokens when no body is stored).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StoredSpamFields {
    pub from: Option<Address>,
    pub reply_to: Vec<Address>,
    pub recipients: usize,
    pub subject: String,
    pub list_id: Option<String>,
    pub preview: Option<String>,
}

fn signed(hash: u64) -> i64 {
    hash.cast_signed()
}

fn count(value: i64) -> u32 {
    u32::try_from(value.max(0)).unwrap_or(u32::MAX)
}

// ---------------------------------------------------------------------------------------------
// The model

pub(crate) fn size(conn: &Connection) -> Result<SpamModelSize, StoreError> {
    let (spam, ham) = conn
        .prepare_cached("SELECT spam, ham FROM spam_model WHERE id = 1")?
        .query_row([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))
        .optional()?
        .unwrap_or_default();
    let tokens: i64 = conn
        .prepare_cached("SELECT count(*) FROM spam_token")?
        .query_row([], |r| r.get(0))?;
    Ok(SpamModelSize {
        spam: count(spam),
        ham: count(ham),
        tokens: count(tokens),
    })
}

/// See [`SpamLookup`].
pub(crate) fn counts(conn: &Connection, hashes: &[u64]) -> Result<SpamLookup, StoreError> {
    let totals = conn
        .prepare_cached("SELECT spam, ham FROM spam_model WHERE id = 1")?
        .query_row([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))
        .optional()?
        .unwrap_or_default();
    let mut stmt = conn.prepare_cached("SELECT spam, ham FROM spam_token WHERE hash = ?1")?;
    let mut out = Vec::with_capacity(hashes.len());
    for hash in hashes {
        let row = stmt
            .query_row([signed(*hash)], |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?))
            })
            .optional()?
            .unwrap_or_default();
        out.push((count(row.0), count(row.1)));
    }
    Ok(SpamLookup {
        totals: (count(totals.0), count(totals.1)),
        tokens: out,
    })
}

fn add_totals(conn: &Connection, class: SpamClass, sign: i64) -> Result<(), StoreError> {
    let (spam, ham) = class.deltas(sign);
    conn.prepare_cached(
        "INSERT INTO spam_model (id, spam, ham) VALUES (1, max(?1, 0), max(?2, 0))
         ON CONFLICT (id) DO UPDATE SET spam = max(spam + ?1, 0), ham = max(ham + ?2, 0)",
    )?
    .execute(params![spam, ham])?;
    Ok(())
}

fn add_tokens(
    conn: &Connection,
    class: SpamClass,
    hashes: &[u64],
    sign: i64,
) -> Result<(), StoreError> {
    let (spam, ham) = class.deltas(sign);
    if sign > 0 {
        let mut stmt = conn.prepare_cached(
            "INSERT INTO spam_token (hash, spam, ham) VALUES (?1, ?2, ?3)
             ON CONFLICT (hash) DO UPDATE SET spam = spam + ?2, ham = ham + ?3",
        )?;
        for hash in hashes {
            stmt.execute(params![signed(*hash), spam, ham])?;
        }
    } else {
        let mut lower = conn.prepare_cached(
            "UPDATE spam_token SET spam = max(spam + ?2, 0), ham = max(ham + ?3, 0)
             WHERE hash = ?1",
        )?;
        let mut drop =
            conn.prepare_cached("DELETE FROM spam_token WHERE hash = ?1 AND spam = 0 AND ham = 0")?;
        for hash in hashes {
            lower.execute(params![signed(*hash), spam, ham])?;
            drop.execute([signed(*hash)])?;
        }
    }
    Ok(())
}

/// Takes back what `message` taught; its class, if it was learned.
pub(crate) fn unlearn(
    conn: &Connection,
    message: MessageId,
) -> Result<Option<SpamClass>, StoreError> {
    let record: Option<(String, Vec<u8>)> = conn
        .prepare_cached("SELECT class, tokens FROM spam_trained WHERE message_id = ?1")?
        .query_row([message.0], |r| Ok((r.get(0)?, r.get(1)?)))
        .optional()?;
    let Some((class, tokens)) = record else {
        return Ok(None);
    };
    let Some(class) = SpamClass::parse(&class) else {
        return Ok(None);
    };
    let hashes = hashes_of(&tokens);
    add_tokens(conn, class, &hashes, -1)?;
    add_totals(conn, class, -1)?;
    conn.prepare_cached("DELETE FROM spam_trained WHERE message_id = ?1")?
        .execute([message.0])?;
    Ok(Some(class))
}

fn hashes_of(bytes: &[u8]) -> Vec<u64> {
    bytes
        .as_chunks::<8>()
        .0
        .iter()
        .map(|chunk| u64::from_le_bytes(*chunk))
        .collect()
}

/// Learns `message` as `class` with these tokens (each once), replacing what it taught before.
pub(crate) fn learn(
    conn: &Connection,
    message: MessageId,
    class: SpamClass,
    hashes: &[u64],
    batch: Option<BatchId>,
    now: i64,
) -> Result<Learned, StoreError> {
    let previous = unlearn(conn, message)?;
    let mut unique = hashes.to_vec();
    unique.sort_unstable();
    unique.dedup();
    add_tokens(conn, class, &unique, 1)?;
    add_totals(conn, class, 1)?;
    let bytes: Vec<u8> = unique.iter().flat_map(|h| h.to_le_bytes()).collect();
    conn.prepare_cached(
        "INSERT INTO spam_trained (message_id, class, tokens, batch, trained_at)
         VALUES (?1, ?2, ?3, ?4, ?5)",
    )?
    .execute(params![
        message.0,
        class.as_str(),
        bytes,
        batch.map(|b| b.0),
        now
    ])?;
    Ok(Learned { previous })
}

/// The class `message` was learned as.
pub(crate) fn learned_class(
    conn: &Connection,
    message: MessageId,
) -> Result<Option<SpamClass>, StoreError> {
    let class: Option<String> = conn
        .prepare_cached("SELECT class FROM spam_trained WHERE message_id = ?1")?
        .query_row([message.0], |r| r.get(0))
        .optional()?;
    Ok(class.as_deref().and_then(SpamClass::parse))
}

/// Unlearns every message the batch taught; returns them.
pub(crate) fn unlearn_batch(
    conn: &Connection,
    batch: BatchId,
) -> Result<Vec<MessageId>, StoreError> {
    let messages: Vec<MessageId> = conn
        .prepare_cached("SELECT message_id FROM spam_trained WHERE batch = ?1")?
        .query_map([batch.0], |r| Ok(MessageId(r.get(0)?)))?
        .collect::<Result<_, _>>()?;
    for message in &messages {
        unlearn(conn, *message)?;
    }
    Ok(messages)
}

/// Drops the tokens only one learned mail had, once the model holds more than `max` tokens.
/// Returns how many went.
pub(crate) fn prune(conn: &Connection, max: u32) -> Result<usize, StoreError> {
    let tokens: i64 = conn
        .prepare_cached("SELECT count(*) FROM spam_token")?
        .query_row([], |r| r.get(0))?;
    if tokens <= i64::from(max) {
        return Ok(0);
    }
    Ok(conn
        .prepare_cached("DELETE FROM spam_token WHERE spam + ham <= 1")?
        .execute([])?)
}

/// Forgets everything learned and every verdict.
pub(crate) fn reset(conn: &Connection) -> Result<(), StoreError> {
    conn.execute_batch(
        "DELETE FROM spam_token; DELETE FROM spam_model; DELETE FROM spam_trained;
         DELETE FROM spam_verdict;",
    )?;
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Verdicts

pub(crate) fn set_verdict(
    conn: &Connection,
    message: MessageId,
    mark: SpamFilterMark,
    score: f64,
    batch: Option<BatchId>,
    now: i64,
) -> Result<(), StoreError> {
    let mark = match mark {
        SpamFilterMark::Moved => "moved",
        SpamFilterMark::Suspected => "suspected",
    };
    conn.prepare_cached(
        "INSERT INTO spam_verdict (message_id, mark, score, batch, at) VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT (message_id) DO UPDATE SET mark = ?2, score = ?3, batch = ?4, at = ?5",
    )?
    .execute(params![
        message.0,
        mark,
        score.clamp(0.0, 1.0),
        batch.map(|b| b.0),
        now
    ])?;
    Ok(())
}

pub(crate) fn mark_of(text: &str) -> Option<SpamFilterMark> {
    match text {
        "moved" => Some(SpamFilterMark::Moved),
        "suspected" => Some(SpamFilterMark::Suspected),
        _ => None,
    }
}

/// Removes the verdict on `message`; the mark it had.
pub(crate) fn clear_verdict(
    conn: &Connection,
    message: MessageId,
) -> Result<Option<SpamFilterMark>, StoreError> {
    let mark: Option<String> = conn
        .prepare_cached("DELETE FROM spam_verdict WHERE message_id = ?1 RETURNING mark")?
        .query_row([message.0], |r| r.get(0))
        .optional()?;
    Ok(mark.as_deref().and_then(mark_of))
}

/// The verdict on `message`, with whether "Move to spam" can run for it.
pub(crate) fn verdict(
    conn: &Connection,
    message: MessageId,
) -> Result<Option<SpamVerdict>, StoreError> {
    Ok(conn
        .prepare_cached(
            "SELECT v.mark, v.score, v.at,
                    a.writes_enabled = 1
                    AND EXISTS (SELECT 1 FROM mailbox b WHERE b.account_id = a.id AND b.role = 'junk'),
                    (SELECT l.mailbox_id FROM message_location l
                     CROSS JOIN mailbox b ON b.id = l.mailbox_id
                     WHERE l.message_id = v.message_id
                     ORDER BY CASE b.role WHEN (CASE v.mark WHEN 'moved' THEN 'junk' ELSE 'inbox' END)
                                          THEN 0 ELSE 1 END, l.mailbox_id
                     LIMIT 1)
             FROM spam_verdict v
             CROSS JOIN message m ON m.id = v.message_id
             CROSS JOIN account a ON a.id = m.account_id
             WHERE v.message_id = ?1",
        )?
        .query_row([message.0], |r| {
            let mark: String = r.get(0)?;
            let (score, at, can_move) = (r.get(1)?, r.get(2)?, r.get(3)?);
            let mailbox_id = r.get::<_, Option<i64>>(4)?.map(MailboxId);
            Ok(mark_of(&mark).map(|mark| SpamVerdict {
                message_id: message,
                mailbox_id,
                mark,
                score,
                at,
                can_move,
            }))
        })
        .optional()?
        .flatten())
}

/// The messages the filter moved in this batch.
pub(crate) fn moved_in_batch(
    conn: &Connection,
    batch: BatchId,
) -> Result<Vec<MessageId>, StoreError> {
    Ok(conn
        .prepare_cached("SELECT message_id FROM spam_verdict WHERE batch = ?1 AND mark = 'moved'")?
        .query_map([batch.0], |r| Ok(MessageId(r.get(0)?)))?
        .collect::<Result<_, _>>()?)
}

// ---------------------------------------------------------------------------------------------
// What the engine reads

pub(crate) fn sender_facts(
    conn: &Connection,
    message: MessageId,
) -> Result<Option<SpamSenderFacts>, StoreError> {
    let row = conn
        .prepare_cached(
            "SELECT (SELECT a.uuid FROM account a WHERE a.id = m.account_id), m.account_id,
                    m.from_addr, m.sender_key, m.important, m.in_reply_to, m.references_hdr
             FROM message m WHERE m.id = ?1",
        )?
        .query_row([message.0], |r| {
            Ok((
                account_id_at(r, 0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, Option<String>>(3)?,
                r.get::<_, bool>(4)?,
                r.get::<_, Option<String>>(5)?,
                r.get::<_, Option<String>>(6)?,
            ))
        })
        .optional()?;
    let Some((account, account_row, from, sender_key, important, in_reply_to, references)) = row
    else {
        return Ok(None);
    };
    let known_contact = match &sender_key {
        Some(key) => conn
            .prepare_cached(
                "SELECT EXISTS (SELECT 1 FROM contact c WHERE c.address_key = ?1
                                AND (c.sent > 0 OR c.user_name IS NOT NULL OR c.note IS NOT NULL
                                     OR c.contact_group IS NOT NULL))
                     OR EXISTS (SELECT 1 FROM contact_list_member l WHERE l.address_key = ?1)",
            )?
            .query_row([key], |r| r.get(0))?,
        None => false,
    };
    let mut referenced_senders = Vec::new();
    let ids = in_reply_to.iter().map(String::as_str).chain(
        references
            .as_deref()
            .unwrap_or_default()
            .split_whitespace()
            .rev(),
    );
    let mut stmt = conn.prepare_cached(
        "SELECT from_addr FROM message WHERE account_id = ?1 AND message_id_hdr = ?2 LIMIT 3",
    )?;
    for id in ids.take(MAX_REFERENCES_CHECKED) {
        let rows = stmt.query_map(params![account_row, id], |r| r.get::<_, Option<String>>(0))?;
        for from in rows {
            if let Some(from) = from? {
                referenced_senders.push(from);
            }
        }
    }
    Ok(Some(SpamSenderFacts {
        account: Some(account),
        from,
        sender_key,
        important,
        known_contact,
        referenced_senders,
    }))
}

pub(crate) fn fields(
    conn: &Connection,
    message: MessageId,
) -> Result<Option<StoredSpamFields>, StoreError> {
    Ok(conn
        .prepare_cached(
            "SELECT from_name, from_addr, reply_to_addrs, to_addrs, cc_addrs, subject, list_key,
                    preview
             FROM message WHERE id = ?1",
        )?
        .query_row([message.0], |r| {
            let from_name: Option<String> = r.get(0)?;
            let from_addr: Option<String> = r.get(1)?;
            let to = crate::messages::addresses_at(r, 3)?;
            let cc = crate::messages::addresses_at(r, 4)?;
            Ok(StoredSpamFields {
                from: from_addr.map(|email| Address {
                    name: from_name,
                    email,
                }),
                reply_to: crate::messages::addresses_at(r, 2)?,
                recipients: to.len() + cc.len(),
                subject: r.get(5)?,
                list_id: r.get(6)?,
                preview: r.get(7)?,
            })
        })
        .optional()?)
}

/// Messages to learn from: the newest of the accounts' junk folders (`Spam`) or of their
/// inboxes (`Ham`: replied to first, then read), not learned yet and without a verdict;
/// at most `limit`, newest first across the accounts.
pub(crate) fn learning_candidates(
    conn: &Connection,
    accounts: &[AccountId],
    class: SpamClass,
    limit: u32,
) -> Result<Vec<MessageId>, StoreError> {
    let role = match class {
        SpamClass::Spam => MailboxRole::Junk,
        SpamClass::Ham => MailboxRole::Inbox,
    }
    .as_db();
    let mut mailboxes = conn.prepare_cached(
        "SELECT b.id FROM mailbox b CROSS JOIN account a ON a.id = b.account_id
         WHERE a.uuid = ?1 AND b.role = ?2",
    )?;
    // The newest locations of one mailbox (primary key backwards), filtered per row.
    let mut spam = conn.prepare_cached(
        "SELECT l.message_id FROM message_location l
         WHERE l.mailbox_id = ?1
           AND NOT EXISTS (SELECT 1 FROM spam_trained t WHERE t.message_id = l.message_id)
         ORDER BY l.message_id DESC LIMIT ?2",
    )?;
    let mut replied = conn.prepare_cached(
        "SELECT l.message_id FROM message_location l
         WHERE l.mailbox_id = ?1 AND l.answered = 1
           AND NOT EXISTS (SELECT 1 FROM spam_trained t WHERE t.message_id = l.message_id)
           AND NOT EXISTS (SELECT 1 FROM spam_verdict v WHERE v.message_id = l.message_id)
         ORDER BY l.message_id DESC LIMIT ?2",
    )?;
    let mut read = conn.prepare_cached(
        "SELECT l.message_id FROM message_location l
         WHERE l.mailbox_id = ?1 AND l.seen = 1 AND l.answered = 0
           AND NOT EXISTS (SELECT 1 FROM spam_trained t WHERE t.message_id = l.message_id)
           AND NOT EXISTS (SELECT 1 FROM spam_verdict v WHERE v.message_id = l.message_id)
         ORDER BY l.message_id DESC LIMIT ?2",
    )?;
    let mut first = Vec::new();
    let mut second = Vec::new();
    for account in accounts {
        let boxes: Vec<i64> = mailboxes
            .query_map(params![account.to_string(), role], |r| r.get(0))?
            .collect::<Result<_, _>>()?;
        for mailbox in boxes {
            let ids =
                |stmt: &mut rusqlite::CachedStatement<'_>| -> Result<Vec<MessageId>, StoreError> {
                    Ok(stmt
                        .query_map(params![mailbox, limit], |r| Ok(MessageId(r.get(0)?)))?
                        .collect::<Result<_, _>>()?)
                };
            match class {
                SpamClass::Spam => first.extend(ids(&mut spam)?),
                SpamClass::Ham => {
                    first.extend(ids(&mut replied)?);
                    second.extend(ids(&mut read)?);
                }
            }
        }
    }
    let limit = usize::try_from(limit).unwrap_or(usize::MAX);
    first.sort_unstable_by(|a, b| b.cmp(a));
    first.dedup();
    first.truncate(limit);
    if first.len() < limit {
        second.sort_unstable_by(|a, b| b.cmp(a));
        second.dedup();
        for id in second {
            if first.len() >= limit {
                break;
            }
            if !first.contains(&id) {
                first.push(id);
            }
        }
    }
    Ok(first)
}

// ---------------------------------------------------------------------------------------------
// Store API

impl Store {
    /// How many mails were learned per class, and how many tokens the model holds.
    pub async fn spam_model_size(&self) -> Result<SpamModelSize, StoreError> {
        self.read(size).await
    }

    /// Mails learned per class and the learned counts of `hashes` (same order).
    pub async fn spam_counts(&self, hashes: Vec<u64>) -> Result<SpamLookup, StoreError> {
        self.read(move |c| counts(c, &hashes)).await
    }

    /// Learns messages, each with its tokens, in one transaction; prunes the model when it grew
    /// past [`MAX_SPAM_TOKENS_STORED`]. Messages that are gone are skipped. Returns per message
    /// what changed (`None`: skipped).
    pub async fn learn_spam(
        &self,
        items: Vec<(MessageId, Vec<u64>)>,
        class: SpamClass,
        batch: Option<BatchId>,
        now: i64,
    ) -> Result<Vec<Option<Learned>>, StoreError> {
        self.write(move |tx| {
            let mut out = Vec::with_capacity(items.len());
            let mut exists =
                tx.prepare_cached("SELECT EXISTS (SELECT 1 FROM message WHERE id = ?1)")?;
            for (message, hashes) in &items {
                let present: bool = exists.query_row([message.0], |r| r.get(0))?;
                if !present {
                    out.push(None);
                    continue;
                }
                out.push(Some(learn(tx, *message, class, hashes, batch, now)?));
            }
            drop(exists);
            let pruned = prune(tx, MAX_SPAM_TOKENS_STORED)?;
            if pruned > 0 {
                tracing::info!(pruned, "spam filter: rare tokens dropped");
            }
            Ok(out)
        })
        .await
    }

    /// Unlearns messages; the classes they had been learned as.
    pub async fn unlearn_spam(
        &self,
        messages: Vec<MessageId>,
    ) -> Result<Vec<Option<SpamClass>>, StoreError> {
        self.write(move |tx| {
            messages
                .iter()
                .map(|m| unlearn(tx, *m))
                .collect::<Result<Vec<_>, _>>()
        })
        .await
    }

    /// Unlearns what the batch taught (the user's "Spam"/"Not spam" undone).
    pub async fn unlearn_spam_batch(&self, batch: BatchId) -> Result<Vec<MessageId>, StoreError> {
        self.write(move |tx| unlearn_batch(tx, batch)).await
    }

    pub async fn spam_learned_class(
        &self,
        message: MessageId,
    ) -> Result<Option<SpamClass>, StoreError> {
        self.read(move |c| learned_class(c, message)).await
    }

    /// Forgets everything the filter learned and every verdict.
    pub async fn reset_spam_filter(&self) -> Result<(), StoreError> {
        self.write(|tx| reset(tx)).await
    }

    /// Records the filter's verdict on messages (one batch for a move).
    pub async fn set_spam_verdicts(
        &self,
        verdicts: Vec<(MessageId, f64)>,
        mark: SpamFilterMark,
        batch: Option<BatchId>,
        now: i64,
    ) -> Result<(), StoreError> {
        self.write(move |tx| {
            for (message, score) in verdicts {
                set_verdict(tx, message, mark, score, batch, now)?;
            }
            Ok(())
        })
        .await
    }

    /// Removes the verdicts on messages; the marks they had.
    pub async fn clear_spam_verdicts(
        &self,
        messages: Vec<MessageId>,
    ) -> Result<Vec<Option<SpamFilterMark>>, StoreError> {
        self.write(move |tx| {
            messages
                .iter()
                .map(|m| clear_verdict(tx, *m))
                .collect::<Result<Vec<_>, _>>()
        })
        .await
    }

    pub async fn spam_verdict(
        &self,
        message: MessageId,
    ) -> Result<Option<SpamVerdict>, StoreError> {
        self.read(move |c| verdict(c, message)).await
    }

    /// The messages the filter moved in this batch.
    pub async fn spam_moved_in_batch(&self, batch: BatchId) -> Result<Vec<MessageId>, StoreError> {
        self.read(move |c| moved_in_batch(c, batch)).await
    }

    pub async fn spam_sender_facts(
        &self,
        message: MessageId,
    ) -> Result<Option<SpamSenderFacts>, StoreError> {
        self.read(move |c| sender_facts(c, message)).await
    }

    pub async fn spam_fields(
        &self,
        message: MessageId,
    ) -> Result<Option<StoredSpamFields>, StoreError> {
        self.read(move |c| fields(c, message)).await
    }

    /// See [`learning_candidates`].
    pub async fn spam_learning_candidates(
        &self,
        accounts: Vec<AccountId>,
        class: SpamClass,
        limit: u32,
    ) -> Result<Vec<MessageId>, StoreError> {
        self.read(move |c| learning_candidates(c, &accounts, class, limit))
            .await
    }
}
