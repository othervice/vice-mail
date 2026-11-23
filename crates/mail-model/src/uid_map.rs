//! Stage 14, block D (decision 0041): UIDs for servers that have none.
//!
//! The engine thinks in IMAP terms: a mailbox has a UIDVALIDITY, its messages have UIDs that
//! only grow, and a mod-sequence says what changed. JMAP has string ids per message (the same
//! id in every mailbox) and state strings per account. The JMAP adapter therefore numbers the
//! messages of each mailbox itself and keeps that numbering here, so it survives a restart:
//! without it every start would look like a new UIDVALIDITY and rebuild every mailbox.
//!
//! The adapter keeps the map in memory and writes every change through a [`UidMapStore`],
//! which the engine implements over its database (the adapter knows no database). Class
//! "Cache": the map can be rebuilt from the server, at the price of one rebuild per mailbox.

use std::future::Future;
use std::pin::Pin;

use crate::{AccountId, BackendError};

/// One message's number in one mailbox.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct UidMapEntry {
    pub uid: u32,
    /// The server's id of the message (JMAP `Email` id).
    pub remote_id: String,
    /// The adapter's change counter when this entry last changed on the server (its keywords
    /// or its mailboxes); answers `fetch_flags` with `changed_since`.
    pub modseq: u64,
}

/// The numbering state of one mailbox.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct UidMapMailbox {
    /// The server's id of the mailbox (JMAP `Mailbox` id).
    pub mailbox: String,
    pub uid_validity: u32,
    /// The UID the next message added to this mailbox gets.
    pub uid_next: u32,
    /// Whether every message of the mailbox has a number (the first full listing is done);
    /// until then only the listing adds entries, not the change log.
    pub complete: bool,
}

/// Everything stored for one account.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UidMapSnapshot {
    /// The server-side account the map belongs to (JMAP `accountId`); a map of another one is
    /// discarded.
    pub remote_account: String,
    /// The server's change state the map reflects (JMAP `Email` state), `None` before the
    /// first look.
    pub state: Option<String>,
    /// The change counter (the emulated HIGHESTMODSEQ of every mailbox).
    pub modseq: u64,
    pub mailboxes: Vec<(UidMapMailbox, Vec<UidMapEntry>)>,
}

/// One write-through of the adapter: applied in one transaction, in this order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UidMapChanges {
    /// Drop everything stored for the account first (another server account, or a rebuild).
    pub reset: bool,
    pub remote_account: String,
    pub state: Option<String>,
    pub modseq: u64,
    /// Mailboxes no longer on the server (with their entries).
    pub removed_mailboxes: Vec<String>,
    /// New or changed mailbox states.
    pub mailboxes: Vec<UidMapMailbox>,
    /// Entries gone from a mailbox: (mailbox, UID).
    pub removed: Vec<(String, u32)>,
    /// New entries and entries with a new modseq: (mailbox, entry).
    pub upserts: Vec<(String, UidMapEntry)>,
}

impl UidMapChanges {
    /// Whether there is anything besides the state to write.
    pub fn is_empty(&self) -> bool {
        !self.reset
            && self.removed_mailboxes.is_empty()
            && self.mailboxes.is_empty()
            && self.removed.is_empty()
            && self.upserts.is_empty()
    }
}

/// A boxed future of a [`UidMapStore`] (the trait is used as `dyn`).
pub type UidMapFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, BackendError>> + Send + 'a>>;

/// Where an adapter keeps its UID map ([`crate::MailConnector::attach_uid_map_store`]).
/// Failures are [`BackendError::InvalidState`] with the store's message.
pub trait UidMapStore: Send + Sync + 'static {
    /// The stored map of `account`, `None` if there is none.
    fn load_uid_map(&self, account: AccountId) -> UidMapFuture<'_, Option<UidMapSnapshot>>;

    /// Applies `changes` in one transaction.
    fn save_uid_map(&self, account: AccountId, changes: UidMapChanges) -> UidMapFuture<'_, ()>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_changes() {
        let mut changes = UidMapChanges {
            state: Some("s1".into()),
            modseq: 3,
            ..UidMapChanges::default()
        };
        assert!(
            changes.is_empty(),
            "a new state alone is no change of the map"
        );
        changes.removed.push(("m1".into(), 4));
        assert!(!changes.is_empty());
    }
}
