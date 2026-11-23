//! Stage 16 (decision 0033): reading list, mailbox favourites and colours, blocked senders,
//! the unread lists and their query plans.

use mail_model::{AccountColor, MessageId, UnreadFirstCursor, UnreadPhase};
use rusqlite::Connection;

use crate::inbox::{self, InboxListKind};
use crate::test_support::{UV, fixture, header, seen};
use crate::{InboxMailboxes, MarkView, Store, parity};

async fn sync_kinds(store: &Store) -> Vec<(String, String)> {
    store
        .read(|c| {
            Ok(
                c.prepare("SELECT kind, id FROM sync_dirty ORDER BY kind, id")?
                    .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
                    .collect::<Result<Vec<_>, _>>()?,
            )
        })
        .await
        .expect("dirty")
}

#[tokio::test]
async fn reading_list_marks_rows_lists_them_and_is_captured() {
    let f = fixture().await;
    let headers = vec![
        header(1, 1_700_000_000, Some("a@x")),
        header(2, 1_700_000_600, Some("b@x")),
        header(3, 1_700_001_200, None),
    ];
    f.store
        .store_headers(f.inbox, UV, headers, None)
        .await
        .expect("store");
    let page = f
        .store
        .list_messages(f.inbox, None, 10)
        .await
        .expect("list");
    let ids: Vec<MessageId> = page.items.iter().map(|i| i.id).collect();
    assert!(page.items.iter().all(|i| !i.reading_list));
    f.store
        .write(|tx| Ok(tx.execute("DELETE FROM sync_dirty", [])?))
        .await
        .expect("clear");

    // Newest first: 3 (no Message-ID), 2, 1.
    let changed = f
        .store
        .set_reading_list(vec![ids[2], ids[1]], true, 10)
        .await
        .expect("add");
    assert_eq!(changed.len(), 2);
    let again = f
        .store
        .set_reading_list(vec![ids[2]], true, 11)
        .await
        .expect("again");
    assert!(again.is_empty(), "adding twice changes nothing");
    assert!(
        f.store
            .set_reading_list(vec![ids[0]], true, 12)
            .await
            .is_err(),
        "a message without Message-ID cannot be marked"
    );
    let page = f
        .store
        .list_messages(f.inbox, None, 10)
        .await
        .expect("list");
    let marked: Vec<bool> = page.items.iter().map(|i| i.reading_list).collect();
    assert_eq!(marked, vec![false, true, true]);

    let view = f
        .store
        .mark_view(MarkView::ReadingList, vec![f.account], vec![f.inbox], 100)
        .await
        .expect("view");
    let subjects: Vec<&str> = view.iter().map(|i| i.subject.as_str()).collect();
    assert_eq!(
        subjects,
        vec!["Subject 2", "Subject 1"],
        "added order: 1 first"
    );
    let dirty = sync_kinds(&f.store).await;
    assert_eq!(
        dirty,
        vec![
            ("readingList".to_owned(), format!("{}/a@x", f.account)),
            ("readingList".to_owned(), format!("{}/b@x", f.account)),
        ]
    );
    let removed = f
        .store
        .set_reading_list(vec![ids[2]], false, 13)
        .await
        .expect("remove");
    assert_eq!(removed.len(), 1);
    let view = f
        .store
        .mark_view(MarkView::ReadingList, vec![f.account], vec![f.inbox], 100)
        .await
        .expect("view");
    assert_eq!(view.len(), 1);
}

#[tokio::test]
async fn favourites_and_colours_show_in_the_folder_list_and_go_away_when_cleared() {
    let f = fixture().await;
    f.store
        .write(|tx| Ok(tx.execute("DELETE FROM sync_dirty", [])?))
        .await
        .expect("clear");
    let (account, changed) = f
        .store
        .set_mailbox_favorite(f.archive, true, 1)
        .await
        .expect("favourite");
    assert_eq!(account, f.account);
    assert!(changed);
    assert!(
        !f.store
            .set_mailbox_favorite(f.archive, true, 2)
            .await
            .expect("again")
            .1
    );
    f.store
        .set_mailbox_favorite(f.inbox, true, 3)
        .await
        .expect("inbox");
    let color = AccountColor::new(3).expect("colour");
    f.store
        .set_mailbox_color(f.sent, Some(color), 4)
        .await
        .expect("colour");

    let list = f.store.mailboxes(f.account).await.expect("mailboxes");
    let find = |id| list.iter().find(|m| m.id == id).expect("mailbox");
    assert_eq!(find(f.archive).favorite_position, Some(0));
    assert_eq!(find(f.inbox).favorite_position, Some(1));
    assert_eq!(find(f.sent).favorite_position, None);
    assert_eq!(find(f.sent).color, Some(color));

    let favourites = f.store.favorite_mailboxes().await.expect("favourites");
    assert_eq!(
        favourites.iter().map(|m| m.id).collect::<Vec<_>>(),
        vec![f.archive, f.inbox]
    );
    f.store
        .reorder_favorite_mailboxes(vec![f.inbox], 5)
        .await
        .expect("reorder");
    let favourites = f.store.favorite_mailboxes().await.expect("favourites");
    assert_eq!(
        favourites.iter().map(|m| m.id).collect::<Vec<_>>(),
        vec![f.inbox, f.archive]
    );
    let dirty = sync_kinds(&f.store).await;
    assert!(dirty.iter().all(|(k, _)| k == "mailboxPref"));
    assert!(dirty.contains(&("mailboxPref".to_owned(), format!("{}/Sent", f.account))));

    // Cleared: the row goes.
    f.store
        .set_mailbox_color(f.sent, None, 6)
        .await
        .expect("no colour");
    f.store
        .set_mailbox_favorite(f.archive, false, 7)
        .await
        .expect("no favourite");
    let rows: i64 = f
        .store
        .read(|c| Ok(c.query_row("SELECT count(*) FROM mailbox_pref", [], |r| r.get(0))?))
        .await
        .expect("rows");
    assert_eq!(rows, 1, "only the inbox is left");
}

#[tokio::test]
async fn blocked_senders_hide_mail_on_read_only_accounts_until_unblocked() {
    let f = fixture().await;
    let headers = vec![
        header(1, 1_700_000_000, Some("a@x")),
        header(2, 1_700_000_600, Some("b@x")),
    ];
    f.store
        .store_headers(f.inbox, UV, headers, None)
        .await
        .expect("store");
    let scope = || InboxMailboxes::Mailbox(f.inbox);
    let rows = f
        .store
        .inbox_list(scope(), InboxListKind::Rows, None, 10)
        .await
        .expect("rows");
    assert_eq!(rows.items.len(), 2);
    let newest = rows.items[0].id;

    assert!(
        f.store
            .block_sender("sender@example.org".into(), 1)
            .await
            .expect("block")
    );
    assert!(
        !f.store
            .block_sender("sender@example.org".into(), 2)
            .await
            .expect("again")
    );
    assert!(
        f.store
            .is_sender_blocked("sender@example.org".into())
            .await
            .expect("blocked")
    );
    assert!(
        !f.store
            .hide_blocked_message(newest, "other@example.org".into())
            .await
            .expect("not blocked"),
        "only mail of a blocked sender is hidden"
    );
    assert!(
        f.store
            .hide_blocked_message(newest, "sender@example.org".into())
            .await
            .expect("hide")
    );
    for kind in [
        InboxListKind::Rows,
        InboxListKind::Unread,
        InboxListKind::Important,
    ] {
        let page = f
            .store
            .inbox_list(scope(), kind, None, 10)
            .await
            .expect("list");
        assert!(
            page.items.iter().all(|i| i.id != newest),
            "{kind:?} hides it"
        );
    }
    let listed = f.store.blocked_senders().await.expect("list");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].address, "sender@example.org");
    // Decision 0036: the list's note counts what is left out.
    assert_eq!(
        f.store.count_blocked_hidden(scope()).await.expect("count"),
        1
    );

    assert!(
        f.store
            .unblock_sender("sender@example.org".into())
            .await
            .expect("unblock")
    );
    let rows = f
        .store
        .inbox_list(scope(), InboxListKind::Rows, None, 10)
        .await
        .expect("rows");
    assert_eq!(rows.items.len(), 2, "unblocking shows it again");
    assert_eq!(
        f.store.count_blocked_hidden(scope()).await.expect("count"),
        0
    );
}

/// Decision 0036: pins of attachments by account, Message-ID and index; listed with their
/// message, captured for the sync, and written back from a record (also with a `/` in the
/// Message-ID).
#[tokio::test]
async fn attachment_pins_are_listed_captured_and_applied_from_the_sync() {
    use crate::AttachmentPinInput;
    let f = fixture().await;
    let headers = vec![
        header(1, 1_700_000_000, Some("a@x")),
        header(2, 1_700_000_600, Some("b/c@x")),
        header(3, 1_700_001_200, None),
    ];
    f.store
        .store_headers(f.inbox, UV, headers, None)
        .await
        .expect("store");
    let page = f
        .store
        .list_messages(f.inbox, None, 10)
        .await
        .expect("list");
    let ids: Vec<MessageId> = page.items.iter().map(|i| i.id).collect();
    f.store
        .write(|tx| Ok(tx.execute("DELETE FROM sync_dirty", [])?))
        .await
        .expect("clear");
    let input = |index: u32, name: &str| AttachmentPinInput {
        index,
        name: name.into(),
        mime_type: "application/pdf".into(),
        size: 42,
    };
    let (_, changed) = f
        .store
        .set_attachment_pin(ids[1], 2, Some(input(2, "b.pdf")), 10)
        .await
        .expect("pin");
    assert!(changed);
    let (_, changed) = f
        .store
        .set_attachment_pin(ids[1], 2, Some(input(2, "b.pdf")), 11)
        .await
        .expect("again");
    assert!(!changed, "pinning twice changes nothing");
    f.store
        .set_attachment_pin(ids[2], 0, Some(input(0, "a.pdf")), 12)
        .await
        .expect("pin");
    assert!(
        f.store
            .set_attachment_pin(ids[0], 0, Some(input(0, "x.pdf")), 13)
            .await
            .is_err(),
        "a message without Message-ID cannot be pinned"
    );
    assert!(
        f.store
            .set_attachment_pin(ids[1], 1, Some(input(1, "bad\u{7}name")), 13)
            .await
            .is_err(),
        "no controls in a kept name"
    );
    assert_eq!(
        f.store.attachment_pins_of(ids[1]).await.expect("pins"),
        vec![2]
    );
    assert!(
        f.store
            .attachment_pins_of(ids[0])
            .await
            .expect("none")
            .is_empty()
    );

    let view = f
        .store
        .pinned_attachments(vec![f.account], vec![f.inbox], 100)
        .await
        .expect("view");
    let listed: Vec<(&str, u32, &str)> = view
        .iter()
        .map(|p| (p.message.subject.as_str(), p.index, p.name.as_str()))
        .collect();
    assert_eq!(
        listed,
        vec![("Subject 1", 0, "a.pdf"), ("Subject 2", 2, "b.pdf")],
        "newest pin first"
    );
    let dirty = sync_kinds(&f.store).await;
    assert_eq!(
        dirty,
        vec![
            ("attachmentPin".to_owned(), format!("{}/a@x/0", f.account)),
            ("attachmentPin".to_owned(), format!("{}/b/c@x/2", f.account)),
        ]
    );
    let id = format!("{}/b/c@x/2", f.account);
    let content = f
        .store
        .read({
            let id = id.clone();
            move |c| crate::attachment_pins::synced_content(c, &id)
        })
        .await
        .expect("content")
        .expect("some");
    assert_eq!(
        content,
        serde_json::json!({ "name": "b.pdf", "mimeType": "application/pdf", "size": 42,
                            "pinnedAtMs": 10 })
    );

    // From another device: a new pin, then the removal of this one.
    let other = format!("{}/b/c@x/5", f.account);
    f.store
        .write({
            let other = other.clone();
            move |tx| {
                crate::attachment_pins::apply_synced(
                    tx,
                    &other,
                    Some(
                        &serde_json::json!({ "name": "c.txt", "mimeType": "text/plain",
                                              "size": 1, "pinnedAtMs": 20 }),
                    ),
                )?;
                crate::attachment_pins::apply_synced(tx, &id, None)
            }
        })
        .await
        .expect("apply");
    assert_eq!(
        f.store.attachment_pins_of(ids[1]).await.expect("pins"),
        vec![5]
    );
    let refused = f
        .store
        .write(move |tx| {
            crate::attachment_pins::apply_synced(
                tx,
                &other,
                Some(&serde_json::json!({ "name": "", "mimeType": "x", "size": 1,
                                          "pinnedAtMs": 1 })),
            )
        })
        .await;
    assert!(refused.is_err(), "an empty name is refused");

    let (_, changed) = f
        .store
        .set_attachment_pin(ids[1], 5, None, 30)
        .await
        .expect("unpin");
    assert!(changed);
    assert!(
        f.store
            .attachment_pins_of(ids[1])
            .await
            .expect("none")
            .is_empty()
    );
}

#[tokio::test]
async fn unread_only_and_unread_first_page_through_every_message_once() {
    let f = fixture().await;
    // 25 messages; every third unread.
    let headers: Vec<_> = (1..=25u32)
        .map(|uid| {
            let h = header(
                uid,
                1_700_000_000 + i64::from(uid) * 60,
                Some(&format!("{uid}@x")),
            );
            if uid % 3 == 0 { h } else { seen(h) }
        })
        .collect();
    f.store
        .store_headers(f.inbox, UV, headers, None)
        .await
        .expect("store");
    let unread = f
        .store
        .inbox_list(
            InboxMailboxes::Mailbox(f.inbox),
            InboxListKind::Unread,
            None,
            100,
        )
        .await
        .expect("unread");
    assert_eq!(unread.items.len(), 8);
    assert!(unread.items.iter().all(|i| i.unread));
    assert!(unread.items.windows(2).all(|w| w[0].id > w[1].id));

    for limit in [1u32, 3, 8, 9, 100] {
        let mut cursor: Option<UnreadFirstCursor> = None;
        let mut seen_ids = Vec::new();
        let mut flags = Vec::new();
        loop {
            let page = f
                .store
                .unread_first_list(InboxMailboxes::Mailbox(f.inbox), cursor, limit)
                .await
                .expect("page");
            assert!(page.items.len() <= usize::try_from(limit).expect("limit"));
            seen_ids.extend(page.items.iter().map(|i| i.id));
            flags.extend(page.items.iter().map(|i| i.unread));
            match page.next_cursor {
                Some(next) => cursor = Some(next),
                None => break,
            }
            assert!(seen_ids.len() <= 25, "limit {limit}: no endless paging");
        }
        assert_eq!(seen_ids.len(), 25, "limit {limit}: every message once");
        let mut unique = seen_ids.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), 25, "limit {limit}: no duplicates");
        assert!(flags[..8].iter().all(|u| *u), "limit {limit}: unread first");
        assert!(flags[8..].iter().all(|u| !*u), "limit {limit}: then read");
        assert!(seen_ids[..8].windows(2).all(|w| w[0] > w[1]));
        assert!(seen_ids[8..].windows(2).all(|w| w[0] > w[1]));
    }
    // A cursor in the read phase continues there.
    let page = f
        .store
        .unread_first_list(
            InboxMailboxes::Mailbox(f.inbox),
            Some(UnreadFirstCursor {
                phase: UnreadPhase::Read,
                before: None,
            }),
            5,
        )
        .await
        .expect("read phase");
    assert!(page.items.iter().all(|i| !i.unread));
}

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

fn check_plans(conn: &Connection, stage: &str) {
    let queries: Vec<(&str, String, Vec<&str>)> = vec![
        (
            "only unread",
            inbox::inbox_sql(InboxListKind::Unread, 1),
            vec![
                "SEARCH l USING INDEX loc_unseen (mailbox_id=? AND message_id<?)",
                "SEARCH m USING INTEGER PRIMARY KEY (rowid=?)",
                "SEARCH h USING PRIMARY KEY (account_uuid=? AND message_id_hdr=?)",
            ],
        ),
        (
            "unread, three mailboxes",
            inbox::inbox_sql(InboxListKind::Unread, 3),
            vec!["SEARCH l USING INDEX loc_unseen (mailbox_id=? AND message_id<?)"],
        ),
        (
            "read part of unread first",
            inbox::inbox_sql(InboxListKind::Read, 1),
            vec![
                "SEARCH l USING PRIMARY KEY (mailbox_id=? AND message_id<?)",
                "SEARCH m USING INTEGER PRIMARY KEY (rowid=?)",
            ],
        ),
        (
            "inbox rows with hidden mail",
            inbox::inbox_sql(InboxListKind::Rows, 1),
            vec!["SEARCH h USING PRIMARY KEY (account_uuid=? AND message_id_hdr=?)"],
        ),
        (
            "reading list of a message",
            parity::READING_OF.to_owned(),
            vec![
                "SEARCH reading_list USING INDEX sqlite_autoindex_reading_list_1 (account_uuid=? AND message_id_hdr=?)",
            ],
        ),
        (
            "hidden mail of blocked senders, counted",
            parity::COUNT_HIDDEN.to_owned(),
            vec![
                "SEARCH h USING PRIMARY KEY (account_uuid=?)",
                "SEARCH m USING COVERING INDEX message_msgid (account_id=? AND message_id_hdr=?)",
                // The primary key or `loc_message`, both a single probe.
                "SEARCH l USING ",
            ],
        ),
        (
            "blocked sender",
            parity::IS_BLOCKED.to_owned(),
            vec!["SEARCH blocked_sender USING PRIMARY KEY (address=?)"],
        ),
        (
            "row marks",
            crate::marks::ROW_MARKS.to_owned(),
            vec![
                "SEARCH r USING COVERING INDEX sqlite_autoindex_reading_list_1 (account_uuid=? AND message_id_hdr=?)",
            ],
        ),
    ];
    let mut problems = Vec::new();
    for (name, sql, required) in queries {
        let lines = plan(conn, &sql);
        let merged = sql.contains("UNION ALL");
        let bad = lines.iter().any(|l| {
            (l.starts_with("SCAN") && !(merged && l.starts_with("SCAN (subquery-")))
                || (l.contains("TEMP B-TREE") && !merged)
        });
        let missing: Vec<&str> = required
            .into_iter()
            .filter(|want| !lines.iter().any(|l| l.starts_with(want)))
            .collect();
        if bad || !missing.is_empty() {
            problems.push(format!("{name}: plan {lines:#?}, missing {missing:#?}"));
        }
    }
    assert!(problems.is_empty(), "{stage}:\n{}", problems.join("\n"));
}

#[tokio::test]
async fn every_stage_16_query_uses_its_index() {
    let f = fixture().await;
    let headers: Vec<_> = (1..=2_000u32)
        .map(|uid| {
            let h = header(
                uid,
                1_700_000_000 + i64::from(uid) * 60,
                Some(&format!("{uid}@x")),
            );
            if uid % 5 == 0 { h } else { seen(h) }
        })
        .collect();
    for window in headers.chunks(500) {
        f.store
            .store_headers(f.inbox, UV, window.to_vec(), None)
            .await
            .expect("store");
    }
    for stage in ["fresh", "after ANALYZE"] {
        if stage == "after ANALYZE" {
            f.store
                .write(|tx| Ok(tx.execute_batch("ANALYZE")?))
                .await
                .expect("analyze");
        }
        f.store
            .read(move |c| {
                check_plans(c, stage);
                Ok(())
            })
            .await
            .expect("reader");
        f.store
            .write(move |tx| {
                check_plans(tx, stage);
                Ok(())
            })
            .await
            .expect("writer");
    }
}

/// A second account with an INBOX (synced, writes as given).
async fn second_account(
    store: &Store,
    writes: bool,
) -> (mail_model::AccountId, mail_model::MailboxId) {
    use crate::test_support::{new_account, remote};
    let account = store
        .insert_account(new_account(2, "bob@example.net"))
        .await
        .expect("account")
        .id;
    store
        .sync_mailboxes(account, vec![remote("INBOX", &[]), remote("Kept", &[])])
        .await
        .expect("mailboxes");
    let kept = store
        .mailboxes(account)
        .await
        .expect("mailboxes")
        .into_iter()
        .find(|m| m.path == "Kept")
        .expect("kept")
        .id;
    store
        .reset_mailbox(
            kept,
            crate::MailboxBaseline {
                uid_validity: UV,
                uid_next: 1,
                highest_modseq: None,
            },
        )
        .await
        .expect("reset");
    store
        .set_writes_enabled(account, writes)
        .await
        .expect("writes");
    (account, kept)
}

#[tokio::test]
async fn a_transfer_is_checked_recorded_in_the_target_and_removes_the_source_after_the_copy() {
    use mail_model::{
        ErrorCode, IpcError, MessageRef, OperationPayload, OperationState, ServerCopy, TransferMode,
    };

    use crate::OperationResult;

    let f = fixture().await;
    f.store
        .store_headers(
            f.inbox,
            UV,
            vec![
                header(1, 1_700_000_000, Some("m1@x")),
                header(2, 1_700_000_600, None),
            ],
            None,
        )
        .await
        .expect("store");
    let items = f
        .store
        .list_messages(f.inbox, None, 10)
        .await
        .expect("list")
        .items;
    let with_id = MessageRef {
        mailbox_id: f.inbox,
        message_id: items
            .iter()
            .find(|i| i.subject == "Subject 1")
            .expect("1")
            .id,
    };
    let without_id = MessageRef {
        mailbox_id: f.inbox,
        message_id: items
            .iter()
            .find(|i| i.subject == "Subject 2")
            .expect("2")
            .id,
    };
    let (target_account, kept) = second_account(&f.store, false).await;
    let code = |err: crate::StoreError| IpcError::from(mail_model::CoreError::from(err)).code;

    // The target account is read-only.
    let err = f
        .store
        .check_transfer(vec![with_id], kept, TransferMode::Copy)
        .await
        .expect_err("read only target");
    assert_eq!(code(err), ErrorCode::ReadOnly);
    f.store
        .set_writes_enabled(target_account, true)
        .await
        .expect("writes");
    // A move also needs the source account to write (alice is read-only here).
    let err = f
        .store
        .check_transfer(vec![with_id], kept, TransferMode::Move)
        .await
        .expect_err("read only source");
    assert_eq!(code(err), ErrorCode::ReadOnly);
    f.store
        .set_writes_enabled(f.account, true)
        .await
        .expect("writes");
    // No Message-ID: the copy could not be confirmed. Same account: not a transfer.
    for (at, target) in [(without_id, kept), (with_id, f.archive)] {
        let err = f
            .store
            .check_transfer(vec![at], target, TransferMode::Move)
            .await
            .expect_err("refused");
        assert_eq!(code(err), ErrorCode::InvalidArgument);
    }

    let change = f
        .store
        .enqueue_transfer(
            kept,
            TransferMode::Move,
            vec![(with_id, "outgoing/x/transfer.eml".into())],
            "req-1".into(),
            5,
        )
        .await
        .expect("recorded");
    assert_eq!(change.account, target_account);
    assert_eq!(change.operations.len(), 1);
    let op = change.operations[0].clone();
    assert!(matches!(
        &op.payload,
        OperationPayload::Append {
            source: mail_model::AppendSource::Transfer {
                remove: Some(_),
                ..
            },
            ..
        }
    ));
    // Recorded twice (an IPC call repeated): nothing new.
    let again = f
        .store
        .enqueue_transfer(
            kept,
            TransferMode::Move,
            vec![(with_id, "outgoing/x/transfer.eml".into())],
            "req-1".into(),
            6,
        )
        .await
        .expect("repeated");
    assert!(again.repeated && again.operations.is_empty());
    // Nothing happened to the source yet.
    assert_eq!(
        f.store
            .list_messages(f.inbox, None, 10)
            .await
            .expect("list")
            .items
            .len(),
        2
    );
    assert!(
        f.store
            .operations(f.account, None, 10)
            .await
            .expect("ops")
            .is_empty()
    );

    // Confirmed without a copy found: the source stays.
    let mut payload = op.payload.clone();
    f.store
        .begin_operations(vec![(op.id, payload.clone())])
        .await
        .expect("begin");
    let change = f
        .store
        .finish_operations(vec![(op.id, OperationResult::Appended { copy: None })])
        .await
        .expect("finish")
        .expect("change");
    assert!(change.others.is_empty());
    assert_eq!(
        f.store
            .list_messages(f.inbox, None, 10)
            .await
            .expect("list")
            .items
            .len(),
        2
    );

    // A second transfer, confirmed with its copy: the source is removed through its own
    // account's outbox, in the batch of the transfer.
    let change = f
        .store
        .enqueue_transfer(
            kept,
            TransferMode::Move,
            vec![(with_id, "outgoing/y/transfer.eml".into())],
            "req-2".into(),
            7,
        )
        .await
        .expect("recorded");
    let op = change.operations[0].clone();
    payload.clone_from(&op.payload);
    f.store
        .begin_operations(vec![(op.id, payload)])
        .await
        .expect("begin");
    let copy = ServerCopy {
        mailbox: mail_model::TargetMailbox {
            mailbox_id: kept,
            name: "Kept".into(),
        },
        uid_validity: UV,
        uid: 1,
        message_id_header: "m1@x".into(),
    };
    let change = f
        .store
        .finish_operations(vec![(
            op.id,
            OperationResult::Appended { copy: Some(copy) },
        )])
        .await
        .expect("finish")
        .expect("change");
    assert_eq!(change.others.len(), 1);
    let other = &change.others[0];
    assert_eq!(other.account, f.account);
    assert_eq!(other.operations.len(), 1);
    assert!(matches!(
        other.operations[0].payload,
        OperationPayload::Expunge { .. }
    ));
    assert_eq!(other.operations[0].state, OperationState::Queued);
    assert_eq!(other.operations[0].batch, op.batch);
    let left: Vec<String> = f
        .store
        .list_messages(f.inbox, None, 10)
        .await
        .expect("list")
        .items
        .into_iter()
        .map(|i| i.subject)
        .collect();
    assert_eq!(left, ["Subject 2"], "removed locally with the expunge");
}
