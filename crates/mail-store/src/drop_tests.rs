//! Stage 22 (decision 0081): rows dropped on a folder, sorted by account.

use crate::StoreError;
use crate::test_support::new_account;

/// Stage 22 (decision 0081): a drop of rows of several folders and accounts on one folder:
/// those of the folder's account move, those of other accounts are transferred, those already
/// there stay; read-only accounts refuse, naming the account.
#[tokio::test]
async fn a_drop_sorts_the_rows_by_account_and_skips_those_already_there() {
    use mail_model::{ActionError, MessageRef, TransferMode};

    use crate::MailboxBaseline;
    use crate::test_support::{UV, fixture, header, remote};

    let f = fixture().await;
    let bob = f
        .store
        .insert_account(new_account(2, "bob@example.net"))
        .await
        .expect("bob")
        .id;
    f.store
        .sync_mailboxes(bob, vec![remote("INBOX", &[])])
        .await
        .expect("mailboxes");
    let bob_inbox = f.store.mailboxes(bob).await.expect("mailboxes")[0].id;
    for (mailbox, uid, id) in [
        (f.inbox, 1, "a1@x"),
        (f.archive, 2, "a2@x"),
        (f.sent, 3, "a3@x"),
        (bob_inbox, 4, "b1@x"),
    ] {
        f.store
            .reset_mailbox(
                mailbox,
                MailboxBaseline {
                    uid_validity: UV,
                    uid_next: 1,
                    highest_modseq: None,
                },
            )
            .await
            .ok();
        f.store
            .store_headers(
                mailbox,
                UV,
                vec![header(uid, 1_700_000_000, Some(id))],
                None,
            )
            .await
            .expect("store");
    }
    let at = |mailbox| {
        let store = f.store.clone();
        async move {
            let item = store
                .list_messages(mailbox, None, 10)
                .await
                .expect("list")
                .items[0]
                .clone();
            MessageRef {
                mailbox_id: mailbox,
                message_id: item.id,
            }
        }
    };
    let (inbox_row, archive_row, sent_row, bob_row) = (
        at(f.inbox).await,
        at(f.archive).await,
        at(f.sent).await,
        at(bob_inbox).await,
    );
    // Rows of "All inboxes" and of a search, one of them twice, dropped on alice's archive.
    let rows = vec![inbox_row, bob_row, archive_row, sent_row, inbox_row];

    let read_only = |err: StoreError| match err {
        StoreError::Action(ActionError::ReadOnly(account)) => account,
        other => panic!("not read_only: {other:?}"),
    };
    let err = f
        .store
        .plan_drop(rows.clone(), f.archive, TransferMode::Move)
        .await
        .expect_err("alice may not write");
    assert_eq!(read_only(err), f.account);
    f.store
        .set_writes_enabled(f.account, true)
        .await
        .expect("alice writes");
    let err = f
        .store
        .plan_drop(rows.clone(), f.archive, TransferMode::Move)
        .await
        .expect_err("bob's row cannot leave his read-only account");
    assert_eq!(read_only(err), bob);
    // Copying leaves bob's account as it is: allowed.
    let copy = f
        .store
        .plan_drop(rows.clone(), f.archive, TransferMode::Copy)
        .await
        .expect("a copy");
    assert_eq!(copy.other_accounts, [bob_row]);
    f.store
        .set_writes_enabled(bob, true)
        .await
        .expect("bob writes");

    let plan = f
        .store
        .plan_drop(rows, f.archive, TransferMode::Move)
        .await
        .expect("plan");
    assert_eq!(plan.target_account, f.account);
    assert_eq!(plan.same_account, [inbox_row, sent_row]);
    assert_eq!(plan.other_accounts, [bob_row]);
    assert_eq!(plan.skipped, 1, "the archive's own row stays");

    // An unknown target or row refuses the whole drop.
    assert!(
        f.store
            .plan_drop(
                vec![inbox_row],
                mail_model::MailboxId(9_999),
                TransferMode::Move
            )
            .await
            .is_err()
    );
    let gone = MessageRef {
        mailbox_id: f.inbox,
        message_id: mail_model::MessageId(9_999),
    };
    assert!(
        f.store
            .plan_drop(vec![gone], f.archive, TransferMode::Move)
            .await
            .is_err()
    );
}
