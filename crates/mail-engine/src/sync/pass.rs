//! One sync pass over the mailboxes of an account, on one connection.
//!
//! Per mailbox (inbox first, so it is usable at once):
//! 1. EXAMINE. A new or changed UIDVALIDITY resets the mailbox (`Store::reset_mailbox`): its
//!    locations are dropped, messages stay as orphans and merge back by Message-ID.
//! 2. Nothing to do if the first sync is complete and UIDNEXT, HIGHESTMODSEQ and the number of
//!    messages are unchanged (CONDSTORE servers only; without mod-sequences flags can change
//!    unseen).
//! 3. `UID SEARCH ALL`; stored UIDs the server no longer has are removed.
//! 4. Flags of the stored messages: `CHANGEDSINCE` the stored HIGHESTMODSEQ, otherwise window
//!    by window.
//! 5. New messages (stored UIDNEXT up to the UIDNEXT of this EXAMINE), newest window first.
//!    Unread ones are announced with `newMail`, except during the mailbox's first sync.
//! 6. First sync: the rest below `sync_low_uid`, newest first, one window per transaction;
//!    progress is the lowest stored UID, so an interrupted sync continues where it stopped.
//!
//! A full pass lists the mailboxes first (one LIST-STATUS round trip) and deletes orphaned
//! messages at the end, unless a mailbox was in its first sync or skipped.

use std::collections::BTreeSet;

use mail_model::{
    BackendError, CoreError, MailBackend, MailboxId, MailboxRole, MessageHeader, ModSeq,
    SelectedMailbox, SyncProgress, Uid, UidSet,
};
use mail_store::{MailboxBaseline, MailboxSyncState, Store, StoreError};

use super::SyncConfig;
use super::session::Reporter;

/// Which mailboxes a pass covers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Scope {
    /// LIST, every selectable mailbox, orphan clean-up.
    Full,
    /// The inbox if `inbox` (IDLE reported a change) and the stored `mailboxes` (the outbox
    /// changed them on the server), without LIST. Falls back to `Full` if none of them is
    /// known yet (before the first LIST).
    Partial {
        inbox: bool,
        mailboxes: BTreeSet<MailboxId>,
    },
}

#[cfg(test)]
impl Scope {
    /// Only the inbox.
    pub(crate) fn inbox() -> Self {
        Self::Partial {
            inbox: true,
            mailboxes: BTreeSet::new(),
        }
    }
}

/// Why a pass failed.
#[derive(Debug)]
pub(crate) enum SyncError {
    Backend(BackendError),
    Store(StoreError),
}

impl From<BackendError> for SyncError {
    fn from(err: BackendError) -> Self {
        Self::Backend(err)
    }
}

impl From<StoreError> for SyncError {
    fn from(err: StoreError) -> Self {
        Self::Store(err)
    }
}

impl From<SyncError> for CoreError {
    fn from(err: SyncError) -> Self {
        match err {
            SyncError::Backend(err) => Self::Mail(err),
            // The mailbox was reset while a write for its old UIDVALIDITY was on its way (for
            // example by the next session after a reconnect). The write was refused, nothing
            // is lost; the next pass starts from the stored state. Transient, not "internal".
            SyncError::Store(err @ StoreError::UidValidityMismatch { .. }) => Self::Mail(
                BackendError::Protocol(format!("mailbox changed during the sync: {err}")),
            ),
            SyncError::Store(err) => Self::from(err),
        }
    }
}

/// What a pass saw.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PassOutcome {
    /// Some mailbox was in its first sync.
    pub first_sync_seen: bool,
    /// Some mailbox could not be opened (vanished between LIST and EXAMINE, or refused).
    pub skipped: bool,
}

fn is_inbox(state: &MailboxSyncState) -> bool {
    state.role == MailboxRole::Inbox || state.name.eq_ignore_ascii_case("INBOX")
}

/// Runs one pass. See the module docs.
pub(crate) async fn run<B: MailBackend>(
    backend: &mut B,
    store: &Store,
    config: &SyncConfig,
    reporter: &Reporter,
    scope: Scope,
) -> Result<PassOutcome, SyncError> {
    let account = reporter.account();
    let mut scope = scope;
    let mut states = Vec::new();
    if let Scope::Partial { inbox, mailboxes } = &scope {
        states = store
            .mailbox_sync_states(account)
            .await?
            .into_iter()
            .filter(|s| (*inbox && is_inbox(s)) || mailboxes.contains(&s.mailbox_id))
            .collect();
        if states.is_empty() {
            scope = Scope::Full;
        }
    }
    if scope == Scope::Full {
        let remote = backend.list_mailboxes().await?;
        let report = store.sync_mailboxes(account, remote).await?;
        if report.changed {
            tracing::debug!(
                %account,
                added = report.added.len(),
                removed = report.removed.len(),
                "mailbox list changed"
            );
            reporter.mailboxes_changed();
        }
        states = store.mailbox_sync_states(account).await?;
    }
    states.retain(|s| s.selectable);
    // Stable: the inbox first, the rest in path order as the store returns them.
    states.sort_by_key(|s| !is_inbox(s));

    let mut outcome = PassOutcome::default();
    for state in &states {
        match sync_mailbox(backend, store, config, reporter, state).await? {
            Some(first_sync) => outcome.first_sync_seen |= first_sync,
            None => outcome.skipped = true,
        }
    }

    if scope == Scope::Full && !outcome.first_sync_seen && !outcome.skipped {
        let deleted = store.delete_orphan_messages(account).await?;
        if deleted > 0 {
            tracing::debug!(%account, deleted, "orphaned messages deleted");
        }
    }
    Ok(outcome)
}

/// Syncs one mailbox. `Some(first_sync)` says whether it was in its first sync; `None` means it
/// could not be opened and was skipped (the next full pass lists the mailboxes again).
async fn sync_mailbox<B: MailBackend>(
    backend: &mut B,
    store: &Store,
    config: &SyncConfig,
    reporter: &Reporter,
    state: &MailboxSyncState,
) -> Result<Option<bool>, SyncError> {
    let id = state.mailbox_id;
    let selected = match backend.open_mailbox(&state.name).await {
        Ok(selected) => selected,
        // Gone since LIST, or refused (no permission, temporarily unavailable): one unreadable
        // folder must not keep the rest of the account from syncing.
        Err(err @ (BackendError::MailboxNotFound(_) | BackendError::Rejected(_))) => {
            tracing::warn!(mailbox = id.0, error = %err, "mailbox skipped in this pass");
            return Ok(None);
        }
        Err(err) => return Err(err.into()),
    };
    let uv = selected.uid_validity;

    let mut uid_next = state.uid_next;
    let mut modseq = state.highest_modseq;
    let mut low = state.sync_low_uid;
    if state.uid_validity != Some(uv) {
        let baseline = MailboxBaseline {
            uid_validity: uv,
            uid_next: selected.uid_next,
            highest_modseq: selected.highest_modseq,
        };
        let removed = store.reset_mailbox(id, baseline).await?;
        if let Some(old) = state.uid_validity {
            tracing::info!(mailbox = id.0, %old, new = %uv, removed, "UIDVALIDITY changed; rebuilding the mailbox");
        }
        uid_next = Some(selected.uid_next);
        modseq = selected.highest_modseq;
        low = Some(selected.uid_next);
        if removed > 0 {
            reporter.messages_changed(id);
        }
    }
    let first_sync = low != Some(1);

    let known = store.known_uids(id).await?;
    let unchanged = !first_sync
        && uid_next == Some(selected.uid_next)
        && modseq.is_some()
        && modseq == selected.highest_modseq
        && known.len() == u64::from(selected.exists);
    if unchanged {
        return Ok(Some(false));
    }

    let server = backend.search_uids(&selected).await?;

    // Deletions and flags of what is stored: one event for both.
    let mut changed = false;
    let gone = known.difference(&server);
    if !gone.is_empty() {
        changed |= store.remove_uids(id, uv, gone.clone()).await? > 0;
    }
    let known = known.difference(&gone);
    changed |= sync_flags(backend, store, config, &selected, id, &known, modseq).await?;
    if modseq != selected.highest_modseq {
        store
            .set_highest_modseq(id, uv, selected.highest_modseq)
            .await?;
    }
    if changed {
        reporter.messages_changed(id);
    }

    // New messages. Those that arrived after this EXAMINE wait for the next pass, so they are
    // never fetched (and announced) twice.
    let stored_next = uid_next.unwrap_or(selected.uid_next);
    let mut fetched_new = 0;
    if selected.uid_next > stored_next {
        let mut rest = within(&server, stored_next, selected.uid_next - 1);
        let mut unread = 0u32;
        while !rest.is_empty() {
            let (window, more) = rest.split_highest(u64::from(config.window));
            let headers = backend.fetch_headers(&selected, &window).await?;
            fetched_new += headers.len();
            unread = unread.saturating_add(count_unread(&headers));
            store.store_headers(id, uv, headers, None).await?;
            reporter.messages_changed(id);
            rest = more;
        }
        store.set_uid_next(id, uv, selected.uid_next).await?;
        if !first_sync && unread > 0 {
            reporter.new_mail(id, unread);
        }
    }

    // First sync, newest first.
    if let Some(low) = low.filter(|l| *l > 1) {
        let mut remaining = within(&server, 1, low - 1);
        if remaining.is_empty() {
            store.store_headers(id, uv, Vec::new(), Some(1)).await?;
        }
        let mut size = if known.is_empty() && fetched_new == 0 {
            config.first_window
        } else {
            config.window
        };
        let total = server.len();
        while !remaining.is_empty() {
            let (window, rest) = remaining.split_highest(u64::from(size.max(1)));
            size = config.window;
            let headers = backend.fetch_headers(&selected, &window).await?;
            let new_low = if rest.is_empty() {
                1
            } else {
                window.min().unwrap_or(1)
            };
            store.store_headers(id, uv, headers, Some(new_low)).await?;
            let done = total - rest.len();
            reporter.progress(SyncProgress {
                mailbox_id: id,
                done: saturate(done),
                total: selected.exists.max(saturate(done)),
            });
            reporter.messages_changed(id);
            remaining = rest;
        }
    }
    Ok(Some(first_sync))
}

/// Flags of the stored messages. Returns whether any stored flag changed.
async fn sync_flags<B: MailBackend>(
    backend: &mut B,
    store: &Store,
    config: &SyncConfig,
    selected: &SelectedMailbox,
    id: MailboxId,
    known: &UidSet,
    stored_modseq: Option<ModSeq>,
) -> Result<bool, SyncError> {
    let Some(max) = known.max() else {
        return Ok(false);
    };
    let updates = match (stored_modseq, selected.highest_modseq) {
        (Some(old), Some(new)) if new == old => return Ok(false),
        (Some(old), Some(new)) if new > old => {
            backend
                .fetch_flags(selected, &UidSet::range(1, max), Some(old))
                .await?
        }
        // No mod-sequences (or they went backwards): compare every stored message.
        _ => {
            let mut updates = Vec::new();
            let mut rest = known.clone();
            while !rest.is_empty() {
                let (window, more) = rest.split_highest(u64::from(config.flag_window.max(1)));
                updates.extend(backend.fetch_flags(selected, &window, None).await?);
                rest = more;
            }
            updates
        }
    };
    if updates.is_empty() {
        return Ok(false);
    }
    Ok(store
        .update_flags(id, selected.uid_validity, updates)
        .await?
        > 0)
}

fn count_unread(headers: &[MessageHeader]) -> u32 {
    saturate(
        headers
            .iter()
            .filter(|h| !h.flags.seen && !h.flags.deleted)
            .count() as u64,
    )
}

fn saturate(n: u64) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

/// The UIDs of `set` in `first..=last`.
fn within(set: &UidSet, first: Uid, last: Uid) -> UidSet {
    let mut outside = Vec::with_capacity(2);
    if first > 1 {
        outside.push((1, first - 1));
    }
    if last < Uid::MAX {
        outside.push((last + 1, Uid::MAX));
    }
    set.difference(&UidSet::from_ranges(outside))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn within_cuts_both_ends() {
        let set = UidSet::from_uids([1, 2, 5, 9, 10, 4_000_000_000]);
        assert_eq!(within(&set, 2, 9), UidSet::from_uids([2, 5, 9]));
        assert_eq!(within(&set, 1, Uid::MAX), set);
        assert_eq!(
            within(&set, 10, Uid::MAX),
            UidSet::from_uids([10, 4_000_000_000])
        );
        assert!(within(&set, 6, 8).is_empty());
    }

    #[test]
    fn uidvalidity_mismatch_is_transient_not_internal() {
        let err: CoreError = SyncError::Store(StoreError::UidValidityMismatch {
            mailbox: 1,
            stored: Some(2),
            given: 1,
        })
        .into();
        assert!(err.is_transient());
        assert_eq!(err.code(), mail_model::ErrorCode::Protocol);
        let err: CoreError = SyncError::Store(StoreError::NotFound("mailbox 1".into())).into();
        assert_eq!(err.code(), mail_model::ErrorCode::NotFound);
    }
}
