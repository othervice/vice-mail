//! Stage 21 (decision 0063): "Mark folder as read" through the outbox.
//!
//! One operation per folder (`mark_mailbox`), not one per message: the payload holds the UIDs
//! that were unread when the user asked, as a [`UidSet`]. Recording it marks exactly those
//! locations as read in the same transaction (the counters drop at once); the outbox then sends
//! `UID STORE <set> +FLAGS (\Seen)` in bounded parts (JMAP: `Email/set` in batches). Mail that
//! arrives later is never touched, a repeat after a crash is harmless (setting a flag twice
//! changes nothing), and undo clears `\Seen` on the same set.
//!
//! Like a pending flag operation, a pending mark wins over what a sync writes until the server
//! confirmed it: `outbox::reapply_pending_in_mailbox` applies the folder's pending marks again,
//! in recorded order together with the per-message flag operations.

use mail_model::{
    AccountId, ActionKind, MailboxId, OperationId, OperationPayload, TargetMailbox, Uid, UidSet,
    UidValidity,
};
use rusqlite::{Connection, OptionalExtension, params};

use crate::outbox::{Recorder, generation, payload_at, repeated, require_writes};
use crate::settings::now_millis;
use crate::{OutboxChange, Store, StoreError, accounts};

/// Most folders one call marks (an account's inbox and its own folders).
pub const MAX_MARK_MAILBOXES: usize = 2_000;

/// A folder to mark: its account, wire name, generation, whether it holds messages.
pub(crate) const MAILBOX_FOR_MARK: &str =
    "SELECT account_id, name, uidvalidity, selectable FROM mailbox WHERE id = ?1";

/// UIDs of the folder's unread locations (`loc_unseen`).
pub(crate) const UNSEEN_UIDS: &str = "
    SELECT uid FROM message_location INDEXED BY loc_unseen WHERE mailbox_id = ?1 AND seen = 0";

/// Every unread location of the folder marked as read (when the set covers them all).
pub(crate) const MARK_ALL_SEEN: &str = "
    UPDATE message_location INDEXED BY loc_unseen SET seen = 1 WHERE mailbox_id = ?1 AND seen = 0";

pub(crate) const SET_SEEN_AT: &str =
    "UPDATE message_location SET seen = ?3 WHERE mailbox_id = ?1 AND uid = ?2 AND seen <> ?3";

pub(crate) const SET_SEEN_RANGE: &str = "
    UPDATE message_location SET seen = ?4
    WHERE mailbox_id = ?1 AND uid BETWEEN ?2 AND ?3 AND seen <> ?4";

/// Pending marks of a folder, in recorded order: the part of `operation_location` without a
/// UID (a mark names the folder, not a message).
pub(crate) const PENDING_MARKS_IN: &str = "
    SELECT id, payload FROM operation
    WHERE mailbox_id = ?1 AND uid IS NULL AND kind = 'mark_mailbox'
      AND state IN ('queued', 'in_flight')
    ORDER BY id";

/// Sets `\Seen` to `seen` on the locations of `uids` in the folder, if it is still in that
/// generation. Returns how many locations changed.
///
/// Marking as read looks only at the folder's unread locations (few, once a mark applied: new
/// mail, or what a sync just wrote); marking as unread walks the set's ranges.
pub(crate) fn set_seen(
    conn: &Connection,
    mailbox: MailboxId,
    uid_validity: UidValidity,
    uids: &UidSet,
    seen: bool,
) -> Result<usize, StoreError> {
    if uids.is_empty() || generation(conn, mailbox)? != Some(uid_validity) {
        return Ok(0);
    }
    let mut changed = 0;
    if seen {
        let unseen = conn
            .prepare_cached(UNSEEN_UIDS)?
            .query_map([mailbox.0], |r| r.get::<_, Uid>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        let hits: Vec<Uid> = unseen
            .iter()
            .copied()
            .filter(|uid| uids.contains(*uid))
            .collect();
        if hits.is_empty() {
            return Ok(0);
        }
        if hits.len() == unseen.len() {
            return conn
                .prepare_cached(MARK_ALL_SEEN)?
                .execute([mailbox.0])
                .map_err(StoreError::sqlite("mark folder as read"));
        }
        let mut stmt = conn.prepare_cached(SET_SEEN_AT)?;
        for uid in hits {
            changed += stmt
                .execute(params![mailbox.0, uid, true])
                .map_err(StoreError::sqlite("mark as read"))?;
        }
    } else {
        let mut stmt = conn.prepare_cached(SET_SEEN_RANGE)?;
        for (first, last) in uids.ranges() {
            changed += stmt
                .execute(params![mailbox.0, first, last, false])
                .map_err(StoreError::sqlite("mark as unread"))?;
        }
    }
    Ok(changed)
}

/// The folder's pending marks, in recorded order.
pub(crate) fn pending_marks(
    conn: &Connection,
    mailbox: MailboxId,
) -> Result<Vec<(OperationId, OperationPayload)>, StoreError> {
    Ok(conn
        .prepare_cached(PENDING_MARKS_IN)?
        .query_map([mailbox.0], |r| {
            Ok((OperationId(r.get(0)?), payload_at(r, 1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?)
}

/// See [`Store::enqueue_mark_read`].
pub(crate) fn enqueue(
    conn: &Connection,
    account: AccountId,
    mailboxes: &[MailboxId],
    request: &str,
    now: i64,
) -> Result<OutboxChange, StoreError> {
    if mailboxes.is_empty() || mailboxes.len() > MAX_MARK_MAILBOXES {
        return Err(StoreError::InvalidArgument(format!(
            "{} folders to mark as read; 1 to {MAX_MARK_MAILBOXES}",
            mailboxes.len()
        )));
    }
    let account_row = accounts::row_id(conn, account)?;
    require_writes(conn, account_row)?;
    if let Some(batch) = repeated(conn, request)? {
        let mut change = OutboxChange::new(account);
        change.batch = Some(batch);
        change.repeated = true;
        return Ok(change);
    }
    // Every folder is checked before anything is recorded.
    let mut folders: Vec<(MailboxId, String, Option<UidValidity>)> = Vec::new();
    for &mailbox in mailboxes {
        if folders.iter().any(|(id, ..)| *id == mailbox) {
            continue;
        }
        let (name, uid_validity) = conn
            .prepare_cached(MAILBOX_FOR_MARK)?
            .query_row([mailbox.0], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Option<u32>>(2)?,
                    r.get::<_, bool>(3)?,
                ))
            })
            .optional()?
            .filter(|(row, ..)| *row == account_row)
            .map(|(_, name, uid_validity, selectable)| {
                (name, uid_validity.filter(|_| selectable).map(UidValidity))
            })
            .ok_or_else(|| StoreError::NotFound(format!("mailbox {mailbox}")))?;
        folders.push((mailbox, name, uid_validity));
    }
    let mut recorder = Recorder::new(
        conn,
        account_row,
        account,
        ActionKind::MarkMailboxRead,
        request,
        now,
    );
    for (mailbox, name, uid_validity) in folders {
        // A placeholder without messages, or a folder the sync has not opened yet.
        let Some(uid_validity) = uid_validity else {
            continue;
        };
        let uids = UidSet::from_uids(
            conn.prepare_cached(UNSEEN_UIDS)?
                .query_map([mailbox.0], |r| r.get::<_, Uid>(0))?
                .collect::<Result<Vec<_>, _>>()?,
        );
        if uids.is_empty() {
            continue;
        }
        recorder.record(OperationPayload::MarkMailbox {
            mailbox: TargetMailbox {
                mailbox_id: mailbox,
                name,
            },
            uid_validity,
            uids,
            seen: true,
        })?;
    }
    Ok(recorder.change)
}

impl Store {
    /// Stage 21 (decision 0063): marks every unread message of these folders (all of
    /// `account`) as read, at once locally, and records one `mark_mailbox` operation per folder
    /// with the UIDs that were unread, in one batch (one undo). Folders without unread mail
    /// record nothing; with none at all the change has no batch.
    ///
    /// Refused before anything is written: [`mail_model::ActionError::ReadOnly`] (account
    /// without `writes_enabled`), `NotFound` (a folder of another account or none),
    /// `InvalidArgument` (no folders, or more than [`MAX_MARK_MAILBOXES`]). `request` makes a
    /// repeated call change nothing, as in [`Store::enqueue_action`].
    pub async fn enqueue_mark_read(
        &self,
        account: AccountId,
        mailboxes: Vec<MailboxId>,
        request: String,
    ) -> Result<OutboxChange, StoreError> {
        self.write(move |tx| enqueue(tx, account, &mailboxes, &request, now_millis()))
            .await
    }
}
