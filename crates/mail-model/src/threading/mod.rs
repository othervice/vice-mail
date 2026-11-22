//! Conversations (stage 7, decision 0013): which thread a message belongs to. Pure logic without
//! IO, per account; threads never span accounts.
//!
//! **By references.** A thread is the set of Message-IDs its messages carry or point to: their
//! own `Message-ID`, `In-Reply-To` and `References` (the first and the last 20, see
//! [`REFERENCES_TAIL`]). A new message joins the thread that already holds one of its ids. If
//! its ids are held by several threads (a missing message in between has arrived), they are
//! merged into the oldest (smallest [`ThreadId`]). Ids that point to messages not seen yet are
//! kept too, so a reply that arrives before its original, or two replies to an original that
//! never arrives, still meet. Cycles and self-references are harmless: this is a union of id
//! sets, not a tree. The grouping by references does not depend on the order of arrival.
//!
//! **A reused Message-ID** (bulk mailers send many mails with one id) is not used to link when
//! the existing message with that id has a different subject; the message then threads by its
//! references only. The first message seen with an id owns it, and replies naming the id join
//! that message's thread; a rebuild in date order makes the earlier message the owner.
//!
//! **By subject, carefully**, only for a message without any reference header (Outlook and some
//! mobile clients). It joins the thread of an existing message when all of these hold:
//! - same normalized subject ([`normalize_subject`]), specific enough ([`is_specific_subject`]:
//!   not `Hallo`, `Rechnung`, `Quick question`, …)
//! - one of the two is a reply (`Re:`, `AW:`, …) without references and not older than the other
//!   (up to an hour of clock skew); two originals with the same subject are separate
//!   conversations
//! - at most [`SUBJECT_WINDOW`] (7 days) apart
//! - a shared participant other than the account's own addresses ([`ThreadConfig`])
//! - neither is mailing-list or bulk mail (`List-Id`, `List-Unsubscribe`, `Precedence`,
//!   `Auto-Submitted`): lists share the list address and repeat subjects, and list software
//!   keeps the references anyway
//!
//! The closest such message in time wins (ties: the smaller thread id); its thread is the only
//! one the subject can add. An original without references that is already linked by its own id
//! (replies with references arrived first) still looks for such a reply outside its thread and
//! merges it, so the result does not depend on which of the two arrived first. What remains
//! order-dependent: a reply without references that arrives before every message near it in
//! time stays alone unless a reference-less original of that thread comes later; it is never
//! wrongly merged because of the order.
//!
//! The decision ([`assign_thread`]) reads existing state through [`ThreadLookup`] and returns a
//! [`ThreadAssignment`] that the caller applies; [`ThreadIndex`] is the in-memory implementation
//! (tests, rebuilds), the store implements the same three lookups in SQL.

mod memory;
mod subject;
#[cfg(test)]
mod tests;

use std::collections::BTreeSet;

pub use memory::ThreadIndex;
pub use subject::{NormalizedSubject, is_specific_subject, normalize_subject, subject_key};

use crate::{MessageHeader, MessageId, normalize_sender};

/// Largest distance in time for threading by subject: replies without references usually come
/// within days, and weekly mails with a fixed subject are 7 days apart and still need a reply
/// prefix and a shared participant.
pub const SUBJECT_WINDOW: i64 = 7 * 24 * 60 * 60;
/// A reply may be dated this much before the message it answers (wrong clocks).
pub const CLOCK_SKEW: i64 = 60 * 60;
/// Ids taken from `In-Reply-To`.
pub const MAX_IN_REPLY_TO: usize = 4;
/// Ids taken from the start of `References` (the thread root).
pub const REFERENCES_HEAD: usize = 1;
/// Ids taken from the end of `References` (the nearest ancestors). Clients trim long lists the
/// same way (first plus last), so a long chain links through these.
pub const REFERENCES_TAIL: usize = 20;
/// Longer Message-IDs are ignored.
pub const MAX_MESSAGE_ID_LEN: usize = 250;

/// Local id of a thread (`thread.id`). The store allocates it; [`ThreadIndex`] counts from 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ThreadId(pub i64);

/// What threading needs to know about one message. The store can rebuild it from its `message`
/// row (`message_id_hdr`, `in_reply_to`, `references_hdr`, addresses, list headers, `id`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThreadMessage {
    pub message: MessageId,
    /// `Message-ID`, with or without angle brackets.
    pub message_id: Option<String>,
    /// `In-Reply-To`: ids with or without brackets; an entry may hold several ids or stray text.
    pub in_reply_to: Vec<String>,
    /// `References`, oldest first, same forms as `in_reply_to`.
    pub references: Vec<String>,
    /// Decoded subject as sent.
    pub subject: String,
    /// Addresses from `From`, `To` and `Cc`.
    pub participants: Vec<String>,
    /// Sort date (Unix seconds).
    pub date: i64,
    /// Mailing-list or bulk mail; see [`is_bulk`].
    pub bulk: bool,
}

impl ThreadMessage {
    /// From the header data the adapter fetched; `message` is the local id the store gave it.
    pub fn from_header(message: MessageId, header: &MessageHeader) -> Self {
        Self {
            message,
            message_id: header.message_id.clone(),
            in_reply_to: header.in_reply_to.iter().cloned().collect(),
            references: header.references.clone(),
            subject: header.subject.clone(),
            participants: header
                .from
                .iter()
                .chain(&header.to)
                .chain(&header.cc)
                .map(|a| a.email.clone())
                .collect(),
            date: header.sort_date(),
            bulk: is_bulk(
                header.list_id.as_deref(),
                header.list_unsubscribe.as_deref(),
                header.precedence.as_deref(),
                header.auto_submitted.as_deref(),
            ),
        }
    }
}

/// Mailing-list or bulk mail: `List-Id` or `List-Unsubscribe` present, `Precedence: bulk`,
/// `list` or `junk`, or `Auto-Submitted` other than `no` (RFC 3834). Raw header values.
pub fn is_bulk(
    list_id: Option<&str>,
    list_unsubscribe: Option<&str>,
    precedence: Option<&str>,
    auto_submitted: Option<&str>,
) -> bool {
    let present = |v: Option<&str>| v.is_some_and(|v| !v.trim().is_empty());
    present(list_id)
        || present(list_unsubscribe)
        || precedence.is_some_and(|p| {
            matches!(
                p.trim().to_ascii_lowercase().as_str(),
                "bulk" | "list" | "junk"
            )
        })
        || auto_submitted.is_some_and(|a| {
            let token = a.split(';').next().unwrap_or_default().trim();
            !token.is_empty() && !token.eq_ignore_ascii_case("no")
        })
}

/// Per-account settings.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ThreadConfig {
    /// The account's own addresses (account address and identities), normalized. They are in
    /// almost every message, so they never count as a shared participant.
    pub own_addresses: BTreeSet<String>,
}

impl ThreadConfig {
    pub fn new<I, S>(own_addresses: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        Self {
            own_addresses: own_addresses
                .into_iter()
                .filter_map(|a| normalize_sender(a.as_ref()))
                .collect(),
        }
    }

    fn others(&self, participants: &[String]) -> BTreeSet<String> {
        participants
            .iter()
            .filter_map(|a| normalize_sender(a))
            .filter(|a| !self.own_addresses.contains(a))
            .collect()
    }
}

/// Read access to the threads of one account, as stored so far. The store answers with
/// `account_id = ?` in every query.
pub trait ThreadLookup {
    type Error;

    /// Threads that already hold any of `ids` (as own id or reference of one of their messages).
    /// Order and duplicates do not matter.
    fn threads_with_ids(&self, ids: &[&str]) -> Result<Vec<ThreadId>, Self::Error>;

    /// Subjects of the messages whose own Message-ID is `id` (empty if there is none).
    fn subjects_with_message_id(&self, id: &str) -> Result<Vec<String>, Self::Error>;

    /// Messages whose subject key ([`subject_key`]) is `key` and whose date is in
    /// `from..=until`, with their thread.
    fn messages_with_subject(
        &self,
        key: &str,
        from: i64,
        until: i64,
    ) -> Result<Vec<(ThreadId, ThreadMessage)>, Self::Error>;
}

/// Where a message goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ThreadTarget {
    Existing(ThreadId),
    /// A new thread; the caller allocates its id.
    New,
}

/// Why.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ThreadReason {
    /// Linked by Message-ID, In-Reply-To or References.
    References,
    /// No reference headers; same subject, shared participant, close in time.
    Subject,
    /// Nothing related yet.
    New,
}

/// The result of [`assign_thread`]. To apply it: create the thread for [`ThreadTarget::New`];
/// move the messages and ids of every `absorbed` thread to the target and delete those
/// threads; store `link_ids` for the target; set the message's thread.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThreadAssignment {
    pub target: ThreadTarget,
    /// Threads merged into the target, ascending.
    pub absorbed: Vec<ThreadId>,
    /// Ids (without brackets) to register for the target: the message's own id (unless reused)
    /// and its references, bounded.
    pub link_ids: Vec<String>,
    /// The message's subject key, for the store's `subject_key` column (subject lookups).
    pub subject_key: Option<String>,
    pub reason: ThreadReason,
    /// The message's own Message-ID belongs to an existing message with a different subject and
    /// was not used to link (worth a debug log line).
    pub reused_message_id: bool,
}

/// Decides the thread of `message`. Deterministic: the same state and message give the same
/// answer. Errors only come from `lookup`.
pub fn assign_thread<L: ThreadLookup + ?Sized>(
    lookup: &L,
    config: &ThreadConfig,
    message: &ThreadMessage,
) -> Result<ThreadAssignment, L::Error> {
    let subject = normalize_subject(&message.subject);
    let subject_key = (!subject.key.is_empty()).then(|| subject.key.clone());
    let own = message.message_id.as_deref().and_then(own_id);
    let references = reference_ids(message, own);

    let mut reused_message_id = false;
    let own_link = match own {
        Some(id) => {
            let subjects = lookup.subjects_with_message_id(id)?;
            let same = subjects.is_empty()
                || subjects
                    .iter()
                    .any(|s| normalize_subject(s).key == subject.key);
            reused_message_id = !same;
            same.then_some(id)
        }
        None => None,
    };
    let link_ids: Vec<&str> = own_link
        .into_iter()
        .chain(references.iter().copied())
        .collect();

    let mut threads = if link_ids.is_empty() {
        Vec::new()
    } else {
        lookup.threads_with_ids(&link_ids)?
    };
    threads.sort_unstable();
    threads.dedup();
    // Without reference headers the subject may link one more thread: the message's only one,
    // or, for an original already linked by its own id, the thread of a reply without references
    // that arrived earlier (the same result as if the reply had come later).
    let by_subject = if references.is_empty() {
        subject_match(lookup, config, message, &subject, &threads)?
    } else {
        None
    };
    let reason = match (threads.is_empty(), by_subject) {
        (false, _) => ThreadReason::References,
        (true, Some(_)) => ThreadReason::Subject,
        (true, None) => ThreadReason::New,
    };
    threads.extend(by_subject);
    threads.sort_unstable();
    let (target, absorbed) = match threads.split_first() {
        Some((&first, rest)) => (ThreadTarget::Existing(first), rest.to_vec()),
        None => (ThreadTarget::New, Vec::new()),
    };
    Ok(ThreadAssignment {
        target,
        absorbed,
        link_ids: link_ids.iter().map(|id| (*id).to_owned()).collect(),
        subject_key,
        reason,
        reused_message_id,
    })
}

/// The thread of the closest message that may be threaded with `message` by subject, outside
/// the threads in `linked`.
fn subject_match<L: ThreadLookup + ?Sized>(
    lookup: &L,
    config: &ThreadConfig,
    message: &ThreadMessage,
    subject: &NormalizedSubject,
    linked: &[ThreadId],
) -> Result<Option<ThreadId>, L::Error> {
    if message.bulk || !is_specific_subject(&subject.key) {
        return Ok(None);
    }
    let mine = config.others(&message.participants);
    if mine.is_empty() {
        return Ok(None);
    }
    let from = message.date.saturating_sub(SUBJECT_WINDOW);
    let until = message.date.saturating_add(SUBJECT_WINDOW);
    let mut best: Option<(i64, ThreadId)> = None;
    for (thread, candidate) in lookup.messages_with_subject(&subject.key, from, until)? {
        if candidate.message == message.message || candidate.bulk || linked.contains(&thread) {
            continue;
        }
        let theirs = normalize_subject(&candidate.subject);
        if theirs.key != subject.key {
            continue;
        }
        // One of the two is a reply without references, not older than the other. (The message
        // itself has none; a candidate reply with references would not have been threaded by
        // subject had it arrived after the message, so it is not used now either.)
        let reply_fits = (subject.reply
            && candidate.date <= message.date.saturating_add(CLOCK_SKEW))
            || (theirs.reply
                && message.date <= candidate.date.saturating_add(CLOCK_SKEW)
                && !has_references(&candidate));
        let distance = candidate.date.abs_diff(message.date);
        if !reply_fits || distance > SUBJECT_WINDOW.unsigned_abs() {
            continue;
        }
        if mine.is_disjoint(&config.others(&candidate.participants)) {
            continue;
        }
        let rank = (i64::try_from(distance).unwrap_or(i64::MAX), thread);
        if best.is_none_or(|b| rank < b) {
            best = Some(rank);
        }
    }
    Ok(best.map(|(_, thread)| thread))
}

fn has_references(message: &ThreadMessage) -> bool {
    !reference_ids(message, message.message_id.as_deref().and_then(own_id)).is_empty()
}

/// A message's own id without brackets, if usable.
fn own_id(raw: &str) -> Option<&str> {
    let id = raw.trim();
    let id = id
        .strip_prefix('<')
        .and_then(|i| i.strip_suffix('>'))
        .unwrap_or(id)
        .trim();
    valid_id(id).then_some(id)
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_MESSAGE_ID_LEN
        && !id
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || matches!(c, '<' | '>'))
}

/// Ids in one header entry: everything in `<…>`, or, without brackets, every word with an `@`
/// (so stray text like `your message of Monday` never becomes an id).
fn ids_in(entry: &str) -> Vec<&str> {
    if entry.contains('<') {
        entry
            .split('<')
            .skip(1)
            .filter_map(|part| part.split_once('>').map(|(id, _)| id.trim()))
            .filter(|id| valid_id(id))
            .collect()
    } else {
        entry
            .split_whitespace()
            .filter(|id| id.contains('@') && valid_id(id))
            .collect()
    }
}

/// In-Reply-To ids, then the nearest References, then the root; unique, without the own id.
fn reference_ids<'a>(message: &'a ThreadMessage, own: Option<&str>) -> Vec<&'a str> {
    let references: Vec<&str> = message.references.iter().flat_map(|r| ids_in(r)).collect();
    let tail_start = references
        .len()
        .saturating_sub(REFERENCES_TAIL)
        .max(REFERENCES_HEAD.min(references.len()));
    let candidates = message
        .in_reply_to
        .iter()
        .flat_map(|r| ids_in(r))
        .take(MAX_IN_REPLY_TO)
        .chain(references[tail_start..].iter().rev().copied())
        .chain(references.iter().take(REFERENCES_HEAD).copied());
    let mut out: Vec<&str> = Vec::new();
    for id in candidates {
        if Some(id) != own && !out.contains(&id) {
            out.push(id);
        }
    }
    out
}
