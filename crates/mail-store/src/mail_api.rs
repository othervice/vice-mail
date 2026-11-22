//! Public API of the store for stage 2: accounts, mailboxes, message headers and lists.
//!
//! The signatures and their documented behaviour are the contract with the engine. The SQL
//! lives in `accounts`, `mailboxes` and `messages`; every method here only moves its arguments
//! to the writer thread or a reader and awaits the result, so no call blocks a Tokio worker.
//!
//! **Inherent methods, no trait.** The real store on a temporary directory is fast (the whole
//! schema in memory pages, milliseconds per test) and exact; a fake would have to re-implement
//! the merge rule, id allocation and keyset paging and could drift from them. The engine's
//! unit tests therefore use `Store::open` on a `TempDir`; the backend is the part that gets a
//! fake.
//!
//! **Rules for all writing methods:** one `BEGIN IMMEDIATE` transaction per call on the writer
//! thread (all or nothing); idempotent, so repeating a call after a crash or a dropped
//! connection is harmless. Sync writes carry the UIDVALIDITY they were computed for and fail
//! with [`StoreError::UidValidityMismatch`] (nothing written) if the mailbox was reset since.
//! Unknown ids fail with [`StoreError::NotFound`].

use mail_model::{
    Account, AccountColor, AccountId, AccountStatus, FlagUpdate, Identity, ListCursor, Mailbox,
    MailboxId, MailboxRole, MessageHeader, MessagePage, ModSeq, RemoteMailbox, ServerSettings, Uid,
    UidSet, UidValidity,
};

use crate::{Store, StoreError, accounts, mailboxes, messages};

/// Largest page [`Store::list_messages`] returns.
pub const MAX_PAGE_SIZE: u32 = 500;

/// Input of [`Store::insert_account`]. The engine generates the id and writes the password to
/// the keychain under `credential_ref` before inserting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewAccount {
    pub id: AccountId,
    pub email: String,
    pub display_name: String,
    pub imap: ServerSettings,
    pub smtp: ServerSettings,
    /// Name of the keychain entry. Never the secret.
    pub credential_ref: String,
}

/// An account row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredAccount {
    pub id: AccountId,
    pub email: String,
    pub display_name: String,
    pub color: AccountColor,
    pub imap: ServerSettings,
    pub smtp: ServerSettings,
    pub credential_ref: String,
    /// Always `false` in stage 2.
    pub writes_enabled: bool,
    /// Unix milliseconds.
    pub created_at: i64,
}

impl StoredAccount {
    /// The UI form, with the sync status the engine keeps in memory.
    pub fn into_account(self, status: AccountStatus) -> Account {
        Account {
            id: self.id,
            email: self.email,
            display_name: self.display_name,
            color: self.color,
            imap: self.imap,
            smtp: self.smtp,
            writes_enabled: self.writes_enabled,
            status,
        }
    }
}

/// Sync bookkeeping of one mailbox (internal, not for the UI).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MailboxSyncState {
    pub mailbox_id: MailboxId,
    /// Wire name, for `MailBackend::open_mailbox`.
    pub name: String,
    pub path: String,
    pub role: MailboxRole,
    pub selectable: bool,
    /// Generation of the stored UIDs; `None` before the first sync.
    pub uid_validity: Option<UidValidity>,
    /// UIDNEXT at the start of the last completed new-mail pass: UIDs at or above are new.
    pub uid_next: Option<Uid>,
    /// HIGHESTMODSEQ at the start of the last completed flag pass (CONDSTORE).
    pub highest_modseq: Option<ModSeq>,
    /// First-sync progress: every server message with `sync_low_uid <= UID < uid_next` is
    /// stored. `Some(1)` = first sync complete; `None` = not begun.
    pub sync_low_uid: Option<Uid>,
}

impl MailboxSyncState {
    pub fn first_sync_complete(&self) -> bool {
        self.sync_low_uid == Some(1)
    }
}

/// Starting point after a first open or a UIDVALIDITY change, see [`Store::reset_mailbox`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MailboxBaseline {
    pub uid_validity: UidValidity,
    pub uid_next: Uid,
    pub highest_modseq: Option<ModSeq>,
}

/// Result of [`Store::sync_mailboxes`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MailboxListReport {
    pub added: Vec<MailboxId>,
    pub removed: Vec<MailboxId>,
    /// Anything changed (added, removed, renamed path, attributes, role, parent).
    pub changed: bool,
}

/// Result of [`Store::store_headers`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StoreReport {
    /// New `message` rows.
    pub inserted: usize,
    /// New locations of an existing message (same account and Message-ID, merge rule).
    pub merged: usize,
    /// Locations that existed already; only flags and MODSEQ were refreshed.
    pub updated: usize,
}

impl Store {
    // ---------------------------------------------------------------------------------------
    // Accounts
    // ---------------------------------------------------------------------------------------

    /// Inserts an account with `writes_enabled = false`, the next free color
    /// ([`AccountColor::next_free`]) and one default identity (email and display name).
    /// [`StoreError::AccountExists`] if the email is taken (case-insensitive).
    pub async fn insert_account(&self, account: NewAccount) -> Result<StoredAccount, StoreError> {
        let result = self.write(move |tx| accounts::insert(tx, &account)).await;
        if let Ok(stored) = &result {
            tracing::info!(account = %stored.id, "account stored");
        }
        result
    }

    /// All accounts, oldest first.
    pub async fn accounts(&self) -> Result<Vec<StoredAccount>, StoreError> {
        self.read(accounts::all).await
    }

    pub async fn account(&self, id: AccountId) -> Result<Option<StoredAccount>, StoreError> {
        self.read(move |conn| accounts::one(conn, id)).await
    }

    /// Deletes the account with all its identities, mailboxes, messages and locations.
    /// Returns `false` if it did not exist. The keychain entry is the engine's job.
    pub async fn delete_account(&self, id: AccountId) -> Result<bool, StoreError> {
        let deleted = self.write(move |tx| accounts::delete(tx, id)).await?;
        if deleted {
            tracing::info!(account = %id, "account and its local data deleted");
        }
        Ok(deleted)
    }

    /// Identities of the account, default first.
    pub async fn identities(&self, account: AccountId) -> Result<Vec<Identity>, StoreError> {
        self.read(move |conn| accounts::identities(conn, account))
            .await
    }

    // ---------------------------------------------------------------------------------------
    // Mailboxes
    // ---------------------------------------------------------------------------------------

    /// Makes the stored mailboxes of `account` equal to the server's list (by wire name):
    /// inserts new ones, updates path, delimiter, attributes and `selectable`, deletes the
    /// missing ones (their locations go with them; messages left without any location stay
    /// until [`Store::delete_orphan_messages`], so a move or rename keeps local data through
    /// the Message-ID merge). Recomputes `parent_id` from the paths and the roles with
    /// [`mail_model::assign_roles`], keeping roles with source `user`. Status counters are
    /// not stored; sync state of kept mailboxes is untouched.
    pub async fn sync_mailboxes(
        &self,
        account: AccountId,
        remote: Vec<RemoteMailbox>,
    ) -> Result<MailboxListReport, StoreError> {
        self.write(move |tx| mailboxes::sync(tx, account, &remote))
            .await
    }

    /// Mailboxes of the account for the UI, with local unread and total counts, ordered by
    /// path. Unread via `loc_unseen`, total via `loc_uid` (both covering index searches).
    pub async fn mailboxes(&self, account: AccountId) -> Result<Vec<Mailbox>, StoreError> {
        self.read(move |conn| mailboxes::list_for(conn, account))
            .await
    }

    /// Sync bookkeeping of every mailbox of the account, ordered by path.
    pub async fn mailbox_sync_states(
        &self,
        account: AccountId,
    ) -> Result<Vec<MailboxSyncState>, StoreError> {
        self.read(move |conn| mailboxes::sync_states(conn, account))
            .await
    }

    /// The user sets a role (`custom` included) with source `user`. Another mailbox of the
    /// account that had this role automatically loses it (roles recomputed as in
    /// `sync_mailboxes`). [`StoreError::InvalidArgument`] for `inbox`, for the inbox itself
    /// and for non-selectable mailboxes. Returns all mailboxes of the account, as
    /// [`Store::mailboxes`].
    pub async fn set_mailbox_role(
        &self,
        mailbox: MailboxId,
        role: MailboxRole,
    ) -> Result<Vec<Mailbox>, StoreError> {
        self.write(move |tx| mailboxes::set_role(tx, mailbox, role))
            .await
    }

    /// Starts the mailbox over for a new UIDVALIDITY (or its very first sync): deletes all its
    /// locations, sets `uid_validity`, `uid_next` and `highest_modseq` from `baseline` and
    /// `sync_low_uid = uid_next` (nothing stored yet). Messages that lose their last location
    /// stay as orphans, so the re-sync merges them back by Message-ID. Returns the number of
    /// removed locations. The only sync write without a UIDVALIDITY check.
    pub async fn reset_mailbox(
        &self,
        mailbox: MailboxId,
        baseline: MailboxBaseline,
    ) -> Result<usize, StoreError> {
        self.write(move |tx| mailboxes::reset(tx, mailbox, baseline))
            .await
    }

    /// Stores one window of headers (up to 500) and, if given, the new first-sync progress
    /// `sync_low_uid`, in the same transaction. Per header:
    /// 1. A location with this UID exists: refresh flags, keywords and MODSEQ (`updated`).
    /// 2. Else, with a Message-ID: a message of this account with that Message-ID that does
    ///    not lie in this mailbox yet gets a new location (`merged`, decision 0003).
    /// 3. Else a new message row (`inserted`) with `id` = [`MessageHeader::sort_date`], or the
    ///    next free number above it (allocated here, on the writer thread). Messages without
    ///    Message-ID, and duplicates within one mailbox, always get their own row.
    pub async fn store_headers(
        &self,
        mailbox: MailboxId,
        uid_validity: UidValidity,
        headers: Vec<MessageHeader>,
        sync_low_uid: Option<Uid>,
    ) -> Result<StoreReport, StoreError> {
        self.write(move |tx| {
            messages::store_headers(tx, mailbox, uid_validity, headers, sync_low_uid)
        })
        .await
    }

    /// Writes new flags of existing locations; UIDs not stored are ignored. Returns the number
    /// of locations whose flags or keywords actually changed.
    pub async fn update_flags(
        &self,
        mailbox: MailboxId,
        uid_validity: UidValidity,
        updates: Vec<FlagUpdate>,
    ) -> Result<usize, StoreError> {
        self.write(move |tx| messages::update_flags(tx, mailbox, uid_validity, &updates))
            .await
    }

    /// Deletes the locations of these UIDs (deleted or moved away on the server). Messages
    /// left without a location stay until [`Store::delete_orphan_messages`]. Returns the
    /// number of removed locations.
    pub async fn remove_uids(
        &self,
        mailbox: MailboxId,
        uid_validity: UidValidity,
        uids: UidSet,
    ) -> Result<usize, StoreError> {
        self.write(move |tx| messages::remove_uids(tx, mailbox, uid_validity, &uids))
            .await
    }

    /// Every stored UID of the mailbox (`loc_uid`, covering index). Compare with
    /// `MailBackend::search_uids` to find deletions: `known.difference(&server)`.
    pub async fn known_uids(&self, mailbox: MailboxId) -> Result<UidSet, StoreError> {
        self.read(move |conn| messages::known_uids(conn, mailbox))
            .await
    }

    /// Records that every message below `uid_next` has been fetched (end of a new-mail pass).
    pub async fn set_uid_next(
        &self,
        mailbox: MailboxId,
        uid_validity: UidValidity,
        uid_next: Uid,
    ) -> Result<(), StoreError> {
        self.write(move |tx| mailboxes::set_uid_next(tx, mailbox, uid_validity, uid_next))
            .await
    }

    /// Records the HIGHESTMODSEQ a completed flag pass started from (`None`: the mailbox has
    /// no mod-sequences any more, fall back to window-wise flag sync).
    pub async fn set_highest_modseq(
        &self,
        mailbox: MailboxId,
        uid_validity: UidValidity,
        highest_modseq: Option<ModSeq>,
    ) -> Result<(), StoreError> {
        self.write(move |tx| {
            mailboxes::set_highest_modseq(tx, mailbox, uid_validity, highest_modseq)
        })
        .await
    }

    /// Deletes messages of the account that lie in no mailbox. The engine calls it after a
    /// full pass in which no mailbox of the account was in its first sync (otherwise a
    /// message on its way between two mailboxes would lose its local data). Returns the count.
    pub async fn delete_orphan_messages(&self, account: AccountId) -> Result<usize, StoreError> {
        self.write(move |tx| messages::delete_orphans(tx, account))
            .await
    }

    // ---------------------------------------------------------------------------------------
    // Lists
    // ---------------------------------------------------------------------------------------

    /// One page of the mailbox, newest first, keyset-paged (`message_id < cursor`, never
    /// OFFSET; `CROSS JOIN` fixes the join order; a test checks the query plan). `limit` is
    /// 1..=[`MAX_PAGE_SIZE`], else [`StoreError::InvalidArgument`]. `next_cursor` is `None`
    /// when the page is the last one. Dates in the result are milliseconds.
    pub async fn list_messages(
        &self,
        mailbox: MailboxId,
        cursor: Option<ListCursor>,
        limit: u32,
    ) -> Result<MessagePage, StoreError> {
        self.read(move |conn| messages::list(conn, mailbox, cursor, limit))
            .await
    }

    /// Unread count per mailbox of the account (`loc_unseen`), mailboxes without unread
    /// messages included with 0.
    pub async fn unread_counts(
        &self,
        account: AccountId,
    ) -> Result<Vec<(MailboxId, u32)>, StoreError> {
        self.read(move |conn| mailboxes::unread_counts(conn, account))
            .await
    }
}
