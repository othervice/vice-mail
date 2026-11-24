//! Decision 0039 in the store: the inbox lists as conversations (rows, top, unread first), the
//! summary of a conversation row, the messages an action on conversation rows touches, and the
//! query plans of all of them.

use std::collections::{BTreeMap, BTreeSet};

use mail_model::{
    Address, ConversationReach, ListCursor, MailboxId, MessageHeader, MessageId, MessageRef,
    ThreadListItem, UnreadFirstCursor,
};
use rusqlite::Connection;

use crate::inbox::InboxListKind;
use crate::test_support::{Fixture, UV, fixture, header, new_account, remote, seen};
use crate::{InboxMailboxes, MailboxBaseline, Store, conversations, search};

const D: i64 = 1_790_000_000;
const NOW: i64 = D * 1000;

fn from(mut h: MessageHeader, name: &str) -> MessageHeader {
    h.from = vec![Address {
        name: Some(name.into()),
        email: format!("{}@example.org", name.to_lowercase()),
    }];
    h
}

fn reply(mut h: MessageHeader, to: &[&str]) -> MessageHeader {
    h.in_reply_to = to.last().map(|s| (*s).to_owned());
    h.references = to.iter().map(|s| (*s).to_owned()).collect();
    h
}

async fn store_in(f: &Fixture, mailbox: MailboxId, headers: Vec<MessageHeader>) {
    f.store
        .store_headers(mailbox, UV, headers, None)
        .await
        .expect("store headers");
}

/// Every row of a conversation list, by paging with `limit`.
async fn all_rows(
    store: &Store,
    scope: InboxMailboxes,
    kind: InboxListKind,
    limit: u32,
) -> Vec<ThreadListItem> {
    let mut out = Vec::new();
    let mut cursor: Option<ListCursor> = None;
    loop {
        let page = store
            .inbox_conversations(scope.clone(), kind, cursor, limit)
            .await
            .expect("conversation page");
        assert!(page.items.len() <= usize::try_from(limit).expect("limit"));
        out.extend(page.items);
        match page.next_cursor {
            Some(next) => cursor = Some(next),
            None => return out,
        }
    }
}

/// The message ids of the inbox list of `kind` (one row per message).
async fn message_rows(store: &Store, scope: InboxMailboxes, kind: InboxListKind) -> Vec<i64> {
    let mut out = Vec::new();
    let mut cursor = None;
    loop {
        let page = store
            .inbox_list(scope.clone(), kind, cursor, 500)
            .await
            .expect("message page");
        out.extend(page.items.iter().map(|i| i.id.0));
        match page.next_cursor {
            Some(next) => cursor = Some(next),
            None => return out,
        }
    }
}

async fn thread_of(store: &Store, message: i64) -> Option<i64> {
    store
        .read(move |c| {
            Ok(c.query_row(
                "SELECT thread_id FROM message WHERE id = ?1",
                [message],
                |r| r.get(0),
            )?)
        })
        .await
        .expect("thread")
}

async fn id_of(store: &Store, msgid: &'static str) -> i64 {
    store
        .read(move |c| {
            Ok(c.query_row(
                "SELECT id FROM message WHERE message_id_hdr = ?1",
                [msgid],
                |r| r.get(0),
            )?)
        })
        .await
        .expect("message id")
}

fn ids(rows: &[ThreadListItem]) -> Vec<i64> {
    rows.iter().map(|r| r.message.id.0).collect()
}

// ---------------------------------------------------------------------------------------------
// Rows

#[tokio::test]
async fn replies_from_several_people_and_sent_mail_form_one_row() {
    let f = fixture().await;
    let inbox = InboxMailboxes::Mailbox(f.inbox);
    store_in(
        &f,
        f.inbox,
        vec![
            seen(from(header(1, D, Some("a@x")), "Bob")),
            from(reply(header(2, D + 600, Some("b@x")), &["a@x"]), "Carol"),
            seen(from(header(3, D + 700, Some("other@x")), "Erin")),
            from(
                reply(header(4, D + 1_200, Some("c@x")), &["a@x", "b@x"]),
                "Dave",
            ),
            // Bob again: one participant per address, without case.
            {
                let mut h = reply(header(5, D + 1_300, Some("d@x")), &["a@x", "c@x"]);
                h.from = vec![Address {
                    name: Some("Bob B.".into()),
                    email: "BOB@example.org".into(),
                }];
                h.has_attachments = true;
                seen(h)
            },
        ],
    )
    .await;
    // My answer lies in Sent only; it counts, but the inbox row stays the newest in the inbox.
    store_in(
        &f,
        f.sent,
        vec![seen(from(
            reply(header(1, D + 2_000, Some("me@x")), &["a@x", "d@x"]),
            "Alice",
        ))],
    )
    .await;

    let rows = all_rows(&f.store, inbox, InboxListKind::Rows, 50).await;
    assert_eq!(rows.len(), 2, "{rows:#?}");
    let conversation = &rows[0];
    assert_eq!(conversation.message.id.0, id_of(&f.store, "d@x").await);
    assert_eq!(conversation.message.mailbox_id, f.inbox);
    assert_eq!(conversation.message_count, 5);
    assert_eq!(conversation.unread_count, 2);
    assert_eq!(
        conversation.last_activity,
        id_of(&f.store, "me@x").await * 1000
    );
    let names: Vec<&str> = conversation
        .participants
        .iter()
        .map(|a| a.email.as_str())
        .collect();
    assert_eq!(
        names,
        [
            "bob@example.org",
            "carol@example.org",
            "dave@example.org",
            "alice@example.org"
        ]
    );
    assert!(conversation.has_attachments);
    assert!(!conversation.flagged);
    assert!(conversation.thread_id.is_some());
    // The unrelated message is a row of its own.
    assert_eq!(rows[1].message_count, 1);
    assert_eq!(rows[1].participants.len(), 1);

    // The folder's thread list (stage 7) has the same summary.
    let threads = f
        .store
        .list_threads(f.inbox, None, 50)
        .await
        .expect("threads");
    assert_eq!(threads.items, rows);
}

#[tokio::test]
async fn long_threads_count_every_message_and_name_the_senders_of_the_newest() {
    let f = fixture().await;
    // 205 messages: the first five from early senders, then a list of replies from one sender;
    // an attachment and a flag only on the oldest message.
    let mut headers = Vec::new();
    for k in 0..205i64 {
        let uid = u32::try_from(k).expect("uid") + 1;
        let mut h = header(uid, D + k * 60, Some(&format!("{k}@long")));
        if k > 0 {
            h = reply(h, &["0@long"]);
        }
        h = if k < 5 {
            from(h, &format!("Early{k}"))
        } else {
            from(h, "Late")
        };
        if k == 0 {
            h.has_attachments = true;
            h.flags.flagged = true;
        }
        if k % 2 == 0 {
            h = seen(h);
        }
        headers.push(h);
    }
    store_in(&f, f.inbox, headers).await;
    let rows = all_rows(
        &f.store,
        InboxMailboxes::Mailbox(f.inbox),
        InboxListKind::Rows,
        10,
    )
    .await;
    assert_eq!(rows.len(), 1);
    let row = &rows[0];
    assert_eq!(row.message_count, 205);
    assert_eq!(row.unread_count, 102);
    // Senders, attachments and flags come from the newest 200 messages.
    let senders: Vec<&str> = row.participants.iter().map(|a| a.email.as_str()).collect();
    assert_eq!(senders, ["late@example.org"]);
    assert!(!row.has_attachments);
    assert!(!row.flagged);
    assert_eq!(
        usize::try_from(conversations::SUMMARY_MESSAGES).expect("limit"),
        200
    );
}

#[tokio::test]
async fn pages_have_neither_duplicates_nor_gaps() {
    let f = fixture().await;
    // 40 threads of 5 messages, interleaved in time, some read; four threads have their
    // replies in Archive.
    let mut inbox = Vec::new();
    let mut archive = Vec::new();
    for k in 0..200i64 {
        let thread = k % 40;
        let mut h = header(0, D + k * 300, Some(&format!("{k}@t")));
        if k >= 40 {
            h = reply(h, &[&format!("{thread}@t")]);
        }
        if k % 3 == 0 {
            h = seen(h);
        }
        if thread % 5 == 4 && k % 2 == 0 && k >= 40 {
            h.uid = u32::try_from(archive.len()).expect("uid") + 1;
            archive.push(h);
        } else {
            h.uid = u32::try_from(inbox.len()).expect("uid") + 1;
            inbox.push(h);
        }
    }
    store_in(&f, f.inbox, inbox).await;
    store_in(&f, f.archive, archive).await;

    let scope = InboxMailboxes::Mailbox(f.inbox);
    let whole = all_rows(&f.store, scope.clone(), InboxListKind::Rows, 500).await;
    // Expected: the message rows grouped by thread at their newest member.
    let messages = message_rows(&f.store, scope.clone(), InboxListKind::Rows).await;
    let mut expected = Vec::new();
    let mut threads_seen = BTreeSet::new();
    for id in &messages {
        let thread = thread_of(&f.store, *id).await;
        if thread.is_none_or(|t| threads_seen.insert(t)) {
            expected.push(*id);
        }
    }
    assert_eq!(ids(&whole), expected);
    assert_eq!(whole.len(), 40);
    for limit in [1, 2, 3, 7, 13] {
        let paged = all_rows(&f.store, scope.clone(), InboxListKind::Rows, limit).await;
        assert_eq!(ids(&paged), expected, "limit {limit}");
    }
    // Every inbox message belongs to exactly one row; counts cover every mailbox.
    let total: u32 = whole.iter().map(|r| r.message_count).sum();
    assert_eq!(messages.len(), 184);
    assert_eq!(total, 200, "inbox and archive messages");

    // The same for the unread list.
    let unread = all_rows(&f.store, scope.clone(), InboxListKind::Unread, 500).await;
    for limit in [1, 4, 9] {
        let paged = all_rows(&f.store, scope.clone(), InboxListKind::Unread, limit).await;
        assert_eq!(ids(&paged), ids(&unread), "unread, limit {limit}");
    }
    assert!(unread.iter().all(|r| r.unread_count > 0));
}

#[tokio::test]
async fn pinned_and_returned_conversations_stand_on_top_once_and_snoozes_hide_messages() {
    let f = fixture().await;
    let scope = InboxMailboxes::Mailbox(f.inbox);
    store_in(
        &f,
        f.inbox,
        vec![
            from(header(1, D, Some("p1@x")), "Pia"),
            from(reply(header(2, D + 100, Some("p2@x")), &["p1@x"]), "Paul"),
            from(header(3, D + 200, Some("s1@x")), "Sam"),
            from(reply(header(4, D + 300, Some("s2@x")), &["s1@x"]), "Sue"),
            from(header(5, D + 400, Some("r1@x")), "Rita"),
            from(reply(header(6, D + 500, Some("r2@x")), &["r1@x"]), "Rob"),
            from(header(7, D + 600, Some("plain@x")), "Pat"),
        ],
    )
    .await;
    let p1 = MessageId(id_of(&f.store, "p1@x").await);
    let p2 = id_of(&f.store, "p2@x").await;
    let s1 = MessageId(id_of(&f.store, "s1@x").await);
    let s2 = MessageId(id_of(&f.store, "s2@x").await);
    let r1 = MessageId(id_of(&f.store, "r1@x").await);
    let r2 = id_of(&f.store, "r2@x").await;
    let plain = id_of(&f.store, "plain@x").await;

    // Pinning the older message pins the conversation: it leaves the rows, the top shows it
    // once, as its newest message in the inbox.
    assert!(f.store.pin_message(p1, NOW).await.expect("pin"));
    // A returned snooze of the older message: the conversation stands on top as well.
    f.store
        .snooze_message(r1, NOW + 1_000, NOW)
        .await
        .expect("snooze");
    let returned = f.store.return_due_snoozes(NOW + 2_000).await.expect("due");
    assert_eq!(returned.len(), 1);
    // Both messages of the third conversation wait: it is hidden.
    for m in [s1, s2] {
        f.store
            .snooze_message(m, NOW + 86_400_000, NOW)
            .await
            .expect("snooze");
    }

    let rows = all_rows(&f.store, scope.clone(), InboxListKind::Rows, 50).await;
    assert_eq!(ids(&rows), [plain]);
    let pinned = f
        .store
        .inbox_top_conversations(scope.clone(), false, 100)
        .await
        .expect("pinned");
    assert_eq!(ids(&pinned), [p2]);
    assert_eq!(pinned[0].message_count, 2);
    let back = f
        .store
        .inbox_top_conversations(scope.clone(), true, 100)
        .await
        .expect("returned");
    assert_eq!(ids(&back), [r2]);

    // A reply to the snoozed conversation brings it back, with the reply as its row.
    store_in(
        &f,
        f.inbox,
        vec![from(
            reply(header(8, D + 700, Some("s3@x")), &["s1@x", "s2@x"]),
            "Sue",
        )],
    )
    .await;
    let s3 = id_of(&f.store, "s3@x").await;
    let rows = all_rows(&f.store, scope.clone(), InboxListKind::Rows, 50).await;
    assert_eq!(ids(&rows), [s3, plain]);
    assert_eq!(rows[0].message_count, 3);

    // Only one waiting snooze: the conversation's row is its newest message that is not
    // snoozed.
    f.store.unsnooze_message(s1).await.expect("unsnooze");
    f.store
        .unsnooze_message(MessageId(s3))
        .await
        .expect("nothing to unsnooze");
    f.store
        .snooze_message(MessageId(s3), NOW + 86_400_000, NOW)
        .await
        .expect("snooze the reply");
    let rows = all_rows(&f.store, scope.clone(), InboxListKind::Rows, 50).await;
    assert_eq!(ids(&rows), [plain, s1.0]);

    // The pinned conversation stays on top when the pinned message is archived, as long as a
    // message of it lies in the inbox.
    let (inbox, archive) = (f.inbox.0, f.archive.0);
    f.store
        .write(move |tx| {
            tx.execute(
                "UPDATE message_location SET mailbox_id = ?2 WHERE message_id = ?1 AND mailbox_id = ?3",
                rusqlite::params![p1.0, archive, inbox],
            )?;
            Ok(())
        })
        .await
        .expect("archive p1");
    let pinned = f
        .store
        .inbox_top_conversations(scope, false, 100)
        .await
        .expect("pinned");
    assert_eq!(ids(&pinned), [p2]);
}

#[tokio::test]
async fn unread_first_has_each_conversation_once() {
    let f = fixture().await;
    let scope = InboxMailboxes::Mailbox(f.inbox);
    store_in(
        &f,
        f.inbox,
        vec![
            // Thread A: the older message is unread, the newer one read.
            from(header(1, D, Some("a1@x")), "Ann"),
            seen(from(
                reply(header(2, D + 100, Some("a2@x")), &["a1@x"]),
                "Ben",
            )),
            // Thread B: all read.
            seen(from(header(3, D + 200, Some("b1@x")), "Cid")),
            seen(from(
                reply(header(4, D + 300, Some("b2@x")), &["b1@x"]),
                "Dan",
            )),
            // Single unread and single read messages.
            from(header(5, D + 400, Some("c@x")), "Eve"),
            seen(from(header(6, D + 500, Some("d@x")), "Fay")),
        ],
    )
    .await;
    let a1 = id_of(&f.store, "a1@x").await;
    let b2 = id_of(&f.store, "b2@x").await;
    let c = id_of(&f.store, "c@x").await;
    let d = id_of(&f.store, "d@x").await;
    for limit in [1u32, 2, 3, 10] {
        let mut cursor: Option<UnreadFirstCursor> = None;
        let mut rows = Vec::new();
        loop {
            let page = f
                .store
                .unread_first_conversations(scope.clone(), cursor, limit)
                .await
                .expect("page");
            rows.extend(page.items);
            match page.next_cursor {
                Some(next) => cursor = Some(next),
                None => break,
            }
        }
        // Unread conversations first (at their newest unread message), then the others.
        assert_eq!(ids(&rows), [c, a1, d, b2], "limit {limit}");
        assert_eq!(rows[1].message_count, 2);
        assert_eq!(rows[1].unread_count, 1);
    }
}

#[tokio::test]
async fn threads_stay_within_their_account_in_the_unified_inbox() {
    let f = fixture().await;
    let second = f
        .store
        .insert_account(new_account(2, "bob@example.org"))
        .await
        .expect("account")
        .id;
    f.store
        .sync_mailboxes(second, vec![remote("INBOX", &[])])
        .await
        .expect("mailboxes");
    let inbox2 = f.store.mailboxes(second).await.expect("mailboxes")[0].id;
    f.store
        .reset_mailbox(
            inbox2,
            MailboxBaseline {
                uid_validity: UV,
                uid_next: 1,
                highest_modseq: None,
            },
        )
        .await
        .expect("reset");
    // The same conversation (same Message-IDs) reached both accounts.
    let conversation = |offset: i64| {
        vec![
            from(header(1, D + offset, Some("x1@x")), "Xena"),
            from(
                reply(header(2, D + offset + 100, Some("x2@x")), &["x1@x"]),
                "Yuri",
            ),
        ]
    };
    store_in(&f, f.inbox, conversation(0)).await;
    store_in(&f, inbox2, conversation(5)).await;

    let rows = all_rows(
        &f.store,
        InboxMailboxes::Unified(None),
        InboxListKind::Rows,
        1,
    )
    .await;
    assert_eq!(rows.len(), 2, "{rows:#?}");
    assert_ne!(rows[0].message.account_id, rows[1].message.account_id);
    assert_ne!(rows[0].thread_id, rows[1].thread_id);
    assert!(rows.iter().all(|r| r.message_count == 2));
}

// ---------------------------------------------------------------------------------------------
// What an action on conversation rows touches

#[tokio::test]
async fn targets_follow_the_reach() {
    let f = fixture().await;
    store_in(
        &f,
        f.inbox,
        vec![
            from(header(1, D, Some("t1@x")), "Tom"),
            from(reply(header(2, D + 100, Some("t2@x")), &["t1@x"]), "Tim"),
            from(header(3, D + 200, Some("solo@x")), "Uma"),
        ],
    )
    .await;
    store_in(
        &f,
        f.sent,
        vec![seen(from(
            reply(header(1, D + 300, Some("t3@x")), &["t1@x", "t2@x"]),
            "Alice",
        ))],
    )
    .await;
    // The first message also lies in Archive (two locations).
    let mut copy = from(header(1, D, Some("t1@x")), "Tom");
    copy.uid = 1;
    store_in(&f, f.archive, vec![copy]).await;
    let t1 = id_of(&f.store, "t1@x").await;
    let t2 = id_of(&f.store, "t2@x").await;
    let t3 = id_of(&f.store, "t3@x").await;
    let solo = id_of(&f.store, "solo@x").await;
    let rows = vec![
        MessageRef {
            mailbox_id: f.inbox,
            message_id: MessageId(t2),
        },
        MessageRef {
            mailbox_id: f.inbox,
            message_id: MessageId(solo),
        },
    ];
    let targets = |reach, per_location| {
        let rows = rows.clone();
        let store = &f.store;
        async move {
            store
                .conversation_targets(rows, reach, per_location)
                .await
                .expect("targets")
                .into_iter()
                .map(|i| (i.id.0, i.mailbox_id))
                .collect::<Vec<_>>()
        }
    };
    assert_eq!(
        targets(ConversationReach::Newest, true).await,
        [(solo, f.inbox), (t2, f.inbox)]
    );
    assert_eq!(
        targets(ConversationReach::Listed, true).await,
        [(solo, f.inbox), (t2, f.inbox), (t1, f.inbox)]
    );
    let mut everywhere = targets(ConversationReach::Everywhere, true).await;
    everywhere.sort();
    let mut expected = vec![
        (t1, f.inbox),
        (t1, f.archive),
        (t2, f.inbox),
        (t3, f.sent),
        (solo, f.inbox),
    ];
    expected.sort();
    assert_eq!(everywhere, expected);
    // Marks are per message: once each, in the listed mailbox where it lies there.
    assert_eq!(
        targets(ConversationReach::Everywhere, false).await,
        [(t3, f.sent), (solo, f.inbox), (t2, f.inbox), (t1, f.inbox)]
    );

    // An outdated row is an error the UI can explain; no rows is refused.
    let gone = f
        .store
        .conversation_targets(
            vec![MessageRef {
                mailbox_id: f.inbox,
                message_id: MessageId(1),
            }],
            ConversationReach::Listed,
            true,
        )
        .await;
    assert!(
        matches!(gone, Err(crate::StoreError::NotFound(_))),
        "{gone:?}"
    );
    let none = f
        .store
        .conversation_targets(Vec::new(), ConversationReach::Listed, true)
        .await;
    assert!(matches!(none, Err(crate::StoreError::InvalidArgument(_))));
}

// ---------------------------------------------------------------------------------------------
// The messages of an expanded conversation row (stage 17)

#[tokio::test]
async fn an_expanded_row_lists_every_message_of_its_conversation_oldest_first() {
    let f = fixture().await;
    store_in(
        &f,
        f.inbox,
        vec![
            seen(from(header(1, D, Some("k1@x")), "Kim")),
            from(
                reply(header(2, D + 200, Some("k3@x")), &["k1@x", "k2@x"]),
                "Kim",
            ),
            from(header(3, D + 300, Some("solo@x")), "Uma"),
        ],
    )
    .await;
    // My own answer lies in Sent only.
    store_in(
        &f,
        f.sent,
        vec![seen(from(
            reply(header(1, D + 100, Some("k2@x")), &["k1@x"]),
            "Alice",
        ))],
    )
    .await;
    // The first message also lies in Archive: one child, at the listed folder's copy.
    let mut copy = seen(from(header(1, D, Some("k1@x")), "Kim"));
    copy.uid = 1;
    store_in(&f, f.archive, vec![copy]).await;
    let (k1, k2, k3) = (
        id_of(&f.store, "k1@x").await,
        id_of(&f.store, "k2@x").await,
        id_of(&f.store, "k3@x").await,
    );

    let rows = all_rows(
        &f.store,
        InboxMailboxes::Mailbox(f.inbox),
        InboxListKind::Rows,
        50,
    )
    .await;
    let row = rows
        .iter()
        .find(|r| r.message.id.0 == k3)
        .expect("the conversation's row");
    // The row's children: the thread opened by the row's message in the row's folder.
    let children = f
        .store
        .thread(row.message.id, Some(row.message.mailbox_id))
        .await
        .expect("children");
    let listed: Vec<(i64, MailboxId, bool)> = children
        .messages
        .iter()
        .map(|c| (c.message.id.0, c.message.mailbox_id, c.message.unread))
        .collect();
    assert_eq!(
        listed,
        [
            (k1, f.inbox, false),
            (k2, f.sent, false),
            (k3, f.inbox, true)
        ],
        "oldest first, own mail from Sent, each at the listed folder where it lies there"
    );
    assert_eq!(children.messages[0].mailbox_ids, [f.inbox, f.archive]);
    // "n replies" on the row and the children it unfolds agree.
    assert_eq!(children.total, row.message_count);
    assert_eq!(children.messages.len(), 3);
    assert_eq!(children.thread_id, row.thread_id);
    assert!(
        children
            .messages
            .iter()
            .all(|c| c.message.account_id == row.message.account_id)
    );

    // From Archive, the same conversation names the archived copy.
    let from_archive = f
        .store
        .thread(MessageId(k1), Some(f.archive))
        .await
        .expect("children");
    assert_eq!(from_archive.messages[0].message.mailbox_id, f.archive);

    // A message without replies is its only child; a vanished row is an error the UI can show.
    let solo = rows
        .iter()
        .find(|r| r.message_count == 1)
        .expect("the single message");
    let alone = f
        .store
        .thread(solo.message.id, Some(f.inbox))
        .await
        .expect("children");
    assert_eq!(alone.messages.len(), 1);
    assert_eq!(alone.total, 1);
    let gone = f.store.thread(MessageId(1), Some(f.inbox)).await;
    assert!(
        matches!(gone, Err(crate::StoreError::NotFound(_))),
        "{gone:?}"
    );
}

#[tokio::test]
async fn an_expanded_row_of_a_very_long_conversation_holds_its_newest_messages() {
    let f = fixture().await;
    let n = i64::from(mail_model::MAX_THREAD_MESSAGES) + 5;
    let mut headers = Vec::new();
    for k in 0..n {
        let uid = u32::try_from(k).expect("uid") + 1;
        let mut h = header(uid, D + k * 60, Some(&format!("{k}@many")));
        if k > 0 {
            h = reply(h, &["0@many"]);
        }
        headers.push(seen(h));
    }
    store_in(&f, f.inbox, headers).await;
    let rows = all_rows(
        &f.store,
        InboxMailboxes::Mailbox(f.inbox),
        InboxListKind::Rows,
        10,
    )
    .await;
    assert_eq!(rows.len(), 1);
    let children = f
        .store
        .thread(rows[0].message.id, Some(f.inbox))
        .await
        .expect("children");
    assert_eq!(
        children.messages.len(),
        usize::try_from(mail_model::MAX_THREAD_MESSAGES).expect("max")
    );
    assert_eq!(i64::from(children.total), n);
    assert_eq!(children.total, rows[0].message_count);
    // The newest ones, still oldest first, ending with the row's message.
    let first = id_of_owned(&f.store, "5@many".into()).await;
    assert_eq!(children.messages[0].message.id.0, first);
    assert_eq!(
        children.messages.last().map(|c| c.message.id),
        Some(rows[0].message.id)
    );
    assert!(
        children
            .messages
            .windows(2)
            .all(|w| w[0].message.id < w[1].message.id)
    );
}

#[tokio::test]
async fn search_hits_know_their_conversation() {
    let f = fixture().await;
    store_in(
        &f,
        f.inbox,
        vec![
            from(header(1, D, Some("h1@x")), "Hal"),
            from(reply(header(2, D + 100, Some("h2@x")), &["h1@x"]), "Ida"),
        ],
    )
    .await;
    let h2 = id_of(&f.store, "h2@x").await;
    let (thread, count): (Option<i64>, Option<i64>) = f
        .store
        .read(move |c| Ok(c.query_row(search::HIT_THREAD, [h2], |r| Ok((r.get(0)?, r.get(1)?)))?))
        .await
        .expect("hit thread");
    assert!(thread.is_some());
    assert_eq!(count, Some(2));
}

/// Stage 17 (decision 0049): a conversation row shows its oldest message's subject without reply
/// and forward prefixes, like Thunderbird, in every list and in search results; the row's
/// message keeps its own subject (search matches it).
#[tokio::test]
async fn a_conversation_row_shows_its_oldest_subject_without_prefixes() {
    let f = fixture().await;
    let mut root = from(header(1, D, Some("w1@x")), "Carol");
    root.subject = "Website relaunch".into();
    let mut answer = from(
        reply(header(2, D + 200, Some("w3@x")), &["w1@x", "w2@x"]),
        "Dave",
    );
    answer.subject = "Re: AW: Website relaunch".into();
    let mut solo = from(header(3, D + 300, Some("s1@x")), "Uma");
    solo.subject = "Re: our backup strategy".into();
    store_in(&f, f.inbox, vec![root, answer, solo]).await;
    let mut own = from(reply(header(1, D + 100, Some("w2@x")), &["w1@x"]), "Alice");
    own.subject = "Fwd: Website relaunch (draft)".into();
    store_in(&f, f.sent, vec![seen(own)]).await;
    let (w3, s1) = (id_of(&f.store, "w3@x").await, id_of(&f.store, "s1@x").await);

    let subjects = |rows: &[ThreadListItem]| -> BTreeMap<i64, (String, String)> {
        rows.iter()
            .map(|r| {
                (
                    r.message.id.0,
                    (r.subject.clone(), r.message.subject.clone()),
                )
            })
            .collect()
    };
    let expected = BTreeMap::from([
        (
            w3,
            (
                "Website relaunch".to_owned(),
                "Re: AW: Website relaunch".to_owned(),
            ),
        ),
        // A message alone is its own conversation: its subject without the prefix.
        (
            s1,
            (
                "our backup strategy".to_owned(),
                "Re: our backup strategy".to_owned(),
            ),
        ),
    ]);
    // The sorted inbox's rows and their summaries.
    let rows = all_rows(
        &f.store,
        InboxMailboxes::Mailbox(f.inbox),
        InboxListKind::Rows,
        50,
    )
    .await;
    assert_eq!(subjects(&rows), expected);
    let summary = mail_model::ConversationSummary::of(&rows[0]);
    assert_eq!(summary.subject, rows[0].subject);
    // A folder's thread list.
    let threads = f
        .store
        .list_threads(f.inbox, None, 50)
        .await
        .expect("threads");
    assert_eq!(subjects(&threads.items), expected);
    // Sent: the same conversation, from its row there (the own answer).
    let sent = f
        .store
        .list_threads(f.sent, None, 50)
        .await
        .expect("sent threads");
    assert_eq!(sent.items.len(), 1);
    assert_eq!(sent.items[0].subject, "Website relaunch");
    assert_eq!(
        sent.items[0].message.subject,
        "Fwd: Website relaunch (draft)"
    );
    // Search hits: the conversation's subject beside the message's own.
    let hits = f
        .store
        .search(
            search::SearchFilter {
                mailbox_sets: vec![vec![f.inbox]],
                ..search::SearchFilter::default()
            },
            None,
            50,
        )
        .await
        .expect("search");
    let found: BTreeMap<i64, (String, String)> = hits
        .hits
        .iter()
        .map(|h| {
            (
                h.item.id.0,
                (h.conversation_subject.clone(), h.item.subject.clone()),
            )
        })
        .collect();
    assert_eq!(found.get(&w3), expected.get(&w3));
    assert_eq!(found.get(&s1), expected.get(&s1));
    let w1 = id_of(&f.store, "w1@x").await;
    assert_eq!(
        found.get(&w1),
        Some(&("Website relaunch".to_owned(), "Website relaunch".to_owned()))
    );
}

// ---------------------------------------------------------------------------------------------
// Query plans

fn plan(conn: &Connection, sql: &str) -> Vec<String> {
    let mut stmt = conn
        .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
        .unwrap_or_else(|err| panic!("prepare EXPLAIN QUERY PLAN {sql}: {err}"));
    let nulls = vec![rusqlite::types::Null; stmt.parameter_count()];
    stmt.query_map(rusqlite::params_from_iter(nulls), |r| r.get::<_, String>(3))
        .expect("query plan")
        .collect::<Result<Vec<_>, _>>()
        .expect("plan rows")
}

/// Tables that only hold the user's marks (dozens to a few thousand rows).
const SMALL: [&str; 3] = ["pin", "snooze", "blocked_hidden"];

fn expected() -> Vec<(String, String, bool, Vec<&'static str>)> {
    let thread_check = [
        "SEARCH n USING INDEX message_thread (thread_id=? AND rowid>?)",
        "SEARCH nl USING ",
    ];
    let mut out = Vec::new();
    for (name, kind, first) in [
        (
            "rows",
            InboxListKind::Rows,
            "SEARCH l USING PRIMARY KEY (mailbox_id=? AND message_id<?)",
        ),
        (
            "category",
            InboxListKind::Category(mail_model::MessageCategory::Newsletter),
            "SEARCH l USING INDEX loc_category (mailbox_id=? AND category=? AND message_id<?)",
        ),
        (
            "important",
            InboxListKind::Important,
            "SEARCH l USING INDEX loc_important (mailbox_id=? AND message_id<?)",
        ),
        (
            "unread",
            InboxListKind::Unread,
            "SEARCH l USING INDEX loc_unseen (mailbox_id=? AND message_id<?)",
        ),
        (
            "read",
            InboxListKind::Read,
            "SEARCH l USING PRIMARY KEY (mailbox_id=? AND message_id<?)",
        ),
    ] {
        let mut required = vec![first, "SEARCH m USING INTEGER PRIMARY KEY (rowid=?)"];
        required.extend(thread_check.iter().copied());
        if kind == InboxListKind::Rows {
            required.push("SEARCH t USING INDEX message_thread (thread_id=?)");
            required.push("SEARCH r USING INDEX message_thread (thread_id=?)");
        }
        if kind == InboxListKind::Read {
            required.push("SEARCH u USING INDEX message_thread (thread_id=?)");
        }
        out.push((
            format!("conversations: {name}, one mailbox"),
            conversations::conversations_sql(kind, 1),
            false,
            required,
        ));
        out.push((
            format!("conversations: {name}, three mailboxes"),
            conversations::conversations_sql(kind, 3),
            true,
            vec![
                first,
                "SEARCH n USING INDEX message_thread (thread_id=? AND rowid>?)",
            ],
        ));
    }
    for (name, sql, required) in [
        (
            "size and unread messages of a thread",
            conversations::THREAD_COUNTS,
            vec![
                "SEARCH m USING COVERING INDEX message_thread (thread_id=?)",
                "SEARCH u USING INDEX loc_message (message_id=?)",
            ],
        ),
        (
            "subject of a thread's oldest message",
            conversations::THREAD_ROOT_SUBJECT,
            vec!["SEARCH message USING INDEX message_thread (thread_id=?)"],
        ),
        (
            "newest messages of a thread",
            conversations::THREAD_RECENT,
            vec![
                "SEARCH m USING INDEX message_thread (thread_id=?)",
                "SEARCH f USING INDEX loc_message (message_id=?)",
            ],
        ),
        (
            "summary of a message",
            conversations::MESSAGE_SUMMARY,
            vec!["SEARCH m USING INTEGER PRIMARY KEY (rowid=?)"],
        ),
        (
            "newest of a thread in a mailbox",
            conversations::NEWEST_IN_MAILBOX,
            vec!["SEARCH t USING COVERING INDEX message_thread (thread_id=?)"],
        ),
        (
            "thread in a mailbox",
            conversations::THREAD_IN_MAILBOX,
            vec!["SEARCH t USING COVERING INDEX message_thread (thread_id=?)"],
        ),
        (
            "message in a mailbox",
            conversations::LOCATION_EXISTS,
            vec!["SEARCH message_location USING PRIMARY KEY (mailbox_id=? AND message_id=?)"],
        ),
        (
            "thread of a hit",
            search::HIT_THREAD,
            vec![
                "SEARCH m USING INTEGER PRIMARY KEY (rowid=?)",
                "SEARCH t USING INTEGER PRIMARY KEY (rowid=?)",
            ],
        ),
    ] {
        out.push((name.to_owned(), sql.to_owned(), false, required));
    }
    out
}

fn check_plans(conn: &Connection, stage: &str) {
    let mut problems = Vec::new();
    for (name, sql, merge, required) in expected() {
        let lines = plan(conn, &sql);
        let bad = lines.iter().any(|l| {
            let merge_scan = merge && l.starts_with("SCAN (subquery-");
            let small = SMALL.iter().any(|t| l.starts_with(&format!("SCAN {t}")));
            (l.starts_with("SCAN") && l != "SCAN CONSTANT ROW" && !merge_scan && !small)
                || (l.contains("TEMP B-TREE") && !merge)
        });
        let missing: Vec<&str> = required
            .iter()
            .copied()
            .filter(|want| !lines.iter().any(|l| l.starts_with(want)))
            .collect();
        if bad || !missing.is_empty() {
            problems.push(format!("{name}: plan {lines:#?}, missing {missing:#?}"));
        }
    }
    assert!(
        problems.is_empty(),
        "{stage}: full scan, temporary B-tree or unexpected index:\n{}",
        problems.join("\n")
    );
}

async fn check_all(store: &Store, stage: &'static str) {
    store
        .read(move |conn| {
            check_plans(conn, stage);
            Ok(())
        })
        .await
        .expect("reader plans");
    store
        .write(move |tx| {
            check_plans(tx, stage);
            Ok(())
        })
        .await
        .expect("writer plans");
}

#[tokio::test]
async fn every_conversation_query_uses_its_index() {
    let f = fixture().await;
    check_all(&f.store, "fresh database").await;
    // 3,000 inbox and 1,000 archive messages in threads of up to 4, some read, some marked.
    let mut by_mailbox: BTreeMap<i64, Vec<MessageHeader>> = BTreeMap::new();
    for k in 0..4_000i64 {
        let mailbox = if k % 4 == 3 { f.archive } else { f.inbox };
        let list = by_mailbox.entry(mailbox.0).or_default();
        let uid = u32::try_from(list.len()).expect("uid") + 1;
        let mut h = header(uid, D + k * 600, Some(&format!("{k}@p")));
        if k % 4 != 0 {
            h = reply(h, &[&format!("{}@p", k - k % 4)]);
        }
        if k % 7 != 0 {
            h = seen(h);
        }
        list.push(h);
    }
    for (mailbox, headers) in by_mailbox {
        for window in headers.chunks(500) {
            store_in(&f, MailboxId(mailbox), window.to_vec()).await;
        }
    }
    f.store
        .write(|tx| {
            tx.execute_batch(
                "UPDATE message SET category = id % 3, category_auto = id % 3,
                                    important = (id % 50 = 0);",
            )?;
            Ok(())
        })
        .await
        .expect("categories");
    for k in (0..4_000i64).step_by(97) {
        let id = id_of_owned(&f.store, format!("{k}@p")).await;
        f.store.pin_message(MessageId(id), NOW).await.expect("pin");
    }
    f.store
        .write(|tx| {
            tx.execute_batch("ANALYZE; PRAGMA optimize;")?;
            Ok(())
        })
        .await
        .expect("analyze");
    check_all(&f.store, "after ANALYZE").await;
    // Reopened: the readers load the statistics when they open.
    let Fixture { _dir, store, .. } = f;
    drop(store);
    let reopened = Store::open(
        &crate::test_support::db_path(&_dir),
        crate::StoreOptions::default(),
    )
    .expect("reopen");
    check_all(&reopened, "reopened").await;
}

async fn id_of_owned(store: &Store, msgid: String) -> i64 {
    store
        .read(move |c| {
            Ok(c.query_row(
                "SELECT id FROM message WHERE message_id_hdr = ?1",
                [msgid],
                |r| r.get(0),
            )?)
        })
        .await
        .expect("message id")
}
