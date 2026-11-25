//! Stage 20, task B (decision 0059): reminders when no reply comes, in the store: rows and
//! marks, coming back, and the records of the settings sync (a moved time waits again, the same
//! time keeps whether it came back here).

use mail_model::{Address, MessageId};
use serde_json::json;

use crate::test_support::{UV, fixture, header};
use crate::{InboxMailboxes, SettleScope, SyncApply, TableWrite};

const D: i64 = 1_764_000_000;

#[tokio::test]
async fn reminders_are_rows_marks_and_come_back_above_the_inbox() {
    let f = fixture().await;
    let mut sent = header(1, D, Some("offer@example.org"));
    sent.from = vec![Address {
        name: None,
        email: "alice@example.org".into(),
    }];
    f.store
        .store_headers(f.sent, UV, vec![sent], None)
        .await
        .expect("store");
    let message = MessageId(D);
    let (stored, before) = f
        .store
        .set_follow_up(message, (D + 100) * 1000, D * 1000)
        .await
        .expect("set");
    assert_eq!(before, None);
    assert_eq!(stored.sent_at, D * 1000, "the message's date");
    assert_eq!(
        f.store.next_follow_up_due().await.expect("due"),
        Some((D + 100) * 1000)
    );
    let rows = f
        .store
        .follow_up_rows(vec![f.account], false, 10)
        .await
        .expect("rows");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].mailbox_id, f.sent, "shown in Sent");
    let mark = rows[0].follow_up.expect("mark");
    assert_eq!((mark.due, mark.returned), ((D + 100) * 1000, false));
    let inbox = InboxMailboxes::Mailbox(f.inbox);
    assert!(
        f.store
            .inbox_follow_ups(inbox.clone(), 10)
            .await
            .expect("top")
            .is_empty()
    );

    // Not due yet: nothing; due: it comes back (without an answer in the store).
    let early = f
        .store
        .settle_follow_ups((D + 50) * 1000, SettleScope::Due)
        .await
        .expect("settle");
    assert!(early.returned.is_empty() && early.answered.is_empty());
    let due = f
        .store
        .settle_follow_ups((D + 200) * 1000, SettleScope::Due)
        .await
        .expect("settle");
    assert_eq!(due.returned.len(), 1);
    assert_eq!(due.returned[0].1, [message]);
    let top = f.store.inbox_follow_ups(inbox, 10).await.expect("top");
    assert_eq!(top.len(), 1);
    assert!(top[0].follow_up.is_some_and(|m| m.returned));
    // Only above an inbox: not above another folder.
    assert!(
        f.store
            .inbox_follow_ups(InboxMailboxes::Mailbox(f.archive), 10)
            .await
            .expect("archive")
            .is_empty()
    );

    // Moving the time makes it wait again; removing ends it.
    f.store
        .set_follow_up(message, (D + 900) * 1000, (D + 300) * 1000)
        .await
        .expect("move");
    assert!(
        f.store
            .follow_up_of(message)
            .await
            .expect("of")
            .is_some_and(|r| r.returned_at.is_none() && r.sent_at == D * 1000)
    );
    assert!(
        f.store
            .remove_follow_up(message)
            .await
            .expect("remove")
            .is_some()
    );
    assert!(
        f.store
            .remove_follow_up(message)
            .await
            .expect("again")
            .is_none()
    );
    assert_eq!(f.store.next_follow_up_due().await.expect("due"), None);

    // A message without a Message-ID cannot have one.
    f.store
        .store_headers(f.sent, UV, vec![header(2, D + 1, None)], None)
        .await
        .expect("store");
    let err = f
        .store
        .set_follow_up(MessageId(D + 1), (D + 100) * 1000, D * 1000)
        .await
        .expect_err("no Message-ID");
    assert!(matches!(err, crate::StoreError::InvalidArgument(_)));
}

#[tokio::test]
async fn records_of_the_settings_sync_keep_or_reset_the_return() {
    let f = fixture().await;
    f.store
        .store_headers(f.sent, UV, vec![header(1, D, Some("q@example.org"))], None)
        .await
        .expect("store");
    let id = format!("{}/q@example.org", f.account);
    let put = |due: i64| SyncApply {
        kind: "followUp".into(),
        id: id.clone(),
        expected: None,
        row: None,
        table: TableWrite::Put(json!({ "dueMs": due, "sentAtMs": D * 1000 })),
        rekey_from: None,
    };
    let outcome = f
        .store
        .apply_sync(vec![put((D + 10) * 1000)], None)
        .await
        .expect("apply");
    assert!(outcome.kinds.contains("followUp"), "{outcome:?}");
    let message = MessageId(D);
    let stored = f
        .store
        .follow_up_of(message)
        .await
        .expect("of")
        .expect("row");
    assert_eq!((stored.due, stored.sent_at), ((D + 10) * 1000, D * 1000));
    f.store
        .settle_follow_ups((D + 20) * 1000, SettleScope::Due)
        .await
        .expect("settle");
    // The same time again (another device's echo): it stays back here.
    f.store
        .apply_sync(vec![put((D + 10) * 1000)], None)
        .await
        .expect("apply");
    assert!(
        f.store
            .follow_up_of(message)
            .await
            .expect("of")
            .is_some_and(|r| r.returned_at.is_some())
    );
    // A new time: it waits again.
    f.store
        .apply_sync(vec![put((D + 99) * 1000)], None)
        .await
        .expect("apply");
    assert!(
        f.store
            .follow_up_of(message)
            .await
            .expect("of")
            .is_some_and(|r| r.returned_at.is_none() && r.due == (D + 99) * 1000)
    );
    // A malformed record is refused, a deletion removes it.
    let bad = SyncApply {
        table: TableWrite::Put(json!({ "dueMs": "soon" })),
        ..put(0)
    };
    let outcome = f.store.apply_sync(vec![bad], None).await.expect("apply");
    assert_eq!(outcome.skipped.len(), 1);
    f.store
        .apply_sync(
            vec![SyncApply {
                table: TableWrite::Delete,
                ..put(0)
            }],
            None,
        )
        .await
        .expect("delete");
    assert_eq!(f.store.follow_up_of(message).await.expect("of"), None);
}
