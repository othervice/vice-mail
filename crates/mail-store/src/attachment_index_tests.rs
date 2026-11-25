//! Stage 20, task C (decision 0060): the attachments index in the store: written with the
//! body, listed newest first with filters and keyset pages, Trash and Junk left out, the plan of
//! the query, and a page over many attachments.

use std::sync::Arc;
use std::time::{Duration, Instant};

use mail_model::{
    Address, AttachmentKind, FetchedSection, MessageHeader, MessageId, MessageStructure,
};
use rusqlite::Connection;

use crate::test_support::{UV, fixture, header, remote};
use crate::{AttachmentQuery, BodyText, BodyTextSource, IndexedAttachment};

const D: i64 = 1_764_000_000;

/// Attachments from `X-Att: name;type` lines of the raw message, like the engine's list.
struct Lines;

impl BodyTextSource for Lines {
    fn complete(&self, raw: &[u8]) -> BodyText {
        let text = String::from_utf8_lossy(raw);
        let attachment_index: Vec<IndexedAttachment> = text
            .lines()
            .filter_map(|l| l.strip_prefix("X-Att: "))
            .map(|l| {
                let (name, mime) = l.split_once(';').unwrap_or((l, "application/octet-stream"));
                IndexedAttachment {
                    name: name.to_owned(),
                    mime_type: mime.to_owned(),
                    size: 100,
                    part: None,
                }
            })
            .collect();
        BodyText {
            body: String::new(),
            attachments: attachment_index.iter().map(|a| a.name.clone()).collect(),
            attachment_index,
            receipt: None,
        }
    }

    fn partial(&self, _: &MessageStructure, _: &[FetchedSection]) -> BodyText {
        BodyText::default()
    }
}

fn from(mut h: MessageHeader, name: &str, email: &str) -> MessageHeader {
    h.from = vec![Address {
        name: Some(name.into()),
        email: email.into(),
    }];
    h
}

fn raw(attachments: &[(&str, &str)]) -> Vec<u8> {
    let mut out = String::from("Subject: x\r\n");
    for (name, mime) in attachments {
        out.push_str(&format!("X-Att: {name};{mime}\r\n"));
    }
    out.push_str("\r\nbody\r\n");
    out.into_bytes()
}

#[tokio::test]
async fn the_view_lists_stored_attachments_with_filters_and_pages() {
    let f = fixture().await;
    f.store.set_body_text_source(Arc::new(Lines));
    f.store
        .sync_mailboxes(
            f.account,
            vec![
                remote("INBOX", &[]),
                remote("Archive", &["\\Archive"]),
                remote("Sent", &["\\Sent"]),
                remote("Trash", &["\\Trash"]),
            ],
        )
        .await
        .expect("mailboxes");
    let trash = f
        .store
        .mailboxes(f.account)
        .await
        .expect("mailboxes")
        .into_iter()
        .find(|m| m.role == mail_model::MailboxRole::Trash)
        .expect("trash")
        .id;
    f.store
        .reset_mailbox(
            trash,
            crate::MailboxBaseline {
                uid_validity: UV,
                uid_next: 1,
                highest_modseq: None,
            },
        )
        .await
        .expect("reset");
    f.store
        .store_headers(
            f.inbox,
            UV,
            vec![
                from(
                    header(1, D + 1, Some("a@x")),
                    "Anna Bank",
                    "anna@bank.example",
                ),
                from(header(2, D + 2, Some("b@x")), "Bob", "bob@friends.example"),
                from(
                    header(3, D + 3, Some("c@x")),
                    "Anna Bank",
                    "anna@bank.example",
                ),
            ],
            None,
        )
        .await
        .expect("inbox");
    f.store
        .store_headers(trash, UV, vec![header(1, D + 4, Some("d@x"))], None)
        .await
        .expect("trash");
    f.store
        .store_bodies(vec![
            (
                MessageId(D + 1),
                raw(&[("Statement 2026.pdf", "application/pdf")]),
            ),
            (
                MessageId(D + 2),
                raw(&[
                    ("Holiday.JPG", "image/jpeg"),
                    ("Budget.xlsx", "application/octet-stream"),
                ]),
            ),
            (
                MessageId(D + 3),
                raw(&[("Contract.pdf", "application/pdf")]),
            ),
            (MessageId(D + 4), raw(&[("Old.pdf", "application/pdf")])),
        ])
        .await
        .expect("bodies");

    let query = |q: AttachmentQuery| {
        let store = f.store.clone();
        async move {
            store
                .list_attachments(q, None, 50)
                .await
                .expect("list")
                .0
                .into_iter()
                .map(|a| a.name)
                .collect::<Vec<_>>()
        }
    };
    let all = AttachmentQuery {
        accounts: vec![f.account],
        ..AttachmentQuery::default()
    };
    // Newest message first, in the pane's order within a message; Trash left out.
    assert_eq!(
        query(all.clone()).await,
        [
            "Contract.pdf",
            "Budget.xlsx",
            "Holiday.JPG",
            "Statement 2026.pdf"
        ]
    );
    let (rows, _) = f
        .store
        .list_attachments(all.clone(), None, 50)
        .await
        .expect("rows");
    assert_eq!(rows[1].index, 1);
    assert_eq!(rows[1].kind, AttachmentKind::Spreadsheet);
    assert_eq!(rows[2].kind, AttachmentKind::Image);
    assert_eq!(rows[0].mailbox, f.inbox);
    assert!(rows.iter().all(|r| r.local), "whole bodies are stored");
    assert_eq!(
        rows[0].from.as_ref().map(|a| a.email.as_str()),
        Some("anna@bank.example")
    );
    assert_eq!(rows[0].date, (D + 3) * 1000);

    let pdf = AttachmentQuery {
        kind: Some(AttachmentKind::Pdf),
        ..all.clone()
    };
    assert_eq!(query(pdf).await, ["Contract.pdf", "Statement 2026.pdf"]);
    let named = AttachmentQuery {
        name: Some("statement".into()),
        ..all.clone()
    };
    assert_eq!(query(named).await, ["Statement 2026.pdf"]);
    let sender = AttachmentQuery {
        sender: Some("bob".into()),
        ..all.clone()
    };
    assert_eq!(query(sender).await, ["Budget.xlsx", "Holiday.JPG"]);
    let by_name = AttachmentQuery {
        sender: Some("anna bank".into()),
        ..all.clone()
    };
    assert_eq!(query(by_name).await, ["Contract.pdf", "Statement 2026.pdf"]);
    let nobody = AttachmentQuery {
        accounts: Vec::new(),
        ..all.clone()
    };
    assert!(query(nobody).await.is_empty());

    // Keyset pages of two.
    let (first, next) = f
        .store
        .list_attachments(all.clone(), None, 2)
        .await
        .expect("page 1");
    assert_eq!(first.len(), 2);
    let (second, last) = f
        .store
        .list_attachments(all.clone(), next, 2)
        .await
        .expect("page 2");
    assert_eq!(
        second.iter().map(|a| a.name.as_str()).collect::<Vec<_>>(),
        ["Holiday.JPG", "Statement 2026.pdf"]
    );
    assert_eq!(last, None);

    // A new body replaces the rows; a deleted message takes them along.
    f.store
        .store_bodies(vec![(MessageId(D + 3), raw(&[]))])
        .await
        .expect("again");
    assert_eq!(
        query(all.clone()).await,
        ["Budget.xlsx", "Holiday.JPG", "Statement 2026.pdf"]
    );
}

fn plan(conn: &Connection, sql: &str) -> Vec<String> {
    let mut stmt = conn
        .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
        .expect("prepare");
    let nulls = vec![rusqlite::types::Null; stmt.parameter_count()];
    stmt.query_map(rusqlite::params_from_iter(nulls), |r| r.get::<_, String>(3))
        .expect("plan")
        .collect::<Result<Vec<_>, _>>()
        .expect("rows")
}

#[tokio::test]
async fn the_view_walks_an_index_and_stays_fast_with_many_attachments() {
    let f = fixture().await;
    f.store.set_body_text_source(Arc::new(Lines));
    // 4,000 messages with 5 attachments each: 20,000 rows.
    let messages = 4_000u32;
    let headers: Vec<MessageHeader> = (1..=messages)
        .map(|uid| {
            let mut h = header(uid, D + i64::from(uid), Some(&format!("{uid}@x")));
            h.from = vec![Address {
                name: Some(format!("Sender {}", uid % 50)),
                email: format!("s{}@example.org", uid % 50),
            }];
            h
        })
        .collect();
    for chunk in headers.chunks(500) {
        f.store
            .store_headers(f.inbox, UV, chunk.to_vec(), None)
            .await
            .expect("headers");
    }
    let bodies: Vec<(MessageId, Vec<u8>)> = (1..=messages)
        .map(|uid| {
            (
                MessageId(D + i64::from(uid)),
                raw(&[
                    ("scan.pdf", "application/pdf"),
                    ("photo.jpg", "image/jpeg"),
                    ("sheet.xlsx", "application/octet-stream"),
                    ("notes.txt", "text/plain"),
                    (&format!("report-{uid}.zip"), "application/zip"),
                ]),
            )
        })
        .collect();
    for chunk in bodies.chunks(400) {
        f.store.store_bodies(chunk.to_vec()).await.expect("bodies");
    }
    let base = AttachmentQuery {
        accounts: vec![f.account],
        ..AttachmentQuery::default()
    };
    let cases = [
        base.clone(),
        AttachmentQuery {
            kind: Some(AttachmentKind::Archive),
            ..base.clone()
        },
        AttachmentQuery {
            name: Some("report-1".into()),
            ..base.clone()
        },
        AttachmentQuery {
            sender: Some("s7@".into()),
            ..base.clone()
        },
    ];
    for case in cases {
        // The first page as the view asks for it; warm, then measured.
        f.store
            .list_attachments(case.clone(), None, 200)
            .await
            .expect("warm");
        let started = Instant::now();
        let (rows, _) = f
            .store
            .list_attachments(case.clone(), None, 200)
            .await
            .expect("page");
        let took = started.elapsed();
        assert!(!rows.is_empty(), "{case:?}");
        // Debug build; a release build is several times faster.
        assert!(took < Duration::from_millis(400), "{case:?}: {took:?}");
    }

    // The plan: the primary key or the kind index backwards, the message by its key.
    let conn = Connection::open(crate::test_support::db_path(&f._dir)).expect("open");
    let plain = crate::attachment_index::list_sql(&base, 1);
    let kind = crate::attachment_index::list_sql(
        &AttachmentQuery {
            kind: Some(AttachmentKind::Pdf),
            ..base.clone()
        },
        1,
    );
    let plain_plan = plan(&conn, &plain).join("\n");
    assert!(
        plain_plan.contains("SCAN a") || plain_plan.contains("SEARCH a USING PRIMARY KEY"),
        "{plain_plan}"
    );
    assert!(
        plain_plan.contains("SEARCH m USING INTEGER PRIMARY KEY"),
        "{plain_plan}"
    );
    assert!(!plain_plan.contains("SCAN m"), "{plain_plan}");
    assert!(
        !plain_plan.contains("USE TEMP B-TREE FOR ORDER BY"),
        "{plain_plan}"
    );
    let kind_plan = plan(&conn, &kind).join("\n");
    assert!(kind_plan.contains("attachment_kind"), "{kind_plan}");
    assert!(
        !kind_plan.contains("USE TEMP B-TREE FOR ORDER BY"),
        "{kind_plan}"
    );
}
