//! Stage 21 (decision 0063): "Mark folder as read" in the store: one operation per folder with
//! the UIDs that were unread, its local effect, sync writes, revert, retry, undo, write access
//! and the query plans. Against the real store in a temporary directory.

use mail_model::{
    ActionError, ActionKind, ErrorCode, FlagUpdate, Flags, IpcError, MailboxId, ModSeq,
    OperationKind, OperationPayload, OperationState, UidSet, UidValidity,
};
use rusqlite::Connection;

use crate::test_support::{Fixture, UV, count, fixture, header, new_account, remote};
use crate::{
    MailboxBaseline, OperationResult, OutboxChange, StoreError, StoredOperation, mark_read,
};

const D: i64 = 1_764_000_000;

/// The fixture with write access and `n` unread messages in the inbox (UIDs 1..=n).
async fn writable(n: u32) -> Fixture {
    let f = fixture().await;
    f.store
        .set_writes_enabled(f.account, true)
        .await
        .expect("writes on");
    let headers = (1..=n)
        .map(|uid| {
            let mut h = header(uid, D + i64::from(uid), Some(&format!("{uid}@mark")));
            h.modseq = Some(ModSeq(10 + u64::from(uid)));
            h
        })
        .collect();
    f.store
        .store_headers(f.inbox, UV, headers, None)
        .await
        .expect("headers");
    f
}

/// UIDs of the unread messages of a folder, ascending.
async fn unread(f: &Fixture, mailbox: MailboxId) -> Vec<u32> {
    f.store
        .read(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT uid FROM message_location WHERE mailbox_id = ?1 AND seen = 0 ORDER BY uid",
            )?;
            Ok(stmt
                .query_map([mailbox.0], |r| r.get::<_, u32>(0))?
                .collect::<Result<Vec<_>, _>>()?)
        })
        .await
        .expect("unread")
}

async fn counter(f: &Fixture, mailbox: MailboxId) -> u32 {
    f.store
        .mailboxes(f.account)
        .await
        .expect("mailboxes")
        .into_iter()
        .find(|m| m.id == mailbox)
        .map(|m| m.unread)
        .expect("mailbox")
}

async fn mark(f: &Fixture, mailboxes: &[MailboxId], request: &str) -> OutboxChange {
    f.store
        .enqueue_mark_read(f.account, mailboxes.to_vec(), request.to_owned())
        .await
        .unwrap_or_else(|err| panic!("mark {request}: {err}"))
}

/// The server's flags as a sync writes them: `seen` on these UIDs, the rest unread.
async fn server_says(f: &Fixture, uids: &[(u32, bool)]) {
    let updates = uids
        .iter()
        .map(|(uid, seen)| FlagUpdate {
            uid: *uid,
            flags: Flags {
                seen: *seen,
                ..Flags::default()
            },
            modseq: Some(ModSeq(100 + u64::from(*uid))),
            gmail_labels: None,
        })
        .collect();
    f.store
        .update_flags(f.inbox, UV, updates)
        .await
        .expect("sync write");
}

fn uids_of(op: &StoredOperation) -> (UidSet, bool) {
    match &op.payload {
        OperationPayload::MarkMailbox { uids, seen, .. } => (uids.clone(), *seen),
        other => panic!("not a folder mark: {other:?}"),
    }
}

async fn finish(f: &Fixture, op: &StoredOperation, result: OperationResult) {
    f.store
        .begin_operations(vec![(op.id, op.payload.clone())])
        .await
        .expect("begin");
    f.store
        .finish_operations(vec![(op.id, result)])
        .await
        .expect("finish");
}

fn confirmed() -> OperationResult {
    OperationResult::Confirmed {
        flags: None,
        moved_to: None,
        notice: None,
    }
}

#[tokio::test]
async fn a_folder_is_marked_read_with_one_operation_and_its_counter_drops_at_once() {
    let f = writable(5).await;
    server_says(&f, &[(2, true)]).await;
    assert_eq!(counter(&f, f.inbox).await, 4);

    let change = mark(&f, &[f.inbox], "mark-1").await;
    assert!(change.batch.is_some() && !change.repeated);
    assert_eq!(change.mailboxes, [f.inbox], "the inbox changed locally");
    let [op] = change.operations.as_slice() else {
        panic!("one operation: {:?}", change.operations);
    };
    assert_eq!(op.action, ActionKind::MarkMailboxRead);
    assert_eq!(op.payload.kind(), OperationKind::MarkMailbox);
    assert_eq!(op.state, OperationState::Queued);
    assert_eq!((op.mailbox_id, op.message_id), (Some(f.inbox), None));
    let (uids, seen) = uids_of(op);
    assert_eq!((uids.to_string(), seen), ("1,3:5".to_owned(), true));
    assert_eq!(op.info().message_count, Some(4));
    if let OperationPayload::MarkMailbox {
        mailbox,
        uid_validity,
        ..
    } = &op.payload
    {
        assert_eq!((mailbox.name.as_str(), *uid_validity), ("INBOX", UV));
    }
    assert_eq!(counter(&f, f.inbox).await, 0);
    assert!(unread(&f, f.inbox).await.is_empty());

    // The same request again changes nothing; nothing unread records nothing.
    let again = mark(&f, &[f.inbox], "mark-1").await;
    assert!(again.repeated && again.operations.is_empty());
    assert_eq!(again.batch, change.batch);
    let nothing = mark(&f, &[f.inbox, f.archive, f.inbox], "mark-2").await;
    assert!(nothing.batch.is_none() && nothing.operations.is_empty());
    assert_eq!(
        count(&f.store, "SELECT count(*) FROM operation").await,
        1,
        "{nothing:?}"
    );
}

#[tokio::test]
async fn several_folders_are_one_batch_and_one_undo() {
    let f = writable(3).await;
    f.store
        .store_headers(
            f.archive,
            UV,
            vec![
                header(7, D, Some("7@mark")),
                header(9, D + 1, Some("9@mark")),
            ],
            None,
        )
        .await
        .expect("archive");
    let change = mark(&f, &[f.inbox, f.archive, f.sent], "several").await;
    assert_eq!(change.operations.len(), 2, "the empty Sent records nothing");
    assert!(
        change
            .operations
            .iter()
            .all(|op| Some(op.batch) == change.batch)
    );
    assert_eq!(uids_of(&change.operations[1]).0.to_string(), "7,9");
    assert_eq!(
        (counter(&f, f.inbox).await, counter(&f, f.archive).await),
        (0, 0)
    );

    let batch = change.batch.expect("batch");
    let undone = f
        .store
        .undo_batch(batch, "undo-several".into())
        .await
        .expect("undo");
    assert_eq!(
        undone.removed.len(),
        2,
        "queued marks are cancelled, not sent back"
    );
    assert!(undone.operations.is_empty());
    assert_eq!(unread(&f, f.inbox).await, [1, 2, 3]);
    assert_eq!(unread(&f, f.archive).await, [7, 9]);
    assert_eq!(count(&f.store, "SELECT count(*) FROM operation").await, 0);
}

#[tokio::test]
async fn read_only_accounts_foreign_and_missing_folders_are_refused_before_recording() {
    let f = fixture().await;
    f.store
        .store_headers(f.inbox, UV, vec![header(1, D, Some("1@x"))], None)
        .await
        .expect("headers");
    let err = f
        .store
        .enqueue_mark_read(f.account, vec![f.inbox], "ro".into())
        .await
        .expect_err("read-only");
    assert!(
        matches!(err, StoreError::Action(ActionError::ReadOnly(a)) if a == f.account),
        "{err}"
    );
    assert_eq!(counter(&f, f.inbox).await, 1);

    f.store
        .set_writes_enabled(f.account, true)
        .await
        .expect("writes on");
    let other = f
        .store
        .insert_account(new_account(2, "bob@example.org"))
        .await
        .expect("bob")
        .id;
    f.store
        .sync_mailboxes(other, vec![remote("INBOX", &[])])
        .await
        .expect("bob's folders");
    let bobs = f.store.mailboxes(other).await.expect("bob")[0].id;
    for (mailboxes, what) in [
        (vec![f.inbox, bobs], "another account's folder"),
        (vec![MailboxId(999_999)], "no such folder"),
    ] {
        let err = f
            .store
            .enqueue_mark_read(f.account, mailboxes, what.into())
            .await
            .expect_err(what);
        assert!(matches!(err, StoreError::NotFound(_)), "{what}: {err}");
    }
    let err = f
        .store
        .enqueue_mark_read(f.account, Vec::new(), "none".into())
        .await
        .expect_err("no folders");
    assert!(matches!(err, StoreError::InvalidArgument(_)), "{err}");
    // Nothing recorded, nothing marked: the inbox of the refused call stays unread.
    assert_eq!(count(&f.store, "SELECT count(*) FROM operation").await, 0);
    assert_eq!(counter(&f, f.inbox).await, 1);
}

#[tokio::test]
async fn a_pending_mark_wins_over_sync_writes_in_recorded_order_and_new_mail_stays_unread() {
    let f = writable(4).await;
    let change = mark(&f, &[f.inbox], "mark").await;
    let refs = f
        .store
        .list_messages(f.inbox, None, 10)
        .await
        .expect("list")
        .items;
    let two = refs
        .iter()
        .find(|i| i.subject == "Subject 2")
        .map(|i| mail_model::MessageRef {
            mailbox_id: f.inbox,
            message_id: i.id,
        })
        .expect("message 2");
    // After the mark, message 2 is marked unread again (an operation of its own, later).
    f.store
        .enqueue_action(
            mail_model::MailAction::MarkUnread,
            vec![two],
            "unread-2".into(),
        )
        .await
        .expect("unread 2");
    assert_eq!(unread(&f, f.inbox).await, [2]);

    // The sync still reads the server's old state (all unread): the user's changes win, in order.
    server_says(&f, &[(1, false), (2, false), (3, false), (4, false)]).await;
    assert_eq!(unread(&f, f.inbox).await, [2]);
    // New mail is not covered by the mark.
    f.store
        .store_headers(f.inbox, UV, vec![header(5, D + 5, Some("5@mark"))], None)
        .await
        .expect("new mail");
    assert_eq!(unread(&f, f.inbox).await, [2, 5]);

    // Once confirmed, the server's state counts again.
    let op = &change.operations[0];
    finish(&f, op, confirmed()).await;
    server_says(&f, &[(1, true), (3, true), (4, true)]).await;
    assert_eq!(unread(&f, f.inbox).await, [2, 5]);
}

#[tokio::test]
async fn a_failed_mark_is_reverted_retried_and_a_confirmed_one_is_undone_on_its_uids() {
    let f = writable(3).await;
    let change = mark(&f, &[f.inbox], "mark").await;
    let op = change.operations[0].clone();
    let error = IpcError {
        code: ErrorCode::ServerRejected,
        message: "refused".into(),
        detail: None,
    };
    finish(&f, &op, OperationResult::Failed { error }).await;
    let failed = f.store.operation(op.id).await.expect("read").expect("op");
    assert_eq!(failed.state, OperationState::Failed);
    assert_eq!(unread(&f, f.inbox).await, [1, 2, 3], "reverted");

    let retried = f
        .store
        .retry_operation(op.id, "retry".into())
        .await
        .expect("retry");
    assert_eq!(retried.removed, [op.id]);
    let again = retried.operations[0].clone();
    assert_eq!(again.batch, op.batch);
    assert_eq!(uids_of(&again), uids_of(&op));
    assert!(unread(&f, f.inbox).await.is_empty(), "marked again");
    finish(&f, &again, confirmed()).await;

    // Undo after the server did it: `\Seen` comes off the same UIDs; a message that arrived
    // read since keeps its flag.
    f.store
        .store_headers(f.inbox, UV, vec![seen_header(4)], None)
        .await
        .expect("new, read");
    let undone = f
        .store
        .undo_batch(op.batch, "undo".into())
        .await
        .expect("undo");
    let [inverse] = undone.operations.as_slice() else {
        panic!("one counter-operation: {undone:?}");
    };
    assert_eq!(inverse.action, ActionKind::Undo);
    assert_eq!(uids_of(inverse), (UidSet::from_uids([1, 2, 3]), false));
    assert_eq!(unread(&f, f.inbox).await, [1, 2, 3]);
    assert_eq!(undone.mailboxes, [f.inbox]);
}

fn seen_header(uid: u32) -> mail_model::MessageHeader {
    crate::test_support::seen(header(
        uid,
        D + i64::from(uid),
        Some(&format!("{uid}@mark")),
    ))
}

#[tokio::test]
async fn turning_writes_off_fails_a_queued_mark_and_a_rebuilt_folder_refuses_its_undo() {
    let f = writable(2).await;
    let change = mark(&f, &[f.inbox], "mark").await;
    f.store
        .set_writes_enabled(f.account, false)
        .await
        .expect("writes off");
    let op = f
        .store
        .operation(change.operations[0].id)
        .await
        .expect("read")
        .expect("op");
    assert_eq!(op.state, OperationState::Failed);
    assert_eq!(op.last_error.map(|e| e.code), Some(ErrorCode::ReadOnly));
    assert_eq!(unread(&f, f.inbox).await, [1, 2]);

    f.store
        .set_writes_enabled(f.account, true)
        .await
        .expect("writes on");
    let change = mark(&f, &[f.inbox], "mark-2").await;
    let op = change.operations[0].clone();
    // A conflict (the folder was rebuilt) puts the messages back in the old generation.
    finish(
        &f,
        &op,
        OperationResult::Conflict {
            reason: mail_model::ConflictReason::MailboxReset,
            flags: None,
        },
    )
    .await;
    assert_eq!(unread(&f, f.inbox).await, [1, 2]);

    let change = mark(&f, &[f.inbox], "mark-3").await;
    let op = change.operations[0].clone();
    finish(&f, &op, confirmed()).await;
    f.store
        .reset_mailbox(
            f.inbox,
            MailboxBaseline {
                uid_validity: UidValidity(UV.0 + 1),
                uid_next: 1,
                highest_modseq: None,
            },
        )
        .await
        .expect("rebuilt");
    let err = f
        .store
        .undo_batch(op.batch, "undo".into())
        .await
        .expect_err("not undoable");
    assert!(
        matches!(err, StoreError::Action(ActionError::NotUndoable(_))),
        "{err}"
    );
}

fn plan(conn: &Connection, sql: &str) -> Vec<String> {
    let mut stmt = conn
        .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
        .unwrap_or_else(|err| panic!("prepare {sql}: {err}"));
    let nulls = vec![rusqlite::types::Null; stmt.parameter_count()];
    stmt.query_map(rusqlite::params_from_iter(nulls), |r| r.get::<_, String>(3))
        .expect("plan")
        .collect::<Result<Vec<_>, _>>()
        .expect("rows")
}

#[tokio::test]
async fn the_mark_queries_use_their_index() {
    let f = writable(3).await;
    mark(&f, &[f.inbox], "mark").await;
    f.store
        .read(|conn| {
            for (sql, want) in [
                (
                    mark_read::MAILBOX_FOR_MARK,
                    "SEARCH mailbox USING INTEGER PRIMARY KEY (rowid=?)",
                ),
                (
                    mark_read::UNSEEN_UIDS,
                    "SEARCH message_location USING INDEX loc_unseen (mailbox_id=?)",
                ),
                (
                    mark_read::MARK_ALL_SEEN,
                    "SEARCH message_location USING INDEX loc_unseen (mailbox_id=?)",
                ),
                (
                    mark_read::SET_SEEN_AT,
                    "SEARCH message_location USING INDEX loc_uid (mailbox_id=? AND uid=?)",
                ),
                (
                    mark_read::SET_SEEN_RANGE,
                    "SEARCH message_location USING INDEX loc_uid (mailbox_id=? AND uid>? AND uid<?)",
                ),
                (
                    mark_read::PENDING_MARKS_IN,
                    "SEARCH operation USING INDEX operation_location (mailbox_id=? AND uid=?)",
                ),
            ] {
                let lines = plan(conn, sql);
                assert!(
                    lines.iter().any(|l| l.starts_with(want)),
                    "{sql}: {lines:#?}"
                );
                assert!(
                    lines.iter().all(|l| !l.contains("TEMP B-TREE") && !l.starts_with("SCAN")),
                    "{sql}: {lines:#?}"
                );
            }
            Ok(())
        })
        .await
        .expect("plans");
}
