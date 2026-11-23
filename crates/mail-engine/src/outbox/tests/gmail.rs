//! Stage 14, block A (decision 0034): the outbox on a Gmail account. Archive removes the inbox
//! label (`STORE -X-GM-LABELS (\Inbox)`) instead of moving to "All Mail"; labels are stored
//! without `UNCHANGEDSINCE`; other servers have no labels.

use mail_model::{ErrorCode, MailAction, OperationState};

use super::{Bed, lock};

#[tokio::test]
async fn archive_on_gmail_removes_the_inbox_label_instead_of_moving() {
    let bed = Bed::new(2, true).await;
    bed.store
        .set_account_gmail(bed.account, true)
        .await
        .expect("gmail");
    let first = bed.inbox_ref(1).await;
    let result = bed
        .outbox
        .apply(MailAction::Archive, vec![first])
        .await
        .expect("archive without an archive role on Gmail");
    let op = bed.op_of(result.batch).await;
    bed.wait(op.id, OperationState::Confirmed).await;
    // Gone from the inbox list at once (the server drops it from INBOX itself).
    let inbox = bed.mailbox("INBOX").await;
    let listed = bed
        .store
        .list_messages(inbox, None, 10)
        .await
        .expect("list")
        .items;
    assert!(listed.iter().all(|i| i.id != first.message_id));

    let second = bed.inbox_ref(2).await;
    let result = bed
        .outbox
        .apply(
            MailAction::AddLabel {
                label: "Work".into(),
            },
            vec![second],
        )
        .await
        .expect("label");
    let op = bed.op_of(result.batch).await;
    bed.wait(op.id, OperationState::Confirmed).await;
    assert_eq!(
        bed.store
            .message_labels(second.message_id)
            .await
            .expect("labels"),
        Some(vec!["\\Inbox".to_owned(), "Work".to_owned()])
    );
    bed.outbox.shutdown().await;

    let server = lock(&bed.server);
    assert_eq!(server.count("UID MOVE"), 0, "{:?}", server.log);
    assert_eq!(server.count("UID COPY"), 0, "{:?}", server.log);
    let labels: Vec<&String> = server
        .log
        .iter()
        .filter(|l| l.starts_with("X-GM-LABELS"))
        .collect();
    assert_eq!(
        labels,
        [
            "X-GM-LABELS -\\Inbox since=false",
            "X-GM-LABELS +Work since=false"
        ],
        "{:?}",
        server.log
    );
}

#[tokio::test]
async fn other_servers_have_no_labels_and_archive_still_moves() {
    let bed = Bed::new(1, true).await;
    let at = bed.inbox_ref(1).await;
    let err = bed
        .outbox
        .apply(
            MailAction::AddLabel {
                label: "Work".into(),
            },
            vec![at],
        )
        .await
        .expect_err("no labels");
    assert_eq!(err.code(), ErrorCode::UnsupportedServer, "{err}");
    let result = bed
        .outbox
        .apply(MailAction::Archive, vec![at])
        .await
        .expect("archive");
    let op = bed.op_of(result.batch).await;
    bed.wait(op.id, OperationState::Confirmed).await;
    bed.outbox.shutdown().await;
    let server = lock(&bed.server);
    assert_eq!(server.count("UID MOVE"), 1, "{:?}", server.log);
    assert!(server.log.iter().all(|l| !l.starts_with("X-GM-LABELS")));
}
