//! Stage 17, block B in the store (decision 0050): Reply-To with new messages, the sources of
//! the directory job and its backfill, kinds and the people-only filter, notes and pictures
//! with their sync records, a contact's mail, search speed at 10,000 contacts, query plans.

use std::sync::Arc;
use std::time::Instant;

use mail_model::{
    Address, CategoryReason, ContactNameRecord, MessageCategory, MessageHeader, MessageId,
};
use rusqlite::Connection;

use crate::contacts_view::{self, SenderLocation};
use crate::test_support::{UV, fixture, header};
use crate::{Classified, Classifier, LocalFacts, SenderKeys, Store, StoredContact};

const D: i64 = 1_764_000_000;

/// Keys from the From address; Reply-To `help@<domain>` for every message with a header block.
struct WithReplyTo;

impl Classifier for WithReplyTo {
    fn sender_keys(&self, header: &MessageHeader) -> SenderKeys {
        let email = header.from.first().map(|a| a.email.to_lowercase());
        SenderKeys {
            domain: email
                .as_deref()
                .and_then(|e| e.split_once('@'))
                .map(|(_, d)| d.to_owned()),
            address: email,
            list_id: None,
        }
    }

    fn classify(&self, header: &MessageHeader, _local: &LocalFacts<'_>) -> Classified {
        Classified {
            category: MessageCategory::Person,
            reason: CategoryReason::NoBulkSignals,
            one_click: None,
            reply_to: if header.category_headers.is_empty() {
                Vec::new()
            } else {
                vec![Address {
                    name: Some("Help".into()),
                    email: "help@example.org".into(),
                }]
            },
        }
    }
}

fn from(mut h: MessageHeader, email: &str) -> MessageHeader {
    h.from[0].email = email.into();
    h
}

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

async fn clear_dirty(store: &Store) {
    store
        .write(|tx| Ok(tx.execute("DELETE FROM sync_dirty", [])?))
        .await
        .expect("clear");
}

fn contact(key: &str, rank: f64, name: &str, kind: i64) -> StoredContact {
    let (local, domain) = key.split_once('@').expect("address");
    StoredContact {
        key: key.into(),
        local_part: local.into(),
        domain: domain.into(),
        group: None,
        sent: u32::from(kind == 0),
        received: 1,
        last_sent: None,
        last_received: Some(D),
        rank,
        names: vec![ContactNameRecord {
            name: name.into(),
            count: 1,
            last_seen: D,
        }],
        user_name: None,
        search: format!(" {} {key} {local} {domain}", name.to_lowercase()),
        seen: 0,
        last_seen: None,
        personal: kind == 0,
        kind,
        note: None,
    }
}

#[tokio::test]
async fn reply_to_is_kept_and_sources_of_the_job_and_the_backfill_are_apart() {
    let f = fixture().await;
    f.store.set_classifier(Arc::new(WithReplyTo));
    let mut fetched = header(1, D, Some("a@x"));
    fetched.category_headers =
        b"From: sender@example.org\r\nReply-To: help@example.org\r\n".to_vec();
    f.store
        .store_headers(
            f.inbox,
            UV,
            vec![fetched, header(2, D + 1, Some("b@x"))],
            None,
        )
        .await
        .expect("store");
    let todo = f.store.contacts_todo(10).await.expect("todo");
    assert_eq!(todo.len(), 2);
    assert!(todo.iter().all(|s| !s.backfill));
    let reply_to: Vec<Vec<String>> = todo
        .iter()
        .map(|s| s.reply_to.iter().map(|a| a.email.clone()).collect())
        .collect();
    assert_eq!(reply_to, [vec!["help@example.org".to_owned()], Vec::new()]);
    assert!(
        f.store
            .contacts_backfill(10)
            .await
            .expect("backfill")
            .is_empty()
    );

    // Read by the rules of stage 10 (before the update): the backfill's, not the job's.
    f.store
        .write(|tx| Ok(tx.execute("UPDATE message SET contacts_seen = 1", [])?))
        .await
        .expect("as before");
    assert!(f.store.contacts_todo(10).await.expect("todo").is_empty());
    let backfill = f.store.contacts_backfill(10).await.expect("backfill");
    assert_eq!(backfill.len(), 2);
    assert!(backfill.iter().all(|s| s.backfill));
    assert!(backfill[0].message < backfill[1].message, "oldest first");
    f.store
        .record_contacts(Vec::new(), backfill.iter().map(|s| s.message).collect())
        .await
        .expect("record");
    assert!(
        f.store
            .contacts_backfill(10)
            .await
            .expect("backfill")
            .is_empty()
    );
    assert!(f.store.contacts_todo(10).await.expect("todo").is_empty());
}

#[tokio::test]
async fn people_only_leaves_out_list_and_system_addresses_and_kinds_can_be_recomputed() {
    let f = fixture().await;
    f.store
        .record_contacts(
            vec![
                contact("anna@x.example", 9.0, "Anna", 0),
                contact("news@shop.example", f64::NEG_INFINITY, "Shop", 1),
                contact("noreply@bank.example", f64::NEG_INFINITY, "Bank", 2),
                contact("otto@z.example", 1.0, "Otto", 0),
            ],
            Vec::new(),
        )
        .await
        .expect("record");
    let keys = |page: &crate::StoredContactPage| -> Vec<String> {
        page.groups.iter().map(|g| g.head().key.clone()).collect()
    };
    let people = f
        .store
        .contact_page(Vec::new(), None, 10, true)
        .await
        .expect("people");
    assert_eq!(keys(&people), ["anna@x.example", "otto@z.example"]);
    let all = f
        .store
        .contact_page(Vec::new(), None, 10, false)
        .await
        .expect("all");
    assert_eq!(all.groups.len(), 4);
    let by_domain = f
        .store
        .contact_page(vec!["shop.example".into()], None, 10, false)
        .await
        .expect("domain");
    assert_eq!(keys(&by_domain), ["news@shop.example"]);

    let rows = f.store.contact_kind_rows(0, 2).await.expect("rows");
    assert_eq!(rows.len(), 2);
    let rest = f
        .store
        .contact_kind_rows(rows[1].0, 10)
        .await
        .expect("rest");
    assert_eq!(rest.len(), 2);
    let otto = rest.iter().find(|r| r.1 == "otto@z.example").expect("otto");
    f.store
        .set_contact_kinds(vec![(otto.0, 1)])
        .await
        .expect("kinds");
    let people = f
        .store
        .contact_page(Vec::new(), None, 10, true)
        .await
        .expect("people");
    assert_eq!(keys(&people), ["anna@x.example"]);
}

#[tokio::test]
async fn notes_and_pictures_are_the_user_s_and_are_captured_for_the_sync() {
    let f = fixture().await;
    f.store
        .record_contacts(
            vec![
                contact("anna@x.example", 9.0, "Anna", 0),
                contact("anna@y.example", 1.0, "Anna", 0),
            ],
            Vec::new(),
        )
        .await
        .expect("record");
    clear_dirty(&f.store).await;
    assert!(
        f.store
            .set_contact_note("anna@x.example".into(), Some("Met at the fair".into()))
            .await
            .expect("note")
    );
    assert!(
        !f.store
            .set_contact_note("nobody@x.example".into(), Some("x".into()))
            .await
            .expect("no contact")
    );
    let jpeg = vec![0xFF, 0xD8, 0xFF, 0xE0, 1, 2, 3];
    let first = f
        .store
        .set_contact_picture(
            "anna@x.example".into(),
            vec!["anna@y.example".into()],
            jpeg.clone(),
        )
        .await
        .expect("picture");
    assert_eq!(
        sync_kinds(&f.store).await,
        [
            ("contact".to_owned(), "anna@x.example".to_owned()),
            ("contactPicture".to_owned(), "anna@x.example".to_owned()),
        ]
    );
    let content = f
        .store
        .read(|c| crate::contacts_dir::synced_content(c, "anna@x.example"))
        .await
        .expect("content");
    assert_eq!(
        content,
        Some(serde_json::json!({ "note": "Met at the fair" }))
    );
    let synced = f
        .store
        .read(|c| contacts_view::synced_picture(c, "anna@x.example"))
        .await
        .expect("picture content")
        .expect("some");
    assert_eq!(synced, serde_json::json!({ "jpeg": "/9j/4AECAw==" }));

    // The contact's picture is found through any of its addresses' keys, head first.
    let found = f
        .store
        .contact_picture_of(vec!["anna@y.example".into(), "anna@x.example".into()])
        .await
        .expect("read")
        .expect("found");
    assert_eq!(found.key, "anna@x.example");
    assert_eq!(found.display.as_deref(), Some(jpeg.as_slice()));
    let again = f
        .store
        .set_contact_picture("anna@x.example".into(), Vec::new(), jpeg.clone())
        .await
        .expect("again");
    assert!(again > first, "every picture gets a new revision");

    // From another device: stored, but not shown before this device encoded it again.
    clear_dirty(&f.store).await;
    let other = serde_json::json!({ "jpeg": contacts_view::base64(&[0xFF, 0xD8, 0xFF, 9]) });
    f.store
        .write(move |tx| contacts_view::apply_synced_picture(tx, "otto@z.example", Some(&other)))
        .await
        .expect("apply");
    // The sync pass removes the mark of what it applied (`apply_remote`).
    clear_dirty(&f.store).await;
    let otto = f
        .store
        .contact_picture_of(vec!["otto@z.example".into()])
        .await
        .expect("read")
        .expect("otto");
    assert_eq!(otto.display, None);
    assert_eq!(otto.image, [0xFF, 0xD8, 0xFF, 9]);
    assert!(
        !f.store
            .set_contact_picture_display("otto@z.example".into(), otto.revision - 1, vec![1])
            .await
            .expect("stale")
    );
    assert!(
        f.store
            .set_contact_picture_display("otto@z.example".into(), otto.revision, vec![0xFF, 0xD8])
            .await
            .expect("display")
    );
    assert!(
        sync_kinds(&f.store).await.is_empty(),
        "the local encoding is not synchronised: {:?}",
        sync_kinds(&f.store).await
    );
    let refused = f
        .store
        .write(|tx| {
            contacts_view::apply_synced_picture(
                tx,
                "otto@z.example",
                Some(&serde_json::json!({ "jpeg": "not base64!" })),
            )
        })
        .await;
    assert!(refused.is_err());
    let listed = f.store.contact_pictures().await.expect("list");
    assert_eq!(
        listed.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>(),
        ["anna@x.example", "otto@z.example"]
    );
    assert_eq!(
        f.store
            .remove_contact_pictures(vec!["anna@x.example".into(), "anna@y.example".into()])
            .await
            .expect("remove"),
        1
    );
    assert_eq!(
        sync_kinds(&f.store).await,
        [("contactPicture".to_owned(), "anna@x.example".to_owned())]
    );
}

#[tokio::test]
async fn a_sender_s_mail_outside_junk_sent_and_trash() {
    let f = fixture().await;
    f.store.set_classifier(Arc::new(WithReplyTo));
    f.store
        .store_headers(
            f.inbox,
            UV,
            vec![
                from(header(1, D, Some("a@x")), "spam@x.example"),
                from(header(2, D + 10, Some("b@x")), "Spam@X.example"),
                from(header(3, D + 20, Some("c@x")), "friend@x.example"),
            ],
            None,
        )
        .await
        .expect("inbox");
    f.store
        .store_headers(
            f.archive,
            UV,
            vec![from(header(1, D + 30, Some("d@x")), "spam@x.example")],
            None,
        )
        .await
        .expect("archive");
    f.store
        .store_headers(
            f.sent,
            UV,
            vec![from(header(1, D + 40, Some("e@x")), "spam@x.example")],
            None,
        )
        .await
        .expect("sent");
    let found = f
        .store
        .sender_locations("spam@x.example".into())
        .await
        .expect("locations");
    let places: Vec<(i64, MessageId)> = found
        .iter()
        .map(
            |SenderLocation {
                 mailbox,
                 message,
                 account,
             }| {
                assert_eq!(*account, f.account);
                (mailbox.0, *message)
            },
        )
        .collect();
    assert_eq!(places.len(), 3, "{places:?}");
    assert!(places.iter().all(|(mailbox, _)| *mailbox != f.sent.0));
    assert!(places.windows(2).all(|w| w[0].1 >= w[1].1), "newest first");
    let recent = f
        .store
        .sender_messages("spam@x.example".into(), 2)
        .await
        .expect("recent");
    assert_eq!(recent.len(), 2);
}

#[tokio::test]
async fn search_over_10000_contacts_by_name_address_and_domain_is_fast() {
    let f = fixture().await;
    let mut rows = Vec::with_capacity(10_000);
    for i in 0..10_000u32 {
        let name = format!("Person{i} Family{}", i % 97);
        let key = format!("person{i}@domain{}.example", i % 251);
        rows.push(contact(&key, f64::from(i), &name, i64::from(i % 3 == 0)));
    }
    f.store
        .record_contacts(rows, Vec::new())
        .await
        .expect("record");
    // Warm the reader and the statement cache once, as the view does on its first page.
    f.store
        .contact_page(vec!["x".into()], None, 50, true)
        .await
        .expect("warm");
    for (words, people_only) in [
        (vec!["person42".to_owned()], false),
        (vec!["family7".to_owned()], true),
        (vec!["person9999@domain".to_owned()], false),
        (vec!["domain13.example".to_owned()], false),
        // No match: the whole index is read.
        (vec!["zzz".to_owned()], false),
        (vec!["person1".to_owned(), "family1".to_owned()], true),
    ] {
        let started = Instant::now();
        let page = f
            .store
            .contact_page(words.clone(), None, 50, people_only)
            .await
            .expect("search");
        let took = started.elapsed();
        eprintln!(
            "contact search {words:?}: {} hits in {took:?}",
            page.groups.len()
        );
        assert!(
            took.as_millis() < 100,
            "{words:?} took {took:?} at 10,000 contacts"
        );
    }
}

#[tokio::test]
async fn new_statements_use_their_indexes() {
    let f = fixture().await;
    let plan = |conn: &Connection, sql: &str| -> Vec<String> {
        let mut stmt = conn
            .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
            .expect("plan");
        let nulls = vec![rusqlite::types::Null; stmt.parameter_count()];
        stmt.query_map(rusqlite::params_from_iter(nulls), |r| r.get::<_, String>(3))
            .expect("rows")
            .collect::<Result<Vec<_>, _>>()
            .expect("plan")
    };
    for (sql, expected) in [
        (
            contacts_view::sender_locations_sql(),
            "SEARCH m USING COVERING INDEX message_sender (sender_key=?)",
        ),
        (
            contacts_view::SENDER_MESSAGES.to_owned(),
            "SEARCH message USING COVERING INDEX message_sender (sender_key=?)",
        ),
        (
            contacts_view::PICTURE_BY_KEY.to_owned(),
            "SEARCH contact_picture USING INDEX sqlite_autoindex_contact_picture_1 (address_key=?)",
        ),
    ] {
        let lines = f
            .store
            .read({
                let sql = sql.clone();
                move |conn| Ok(plan(conn, &sql))
            })
            .await
            .expect("plan");
        assert!(lines.iter().any(|l| l == expected), "{sql}\n{lines:#?}");
    }
}
