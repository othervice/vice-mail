//! Stage 7 in the store: the full-text index stays in line with `message` through inserts, body
//! changes, deletes and a rebuild; threads match the reference grouping of `mail-model`; thread
//! lists, the unified inbox, groups, hidden accounts, search filters and identities.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use mail_model::search::{RowidRange, TextMatch};
use mail_model::threading::{ThreadConfig, ThreadIndex, ThreadMessage};
use mail_model::{
    AccountId, Address, BodySection, Disposition, FetchedSection, IdentityInput, MailboxId,
    MailboxRole, MessageHeader, MessageId, MessageStructure, MimePart, PartPath, UidSet,
};
use rusqlite::Connection;

use crate::test_support::{Fixture, UV, fixture, header, new_account, remote, seen};
use crate::{
    BodyText, BodyTextSource, IndexStep, MailboxBaseline, SearchFilter, Store, StoreError, fts,
};

const D: i64 = 1_790_000_000;

/// Index text as a test can predict it: the body is everything after the header, the
/// attachments are the values of `X-Attachment:` header lines; a partial body is the text of
/// its body-text sections, its attachments the file names of the structure (like the engine).
pub(crate) struct TestText;

impl BodyTextSource for TestText {
    fn complete(&self, raw: &[u8]) -> BodyText {
        let text = String::from_utf8_lossy(raw);
        let (head, body) = text.split_once("\r\n\r\n").unwrap_or(("", &text));
        let attachments: Vec<String> = head
            .lines()
            .filter_map(|l| l.strip_prefix("X-Attachment: "))
            .map(str::to_owned)
            .collect();
        BodyText {
            body: body.to_owned(),
            // Stage 20: the same names for the attachments view.
            attachment_index: attachments
                .iter()
                .map(|name| crate::IndexedAttachment {
                    name: name.clone(),
                    mime_type: "application/octet-stream".into(),
                    size: 1,
                    part: None,
                })
                .collect(),
            attachments,
            receipt: None,
        }
    }

    fn partial(&self, structure: &MessageStructure, sections: &[FetchedSection]) -> BodyText {
        BodyText {
            attachment_index: Vec::new(),
            body: sections
                .iter()
                .filter(|s| match &s.section {
                    BodySection::Part(path) => {
                        structure.part(path).is_some_and(MimePart::is_body_text)
                    }
                    _ => false,
                })
                .map(|s| String::from_utf8_lossy(&s.data).into_owned())
                .collect::<Vec<_>>()
                .join("\n"),
            attachments: structure
                .parts
                .iter()
                .filter_map(|p| p.file_name.clone())
                .collect(),
            receipt: None,
        }
    }
}

pub(crate) fn raw(body: &str, attachments: &[&str]) -> Vec<u8> {
    let mut out = String::from("Subject: x\r\n");
    for a in attachments {
        out.push_str(&format!("X-Attachment: {a}\r\n"));
    }
    out.push_str("\r\n");
    out.push_str(body);
    out.into_bytes()
}

pub(crate) async fn with_text() -> Fixture {
    let f = fixture().await;
    f.store.set_body_text_source(Arc::new(TestText));
    f
}

fn texted(mut h: MessageHeader, subject: &str, from: &str) -> MessageHeader {
    h.subject = subject.into();
    h.from = vec![Address {
        name: Some(from.into()),
        email: format!("{}@example.org", from.to_lowercase()),
    }];
    h
}

async fn ids_of(store: &Store, mailbox: MailboxId) -> Vec<i64> {
    crate::test_support::all_ids(store, mailbox, 500).await
}

/// Every message id, ascending.
async fn message_ids(store: &Store) -> Vec<i64> {
    store
        .read(|c| {
            Ok(c.prepare("SELECT id FROM message ORDER BY id")?
                .query_map([], |r| r.get(0))?
                .collect::<Result<Vec<i64>, _>>()?)
        })
        .await
        .expect("ids")
}

/// The index terms (term, rowid, column, offset) of `table` in schema `schema`.
fn instances(conn: &Connection, schema: &str, table: &str) -> BTreeSet<(String, i64, String, i64)> {
    let vocab = format!("v_{table}");
    conn.execute_batch(&format!(
        "CREATE VIRTUAL TABLE IF NOT EXISTS temp.{vocab} USING fts5vocab({schema}, {table}, instance)"
    ))
    .expect("vocab");
    let rows = conn
        .prepare(&format!("SELECT term, doc, col, offset FROM temp.{vocab}"))
        .expect("prepare vocab")
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
        .expect("vocab rows")
        .collect::<Result<BTreeSet<_>, _>>()
        .expect("collect");
    conn.execute_batch(&format!("DROP TABLE temp.{vocab}"))
        .expect("drop vocab");
    rows
}

/// Asserts that `message_fts` and (decision 0074) `message_fts_tri` hold exactly one row per
/// message, with the stored headers and the expected body texts (`texts`; messages not in it:
/// headers only). Compares the index terms with indexes built from scratch out of the same data.
pub(crate) async fn assert_index_matches(
    store: &Store,
    texts: &BTreeMap<i64, BodyText>,
    when: &str,
) {
    let texts = texts.clone();
    let when = when.to_owned();
    store
        .write(move |tx| {
            let ids: Vec<i64> = tx
                .prepare("SELECT id FROM message ORDER BY id")?
                .query_map([], |r| r.get(0))?
                .collect::<Result<_, _>>()?;
            let mut rows = Vec::new();
            for id in &ids {
                let (mut row, _) = fts::stored_row(tx, *id)?.expect("row");
                if let Some(text) = texts.get(id) {
                    row.body = text.body.clone();
                    row.attachments = text.attachments.join("\n");
                }
                rows.push((*id, row));
            }
            for (table, tokenize, body) in [
                ("message_fts", "unicode61 remove_diacritics 2", true),
                (
                    "message_fts_tri",
                    "trigram remove_diacritics 1",
                    mail_model::search::SUBSTRING_BODY,
                ),
            ] {
                let fts_ids: Vec<i64> = tx
                    .prepare(&format!("SELECT rowid FROM {table} ORDER BY rowid"))?
                    .query_map([], |r| r.get(0))?
                    .collect::<Result<_, _>>()?;
                assert_eq!(fts_ids, ids, "{when}: rows of {table} are the messages");
                tx.execute_batch(&format!(
                    "CREATE VIRTUAL TABLE temp.expected_fts USING fts5(
                       subject, \"from\", \"to\", cc, body, attachments, content = '',
                       tokenize = '{tokenize}')"
                ))?;
                for (id, row) in &rows {
                    tx.execute(
                        "INSERT INTO temp.expected_fts (rowid, subject, \"from\", \"to\", cc, body, attachments)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                        rusqlite::params![
                            id,
                            row.subject,
                            row.from,
                            row.to,
                            row.cc,
                            if body { row.body.as_str() } else { "" },
                            row.attachments
                        ],
                    )?;
                }
                let live = instances(tx, "main", table);
                let expected = instances(tx, "temp", "expected_fts");
                tx.execute_batch("DROP TABLE temp.expected_fts")?;
                let missing: Vec<_> = expected.difference(&live).take(5).collect();
                let extra: Vec<_> = live.difference(&expected).take(5).collect();
                assert!(
                    missing.is_empty() && extra.is_empty(),
                    "{when}: {table} differs; missing {missing:?}, extra {extra:?}"
                );
            }
            Ok(())
        })
        .await
        .expect("compare index");
}

pub(crate) async fn run_index_job(store: &Store) -> usize {
    let mut steps = 0;
    loop {
        match store.search_index_step().await.expect("index step") {
            IndexStep::Progress(_) => steps += 1,
            IndexStep::Finished | IndexStep::Idle => return steps,
        }
    }
}

fn find(f: &str) -> SearchFilter {
    SearchFilter {
        text: TextMatch::words(f),
        ..SearchFilter::default()
    }
}

pub(crate) async fn search_ids(store: &Store, filter: SearchFilter) -> Vec<i64> {
    let mut out = Vec::new();
    let mut cursor = None;
    loop {
        let page = store
            .search(filter.clone(), cursor, 3)
            .await
            .expect("search");
        out.extend(page.hits.iter().map(|h| h.item.id.0));
        match page.next_cursor {
            Some(c) => cursor = Some(c),
            None => return out,
        }
    }
}

fn structure_with_attachment() -> MessageStructure {
    let part = |path: Vec<u32>, mime: &str, name: Option<&str>| MimePart {
        path: PartPath::new(path),
        depth: 1,
        mime_type: mime.into(),
        charset: None,
        file_name: name.map(str::to_owned),
        disposition: name.map(|_| Disposition::Attachment),
        content_id: None,
        encoding: "7bit".into(),
        size: 100,
    };
    MessageStructure {
        size: 5_000_000,
        parts: vec![
            MimePart {
                path: None,
                depth: 0,
                mime_type: "multipart/mixed".into(),
                charset: None,
                file_name: None,
                disposition: None,
                content_id: None,
                encoding: "7bit".into(),
                size: 0,
            },
            part(vec![1], "text/plain", None),
            part(vec![2], "application/pdf", Some("Kontoauszug_2026.pdf")),
        ],
    }
}

fn section(path: u32, text: &str) -> FetchedSection {
    FetchedSection {
        section: BodySection::Part(PartPath::new(vec![path]).expect("path")),
        data: text.as_bytes().to_vec(),
    }
}

// ---------------------------------------------------------------------------------------------
// Index
// ---------------------------------------------------------------------------------------------

#[tokio::test]
async fn the_index_follows_inserts_changes_deletes_and_a_rebuild() {
    let f = with_text().await;
    let store = &f.store;
    let mut texts: BTreeMap<i64, BodyText> = BTreeMap::new();

    // Insert: two windows, out of date order, a merge (same Message-ID in the archive), a
    // duplicate Message-ID in one mailbox and colliding dates.
    let mut window: Vec<MessageHeader> = (1..=30)
        .rev()
        .map(|uid| {
            texted(
                header(uid, D + i64::from(uid % 10) * 60, Some(&format!("{uid}@m"))),
                &format!("Rechnung Nummer {uid} für Grüße"),
                if uid % 2 == 0 { "Müller" } else { "Anna" },
            )
        })
        .collect();
    window[3].message_id = Some("1@m".into());
    store
        .store_headers(f.inbox, UV, window[..15].to_vec(), None)
        .await
        .expect("window 1");
    store
        .store_headers(f.inbox, UV, window[15..].to_vec(), None)
        .await
        .expect("window 2");
    store
        .store_headers(
            f.archive,
            UV,
            vec![header(1, D, Some("5@m")), header(2, D + 7, None)],
            None,
        )
        .await
        .expect("archive");
    assert_index_matches(store, &texts, "after inserts").await;

    // Change: complete bodies (in one pack), a partial body, its attachment fetched later
    // (no text: the indexed text stays), then a complete body replacing a partial one.
    let ids = ids_of(store, f.inbox).await;
    let bodies: Vec<(MessageId, Vec<u8>)> = ids[..5]
        .iter()
        .enumerate()
        .map(|(i, id)| {
            (
                MessageId(*id),
                raw(&format!("Kontostand {i} Überweisung"), &["Beleg.pdf"]),
            )
        })
        .collect();
    for (id, body) in &bodies {
        texts.insert(id.0, TestText.complete(body));
    }
    store.store_bodies(bodies).await.expect("bodies");
    assert_index_matches(store, &texts, "after complete bodies").await;

    let large = MessageId(ids[6]);
    let structure = structure_with_attachment();
    store
        .store_body_sections(
            large,
            structure.clone(),
            vec![section(1, "Großer Anhang folgt")],
        )
        .await
        .expect("sections");
    texts.insert(
        large.0,
        TestText.partial(&structure, &[section(1, "Großer Anhang folgt")]),
    );
    assert_index_matches(store, &texts, "after a partial body").await;
    store
        .store_body_sections(large, structure, vec![section(2, "%PDF-1.7 binary")])
        .await
        .expect("attachment");
    assert_index_matches(store, &texts, "after an attachment of a partial body").await;
    let whole = raw("Jetzt vollständig", &[]);
    texts.insert(large.0, TestText.complete(&whole));
    store.store_body(large, whole).await.expect("whole");
    assert_index_matches(
        store,
        &texts,
        "after a complete body replaced a partial one",
    )
    .await;
    // The text is found, the headers too, with the date filter as a rowid range.
    assert_eq!(search_ids(store, find("vollständig")).await, [large.0]);
    assert_eq!(search_ids(store, find("vollstandig")).await, [large.0]);
    assert_eq!(
        search_ids(store, find("{attachments} : \"beleg\""))
            .await
            .len(),
        5
    );

    // Delete: removed locations, orphans, a body deleted from the cache (text stays).
    store
        .remove_uids(f.inbox, UV, UidSet::range(1, 10))
        .await
        .expect("remove");
    store
        .delete_orphan_messages(f.account)
        .await
        .expect("orphans");
    let left = message_ids(store).await;
    texts.retain(|id, _| left.contains(id));
    assert_index_matches(store, &texts, "after deletes").await;
    store
        .delete_body(MessageId(ids[0]))
        .await
        .expect("delete body");
    assert_index_matches(store, &texts, "after a body left the cache").await;

    // Rebuild: the new table is filled in batches while messages come and go and bodies
    // arrive; searches keep working on the old table until the swap.
    assert!(store.rebuild_search_index().await.expect("rebuild"));
    assert!(!store.rebuild_search_index().await.expect("second rebuild"));
    assert_eq!(
        store.search_index_status().await.expect("status").state,
        mail_model::SearchIndexState::Rebuilding
    );
    let before = search_ids(store, find("rechnung")).await;
    store
        .store_headers(
            f.inbox,
            UV,
            vec![texted(
                header(40, D + 5_000, Some("40@m")),
                "Rechnung neu",
                "Neu",
            )],
            None,
        )
        .await
        .expect("new during rebuild");
    let ids_now = ids_of(store, f.inbox).await;
    let newest = MessageId(ids_now[0]);
    let late = raw("spät angekommen", &[]);
    texts.insert(newest.0, TestText.complete(&late));
    store
        .store_body(newest, late)
        .await
        .expect("body during rebuild");
    store
        .remove_uids(f.inbox, UV, UidSet::range(11, 12))
        .await
        .expect("remove during rebuild");
    store
        .delete_orphan_messages(f.account)
        .await
        .expect("orphans during rebuild");
    let left = message_ids(store).await;
    texts.retain(|id, _| left.contains(id));
    let during = search_ids(store, find("rechnung")).await;
    assert!(
        during.len() >= before.len().saturating_sub(2),
        "search works during the rebuild"
    );
    assert!(run_index_job(store).await >= 1);
    assert_eq!(
        store.search_index_status().await.expect("status").state,
        mail_model::SearchIndexState::Ready
    );
    assert_index_matches(store, &texts, "after the rebuild").await;
    let tables: Vec<String> = store
        .read(|c| {
            Ok(c.prepare("SELECT name FROM sqlite_schema WHERE name LIKE 'message_fts%new%' OR name LIKE 'message_fts%old%'")?
                .query_map([], |r| r.get(0))?
                .collect::<Result<_, _>>()?)
        })
        .await
        .expect("tables");
    assert!(tables.is_empty(), "no left-over index tables: {tables:?}");
    let triggers: Vec<String> = store
        .read(|c| {
            Ok(c.prepare("SELECT name FROM sqlite_schema WHERE type = 'trigger' AND name LIKE 'message_fts%' ORDER BY name")?
                .query_map([], |r| r.get(0))?
                .collect::<Result<_, _>>()?)
        })
        .await
        .expect("triggers");
    assert_eq!(triggers, ["message_fts_delete", "message_fts_tri_delete"]);

    // Deleting the account empties the index.
    assert!(store.delete_account(f.account).await.expect("delete"));
    assert_index_matches(store, &BTreeMap::new(), "after the account was deleted").await;
}

#[tokio::test]
async fn existing_messages_are_indexed_and_threaded_after_the_migration() {
    use crate::migrations::MIGRATIONS;
    use crate::store::Schema;
    let dir = tempfile::TempDir::new().expect("temp dir");
    let path = crate::test_support::db_path(&dir);
    let stage4 = Schema::from_vec(MIGRATIONS[..4].to_vec());
    let store = Store::open_with(&path, crate::StoreOptions::default(), &stage4).expect("stage 4");
    let account = store
        .insert_account(new_account(1, "alice@example.org"))
        .await
        .expect("account")
        .id;
    store
        .sync_mailboxes(account, vec![remote("INBOX", &[])])
        .await
        .expect("mailboxes");
    // The folder list of today's schema needs stage 16's tables; the sync state does not.
    let inbox = store.mailbox_sync_states(account).await.expect("mailboxes")[0].mailbox_id;
    store
        .reset_mailbox(
            inbox,
            MailboxBaseline {
                uid_validity: UV,
                uid_next: 1,
                highest_modseq: None,
            },
        )
        .await
        .expect("reset");
    let mut headers: Vec<MessageHeader> = (1..=1_200)
        .map(|uid| header(uid, D + i64::from(uid) * 60, Some(&format!("{uid}@old"))))
        .collect();
    // Replies: every third message answers the one before.
    for (i, h) in headers.iter_mut().enumerate() {
        if i % 3 == 2 {
            h.in_reply_to = Some(format!("{}@old", i));
            h.references = vec![format!("{}@old", i)];
        }
    }
    let h2 = headers.clone();
    store
        .write(move |tx| crate::test_support::stage2_store_headers(tx, inbox, &h2))
        .await
        .expect("old headers");
    // stage2_store_headers does not keep references; write them as the old code did.
    let refs: Vec<(i64, Option<String>, Option<String>)> = headers
        .iter()
        .map(|h| {
            (
                h.sort_date(),
                h.in_reply_to.clone(),
                h.references.first().cloned(),
            )
        })
        .collect();
    store
        .write(move |tx| {
            for (id, irt, refs) in &refs {
                tx.execute(
                    "UPDATE message SET in_reply_to = ?2, references_hdr = ?3 WHERE id = ?1",
                    rusqlite::params![id, irt, refs],
                )?;
            }
            Ok(())
        })
        .await
        .expect("references");
    drop(store);

    let store = Store::open(&path, crate::StoreOptions::default()).expect("migrate");
    store.set_body_text_source(Arc::new(TestText));
    let status = store.search_index_status().await.expect("status");
    assert_eq!(status.state, mail_model::SearchIndexState::Building);
    assert_eq!(status.percent, 0);
    // A message stored meanwhile is indexed and threaded at once.
    store
        .store_headers(
            inbox,
            UV,
            vec![texted(
                header(2_000, D + 999_999, Some("new@m")),
                "Ganz neu",
                "Neu",
            )],
            None,
        )
        .await
        .expect("new");
    assert_eq!(run_index_job(&store).await, 3);
    assert_index_matches(&store, &BTreeMap::new(), "after the initial fill").await;
    assert_eq!(
        store.search_index_status().await.expect("status").state,
        mail_model::SearchIndexState::Ready
    );
    let mut threaded = 0;
    loop {
        let n = store.thread_backfill_step().await.expect("backfill");
        if n == 0 {
            break;
        }
        threaded += n;
    }
    assert_eq!(threaded, 1_200);
    let page = store.list_threads(inbox, None, 500).await.expect("threads");
    // 1,201 messages: 400 pairs (original + reply) and 401 single messages.
    let all = all_threads(&store, inbox).await;
    assert_eq!(all.len(), 801, "{:?}", page.items.len());
    assert_eq!(all.iter().map(|t| t.message_count).sum::<u32>(), 1_201);
}

async fn all_threads(store: &Store, mailbox: MailboxId) -> Vec<mail_model::ThreadListItem> {
    let mut out = Vec::new();
    let mut cursor = None;
    loop {
        let page = store
            .list_threads(mailbox, cursor, 7)
            .await
            .expect("thread page");
        out.extend(page.items);
        match page.next_cursor {
            Some(c) => cursor = Some(c),
            None => return out,
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Threads
// ---------------------------------------------------------------------------------------------

fn reply(mut h: MessageHeader, to: &[&str]) -> MessageHeader {
    h.in_reply_to = to.last().map(|s| (*s).to_owned());
    h.references = to.iter().map(|s| (*s).to_owned()).collect();
    h
}

#[tokio::test]
async fn threads_match_the_reference_grouping_and_follow_deletes() {
    let f = with_text().await;
    let store = &f.store;
    // A conversation over Inbox and Sent, a mailing list thread, an unrelated mail with the
    // same subject and a reply that arrives before its original.
    let alice = |h: MessageHeader| texted(h, "Projekt Apfel Planung", "Bob");
    let inbox = vec![
        // Reply first (arrives before its original, which comes in the second window).
        reply(alice(header(1, D + 600, Some("b@x"))), &["a@x"]),
        alice(header(2, D + 900, Some("c@x"))),
        reply(alice(header(3, D + 1_200, Some("d@x"))), &["a@x", "b@x"]),
        // Same subject, no references, not a reply: a new conversation.
        texted(
            header(4, D + 3_000, Some("e@x")),
            "Projekt Apfel Planung",
            "Carol",
        ),
        // List mail.
        {
            let mut h = texted(
                header(5, D + 4_000, Some("l1@list")),
                "[dev] Release",
                "Dan",
            );
            h.list_id = Some("dev.example.org".into());
            h
        },
        {
            let mut h = reply(
                texted(
                    header(6, D + 4_100, Some("l2@list")),
                    "Re: [dev] Release",
                    "Eve",
                ),
                &["l1@list"],
            );
            h.list_id = Some("dev.example.org".into());
            h
        },
    ];
    let sent = vec![
        // The original of 1 and 3, sent by us.
        texted(header(1, D, Some("a@x")), "Projekt Apfel Planung", "Alice"),
        // A reply to 2 without references (Outlook): joins by subject.
        texted(
            header(2, D + 1_000, Some("f@x")),
            "AW: Projekt Apfel Planung",
            "Alice",
        ),
    ];
    store
        .store_headers(f.inbox, UV, inbox.clone(), None)
        .await
        .expect("inbox");
    store
        .store_headers(f.sent, UV, sent.clone(), None)
        .await
        .expect("sent");

    // The reference: mail-model's in-memory threads, same messages in the same order.
    let mut index = ThreadIndex::new(ThreadConfig::new(["alice@example.org"]));
    let mut by_msgid = BTreeMap::new();
    for (mailbox, headers) in [(f.inbox, &inbox), (f.sent, &sent)] {
        let ids = ids_of(store, mailbox).await;
        let mut sorted = headers.clone();
        sorted.sort_by_key(|h| (h.sort_date(), h.uid));
        let mut ids_asc = ids.clone();
        ids_asc.sort_unstable();
        for (h, id) in sorted.iter().zip(ids_asc) {
            index.insert(ThreadMessage::from_header(MessageId(id), h));
            by_msgid.insert(h.message_id.clone().unwrap_or_default(), id);
        }
    }
    let stored = partition(store).await;
    assert_eq!(stored, index.partition());
    // Expected: {a, b, d} by references, {c, f} by subject, {e} alone, the list thread.
    let group_of = |m: &str| {
        stored
            .iter()
            .position(|g| g.contains(&MessageId(by_msgid[m])))
            .expect("grouped")
    };
    assert_eq!(group_of("a@x"), group_of("b@x"));
    assert_eq!(group_of("a@x"), group_of("d@x"));
    assert_eq!(
        group_of("c@x"),
        group_of("f@x"),
        "reply without references by subject"
    );
    assert_ne!(
        group_of("e@x"),
        group_of("c@x"),
        "same subject, no relation"
    );
    assert_eq!(group_of("l1@list"), group_of("l2@list"));
    assert_counts_match(store).await;

    // The thread list of the inbox: one row per thread, its newest inbox message, counts over
    // all mailboxes, unread over all.
    let rows = all_threads(store, f.inbox).await;
    let row_ids: Vec<i64> = rows.iter().map(|r| r.message.id.0).collect();
    assert_eq!(rows.len(), 4, "{row_ids:?}");
    let abd = rows
        .iter()
        .find(|r| r.message.id.0 == by_msgid["d@x"])
        .expect("thread a-b-d by its newest inbox message");
    assert_eq!(abd.message_count, 3);
    // Every fixture message is unread, the one in Sent too.
    assert_eq!(abd.unread_count, 3);
    let cf = rows
        .iter()
        .find(|r| r.message.id.0 == by_msgid["c@x"])
        .expect("thread c-f");
    assert_eq!(cf.message_count, 2);
    assert_eq!(
        cf.last_activity,
        by_msgid["f@x"] * 1000,
        "the reply in Sent is the activity"
    );

    // A thread opened from a message: all mailboxes, oldest first.
    let view = store
        .thread(MessageId(by_msgid["b@x"]), Some(f.inbox))
        .await
        .expect("thread");
    let ids: Vec<i64> = view.messages.iter().map(|m| m.message.id.0).collect();
    assert_eq!(ids, [by_msgid["a@x"], by_msgid["b@x"], by_msgid["d@x"]]);
    assert_eq!(view.messages[0].message.mailbox_id, f.sent);
    assert_eq!(view.messages[1].message.mailbox_id, f.inbox);
    assert_eq!(view.total, 3);

    // Deletes: a thread loses a message (newest recomputed), an empty thread goes with its ids.
    store
        .remove_uids(f.inbox, UV, UidSet::from_uids([3, 4]))
        .await
        .expect("remove");
    store
        .delete_orphan_messages(f.account)
        .await
        .expect("orphans");
    assert_counts_match(store).await;
    let threads: i64 = crate::test_support::count(store, "SELECT count(*) FROM thread").await;
    let groups = partition(store).await.len();
    assert_eq!(usize::try_from(threads).expect("count"), groups);
    let refs_without_thread: i64 = crate::test_support::count(
        store,
        "SELECT count(*) FROM thread_ref r WHERE NOT EXISTS (SELECT 1 FROM thread t WHERE t.id = r.thread_id)",
    )
    .await;
    assert_eq!(refs_without_thread, 0);
    assert!(store.delete_account(f.account).await.expect("delete"));
    assert_eq!(
        crate::test_support::count(store, "SELECT count(*) FROM thread").await
            + crate::test_support::count(store, "SELECT count(*) FROM thread_ref").await,
        0
    );
}

/// Threads as sets of messages, like `ThreadIndex::partition`.
async fn partition(store: &Store) -> Vec<Vec<MessageId>> {
    store
        .read(|c| {
            let rows: Vec<(i64, i64)> = c
                .prepare("SELECT thread_id, id FROM message ORDER BY id")?
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<Result<_, _>>()?;
            let mut groups: BTreeMap<i64, Vec<MessageId>> = BTreeMap::new();
            for (thread, id) in rows {
                groups.entry(thread).or_default().push(MessageId(id));
            }
            let mut out: Vec<Vec<MessageId>> = groups.into_values().collect();
            out.sort();
            Ok(out)
        })
        .await
        .expect("partition")
}

/// `thread.message_count` and `last_message_id` agree with the messages.
async fn assert_counts_match(store: &Store) {
    let wrong = crate::test_support::count(
        store,
        "SELECT count(*) FROM thread t
         WHERE t.message_count <> (SELECT count(*) FROM message m WHERE m.thread_id = t.id)
            OR t.last_message_id <> (SELECT max(id) FROM message m WHERE m.thread_id = t.id)",
    )
    .await;
    assert_eq!(wrong, 0, "thread counters out of date");
}

// ---------------------------------------------------------------------------------------------
// Unified inbox, groups, hidden accounts
// ---------------------------------------------------------------------------------------------

/// A second and third account with an inbox each, synced to [`UV`].
async fn more_accounts(f: &Fixture) -> Vec<(AccountId, MailboxId)> {
    let mut out = vec![(f.account, f.inbox)];
    for (n, email) in [(2, "bob@example.org"), (3, "carol@example.org")] {
        let account = f
            .store
            .insert_account(new_account(n, email))
            .await
            .expect("account")
            .id;
        f.store
            .sync_mailboxes(
                account,
                vec![remote("INBOX", &[]), remote("Archiv", &["\\Archive"])],
            )
            .await
            .expect("mailboxes");
        let inbox = f
            .store
            .mailboxes(account)
            .await
            .expect("mailboxes")
            .into_iter()
            .find(|m| m.role == MailboxRole::Inbox)
            .expect("inbox")
            .id;
        f.store
            .reset_mailbox(
                inbox,
                MailboxBaseline {
                    uid_validity: UV,
                    uid_next: 1,
                    highest_modseq: None,
                },
            )
            .await
            .expect("reset");
        out.push((account, inbox));
    }
    out
}

#[tokio::test]
async fn unified_inbox_assigns_every_mail_to_its_account_and_leaves_hidden_ones_out() {
    let f = with_text().await;
    let accounts = more_accounts(&f).await;
    // Interleaved dates over three inboxes, plus mail in the first account's archive.
    for (k, (_, inbox)) in accounts.iter().enumerate() {
        let k = i64::try_from(k).expect("small");
        let headers: Vec<_> = (1..=40)
            .map(|uid| {
                header(
                    uid,
                    D + i64::from(uid) * 30 + k * 7,
                    Some(&format!("{k}-{uid}@u")),
                )
            })
            .collect();
        f.store
            .store_headers(*inbox, UV, headers, None)
            .await
            .expect("headers");
    }
    f.store
        .store_headers(
            f.archive,
            UV,
            vec![header(1, D + 99_999, Some("arch@u"))],
            None,
        )
        .await
        .expect("archive");

    let account_of_mailbox = |m: MailboxId| {
        accounts
            .iter()
            .find(|(_, inbox)| *inbox == m)
            .map(|(a, _)| *a)
    };
    let unified = |group: Option<mail_model::AccountGroupId>| {
        let store = f.store.clone();
        async move {
            let mut items = Vec::new();
            let mut cursor = None;
            loop {
                let page = store
                    .list_unified(group.clone(), cursor, 11)
                    .await
                    .expect("unified page");
                items.extend(page.items);
                match page.next_cursor {
                    Some(c) => cursor = Some(c),
                    None => return items,
                }
            }
        }
    };
    let all = unified(None).await;
    assert_eq!(all.len(), 120);
    assert!(
        all.windows(2).all(|w| w[0].id > w[1].id),
        "newest first, no repeats"
    );
    for item in &all {
        assert_eq!(
            account_of_mailbox(item.mailbox_id),
            Some(item.account_id),
            "item {} in mailbox {} belongs to its account",
            item.id,
            item.mailbox_id
        );
    }
    // Each account's part is exactly its inbox list.
    for (account, inbox) in &accounts {
        let own: Vec<i64> = all
            .iter()
            .filter(|i| i.account_id == *account)
            .map(|i| i.id.0)
            .collect();
        assert_eq!(own, ids_of(&f.store, *inbox).await);
    }

    // Hidden: gone from the unified inbox and from "all accounts".
    let (bob, _) = accounts[1];
    assert!(f.store.set_account_hidden(bob, true).await.expect("hide"));
    assert!(!f.store.set_account_hidden(bob, true).await.expect("again"));
    let without = unified(None).await;
    assert_eq!(without.len(), 80);
    assert!(without.iter().all(|i| i.account_id != bob));
    assert_eq!(
        f.store.visible_accounts(None).await.expect("visible"),
        [accounts[0].0, accounts[2].0]
    );
    let threads = f
        .store
        .list_unified_threads(None, None, 200)
        .await
        .expect("unified threads");
    assert_eq!(threads.items.len(), 80);
    assert!(threads.items.iter().all(|t| t.message.account_id != bob));

    // Groups: members in account order, hidden members left out.
    let group = f
        .store
        .create_account_group("  Arbeit ".into(), vec![accounts[2].0, bob])
        .await
        .expect("group");
    assert_eq!(group.name, "Arbeit");
    assert_eq!(group.account_ids, [bob, accounts[2].0]);
    let in_group = unified(Some(group.id.clone())).await;
    assert_eq!(in_group.len(), 40);
    assert!(in_group.iter().all(|i| i.account_id == accounts[2].0));
    assert!(f.store.set_account_hidden(bob, false).await.expect("show"));
    assert_eq!(unified(Some(group.id.clone())).await.len(), 80);
    let renamed = f
        .store
        .update_account_group(group.id.clone(), "Büro".into(), vec![accounts[0].0])
        .await
        .expect("update");
    assert_eq!(
        (renamed.name.as_str(), renamed.account_ids.as_slice()),
        ("Büro", &[accounts[0].0][..])
    );
    let second = f
        .store
        .create_account_group("Privat".into(), vec![])
        .await
        .expect("second");
    assert!(second.position > renamed.position);
    assert!(matches!(
        f.store.create_account_group(" ".into(), vec![]).await,
        Err(StoreError::InvalidArgument(_))
    ));
    assert!(matches!(
        f.store
            .create_account_group("x".into(), vec![crate::test_support::account_id(99)])
            .await,
        Err(StoreError::NotFound(_))
    ));
    // Deleting an account takes it out of groups and the hidden list.
    f.store
        .set_account_hidden(accounts[0].0, true)
        .await
        .expect("hide");
    assert!(f.store.delete_account(accounts[0].0).await.expect("delete"));
    let groups = f.store.account_groups().await.expect("groups");
    assert!(groups.iter().all(|g| g.account_ids.is_empty()));
    assert!(f.store.hidden_accounts().await.expect("hidden").is_empty());
    assert!(
        f.store
            .delete_account_group(second.id.clone())
            .await
            .expect("delete group")
    );
    assert!(
        !f.store
            .delete_account_group(second.id)
            .await
            .expect("again")
    );
}

// ---------------------------------------------------------------------------------------------
// Search filters
// ---------------------------------------------------------------------------------------------

#[tokio::test]
async fn search_filters_and_paging() {
    let f = with_text().await;
    let store = &f.store;
    let headers: Vec<_> = (1..=12)
        .map(|uid| {
            let mut h = texted(
                header(uid, D + i64::from(uid) * 86_400, Some(&format!("{uid}@s"))),
                if uid % 3 == 0 {
                    "Rechnung März"
                } else {
                    "Termin"
                },
                if uid % 2 == 0 { "Müller" } else { "Anna" },
            );
            h.has_attachments = uid % 4 == 0;
            if uid > 6 {
                h = seen(h);
            }
            h.flags.flagged = uid == 5;
            h
        })
        .collect();
    store
        .store_headers(f.inbox, UV, headers, None)
        .await
        .expect("inbox");
    store
        .store_headers(
            f.archive,
            UV,
            vec![seen(texted(
                header(1, D + 3 * 86_400, Some("3@s")),
                "Rechnung März",
                "Anna",
            ))],
            None,
        )
        .await
        .expect("archive copy of 3");
    let id = |uid: i64| D + uid * 86_400;

    let rechnung = || find("\"rechnung\"");
    assert_eq!(
        search_ids(store, rechnung()).await,
        [id(12), id(9), id(6), id(3)]
    );
    // Diacritics folded by the tokenizer, column filter.
    assert_eq!(
        search_ids(store, find("{from} : \"muller\" AND \"marz\"")).await,
        [id(12), id(6)]
    );
    // Dates as a rowid range.
    let ranged = SearchFilter {
        dates: RowidRange {
            from: Some(id(4)),
            until: Some(id(10)),
        },
        ..rechnung()
    };
    assert_eq!(search_ids(store, ranged).await, [id(9), id(6)]);
    // Flags and attachments.
    let unread = SearchFilter {
        unread: Some(true),
        ..rechnung()
    };
    assert_eq!(search_ids(store, unread).await, [id(6), id(3)]);
    let read = SearchFilter {
        unread: Some(false),
        ..rechnung()
    };
    assert_eq!(search_ids(store, read).await, [id(12), id(9)]);
    let attach = SearchFilter {
        has_attachment: Some(true),
        ..rechnung()
    };
    assert_eq!(search_ids(store, attach).await, [id(12)]);
    // Mailboxes: in:, -in:, and unread relative to the mailbox of `in:`.
    let in_archive = SearchFilter {
        mailbox_sets: vec![vec![f.archive]],
        ..rechnung()
    };
    let hits = store
        .search(in_archive.clone(), None, 10)
        .await
        .expect("archive");
    assert_eq!(hits.hits.len(), 1);
    assert_eq!(hits.hits[0].item.mailbox_id, f.archive);
    assert_eq!(hits.hits[0].mailbox_ids, [f.inbox, f.archive]);
    let unread_in_archive = SearchFilter {
        unread: Some(true),
        ..in_archive
    };
    assert!(search_ids(store, unread_in_archive).await.is_empty());
    let not_archive = SearchFilter {
        excluded_mailboxes: vec![f.archive],
        ..rechnung()
    };
    assert_eq!(search_ids(store, not_archive).await, [id(12), id(9), id(6)]);
    // No text: driven by the mailbox (unread uses the location), or by the message key.
    let inbox_unread = SearchFilter {
        mailbox_sets: vec![vec![f.inbox, f.archive]],
        unread: Some(true),
        ..SearchFilter::default()
    };
    assert_eq!(
        search_ids(store, inbox_unread).await,
        (1..=6).rev().map(id).collect::<Vec<_>>()
    );
    let flagged = SearchFilter {
        flagged: Some(true),
        ..SearchFilter::default()
    };
    assert_eq!(search_ids(store, flagged).await, [id(5)]);
    // Only exclusions.
    let not_termin = SearchFilter {
        text: TextMatch::excluding_words("\"termin\""),
        ..SearchFilter::default()
    };
    assert_eq!(
        search_ids(store, not_termin).await,
        [id(12), id(9), id(6), id(3)]
    );
    // Accounts.
    let other = SearchFilter {
        accounts: Some(vec![crate::test_support::account_id(9)]),
        ..rechnung()
    };
    assert!(search_ids(store, other).await.is_empty());
    let mine = SearchFilter {
        accounts: Some(vec![f.account]),
        ..rechnung()
    };
    assert_eq!(search_ids(store, mine).await.len(), 4);
    // A message in two searched mailboxes counts once.
    let both = SearchFilter {
        mailbox_sets: vec![vec![f.inbox, f.archive]],
        ..SearchFilter::default()
    };
    let found = search_ids(store, both).await;
    assert_eq!(found.len(), 12);
    // Candidates from a server search, without a complete local body.
    store
        .store_body(MessageId(id(9)), raw("x", &[]))
        .await
        .expect("body");
    let only = SearchFilter {
        only: Some(vec![MessageId(id(9)), MessageId(id(6)), MessageId(id(2))]),
        without_complete_body: true,
        has_attachment: Some(false),
        ..SearchFilter::default()
    };
    assert_eq!(search_ids(store, only).await, [id(6), id(2)]);
    assert!(matches!(
        store.search(rechnung(), None, 0).await,
        Err(StoreError::InvalidArgument(_))
    ));
}

// ---------------------------------------------------------------------------------------------
// Identities
// ---------------------------------------------------------------------------------------------

#[tokio::test]
async fn identities_have_one_default_and_unique_addresses() {
    let f = fixture().await;
    let store = &f.store;
    let first = store.identities(f.account).await.expect("identities");
    assert_eq!(first.len(), 1);
    assert!(first[0].is_default);
    let input = |email: &str| IdentityInput {
        email: email.into(),
        name: "Alice Work".into(),
        reply_to: Some(" ".into()),
    };
    let work = store
        .add_identity(f.account, input(" alice@work.example "))
        .await
        .expect("add");
    assert_eq!(work.email, "alice@work.example");
    assert_eq!(work.reply_to, None);
    assert!(!work.is_default);
    assert!(matches!(
        store
            .add_identity(f.account, input("ALICE@work.example"))
            .await,
        Err(StoreError::InvalidArgument(_))
    ));
    assert!(matches!(
        store.add_identity(f.account, input("not an address")).await,
        Err(StoreError::InvalidArgument(_))
    ));
    let all = store.set_default_identity(work.id).await.expect("default");
    assert_eq!(all[0].id, work.id);
    assert_eq!(all.iter().filter(|i| i.is_default).count(), 1);
    assert!(matches!(
        store.delete_identity(work.id).await,
        Err(StoreError::InvalidArgument(_))
    ));
    assert_eq!(
        store.delete_identity(first[0].id).await.expect("delete"),
        f.account
    );
    let changed = store
        .update_identity(
            work.id,
            IdentityInput {
                email: "a@work.example".into(),
                name: "A".into(),
                reply_to: Some("team@work.example".into()),
            },
        )
        .await
        .expect("update");
    assert_eq!(changed.reply_to.as_deref(), Some("team@work.example"));
    let uuids = crate::test_support::count(
        store,
        "SELECT count(*) FROM identity WHERE uuid IS NULL OR length(uuid) <> 36",
    )
    .await;
    assert_eq!(uuids, 0, "every identity has a UUID");
}
