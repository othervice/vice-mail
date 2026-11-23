//! Stage 16 (decision 0033) through `AppCore` with the fake server: the reading list changes
//! nothing on the server, unread lists, favourites and colours in the folder list, blocked
//! senders on arrival (junk where the account may write, hidden where it may not), and the
//! refusals of a transfer between accounts.

use std::collections::BTreeMap;

use mail_model::{
    AccountColor, ErrorCode, InboxFilter, IpcError, MailboxRole, MarkScope, MessageRef,
    TransferMode, UnreadFirstCursor,
};

use super::*;
use crate::sync::fake::FakeMailbox;

const LETTER: &[u8] = b"From: Dora <dora@friends.example>\r\nTo: alice@fake.test\r\n\
Subject: A long letter\r\nMessage-ID: <letter-1@friends.example>\r\nContent-Type: text/plain\r\n\r\nRead me later\r\n";

const SPAM: &[u8] = b"From: Pushy <Offers@Spam.Example>\r\nTo: alice@fake.test\r\n\
Subject: You won\r\nMessage-ID: <spam-1@spam.example>\r\nContent-Type: text/plain\r\n\r\nClick\r\n";

const SPAM_AGAIN: &[u8] = b"From: Pushy <offers@spam.example>\r\nTo: alice@fake.test\r\n\
Subject: You won again\r\nMessage-ID: <spam-2@spam.example>\r\nContent-Type: text/plain\r\n\r\nClick\r\n";

fn code(err: mail_model::CoreError) -> ErrorCode {
    IpcError::from(err).code
}

fn add_junk(server: &FakeServer) {
    server.with(|s| {
        let uid_validity = 900;
        s.mailboxes.insert(
            "Junk".to_owned(),
            FakeMailbox {
                uid_validity,
                uid_next: 1,
                modseq: 1,
                attributes: vec!["\\Junk".to_owned()],
                messages: BTreeMap::new(),
                changes: 0,
            },
        );
    });
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_reading_list_is_local_and_listed_newest_added_first() {
    let mut h = Harness::new(server_with(&[LETTER])).await;
    let letter = h.by_subject("A long letter").await;
    assert!(!letter.reading_list);

    // Also with writing on: nothing goes to the server.
    h.writes(true).await;
    h.core
        .set_reading_list(vec![letter.id], true)
        .await
        .expect("add");
    assert!(h.by_subject("A long letter").await.reading_list);
    let listed = h
        .core
        .list_reading_list(MarkScope::All)
        .await
        .expect("view");
    assert_eq!(
        listed.iter().map(|i| i.id).collect::<Vec<_>>(),
        vec![letter.id]
    );
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(
        h.server.read(|s| s.writer_attempts),
        0,
        "no writer connection"
    );
    let keywords = h.server.read(|s| {
        s.mailboxes["INBOX"]
            .messages
            .values()
            .flat_map(|m| m.flags.keywords.clone())
            .collect::<Vec<_>>()
    });
    assert!(
        keywords.is_empty(),
        "no keyword on the server: {keywords:?}"
    );

    h.core
        .set_reading_list(vec![letter.id], false)
        .await
        .expect("remove");
    assert!(
        h.core
            .list_reading_list(MarkScope::All)
            .await
            .expect("view")
            .is_empty()
    );
    // No messages, or too many: refused.
    assert_eq!(
        code(
            h.core
                .set_reading_list(vec![], true)
                .await
                .expect_err("empty")
        ),
        ErrorCode::InvalidArgument
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn only_unread_and_unread_first_through_the_core() {
    let server = server_with(&[LETTER, PERSON, URGENT]);
    // One message that is read already, the oldest.
    server.deliver("INBOX", 1, true);
    let h = Harness::new(server).await;
    let inbox = h.mailbox(MailboxRole::Inbox).await;
    eventually("all four stored", || async {
        h.inbox_items().await.len() == 4
    })
    .await;
    let read = h
        .inbox_items()
        .await
        .into_iter()
        .find(|i| !i.unread)
        .expect("a read message");

    let only = h
        .core
        .list_inbox_filtered(h.scope(inbox), InboxFilter::Unread, None, 50)
        .await
        .expect("only unread");
    assert_eq!(only.items.len(), 3);
    assert!(only.items.iter().all(|i| i.unread));

    let mut cursor: Option<UnreadFirstCursor> = None;
    let mut rows = Vec::new();
    loop {
        let page = h
            .core
            .list_unread_first(h.scope(inbox), cursor, 1)
            .await
            .expect("unread first");
        rows.extend(page.items.iter().map(|i| (i.id, i.unread)));
        match page.next_cursor {
            Some(next) => cursor = Some(next),
            None => break,
        }
    }
    assert_eq!(rows.len(), 4);
    assert_eq!(
        rows.iter().map(|r| r.1).collect::<Vec<_>>(),
        vec![true, true, true, false]
    );
    assert_eq!(rows[3].0, read.id);

    // The views that group unread on top: a device setting.
    let views = h
        .core
        .set_unread_first_view("unified:all".into(), true)
        .await
        .expect("set");
    assert_eq!(views, vec!["unified:all".to_owned()]);
    assert_eq!(
        h.core.get_unread_first_views().await.expect("get"),
        vec!["unified:all".to_owned()]
    );
    assert!(
        h.core
            .set_unread_first_view("unified:all".into(), false)
            .await
            .expect("unset")
            .is_empty()
    );
    assert_eq!(
        code(
            h.core
                .set_unread_first_view(" ".into(), true)
                .await
                .expect_err("empty key")
        ),
        ErrorCode::InvalidArgument
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn favourites_and_colours_come_with_the_folder_list() {
    let mut h = Harness::new(FakeServer::new()).await;
    let archive = h.mailbox(MailboxRole::Archive).await;
    let inbox = h.mailbox(MailboxRole::Inbox).await;
    h.core
        .set_mailbox_favorite(archive, true)
        .await
        .expect("favourite");
    let changed = h
        .event(|e| matches!(e, CoreEvent::MailboxesChanged { .. }))
        .await;
    assert!(
        matches!(changed, CoreEvent::MailboxesChanged { account_id } if account_id == h.account)
    );
    h.core
        .set_mailbox_favorite(inbox, true)
        .await
        .expect("inbox");
    let green = AccountColor::new(2).expect("colour");
    h.core
        .set_mailbox_color(archive, Some(green))
        .await
        .expect("colour");
    let boxes = h.core.list_mailboxes(h.account).await.expect("mailboxes");
    let archive_box = boxes.iter().find(|m| m.id == archive).expect("archive");
    assert_eq!(archive_box.favorite_position, Some(0));
    assert_eq!(archive_box.color, Some(green));
    let order = h
        .core
        .reorder_favorite_mailboxes(vec![inbox, archive])
        .await
        .expect("reorder");
    assert_eq!(
        order.iter().map(|m| m.id).collect::<Vec<_>>(),
        vec![inbox, archive]
    );
    assert_eq!(
        h.core
            .list_favorite_mailboxes()
            .await
            .expect("favourites")
            .len(),
        2
    );
    h.core.set_mailbox_color(archive, None).await.expect("none");
    h.core
        .set_mailbox_favorite(archive, false)
        .await
        .expect("no favourite");
    let boxes = h.core.list_mailboxes(h.account).await.expect("mailboxes");
    let archive_box = boxes.iter().find(|m| m.id == archive).expect("archive");
    assert_eq!(
        (archive_box.favorite_position, archive_box.color),
        (None, None)
    );
    assert_eq!(
        code(
            h.core
                .set_mailbox_favorite(MailboxId(9_999_999), true)
                .await
                .expect_err("unknown")
        ),
        ErrorCode::NotFound
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn blocked_senders_are_hidden_on_read_only_accounts_and_not_announced() {
    let mut h = Harness::new(FakeServer::new()).await;
    let blocked = h
        .core
        .block_sender("Pushy <OFFERS@spam.example>".into())
        .await;
    // A display name is not an address.
    assert_eq!(
        code(blocked.expect_err("not an address")),
        ErrorCode::InvalidArgument
    );
    let blocked = h
        .core
        .block_sender(" OFFERS@Spam.Example ".into())
        .await
        .expect("block");
    assert_eq!(blocked.len(), 1);
    assert_eq!(blocked[0].address, "offers@spam.example");

    h.server.deliver_raw("INBOX", SPAM);
    h.deliver(PERSON).await;
    let inbox = h.mailbox(MailboxRole::Inbox).await;
    eventually("spam stored", || async {
        h.inbox_items().await.iter().any(|i| i.subject == "You won")
    })
    .await;
    eventually("spam hidden from the inbox", || async {
        let page = h
            .core
            .list_inbox(h.scope(inbox), None, 50)
            .await
            .expect("inbox");
        let ids = row_ids(&page.rows);
        let spam = h.by_subject("You won").await;
        !ids.contains(&spam.id) && ids.len() == 1
    })
    .await;
    eventually("person announced", || async {
        h.sink.shown().iter().any(|n| n.body == "Lunch tomorrow?")
    })
    .await;
    assert!(
        !h.sink.shown().iter().any(|n| n.body == "You won"),
        "blocked mail is not announced"
    );
    assert_eq!(h.server.read(|s| s.writer_attempts), 0);
    // Decision 0036: the inbox list's note counts what it leaves out, per scope.
    assert_eq!(
        h.core
            .count_blocked_hidden(h.scope(inbox))
            .await
            .expect("count"),
        1
    );
    assert_eq!(
        h.core
            .count_blocked_hidden(InboxScope::Unified {
                view: mail_model::UnifiedView::All
            })
            .await
            .expect("count"),
        1
    );

    // Unblocked: it shows again.
    assert!(
        h.core
            .unblock_sender("offers@spam.example".into())
            .await
            .expect("unblock")
            .is_empty()
    );
    let page = h
        .core
        .list_inbox(h.scope(inbox), None, 50)
        .await
        .expect("inbox");
    assert_eq!(row_ids(&page.rows).len(), 2);
    assert_eq!(
        h.core
            .count_blocked_hidden(h.scope(inbox))
            .await
            .expect("count"),
        0
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn blocked_senders_go_to_junk_where_the_account_may_write() {
    let server = FakeServer::new();
    add_junk(&server);
    let mut h = Harness::new(server).await;
    h.writes(true).await;
    h.core
        .block_sender("offers@spam.example".into())
        .await
        .expect("block");
    h.server.deliver_raw("INBOX", SPAM_AGAIN);
    eventually("moved to junk on the server", || async {
        h.server.read(|s| {
            s.mailboxes["INBOX"].messages.is_empty() && s.mailboxes["Junk"].messages.len() == 1
        })
    })
    .await;
    assert!(!h.sink.shown().iter().any(|n| n.body == "You won again"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_transfer_is_refused_before_anything_is_fetched_or_recorded() {
    let mut h = Harness::new(server_with(&[LETTER, PERSON])).await;
    let inbox = h.mailbox(MailboxRole::Inbox).await;
    let archive = h.mailbox(MailboxRole::Archive).await;
    let letter = h.by_subject("A long letter").await;
    let at = MessageRef {
        mailbox_id: inbox,
        message_id: letter.id,
    };
    // Read-only target account.
    let err = h
        .core
        .transfer_messages(vec![at], archive, TransferMode::Copy, "t-1".into())
        .await
        .expect_err("read only");
    assert_eq!(code(err), ErrorCode::ReadOnly);
    h.writes(true).await;
    // Same account: an ordinary move, not a transfer.
    let err = h
        .core
        .transfer_messages(vec![at], archive, TransferMode::Move, "t-2".into())
        .await
        .expect_err("same account");
    assert_eq!(code(err), ErrorCode::InvalidArgument);
    // Unknown target, no messages, a bad request id.
    for (messages, target, request) in [
        (vec![at], MailboxId(9_999_999), "t-3"),
        (vec![], archive, "t-4"),
        (vec![at], archive, "not a request id"),
    ] {
        let err = h
            .core
            .transfer_messages(messages, target, TransferMode::Copy, request.into())
            .await
            .expect_err("refused");
        assert!(matches!(
            code(err),
            ErrorCode::InvalidArgument | ErrorCode::NotFound
        ));
    }
    assert!(
        h.core
            .list_operations(h.account, None, 50)
            .await
            .expect("operations")
            .is_empty(),
        "nothing recorded"
    );
}
