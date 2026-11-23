//! Stage 14, block A (decision 0034): Gmail in the store. One message across labels by
//! `X-GM-MSGID`, labels from `X-GM-LABELS`, label changes and archive through the outbox.

use mail_model::{
    ActionError, Flag, FlagUpdate, Flags, GmailInfo, IpcError, MailAction, MailboxId, MailboxRole,
    MessageHeader, MessageId, MessageRef, OperationPayload,
};
use tempfile::TempDir;

use crate::test_support::{UV, count, db_path, fixture, header, new_account, remote};
use crate::{MailboxBaseline, OperationResult, StoreError, StoreOptions};
use crate::{Store, StoreReport};

const D: i64 = 1_764_000_000;

struct Gmail {
    _dir: TempDir,
    store: Store,
    account: mail_model::AccountId,
    inbox: MailboxId,
    all: MailboxId,
    work: MailboxId,
}

/// An account with Gmail's folders: INBOX, `[Gmail]/All Mail` (`\All`), `[Gmail]/Sent Mail`,
/// a label `Work`; writes on, Gmail seen at login.
async fn gmail() -> Gmail {
    let dir = TempDir::new().expect("temp dir");
    let store = Store::open(&db_path(&dir), StoreOptions::default()).expect("open");
    let account = store
        .insert_account(new_account(1, "alice@gmail.com"))
        .await
        .expect("account")
        .id;
    store
        .sync_mailboxes(
            account,
            vec![
                remote("INBOX", &[]),
                remote("[Gmail]", &["\\Noselect"]),
                remote("[Gmail]/All Mail", &["\\All"]),
                remote("[Gmail]/Sent Mail", &["\\Sent"]),
                remote("Work", &[]),
            ],
        )
        .await
        .expect("mailboxes");
    let boxes = store.mailboxes(account).await.expect("mailboxes");
    let by_path = |path: &str| {
        boxes
            .iter()
            .find(|m| m.path == path)
            .map(|m| m.id)
            .expect("mailbox")
    };
    let (inbox, all, work) = (
        by_path("INBOX"),
        by_path("[Gmail]/All Mail"),
        by_path("Work"),
    );
    assert_eq!(
        boxes.iter().find(|m| m.id == all).map(|m| m.role),
        Some(MailboxRole::All)
    );
    for mailbox in [inbox, all, work, by_path("[Gmail]/Sent Mail")] {
        store
            .reset_mailbox(
                mailbox,
                MailboxBaseline {
                    uid_validity: UV,
                    uid_next: 1,
                    highest_modseq: None,
                },
            )
            .await
            .expect("reset");
    }
    store
        .set_writes_enabled(account, true)
        .await
        .expect("writes");
    assert!(store.set_account_gmail(account, true).await.expect("gmail"));
    assert!(!store.set_account_gmail(account, true).await.expect("same"));
    assert!(store.account_is_gmail(account).await.expect("read"));
    Gmail {
        _dir: dir,
        store,
        account,
        inbox,
        all,
        work,
    }
}

fn gm(uid: u32, msgid: u64, message_id: Option<&str>, labels: &[&str]) -> MessageHeader {
    let mut h = header(uid, D + i64::from(uid), message_id);
    h.gmail = Some(GmailInfo {
        msgid,
        labels: labels.iter().map(|l| (*l).to_owned()).collect(),
    });
    h
}

async fn ids_in(store: &Store, mailbox: MailboxId) -> Vec<MessageId> {
    store
        .list_messages(mailbox, None, 100)
        .await
        .expect("list")
        .items
        .into_iter()
        .map(|i| i.id)
        .collect()
}

async fn labels(store: &Store, message: MessageId) -> Vec<String> {
    store
        .message_labels(message)
        .await
        .expect("labels")
        .expect("Gmail message")
}

#[tokio::test]
async fn one_gmail_message_across_labels_is_one_row_and_counted_once() {
    let g = gmail().await;
    // The inbox: one message without Message-ID, one with. X-GM-LABELS leaves out \Inbox.
    let report = g
        .store
        .store_headers(
            g.inbox,
            UV,
            vec![
                gm(1, 101, None, &["\\Important"]),
                gm(2, 102, Some("same@x"), &[]),
            ],
            None,
        )
        .await
        .expect("inbox");
    assert_eq!((report.inserted, report.new_unread), (2, 2));

    // All Mail has both again, plus a different message with the same Message-ID.
    let report = g
        .store
        .store_headers(
            g.all,
            UV,
            vec![
                gm(50, 101, None, &["\\Inbox", "\\Important"]),
                gm(51, 102, Some("same@x"), &["\\Inbox"]),
                gm(52, 103, Some("same@x"), &["Work"]),
            ],
            None,
        )
        .await
        .expect("all mail");
    assert_eq!(
        report,
        StoreReport {
            inserted: 1,
            merged: 2,
            updated: 0,
            new_unread: 1,
        },
        "the inbox messages are not new again; the third one is its own message"
    );
    // And the label folder.
    let report = g
        .store
        .store_headers(g.work, UV, vec![gm(7, 103, Some("same@x"), &[])], None)
        .await
        .expect("work");
    assert_eq!((report.merged, report.new_unread), (1, 0));

    assert_eq!(count(&g.store, "SELECT count(*) FROM message").await, 3);
    assert_eq!(
        count(&g.store, "SELECT count(*) FROM message_location").await,
        6
    );
    let inbox = ids_in(&g.store, g.inbox).await;
    assert_eq!(inbox.len(), 2);
    let all = ids_in(&g.store, g.all).await;
    assert_eq!(all.len(), 3);
    assert!(inbox.iter().all(|id| all.contains(id)), "same rows");

    let first = inbox[1];
    assert_eq!(labels(&g.store, first).await, ["\\Inbox", "\\Important"]);
    let third = ids_in(&g.store, g.work).await[0];
    assert_eq!(labels(&g.store, third).await, ["Work"]);
}

#[tokio::test]
async fn labels_follow_the_flag_sync_and_old_rows_get_their_number() {
    let g = gmail().await;
    // Stored before stage 14: no X-GM-MSGID.
    g.store
        .store_headers(g.inbox, UV, vec![header(1, D, Some("old@x"))], None)
        .await
        .expect("old inbox row");
    let report = g
        .store
        .store_headers(
            g.all,
            UV,
            vec![gm(9, 900, Some("old@x"), &["\\Inbox"])],
            None,
        )
        .await
        .expect("all mail");
    assert_eq!(report.merged, 1, "found by Message-ID, numbered now");
    assert_eq!(
        count(
            &g.store,
            "SELECT count(*) FROM message WHERE gm_msgid = 900"
        )
        .await,
        1
    );
    let id = ids_in(&g.store, g.inbox).await[0];
    assert_eq!(labels(&g.store, id).await, ["\\Inbox"]);

    // A label added in the web interface arrives with the flags.
    let changed = g
        .store
        .update_flags(
            g.inbox,
            UV,
            vec![FlagUpdate {
                uid: 1,
                flags: Flags::default(),
                modseq: None,
                gmail_labels: Some(vec!["Work".into(), "\\Starred".into()]),
            }],
        )
        .await
        .expect("flags");
    assert_eq!(changed, 1, "a label change is a change of the row");
    assert_eq!(
        labels(&g.store, id).await,
        ["\\Inbox", "\\Starred", "Work"],
        "the folder's own label is added back"
    );
    let again = g
        .store
        .update_flags(
            g.inbox,
            UV,
            vec![FlagUpdate {
                uid: 1,
                flags: Flags::default(),
                modseq: None,
                gmail_labels: Some(vec!["\\Starred".into(), "Work".into()]),
            }],
        )
        .await
        .expect("same again");
    assert_eq!(again, 0);
}

#[tokio::test]
async fn other_servers_have_no_labels_and_unchanged_merging() {
    let f = fixture().await;
    f.store
        .store_headers(f.inbox, UV, vec![header(1, D, Some("a@x"))], None)
        .await
        .expect("inbox");
    let report = f
        .store
        .store_headers(f.archive, UV, vec![header(5, D, Some("a@x"))], None)
        .await
        .expect("archive");
    assert_eq!((report.merged, report.new_unread), (1, 0));
    let id = ids_in(&f.store, f.inbox).await[0];
    assert_eq!(f.store.message_labels(id).await.expect("labels"), None);
    assert!(!f.store.account_is_gmail(f.account).await.expect("gmail"));
    f.store
        .set_writes_enabled(f.account, true)
        .await
        .expect("writes");
    let at = MessageRef {
        mailbox_id: f.inbox,
        message_id: id,
    };
    let err = f
        .store
        .enqueue_action(
            MailAction::AddLabel {
                label: "Work".into(),
            },
            vec![at],
            "label-1".into(),
        )
        .await
        .expect_err("no labels here");
    assert!(
        matches!(err, StoreError::Action(ActionError::LabelsUnsupported)),
        "{err:?}"
    );
    // Archive stays a move to the archive folder.
    let change = f
        .store
        .enqueue_action(MailAction::Archive, vec![at], "archive-1".into())
        .await
        .expect("archive");
    assert!(matches!(
        change.operations[0].payload,
        OperationPayload::Move { .. }
    ));
}

#[tokio::test]
async fn labels_change_through_the_outbox_and_revert_on_failure() {
    let g = gmail().await;
    g.store
        .store_headers(g.inbox, UV, vec![gm(1, 101, Some("m@x"), &[])], None)
        .await
        .expect("inbox");
    let id = ids_in(&g.store, g.inbox).await[0];
    let at = MessageRef {
        mailbox_id: g.inbox,
        message_id: id,
    };
    let err = g
        .store
        .enqueue_action(
            MailAction::AddLabel {
                label: "[Gmail]/All Mail".into(),
            },
            vec![at],
            "bad".into(),
        )
        .await
        .expect_err("not a label");
    assert!(
        matches!(err, StoreError::Action(ActionError::InvalidLabel(_))),
        "{err:?}"
    );

    let change = g
        .store
        .enqueue_action(
            MailAction::AddLabel {
                label: "Work".into(),
            },
            vec![at],
            "add".into(),
        )
        .await
        .expect("add label");
    let op = &change.operations[0];
    match &op.payload {
        OperationPayload::SetFlag {
            flag,
            value,
            before,
            ..
        } => {
            assert_eq!(flag, &Flag::Label("Work".into()));
            assert!(*value && !*before);
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(labels(&g.store, id).await, ["\\Inbox", "Work"]);
    // The same again is a no-op; the opposite cancels the queued one.
    let same = g
        .store
        .enqueue_action(
            MailAction::AddLabel {
                label: "Work".into(),
            },
            vec![at],
            "add-again".into(),
        )
        .await
        .expect("again");
    assert!(same.operations.is_empty());

    // The server refuses: the label goes again.
    g.store
        .finish_operations(vec![(
            op.id,
            OperationResult::Failed {
                error: IpcError::from(mail_model::CoreError::Mail(
                    mail_model::BackendError::Rejected("NO".into()),
                )),
            },
        )])
        .await
        .expect("finish");
    assert_eq!(labels(&g.store, id).await, ["\\Inbox"]);
}

#[tokio::test]
async fn archive_on_gmail_removes_the_inbox_label_and_undo_goes_through_all_mail() {
    let g = gmail().await;
    g.store
        .store_headers(g.inbox, UV, vec![gm(1, 101, Some("m@x"), &[])], None)
        .await
        .expect("inbox");
    g.store
        .store_headers(
            g.all,
            UV,
            vec![gm(40, 101, Some("m@x"), &["\\Inbox"])],
            None,
        )
        .await
        .expect("all mail");
    let id = ids_in(&g.store, g.inbox).await[0];
    let in_inbox = MessageRef {
        mailbox_id: g.inbox,
        message_id: id,
    };

    // No archive folder needed; the inbox location goes at once, All Mail keeps the message.
    let change = g
        .store
        .enqueue_action(MailAction::Archive, vec![in_inbox], "archive".into())
        .await
        .expect("archive");
    let [op] = change.operations.as_slice() else {
        panic!("one operation: {change:?}");
    };
    match &op.payload {
        OperationPayload::SetFlag {
            source,
            flag,
            value,
            before,
        } => {
            assert_eq!(source.mailbox_id, g.inbox);
            assert_eq!(source.uid, 1);
            assert_eq!(flag, &Flag::Label("\\Inbox".into()));
            assert!(!*value && *before);
        }
        other => panic!("archive on Gmail is no move: {other:?}"),
    }
    assert!(ids_in(&g.store, g.inbox).await.is_empty());
    assert_eq!(ids_in(&g.store, g.all).await, [id]);
    assert!(labels(&g.store, id).await.is_empty());

    // Undo while queued: cancelled, the message is back in the inbox with its flags.
    let batch = change.batch.expect("batch");
    g.store
        .undo_batch(batch, "undo-1".into())
        .await
        .expect("undo queued");
    assert_eq!(ids_in(&g.store, g.inbox).await, [id]);
    assert_eq!(labels(&g.store, id).await, ["\\Inbox"]);

    // Archive from All Mail works on the inbox location too.
    let change = g
        .store
        .enqueue_action(
            MailAction::Archive,
            vec![MessageRef {
                mailbox_id: g.all,
                message_id: id,
            }],
            "archive-2".into(),
        )
        .await
        .expect("archive from all mail");
    let op = change.operations[0].clone();
    assert_eq!(op.payload.source().map(|s| s.mailbox_id), Some(g.inbox));
    g.store
        .begin_operations(vec![(op.id, op.payload.clone())])
        .await
        .expect("begin");
    g.store
        .finish_operations(vec![(
            op.id,
            OperationResult::Confirmed {
                flags: None,
                moved_to: None,
                notice: None,
            },
        )])
        .await
        .expect("confirmed");
    assert!(ids_in(&g.store, g.inbox).await.is_empty());

    // Undo after the server did it: the inbox label goes back through All Mail, where the
    // message still is (on the server it left the inbox).
    let undo = g
        .store
        .undo_batch(change.batch.expect("batch"), "undo-2".into())
        .await
        .expect("undo confirmed");
    let [inverse] = undo.operations.as_slice() else {
        panic!("one inverse: {undo:?}");
    };
    match &inverse.payload {
        OperationPayload::SetFlag {
            source,
            flag,
            value,
            before,
        } => {
            assert_eq!(source.mailbox_id, g.all);
            assert_eq!(source.uid, 40);
            assert_eq!(flag, &Flag::Label("\\Inbox".into()));
            assert!(*value && !*before);
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(labels(&g.store, id).await, ["\\Inbox"]);
    assert!(g.store.account_is_gmail(g.account).await.expect("gmail"));
}

#[tokio::test]
async fn moving_out_of_all_mail_adds_the_label_and_expunges_nothing() {
    let g = gmail().await;
    g.store
        .store_headers(g.all, UV, vec![gm(40, 101, Some("m@x"), &[])], None)
        .await
        .expect("all mail");
    let id = ids_in(&g.store, g.all).await[0];
    let from_all = MessageRef {
        mailbox_id: g.all,
        message_id: id,
    };
    let change = g
        .store
        .enqueue_action(
            MailAction::Move { target: g.work },
            vec![from_all],
            "move".into(),
        )
        .await
        .expect("move");
    let [op] = change.operations.as_slice() else {
        panic!("one operation: {change:?}");
    };
    match &op.payload {
        OperationPayload::SetFlag {
            source,
            flag,
            value,
            ..
        } => {
            assert_eq!(source.mailbox_id, g.all);
            assert_eq!(flag, &Flag::Label("Work".into()));
            assert!(*value);
        }
        other => panic!("no move out of All Mail on Gmail: {other:?}"),
    }
    assert_eq!(ids_in(&g.store, g.all).await, [id], "stays in All Mail");
    assert_eq!(labels(&g.store, id).await, ["Work"]);

    // From a label folder it is an ordinary move (Gmail turns it into label changes).
    g.store
        .store_headers(g.inbox, UV, vec![gm(1, 102, Some("n@x"), &[])], None)
        .await
        .expect("inbox");
    let other = ids_in(&g.store, g.inbox).await[0];
    let change = g
        .store
        .enqueue_action(
            MailAction::Move { target: g.work },
            vec![MessageRef {
                mailbox_id: g.inbox,
                message_id: other,
            }],
            "move-2".into(),
        )
        .await
        .expect("move from the inbox");
    assert!(matches!(
        change.operations[0].payload,
        OperationPayload::Move { .. }
    ));
}
