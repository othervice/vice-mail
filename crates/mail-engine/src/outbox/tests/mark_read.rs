//! Stage 21 (decision 0063): "Mark folder as read" through the outbox, against the in-memory
//! server: `\Seen` on exactly the UIDs that were unread, in bounded STORE commands, never on mail
//! that arrived later; a cut connection repeats it harmlessly; undo clears the same UIDs; a
//! rebuilt folder is a conflict; read-only accounts never connect.

use std::sync::atomic::Ordering;
use std::time::Duration;

use mail_model::{
    ActionError, ActionKind, ConflictReason, CoreError, OperationKind, OperationState, Uid,
};

use super::{Bed, lock};

/// UIDs of the server's INBOX that are not `\Seen`.
fn unseen_on_server(bed: &Bed) -> Vec<Uid> {
    lock(&bed.server).boxes["INBOX"]
        .messages
        .iter()
        .filter(|(_, m)| !m.flags.seen)
        .map(|(uid, _)| *uid)
        .collect()
}

async fn unread_counter(bed: &Bed) -> u32 {
    let inbox = bed.mailbox("INBOX").await;
    bed.store
        .mailboxes(bed.account)
        .await
        .expect("mailboxes")
        .into_iter()
        .find(|m| m.id == inbox)
        .map(|m| m.unread)
        .expect("inbox")
}

#[tokio::test]
async fn a_folder_is_marked_on_its_recorded_uids_in_parts_and_undone_on_the_same() {
    // 600 messages, every even one read already: 300 unread single UIDs, two STORE commands.
    let bed = Bed::new(600, true).await;
    {
        let mut server = lock(&bed.server);
        if let Some(inbox) = server.boxes.get_mut("INBOX") {
            for (uid, m) in inbox.messages.iter_mut() {
                m.flags.seen = uid % 2 == 0;
            }
        }
    }
    bed.sync().await;
    assert_eq!(unread_counter(&bed).await, 300);
    // Arrives on the server after the last sync: the app never showed it, so it stays unread.
    let late = lock(&bed.server).add("INBOX", "late@fake");

    let inbox = bed.mailbox("INBOX").await;
    let result = bed
        .outbox
        .mark_read(bed.account, vec![inbox], "mark-1".into())
        .await
        .expect("mark");
    assert_eq!(result.queued, 1, "one operation for the folder");
    assert_eq!(unread_counter(&bed).await, 0, "the counter drops at once");
    let op = bed.op_of(result.batch).await;
    assert_eq!(
        (op.action, op.payload.kind()),
        (ActionKind::MarkMailboxRead, OperationKind::MarkMailbox)
    );
    let done = bed.wait(op.id, OperationState::Confirmed).await;
    assert_eq!(done.attempts, 1);
    assert_eq!(unseen_on_server(&bed), [late]);
    assert_eq!(
        lock(&bed.server).count("UID STORE"),
        2,
        "300 ranges in parts of {}",
        super::super::exec::MARK_PART_RANGES
    );

    // Undo: `\Seen` comes off exactly the 300 again, not off the even ones.
    let batch = result.batch.expect("batch");
    let undone = bed.outbox.undo(batch).await.expect("undo");
    let inverse = bed.op_of(undone.batch).await;
    bed.wait(inverse.id, OperationState::Confirmed).await;
    let odd: Vec<Uid> = (1..=600).filter(|uid| uid % 2 == 1).chain([late]).collect();
    assert_eq!(unseen_on_server(&bed), odd);
    assert_eq!(unread_counter(&bed).await, 300);
    bed.outbox.shutdown().await;
}

#[tokio::test]
async fn a_mark_cut_after_the_store_is_sent_again_after_a_restart() {
    let mut bed = Bed::new(3, true).await;
    lock(&bed.server).cut_after = Some("UID STORE");
    let inbox = bed.mailbox("INBOX").await;
    let result = bed
        .outbox
        .mark_read(bed.account, vec![inbox], "mark-cut".into())
        .await
        .expect("mark");
    let op = bed.op_of(result.batch).await;
    bed.wait(op.id, OperationState::InFlight).await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(bed.offline.load(Ordering::SeqCst), "the cut happened");
    bed.restart(true).await;
    bed.offline.store(false, Ordering::SeqCst);
    bed.outbox.start().await.expect("start");
    let done = bed.wait(op.id, OperationState::Confirmed).await;
    assert_eq!(
        done.attempts, 2,
        "sent again: setting a set flag changes nothing"
    );
    assert!(unseen_on_server(&bed).is_empty());
    assert_eq!(unread_counter(&bed).await, 0);
    bed.outbox.shutdown().await;
}

#[tokio::test]
async fn a_rebuilt_folder_is_a_conflict_and_its_messages_show_unread_again() {
    let bed = Bed::new(2, true).await;
    bed.offline.store(true, Ordering::SeqCst);
    let inbox = bed.mailbox("INBOX").await;
    let result = bed
        .outbox
        .mark_read(bed.account, vec![inbox], "mark-reset".into())
        .await
        .expect("mark");
    assert_eq!(unread_counter(&bed).await, 0);
    if let Some(b) = lock(&bed.server).boxes.get_mut("INBOX") {
        b.uid_validity = 8;
    }
    bed.offline.store(false, Ordering::SeqCst);
    bed.outbox.wake(bed.account);
    let op = bed.op_of(result.batch).await;
    let done = bed.wait(op.id, OperationState::Conflict).await;
    assert_eq!(done.conflict, Some(ConflictReason::MailboxReset));
    assert_eq!(lock(&bed.server).count("UID STORE"), 0);
    assert_eq!(unread_counter(&bed).await, 2, "the server's state is kept");
    bed.outbox.shutdown().await;
}

#[tokio::test]
async fn read_only_accounts_record_no_mark_and_never_connect() {
    let bed = Bed::new(2, true).await;
    bed.outbox
        .set_writes_enabled(bed.account, false)
        .await
        .expect("off");
    let inbox = bed.mailbox("INBOX").await;
    let err = bed
        .outbox
        .mark_read(bed.account, vec![inbox], "mark-ro".into())
        .await
        .expect_err("read-only");
    assert_eq!(err, CoreError::Action(ActionError::ReadOnly(bed.account)));
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(lock(&bed.server).log.is_empty(), "no command at all");
    assert_eq!(unread_counter(&bed).await, 2);
    bed.outbox.shutdown().await;
}
