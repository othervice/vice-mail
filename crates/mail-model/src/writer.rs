//! The writing side of a mail backend (stage 4, decision 0012): [`MailWriter`] extends the
//! read-only [`MailBackend`] and is only implemented by connections that may write.
//!
//! **Read-only protection stays structural** (see [`crate::backend`]): code that holds a
//! `MailBackend` still cannot express a write. A writer comes only from
//! [`WriteConnector::connect_writer`], which refuses a configuration with
//! [`Access::ReadOnly`](crate::Access::ReadOnly) before it connects. Underneath, the adapter's
//! read-only allowlist is unchanged; a writer is simply a connection opened with
//! [`Access::ReadWrite`](crate::Access::ReadWrite) (SELECT instead of EXAMINE).
//!
//! Every method is formulated idempotently where the protocol allows it ("set flag", not
//! "toggle"); the outbox in `mail-engine` checks the server before it repeats anything that
//! may already have worked (see its module docs).

use std::future::Future;

use crate::{
    BackendError, ConnectConfig, Flag, FlagUpdate, MailBackend, MailboxRole, MailboxStatus, ModSeq,
    SelectedMailbox, Uid, UidSet, UidValidity,
};

/// APPENDUID (RFC 4315): where an appended message ended up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AppendUid {
    pub uid_validity: UidValidity,
    pub uid: Uid,
}

/// Whether [`MailWriter::store_flags`] sets or clears the flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StoreMode {
    /// `+FLAGS`
    Add,
    /// `-FLAGS`
    Remove,
}

/// Result of [`MailWriter::store_flags`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StoreOutcome {
    /// Flags (and MODSEQ) of the changed messages as the server reported them afterwards.
    /// Servers need not report messages whose flags were already as asked.
    pub updates: Vec<FlagUpdate>,
    /// UIDs the server did not change because they were modified after `unchanged_since`
    /// (CONDSTORE `[MODIFIED ...]`, RFC 7162). Empty without `unchanged_since`.
    pub modified: UidSet,
}

/// COPYUID (RFC 4315): where copied or moved messages ended up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CopyUid {
    /// UIDVALIDITY of the target mailbox the new UIDs belong to.
    pub uid_validity: UidValidity,
    /// Source UID → new UID in the target, in the order the server listed them.
    pub pairs: Vec<(Uid, Uid)>,
}

impl CopyUid {
    /// New UID of `source` in the target, if the server reported it.
    pub fn target_of(&self, source: Uid) -> Option<Uid> {
        self.pairs
            .iter()
            .find_map(|(from, to)| (*from == source).then_some(*to))
    }
}

/// How [`MailWriter::move_messages`] moved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MoveMethod {
    /// `UID MOVE` (RFC 6851), atomic.
    Move,
    /// `UID COPY`, `UID STORE +FLAGS.SILENT (\Deleted)`, then `UID EXPUNGE` (UIDPLUS), or a plain
    /// `EXPUNGE` when no other message of the source mailbox was marked deleted.
    CopyExpunge,
    /// `UID COPY` and `\Deleted`, but no EXPUNGE: the server lacks UIDPLUS and other messages
    /// in the source mailbox are marked deleted, which a plain EXPUNGE would remove too. The
    /// original stays in the source, marked deleted; the user is told.
    CopyLeftDeleted,
}

/// Result of [`MailWriter::move_messages`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MoveOutcome {
    pub method: MoveMethod,
    /// New UIDs in the target. Only from servers with UIDPLUS.
    pub copy_uid: Option<CopyUid>,
}

/// Result of [`MailWriter::expunge_messages`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExpungeOutcome {
    /// The messages are gone.
    Expunged,
    /// Marked `\Deleted` but not expunged: no UIDPLUS, and other messages in the mailbox are
    /// marked deleted too (see [`MoveMethod::CopyLeftDeleted`]).
    LeftDeleted,
}

/// One authenticated connection that may change the server. Only built for
/// [`Access::ReadWrite`](crate::Access::ReadWrite) configurations; mailboxes are opened with
/// SELECT.
///
/// Mailbox-scoped methods take the [`SelectedMailbox`] from [`MailBackend::open_mailbox`] and
/// fail with [`BackendError::InvalidState`] if it is not the open one. UIDs that do not exist
/// (any more) are ignored by the server, as IMAP does: callers check presence first (the outbox
/// does, see its conflict rules).
pub trait MailWriter: MailBackend {
    /// STATUS of a mailbox that is not open (UIDNEXT and UIDVALIDITY are what the outbox needs
    /// before a move). [`BackendError::MailboxNotFound`] if it does not exist.
    fn mailbox_status(
        &mut self,
        name: &str,
    ) -> impl Future<Output = Result<MailboxStatus, BackendError>> + Send;

    /// `UID STORE uids [(UNCHANGEDSINCE m)] ±FLAGS (flags)`. Keywords are checked to be valid
    /// IMAP atoms before anything is sent ([`BackendError::InvalidState`] otherwise; callers
    /// validate with [`Flag::keyword`]). `unchanged_since` needs CONDSTORE on the open mailbox
    /// ([`BackendError::Unsupported`] otherwise).
    fn store_flags(
        &mut self,
        mailbox: &SelectedMailbox,
        uids: &UidSet,
        mode: StoreMode,
        flags: &[Flag],
        unchanged_since: Option<ModSeq>,
    ) -> impl Future<Output = Result<StoreOutcome, BackendError>> + Send;

    /// Moves the messages from the open mailbox to `target` (wire name): `UID MOVE` when the
    /// server has MOVE, otherwise COPY, `\Deleted` and EXPUNGE as described in [`MoveMethod`].
    fn move_messages(
        &mut self,
        mailbox: &SelectedMailbox,
        uids: &UidSet,
        target: &str,
    ) -> impl Future<Output = Result<MoveOutcome, BackendError>> + Send;

    /// Deletes the messages for good: `\Deleted`, then `UID EXPUNGE` (UIDPLUS) or a plain
    /// EXPUNGE when no other message is marked deleted (see [`ExpungeOutcome`]).
    fn expunge_messages(
        &mut self,
        mailbox: &SelectedMailbox,
        uids: &UidSet,
    ) -> impl Future<Output = Result<ExpungeOutcome, BackendError>> + Send;

    /// Creates a mailbox from its decoded path (the adapter encodes it; the path uses the
    /// server's hierarchy delimiter) and subscribes to it. With CREATE-SPECIAL-USE (RFC 6154)
    /// `role` becomes its SPECIAL-USE attribute; if the server refuses that (`[USEATTR]`, for
    /// example because another mailbox has the role), the mailbox is created without it. A
    /// mailbox that exists already is success, so repeating the call is harmless. Returns the
    /// wire name.
    fn create_mailbox(
        &mut self,
        path: &str,
        role: Option<MailboxRole>,
    ) -> impl Future<Output = Result<String, BackendError>> + Send;

    /// Stage 5: `APPEND` of `message` to the mailbox with this wire name, with `flags` (system
    /// flags only: `\Seen`, `\Draft`). Returns APPENDUID when the server has UIDPLUS. A missing
    /// mailbox is [`BackendError::MailboxNotFound`]. The default fails with
    /// [`BackendError::Unsupported`] (test fakes without APPEND).
    fn append(
        &mut self,
        mailbox: &str,
        flags: &[Flag],
        message: &[u8],
    ) -> impl Future<Output = Result<Option<AppendUid>, BackendError>> + Send {
        let _ = (mailbox, flags, message);
        std::future::ready(Err(BackendError::Unsupported("APPEND".into())))
    }

    /// Stage 5: UIDs of the open mailbox whose `Message-ID` header is exactly `message_id`
    /// (without angle brackets). The default reads the headers of the whole mailbox; the IMAP
    /// writer asks the server (`SEARCH HEADER`) and checks the hits, and also reads the newest
    /// messages, because a server's search index may lag behind a fresh APPEND.
    fn find_message_id(
        &mut self,
        mailbox: &SelectedMailbox,
        message_id: &str,
    ) -> impl Future<Output = Result<UidSet, BackendError>> + Send {
        async move {
            let all = self.search_uids(mailbox).await?;
            if all.is_empty() {
                return Ok(UidSet::new());
            }
            let headers = self.fetch_headers(mailbox, &all).await?;
            Ok(UidSet::from_uids(
                headers
                    .iter()
                    .filter(|h| h.message_id.as_deref() == Some(message_id))
                    .map(|h| h.uid),
            ))
        }
    }
}

/// Opens [`MailWriter`] connections.
pub trait WriteConnector: Send + Sync {
    type Writer: MailWriter + 'static;

    /// Connects with `config`, which must have [`Access::ReadWrite`](crate::Access::ReadWrite):
    /// a read-only configuration fails with [`BackendError::ReadOnly`] before any network
    /// access.
    fn connect_writer(
        &self,
        config: &ConnectConfig,
    ) -> impl Future<Output = Result<Self::Writer, BackendError>> + Send;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copy_uid_maps_source_to_target() {
        let copy = CopyUid {
            uid_validity: UidValidity(9),
            pairs: vec![(4, 100), (7, 101)],
        };
        assert_eq!(copy.target_of(7), Some(101));
        assert_eq!(copy.target_of(5), None);
    }
}
