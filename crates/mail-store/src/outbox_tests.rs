//! The outbox in the store (stage 4): local effects, cancelling, sync writes, revert, undo,
//! retry, write access. Against the real store in a temporary directory.

use mail_model::{
    AccountId, ActionError, ConflictReason, ErrorCode, Flag, FlagUpdate, Flags, IpcError,
    MailAction, MailboxId, MailboxRole, MessageActivity, MessageId, MessageRef, ModSeq,
    OperationPayload, OperationState, OutboxCounts, UidValidity,
};

use crate::test_support::{Fixture, UV, count, fixture, header};
use crate::{OperationResult, OutboxChange, StoreError, StoredOperation};

const D: i64 = 1_764_000_000;

/// The fixture with write access and `n` unread messages in the inbox (UIDs 1..=n).
async fn writable(n: u32) -> (Fixture, Vec<MessageRef>) {
    let f = fixture().await;
    f.store
        .set_writes_enabled(f.account, true)
        .await
        .expect("writes on");
    let headers = (1..=n)
        .map(|uid| {
            let mut h = header(uid, D + i64::from(uid), Some(&format!("{uid}@outbox")));
            h.modseq = Some(ModSeq(10 + u64::from(uid)));
            h
        })
        .collect();
    f.store
        .store_headers(f.inbox, UV, headers, None)
        .await
        .expect("headers");
    let refs = refs_in(&f, f.inbox).await;
    (f, refs)
}

async fn refs_in(f: &Fixture, mailbox: MailboxId) -> Vec<MessageRef> {
    let mut items = f
        .store
        .list_messages(mailbox, None, 500)
        .await
        .expect("list")
        .items;
    // Oldest first, so refs[0] has UID 1.
    items.reverse();
    items
        .into_iter()
        .map(|i| MessageRef {
            mailbox_id: mailbox,
            message_id: i.id,
        })
        .collect()
}

async fn flags_of(f: &Fixture, at: MessageRef) -> Option<(bool, bool, Vec<String>)> {
    f.store
        .list_messages(at.mailbox_id, None, 500)
        .await
        .expect("list")
        .items
        .into_iter()
        .find(|i| i.id == at.message_id)
        .map(|i| (!i.unread, i.flagged, i.keywords))
}

async fn act(f: &Fixture, action: MailAction, at: &[MessageRef], request: &str) -> OutboxChange {
    f.store
        .enqueue_action(action, at.to_vec(), request.to_owned())
        .await
        .unwrap_or_else(|err| panic!("enqueue {request}: {err}"))
}

async fn pending(f: &Fixture) -> Vec<StoredOperation> {
    f.store
        .pending_operations(f.account)
        .await
        .expect("pending")
}

fn confirmed() -> OperationResult {
    OperationResult::Confirmed {
        flags: None,
        moved_to: None,
        notice: None,
    }
}

fn action_error(err: StoreError) -> ActionError {
    match err {
        StoreError::Action(err) => err,
        other => panic!("expected an action error, got {other}"),
    }
}

#[tokio::test]
async fn accounts_without_write_access_refuse_every_action_before_recording() {
    let f = fixture().await;
    f.store
        .store_headers(f.inbox, UV, vec![header(1, D, Some("1@x"))], None)
        .await
        .expect("headers");
    let at = refs_in(&f, f.inbox).await;
    let actions = [
        MailAction::MarkRead,
        MailAction::MarkUnread,
        MailAction::Flag,
        MailAction::Unflag,
        MailAction::AddKeyword {
            keyword: "$Label1".into(),
        },
        MailAction::RemoveKeyword {
            keyword: "$Label1".into(),
        },
        MailAction::Archive,
        MailAction::Delete,
        MailAction::DeletePermanently,
        MailAction::Move { target: f.sent },
        MailAction::Spam,
        MailAction::NotSpam,
    ];
    let before = flags_of(&f, at[0]).await;
    for (n, action) in actions.into_iter().enumerate() {
        let err = f
            .store
            .enqueue_action(action.clone(), at.clone(), format!("ro-{n}"))
            .await
            .expect_err("read-only");
        assert_eq!(
            action_error(err),
            ActionError::ReadOnly(f.account),
            "{action:?}"
        );
    }
    let err = f
        .store
        .enqueue_create_mailbox(f.account, "New".into(), None, "ro-create".into())
        .await
        .expect_err("read-only");
    assert_eq!(action_error(err), ActionError::ReadOnly(f.account));
    assert_eq!(count(&f.store, "SELECT count(*) FROM operation").await, 0);
    assert_eq!(flags_of(&f, at[0]).await, before, "no local change");
    let core = mail_model::CoreError::from(StoreError::Action(ActionError::ReadOnly(f.account)));
    assert_eq!(core.code(), ErrorCode::ReadOnly);
}

#[tokio::test]
async fn flags_change_locally_at_once_and_opposites_cancel_out() {
    let (f, at) = writable(3).await;
    let change = act(&f, MailAction::MarkRead, &at[..2], "r1").await;
    assert_eq!(change.operations.len(), 2);
    assert_eq!(change.mailboxes, [f.inbox]);
    let batch = change.batch.expect("batch");
    assert_eq!(
        batch.0, change.operations[0].id.0,
        "batch = first operation"
    );
    assert!(change.operations.iter().all(|op| op.batch == batch));
    assert_eq!(flags_of(&f, at[0]).await.map(|f| f.0), Some(true));

    // Again: already read locally, nothing to record.
    let again = act(&f, MailAction::MarkRead, &at[..1], "r2").await;
    assert!(again.operations.is_empty() && again.removed.is_empty() && again.batch.is_none());

    // Unread before the server saw "read": both go, nothing is left to send.
    let unread = act(&f, MailAction::MarkUnread, &at[..1], "r3").await;
    assert_eq!(unread.removed, [change.operations[0].id]);
    assert!(unread.operations.is_empty());
    assert_eq!(flags_of(&f, at[0]).await.map(|f| f.0), Some(false));
    assert_eq!(pending(&f).await.len(), 1);

    // An operation in flight is not cancelled: the opposite is queued behind it.
    let second = change.operations[1].clone();
    f.store
        .begin_operations(vec![(second.id, second.payload.clone())])
        .await
        .expect("begin");
    let behind = act(&f, MailAction::MarkUnread, &at[1..2], "r4").await;
    assert!(behind.removed.is_empty());
    assert_eq!(behind.operations.len(), 1);
    let states: Vec<_> = pending(&f).await.iter().map(|op| op.state).collect();
    assert_eq!(states, [OperationState::InFlight, OperationState::Queued]);
    assert_eq!(pending(&f).await[0].attempts, 1);

    // Keywords: validated, applied, compared ignoring case.
    let err = f
        .store
        .enqueue_action(
            MailAction::AddKeyword {
                keyword: "two words".into(),
            },
            at.clone(),
            "kw-bad".into(),
        )
        .await
        .expect_err("invalid");
    assert_eq!(
        action_error(err),
        ActionError::InvalidKeyword("two words".into())
    );
    act(
        &f,
        MailAction::AddKeyword {
            keyword: "Projekt".into(),
        },
        &at[2..],
        "kw1",
    )
    .await;
    assert_eq!(
        flags_of(&f, at[2]).await.map(|f| f.2),
        Some(vec!["Projekt".to_owned()])
    );
    let gone = act(
        &f,
        MailAction::RemoveKeyword {
            keyword: "PROJEKT".into(),
        },
        &at[2..],
        "kw2",
    )
    .await;
    assert_eq!(gone.removed.len(), 1, "cancelled the queued add");
    assert_eq!(flags_of(&f, at[2]).await.map(|f| f.2), Some(vec![]));
}

#[tokio::test]
async fn the_same_request_is_recorded_once() {
    let (f, at) = writable(1).await;
    let first = act(&f, MailAction::Flag, &at, "same").await;
    let second = act(&f, MailAction::Flag, &at, "same").await;
    assert!(second.repeated);
    assert_eq!(second.batch, first.batch);
    assert_eq!(pending(&f).await.len(), 1);
}

#[tokio::test]
async fn sync_writes_keep_pending_flags_until_the_server_confirms() {
    let (f, at) = writable(2).await;
    let change = act(&f, MailAction::MarkRead, &at[..1], "s1").await;
    let op = change.operations[0].clone();
    // The sync brings the server's state (still unread): the pending change stays visible.
    let server = |seen: bool, modseq: u64| FlagUpdate {
        uid: 1,
        flags: Flags {
            seen,
            ..Flags::default()
        },
        modseq: Some(ModSeq(modseq)),
    };
    f.store
        .update_flags(f.inbox, UV, vec![server(false, 20)])
        .await
        .expect("sync flags");
    assert_eq!(flags_of(&f, at[0]).await.map(|f| f.0), Some(true));
    f.store
        .store_headers(f.inbox, UV, vec![header(1, D + 1, Some("1@outbox"))], None)
        .await
        .expect("sync headers");
    assert_eq!(flags_of(&f, at[0]).await.map(|f| f.0), Some(true));

    // Confirmed with the server's answer: written, MODSEQ included.
    f.store
        .finish_operations(vec![(
            op.id,
            OperationResult::Confirmed {
                flags: Some(server(true, 21)),
                moved_to: None,
                notice: None,
            },
        )])
        .await
        .expect("finish");
    let stored = f.store.operation(op.id).await.expect("op").expect("exists");
    assert_eq!(stored.state, OperationState::Confirmed);
    assert!(pending(&f).await.is_empty());
    let flag = act(&f, MailAction::Flag, &at[..1], "s2").await;
    assert_eq!(
        pending(&f).await[0].current_modseq,
        Some(ModSeq(21)),
        "the next STORE sees the MODSEQ of the confirmed one"
    );

    // Also when a move queued behind removed the location: the waiting flag operation learns
    // the MODSEQ of an earlier confirmed STORE through its payload.
    let label = act(
        &f,
        MailAction::AddKeyword {
            keyword: "$L".into(),
        },
        &at[1..2],
        "s3",
    )
    .await;
    let flagged = act(&f, MailAction::Flag, &at[1..2], "s4").await;
    act(&f, MailAction::Archive, &at[1..2], "s5").await;
    f.store
        .finish_operations(vec![(
            label.operations[0].id,
            OperationResult::Confirmed {
                flags: Some(FlagUpdate {
                    uid: 2,
                    flags: Flags::from_imap(["$L"]),
                    modseq: Some(ModSeq(40)),
                }),
                moved_to: None,
                notice: None,
            },
        )])
        .await
        .expect("finish");
    let waiting = f
        .store
        .operation(flagged.operations[0].id)
        .await
        .expect("read")
        .expect("exists");
    assert!(
        waiting.current_modseq.is_none(),
        "the location is gone locally"
    );
    assert_eq!(
        waiting.payload.source().and_then(|s| s.modseq),
        Some(ModSeq(40))
    );
    // A conflict with the server's flags adopts them (server wins).
    f.store
        .finish_operations(vec![(
            flag.operations[0].id,
            OperationResult::Conflict {
                reason: ConflictReason::Modified,
                flags: Some(FlagUpdate {
                    uid: 1,
                    flags: Flags::from_imap(["\\Seen", "$Other"]),
                    modseq: Some(ModSeq(30)),
                }),
            },
        )])
        .await
        .expect("conflict");
    assert_eq!(
        flags_of(&f, at[0]).await,
        Some((true, false, vec!["$Other".to_owned()]))
    );
    let stored = f
        .store
        .operation(flag.operations[0].id)
        .await
        .expect("op")
        .expect("exists");
    assert_eq!(stored.state, OperationState::Conflict);
    assert_eq!(stored.conflict, Some(ConflictReason::Modified));
}

#[tokio::test]
async fn moves_remove_the_location_and_failures_put_it_back() {
    let (f, at) = writable(3).await;
    act(&f, MailAction::Flag, &at[..1], "m0").await;
    let change = act(&f, MailAction::Archive, &at[..2], "m1").await;
    assert_eq!(change.operations.len(), 2);
    assert!(
        flags_of(&f, at[0]).await.is_none(),
        "gone from the inbox at once"
    );
    assert_eq!(refs_in(&f, f.inbox).await.len(), 1);
    let OperationPayload::Move { source, target, .. } = &change.operations[0].payload else {
        panic!("a move");
    };
    assert_eq!(target.mailbox_id, f.archive);
    assert!(source.flags.flagged, "the flags it had are kept");
    assert_eq!(source.modseq, Some(ModSeq(11)));

    // The message is not lost to the orphan cleanup while the move is pending or confirmed.
    assert_eq!(
        f.store
            .delete_orphan_messages(f.account)
            .await
            .expect("orphans"),
        0
    );

    // Failure: back where it was, with its flags.
    let error = IpcError {
        code: ErrorCode::ServerRejected,
        message: "refused".into(),
        detail: None,
    };
    let failed = f
        .store
        .finish_operations(vec![(
            change.operations[0].id,
            OperationResult::Failed {
                error: error.clone(),
            },
        )])
        .await
        .expect("finish")
        .expect("changed");
    assert_eq!(failed.mailboxes, [f.inbox]);
    assert_eq!(failed.operations[0].last_error, Some(error));
    assert_eq!(flags_of(&f, at[0]).await.map(|f| f.1), Some(true));

    // Confirmed with COPYUID: known in the target before its sync.
    f.store
        .finish_operations(vec![(
            change.operations[1].id,
            OperationResult::Confirmed {
                flags: None,
                moved_to: Some((UV, 77)),
                notice: None,
            },
        )])
        .await
        .expect("finish");
    let in_archive = MessageRef {
        mailbox_id: f.archive,
        message_id: at[1].message_id,
    };
    assert!(flags_of(&f, in_archive).await.is_some());
    assert!(
        f.store
            .known_uids(f.archive)
            .await
            .expect("uids")
            .contains(77)
    );

    // COPYUID of another generation of the target is not used; the sync merges it later.
    let third = act(&f, MailAction::Archive, &at[2..], "m2").await;
    f.store
        .finish_operations(vec![(
            third.operations[0].id,
            OperationResult::Confirmed {
                flags: None,
                moved_to: Some((UidValidity(UV.0 + 1), 78)),
                notice: None,
            },
        )])
        .await
        .expect("finish");
    assert!(
        !f.store
            .known_uids(f.archive)
            .await
            .expect("uids")
            .contains(78)
    );
    // Still protected from the orphan cleanup until the confirmed operation is purged.
    assert_eq!(
        f.store
            .delete_orphan_messages(f.account)
            .await
            .expect("orphans"),
        0
    );
    assert_eq!(
        f.store
            .purge_operations(f.account, i64::MAX)
            .await
            .expect("purge"),
        2,
        "the two confirmed moves; failed and queued ones stay"
    );
    assert_eq!(
        f.store
            .delete_orphan_messages(f.account)
            .await
            .expect("orphans"),
        1
    );
}

#[tokio::test]
async fn roles_confirmation_and_unknown_messages_are_typed_errors() {
    let (f, at) = writable(1).await;
    // The fixture has no trash and no junk.
    for (action, role) in [
        (MailAction::Delete, MailboxRole::Trash),
        (MailAction::Spam, MailboxRole::Junk),
    ] {
        let err = f
            .store
            .enqueue_action(action, at.clone(), format!("{role:?}"))
            .await
            .expect_err("missing role");
        assert_eq!(action_error(err), ActionError::MissingMailbox(role));
    }
    let stale = MessageRef {
        mailbox_id: f.inbox,
        message_id: MessageId(5),
    };
    let err = f
        .store
        .enqueue_action(MailAction::MarkRead, vec![stale], "stale".into())
        .await
        .expect_err("unknown");
    assert_eq!(action_error(err), ActionError::MessageNotFound(stale));
    let err = f
        .store
        .enqueue_action(MailAction::MarkRead, Vec::new(), "none".into())
        .await
        .expect_err("empty");
    assert_eq!(action_error(err), ActionError::NoMessages);
    let err = f
        .store
        .enqueue_action(
            MailAction::Move {
                target: MailboxId(999),
            },
            at.clone(),
            "target".into(),
        )
        .await
        .expect_err("bad target");
    assert_eq!(
        action_error(err),
        ActionError::InvalidTarget(MailboxId(999))
    );
    assert_eq!(count(&f.store, "SELECT count(*) FROM operation").await, 0);

    // The trash exists once the user gives a mailbox that role; then deleting from it needs
    // the explicit action.
    f.store
        .set_mailbox_role(f.sent, MailboxRole::Trash)
        .await
        .expect("role");
    act(&f, MailAction::Delete, &at, "to-trash").await;
    assert!(
        refs_in(&f, f.sent).await.is_empty(),
        "the trash shows it once the server confirmed"
    );
    f.store
        .store_headers(f.sent, UV, vec![header(9, D, Some("9@trash"))], None)
        .await
        .expect("a message in the trash");
    let in_trash = refs_in(&f, f.sent).await;
    let err = f
        .store
        .enqueue_action(MailAction::Delete, in_trash.clone(), "again".into())
        .await
        .expect_err("needs the explicit action");
    assert_eq!(action_error(err), ActionError::ConfirmationRequired);
    let gone = act(&f, MailAction::DeletePermanently, &in_trash, "for-good").await;
    assert!(matches!(
        gone.operations[0].payload,
        OperationPayload::Expunge { .. }
    ));
}

#[tokio::test]
async fn undo_cancels_queued_and_reverses_confirmed_operations() {
    let (f, at) = writable(3).await;
    // Queued: cancelled, the local change goes back.
    let archived = act(&f, MailAction::Archive, &at[..1], "u1").await;
    let undone = f
        .store
        .undo_batch(archived.batch.expect("batch"), "u1-undo".into())
        .await
        .expect("undo");
    assert_eq!(undone.removed.len(), 1);
    assert!(undone.batch.is_none(), "nothing new to send");
    assert!(flags_of(&f, at[0]).await.is_some(), "back in the inbox");
    assert!(pending(&f).await.is_empty());

    // Confirmed flags: counter-operations.
    let read = act(&f, MailAction::MarkRead, &at[..2], "u2").await;
    let results = read
        .operations
        .iter()
        .map(|op| (op.id, confirmed()))
        .collect();
    f.store.finish_operations(results).await.expect("finish");
    let undone = f
        .store
        .undo_batch(read.batch.expect("batch"), "u2-undo".into())
        .await
        .expect("undo");
    assert_eq!(undone.operations.len(), 2);
    assert!(
        undone
            .operations
            .iter()
            .all(|op| op.action == mail_model::ActionKind::Undo)
    );
    assert_eq!(flags_of(&f, at[0]).await.map(|f| f.0), Some(false));

    // Confirmed move: back from where it is now (known through COPYUID).
    let moved = act(&f, MailAction::Archive, &at[2..], "u3").await;
    f.store
        .finish_operations(vec![(
            moved.operations[0].id,
            OperationResult::Confirmed {
                flags: None,
                moved_to: Some((UV, 50)),
                notice: None,
            },
        )])
        .await
        .expect("finish");
    let back = f
        .store
        .undo_batch(moved.batch.expect("batch"), "u3-undo".into())
        .await
        .expect("undo");
    let OperationPayload::Move { source, target, .. } = &back.operations[0].payload else {
        panic!("a move back");
    };
    assert_eq!((source.mailbox_id, source.uid), (f.archive, 50));
    assert_eq!(target.mailbox_id, f.inbox);

    // A permanent delete cannot be undone, and the refusal changes nothing.
    let deleted = act(&f, MailAction::DeletePermanently, &at[1..2], "u4").await;
    f.store
        .finish_operations(vec![(deleted.operations[0].id, confirmed())])
        .await
        .expect("finish");
    let before = pending(&f).await;
    let err = f
        .store
        .undo_batch(deleted.batch.expect("batch"), "u4-undo".into())
        .await
        .expect_err("not undoable");
    assert!(matches!(action_error(err), ActionError::NotUndoable(_)));
    assert_eq!(pending(&f).await, before);
}

#[tokio::test]
async fn failed_operations_can_be_retried_or_discarded() {
    let (f, at) = writable(1).await;
    let change = act(&f, MailAction::Archive, &at, "f1").await;
    let id = change.operations[0].id;
    let err = f.store.discard_operation(id).await.expect_err("pending");
    assert!(matches!(
        action_error(err),
        ActionError::WrongState(_, OperationState::Queued)
    ));
    let error = IpcError {
        code: ErrorCode::ServerRejected,
        message: "x".into(),
        detail: None,
    };
    f.store
        .finish_operations(vec![(id, OperationResult::Failed { error })])
        .await
        .expect("fail");
    assert!(flags_of(&f, at[0]).await.is_some());

    let retried = f
        .store
        .retry_operation(id, "f1-retry".into())
        .await
        .expect("retry");
    assert_eq!(retried.removed, [id]);
    assert_eq!(retried.batch, change.batch);
    let again = retried.operations[0].clone();
    assert!(again.id > id, "queued at the end");
    assert_eq!(again.state, OperationState::Queued);
    assert_eq!(again.attempts, 0);
    assert!(again.last_error.is_none());
    assert!(
        flags_of(&f, at[0]).await.is_none(),
        "moved away again locally"
    );
    let id = again.id;

    f.store
        .finish_operations(vec![(
            id,
            OperationResult::Conflict {
                reason: ConflictReason::Gone,
                flags: None,
            },
        )])
        .await
        .expect("conflict");
    // The server has it elsewhere: it stays away locally, and a retry has nothing to move.
    assert!(flags_of(&f, at[0]).await.is_none());
    let err = f
        .store
        .retry_operation(id, "f1-retry-2".into())
        .await
        .expect_err("gone");
    assert!(matches!(action_error(err), ActionError::MessageNotFound(_)));
    let discarded = f.store.discard_operation(id).await.expect("discard");
    assert_eq!(discarded.removed, [id]);
    assert!(f.store.operation(id).await.expect("read").is_none());
}

async fn activity_of(f: &Fixture, at: MessageRef) -> Option<MessageActivity> {
    f.store
        .list_messages(at.mailbox_id, None, 500)
        .await
        .expect("list")
        .items
        .into_iter()
        .find(|i| i.id == at.message_id)
        .expect("listed")
        .activity
}

/// The list row shows what the outbox still does with the message (stage 4: a small symbol),
/// and the account's counts follow.
#[tokio::test]
async fn list_rows_and_counts_show_unfinished_operations() {
    let (f, at) = writable(3).await;
    let counts = |pending, failed, conflict| OutboxCounts {
        account_id: f.account,
        pending,
        failed,
        conflict,
    };
    assert_eq!(
        f.store.outbox_counts(f.account).await.expect("counts"),
        counts(0, 0, 0)
    );
    assert!(activity_of(&f, at[0]).await.is_none());

    // Two operations on the first message, one on the second.
    let flag = act(&f, MailAction::Flag, &at[..2], "a1").await;
    let read = act(&f, MailAction::MarkRead, &at[..1], "a2").await;
    assert_eq!(activity_of(&f, at[0]).await, Some(MessageActivity::Pending));
    assert_eq!(activity_of(&f, at[1]).await, Some(MessageActivity::Pending));
    assert!(activity_of(&f, at[2]).await.is_none());
    assert_eq!(
        f.store.outbox_counts(f.account).await.expect("counts"),
        counts(3, 0, 0)
    );

    // Failed wins over pending on the same row; confirmed operations show nothing.
    let error = IpcError {
        code: ErrorCode::ServerRejected,
        message: "x".into(),
        detail: None,
    };
    let first_flag = flag.operations[0].id;
    let second_flag = flag.operations[1].id;
    f.store
        .finish_operations(vec![
            (first_flag, OperationResult::Failed { error }),
            (second_flag, confirmed()),
        ])
        .await
        .expect("finish");
    assert_eq!(activity_of(&f, at[0]).await, Some(MessageActivity::Failed));
    assert!(activity_of(&f, at[1]).await.is_none(), "confirmed");
    assert_eq!(
        f.store.outbox_counts(f.account).await.expect("counts"),
        counts(1, 1, 0)
    );

    f.store
        .finish_operations(vec![(
            read.operations[0].id,
            OperationResult::Conflict {
                reason: ConflictReason::Modified,
                flags: None,
            },
        )])
        .await
        .expect("conflict");
    assert_eq!(activity_of(&f, at[0]).await, Some(MessageActivity::Failed));
    assert_eq!(
        f.store.outbox_counts(f.account).await.expect("counts"),
        counts(0, 1, 1)
    );
    f.store
        .discard_operation(first_flag)
        .await
        .expect("discard");
    assert_eq!(
        activity_of(&f, at[0]).await,
        Some(MessageActivity::Conflict)
    );
    f.store
        .discard_operation(read.operations[0].id)
        .await
        .expect("discard");
    assert!(activity_of(&f, at[0]).await.is_none());
    assert_eq!(
        f.store.outbox_counts(f.account).await.expect("counts"),
        counts(0, 0, 0)
    );

    // A failed move puts the row back, with the symbol.
    let archive = act(&f, MailAction::Archive, &at[2..], "a3").await;
    assert!(flags_of(&f, at[2]).await.is_none(), "moved away locally");
    let error = IpcError {
        code: ErrorCode::ServerRejected,
        message: "x".into(),
        detail: None,
    };
    f.store
        .finish_operations(vec![(
            archive.operations[0].id,
            OperationResult::Failed { error },
        )])
        .await
        .expect("fail");
    assert_eq!(activity_of(&f, at[2]).await, Some(MessageActivity::Failed));

    let other: AccountId = "6f9619ff-8b86-4d11-b42d-00c04fc964ff".parse().expect("id");
    assert!(matches!(
        f.store.outbox_counts(other).await,
        Err(StoreError::NotFound(_))
    ));
}

#[tokio::test]
async fn turning_writes_off_fails_queued_operations_and_reverts_them() {
    let (f, at) = writable(2).await;
    let read = act(&f, MailAction::MarkRead, &at[..1], "w1").await;
    let moved = act(&f, MailAction::Archive, &at[1..], "w2").await;
    // One is already in flight: it waits for writes to come back, then gets checked.
    f.store
        .begin_operations(vec![(
            moved.operations[0].id,
            moved.operations[0].payload.clone(),
        )])
        .await
        .expect("begin");
    let change = f
        .store
        .set_writes_enabled(f.account, false)
        .await
        .expect("writes off");
    assert_eq!(change.operations.len(), 1);
    assert_eq!(change.operations[0].id, read.operations[0].id);
    assert_eq!(change.operations[0].state, OperationState::Failed);
    assert_eq!(
        change.operations[0].last_error.as_ref().map(|e| e.code),
        Some(ErrorCode::ReadOnly)
    );
    assert_eq!(flags_of(&f, at[0]).await.map(|f| f.0), Some(false));
    let left: Vec<_> = pending(&f).await.iter().map(|op| op.state).collect();
    assert_eq!(left, [OperationState::InFlight]);
    let err = f
        .store
        .retry_operation(read.operations[0].id, "w-retry".into())
        .await
        .expect_err("read-only");
    assert_eq!(action_error(err), ActionError::ReadOnly(f.account));
}

#[tokio::test]
async fn creating_a_mailbox_is_an_operation_of_its_own() {
    let (f, _) = writable(0).await;
    let change = f
        .store
        .enqueue_create_mailbox(
            f.account,
            " Archiv ".into(),
            Some(MailboxRole::Archive),
            "c1".into(),
        )
        .await
        .expect("create");
    assert_eq!(
        change.operations[0].payload,
        OperationPayload::CreateMailbox {
            path: "Archiv".into(),
            role: Some(MailboxRole::Archive)
        }
    );
    assert!(
        change.mailboxes.is_empty(),
        "no local row before the server has it"
    );
    for (path, role) in [("", None), ("x", Some(MailboxRole::Inbox))] {
        let err = f
            .store
            .enqueue_create_mailbox(f.account, path.into(), role, format!("bad-{path}"))
            .await
            .expect_err("invalid");
        assert!(matches!(err, StoreError::InvalidArgument(_)), "{err}");
    }
    let info = change.operations[0].info();
    assert_eq!(info.kind, mail_model::OperationKind::CreateMailbox);
    assert!(info.mailbox_id.is_none() && info.message_id.is_none());
    assert_eq!(
        f.store
            .operations(f.account, None, 10)
            .await
            .expect("activity")
            .len(),
        1
    );
}

#[test]
fn flag_helper_is_case_insensitive_for_keywords() {
    assert!(Flag::Keyword("A".into()).same(&Flag::Keyword("a".into())));
}
