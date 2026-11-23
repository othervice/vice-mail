//! Stage 10 in the store (decision 0023): pages of the contact directory in rank order with
//! merged contacts once, names, merges and their sync content, dismissed pairs, records that
//! wait for their address, and the query plans of the new statements.

use mail_model::ContactNameRecord;
use rusqlite::Connection;

use crate::contacts_dir::{self, ContactCursor, MergeOutcome, SeparateOutcome};
use crate::test_support::fixture;
use crate::{Store, StoredContact};

const D: i64 = 1_764_000_000;

fn contact(key: &str, rank: f64, name: &str) -> StoredContact {
    let (local, domain) = key.split_once('@').expect("address");
    StoredContact {
        key: key.into(),
        local_part: local.into(),
        domain: domain.into(),
        group: None,
        sent: 1,
        received: 0,
        last_sent: Some(D),
        last_received: None,
        rank,
        names: vec![ContactNameRecord {
            name: name.into(),
            count: 1,
            last_seen: D,
        }],
        user_name: None,
        search: format!(" {} {key}", name.to_lowercase()),
    }
}

async fn seed(store: &Store) {
    store
        .record_contacts(
            vec![
                contact("anna@x.example", 9.0, "Anna"),
                contact("hans@x.example", 5.0, "Hans Müller"),
                contact("hans.mueller@y.example", 3.0, "Hans Müller"),
                contact("otto@z.example", 1.0, "Otto"),
                contact("old@z.example", f64::NEG_INFINITY, "Old"),
            ],
            Vec::new(),
        )
        .await
        .expect("record");
}

fn keys(page: &crate::StoredContactPage) -> Vec<String> {
    page.groups.iter().map(|g| g.head().key.clone()).collect()
}

#[tokio::test]
async fn pages_come_in_rank_order_and_continue_after_the_cursor() {
    let f = fixture().await;
    seed(&f.store).await;
    let first = f
        .store
        .contact_page(Vec::new(), None, 2)
        .await
        .expect("page");
    assert_eq!(keys(&first), ["anna@x.example", "hans@x.example"]);
    let cursor = first.next.expect("more");
    let decoded = ContactCursor::decode(&cursor.encode()).expect("round trip");
    assert_eq!(decoded, cursor);
    let rest = f
        .store
        .contact_page(Vec::new(), Some(decoded), 10)
        .await
        .expect("page");
    assert_eq!(
        keys(&rest),
        ["hans.mueller@y.example", "otto@z.example", "old@z.example"]
    );
    assert!(rest.next.is_none(), "end");
    let found = f
        .store
        .contact_page(vec!["hans".into()], None, 10)
        .await
        .expect("search");
    assert_eq!(keys(&found), ["hans@x.example", "hans.mueller@y.example"]);
    assert!(ContactCursor::decode("x/1").is_none());
    assert!(ContactCursor::decode("NaN/1").is_none());
}

#[tokio::test]
async fn a_merged_contact_is_listed_once_with_all_addresses_and_syncs_its_merge() {
    let f = fixture().await;
    seed(&f.store).await;
    assert_eq!(
        f.store
            .merge_contacts(
                "hans.mueller@y.example".into(),
                vec!["hans@x.example".into()]
            )
            .await
            .expect("merge"),
        MergeOutcome::Merged("hans.mueller@y.example".into())
    );
    let page = f
        .store
        .contact_page(Vec::new(), None, 10)
        .await
        .expect("page");
    assert_eq!(
        keys(&page),
        [
            "anna@x.example",
            "hans.mueller@y.example",
            "otto@z.example",
            "old@z.example"
        ],
        "the group at the place of its best address, once"
    );
    let hans = &page.groups[1];
    let members: Vec<_> = hans.members.iter().map(|m| m.key.as_str()).collect();
    assert_eq!(
        members,
        ["hans.mueller@y.example", "hans@x.example"],
        "head first"
    );
    // Searching by the merged address finds the group once too.
    let found = f
        .store
        .contact_page(vec!["hans@x".into()], None, 10)
        .await
        .expect("search");
    assert_eq!(keys(&found), ["hans.mueller@y.example"]);

    // Merging again: the same contact.
    assert_eq!(
        f.store
            .merge_contacts(
                "hans@x.example".into(),
                vec!["hans.mueller@y.example".into()]
            )
            .await
            .expect("merge"),
        MergeOutcome::SameContact
    );
    assert_eq!(
        f.store
            .merge_contacts("anna@x.example".into(), vec!["nobody@x.example".into()])
            .await
            .expect("merge"),
        MergeOutcome::NotFound("nobody@x.example".into())
    );
    let anna = f
        .store
        .contact_group("anna@x.example".into())
        .await
        .expect("read")
        .expect("anna");
    assert_eq!(anna.members.len(), 1, "a refused merge changed nothing");
    assert_eq!(anna.head().group, None);

    // Sync content: the merged address points at the head; the head has none (no name).
    let (merged, head) = f
        .store
        .read(|conn| {
            Ok((
                contacts_dir::synced_content(conn, "hans@x.example")?,
                contacts_dir::synced_content(conn, "hans.mueller@y.example")?,
            ))
        })
        .await
        .expect("content");
    assert_eq!(
        merged,
        Some(serde_json::json!({ "mergedInto": "hans.mueller@y.example" }))
    );
    assert_eq!(head, None);

    // Separating the member: two contacts again.
    assert_eq!(
        f.store
            .separate_contact("hans@x.example".into())
            .await
            .expect("separate"),
        SeparateOutcome::Separated
    );
    let page = f
        .store
        .contact_page(Vec::new(), None, 10)
        .await
        .expect("page");
    assert_eq!(page.groups.len(), 5);
    assert!(page.groups.iter().all(|g| g.head().group.is_none()));
    assert_eq!(
        f.store
            .separate_contact("hans@x.example".into())
            .await
            .expect("separate"),
        SeparateOutcome::NotMerged
    );
}

#[tokio::test]
async fn names_merges_and_dismissed_pairs_are_kept_by_the_directory_job() {
    let f = fixture().await;
    seed(&f.store).await;
    assert!(
        f.store
            .set_contact_name(
                "otto@z.example".into(),
                Some("Otto O.".into()),
                " otto o otto@z.example".into()
            )
            .await
            .expect("name")
    );
    f.store
        .merge_contacts("anna@x.example".into(), vec!["old@z.example".into()])
        .await
        .expect("merge");
    // The directory job writes the rows again with what it read before: the name and the merge
    // stay.
    f.store
        .record_contacts(
            vec![
                contact("otto@z.example", 2.0, "Otto"),
                contact("old@z.example", 0.5, "Old"),
            ],
            Vec::new(),
        )
        .await
        .expect("record");
    let otto = f
        .store
        .contact_group("otto@z.example".into())
        .await
        .expect("read")
        .expect("otto");
    assert_eq!(otto.head().user_name.as_deref(), Some("Otto O."));
    let anna = f
        .store
        .contact_group("old@z.example".into())
        .await
        .expect("read")
        .expect("group");
    assert_eq!(anna.head().key, "anna@x.example");
    assert_eq!(anna.members.len(), 2);

    assert!(
        !f.store
            .set_contact_name("nobody@x.example".into(), None, String::new())
            .await
            .expect("name"),
        "unknown contact"
    );
    f.store
        .dismiss_contact_pair("hans@x.example".into(), "hans.mueller@y.example".into())
        .await
        .expect("dismiss");
    f.store
        .dismiss_contact_pair("hans.mueller@y.example".into(), "hans@x.example".into())
        .await
        .expect("dismiss twice");
    assert_eq!(
        f.store.dismissed_contact_pairs().await.expect("pairs"),
        [(
            "hans.mueller@y.example".to_owned(),
            "hans@x.example".to_owned()
        )]
    );
}

#[tokio::test]
async fn a_synchronised_record_waits_until_its_address_appears() {
    let f = fixture().await;
    f.store
        .record_contacts(vec![contact("anna@x.example", 9.0, "Anna")], Vec::new())
        .await
        .expect("record");
    f.store
        .write(|tx| {
            tx.execute(
                "INSERT INTO sync_record (kind, id, version, synced_version, content, updated_at)
                 VALUES ('contact', 'bob@x.example', 'v1', 'v1',
                         '{\"userName\":\"Bobby\",\"mergedInto\":\"anna@x.example\"}', 0)",
                [],
            )?;
            Ok(())
        })
        .await
        .expect("record");
    f.store
        .record_contacts(vec![contact("bob@x.example", 1.0, "Bob")], Vec::new())
        .await
        .expect("record");
    let bob = f
        .store
        .contact_group("bob@x.example".into())
        .await
        .expect("read")
        .expect("bob");
    assert_eq!(bob.head().key, "anna@x.example", "merged into anna");
    let row = bob
        .members
        .iter()
        .find(|m| m.key == "bob@x.example")
        .expect("bob");
    assert_eq!(row.user_name.as_deref(), Some("Bobby"));
}

fn plan(conn: &Connection, sql: &str) -> Vec<String> {
    let mut stmt = conn
        .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
        .unwrap_or_else(|err| panic!("prepare {sql}: {err}"));
    let nulls = vec![rusqlite::types::Null; stmt.parameter_count()];
    stmt.query_map(rusqlite::params_from_iter(nulls), |r| r.get::<_, String>(3))
        .expect("plan")
        .collect::<Result<Vec<_>, _>>()
        .expect("rows")
}

#[tokio::test]
async fn the_directory_queries_use_their_indexes() {
    let f = fixture().await;
    seed(&f.store).await;
    f.store
        .read(|conn| {
            for words in [0, 2] {
                let lines = plan(conn, &contacts_dir::page_sql(words));
                assert!(
                    lines
                        .iter()
                        .any(|l| l == "SEARCH c USING INDEX contact_rank (rank<?)"),
                    "{lines:#?}"
                );
                assert!(
                    lines
                        .iter()
                        .any(|l| l.contains("o USING INDEX contact_group (contact_group=?)")),
                    "{lines:#?}"
                );
                assert!(
                    !lines.iter().any(|l| l.contains("TEMP B-TREE")),
                    "{lines:#?}"
                );
            }
            let lines = plan(conn, contacts_dir::GROUP_MEMBERS);
            assert!(
                lines
                    .iter()
                    .any(|l| l == "SEARCH contact USING INDEX contact_group (contact_group=?)"),
                "{lines:#?}"
            );
            Ok(())
        })
        .await
        .expect("plans");
}
