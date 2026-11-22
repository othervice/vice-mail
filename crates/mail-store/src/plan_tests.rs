//! Query plans (decision 0003): every list, counter and sync query is checked with
//! `EXPLAIN QUERY PLAN`, on a fresh database and after `ANALYZE` with data. No full scan, no
//! temporary B-tree, and the expected index for each.

use mail_model::UidSet;
use rusqlite::Connection;

use crate::test_support::{Fixture, UV, db_path, fixture, header, seen};
use crate::{Store, StoreOptions, accounts, mailboxes, messages};

/// Detail lines of the plan of `sql` (parameters unbound, which does not change the plan).
fn plan(conn: &Connection, sql: &str) -> Vec<String> {
    let mut stmt = conn
        .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
        .expect("prepare EXPLAIN QUERY PLAN");
    // Unbound parameters are NULL; rusqlite wants them bound explicitly.
    let nulls = vec![rusqlite::types::Null; stmt.parameter_count()];
    stmt.query_map(rusqlite::params_from_iter(nulls), |r| r.get::<_, String>(3))
        .expect("query plan")
        .collect::<Result<Vec<_>, _>>()
        .expect("plan rows")
}

/// The queries, each with the plan lines it must contain.
const EXPECTED: &[(&str, &str, &[&str])] = &[
    (
        "list page",
        messages::LIST_PAGE,
        &[
            "SEARCH l USING PRIMARY KEY (mailbox_id=? AND message_id<?)",
            "SEARCH m USING INTEGER PRIMARY KEY (rowid=?)",
        ],
    ),
    (
        "account of a mailbox",
        messages::MAILBOX_ACCOUNT,
        &[
            "SEARCH b USING INTEGER PRIMARY KEY (rowid=?)",
            "SEARCH a USING INTEGER PRIMARY KEY (rowid=?)",
        ],
    ),
    (
        "mailbox before a role change",
        mailboxes::MAILBOX_FOR_ROLE,
        &[
            "SEARCH b USING INTEGER PRIMARY KEY (rowid=?)",
            "SEARCH a USING INTEGER PRIMARY KEY (rowid=?)",
        ],
    ),
    (
        "mailboxes for the list refresh",
        mailboxes::LOAD,
        &["SEARCH mailbox USING INDEX mailbox_path (account_id=?)"],
    ),
    (
        "mailboxes with counts",
        mailboxes::LIST_MAILBOXES,
        &[
            "SEARCH b USING INDEX mailbox_path (account_id=?)",
            "SEARCH l USING COVERING INDEX loc_unseen (mailbox_id=?)",
            "SEARCH l USING COVERING INDEX loc_uid (mailbox_id=?)",
        ],
    ),
    (
        "unread counts",
        mailboxes::UNREAD_COUNTS,
        &[
            "SEARCH b USING COVERING INDEX mailbox_path (account_id=?)",
            "SEARCH l USING COVERING INDEX loc_unseen (mailbox_id=?)",
        ],
    ),
    (
        "sync states",
        mailboxes::SYNC_STATES,
        &["SEARCH mailbox USING INDEX mailbox_path (account_id=?)"],
    ),
    (
        "account by uuid",
        accounts::ACCOUNT_ROW,
        &["SEARCH account USING COVERING INDEX sqlite_autoindex_account_1 (uuid=?)"],
    ),
    (
        "location by UID",
        messages::LOCATION_BY_UID,
        &["SEARCH message_location USING INDEX loc_uid (mailbox_id=? AND uid=?)"],
    ),
    (
        "flag update",
        messages::UPDATE_LOCATION,
        &["SEARCH message_location USING COVERING INDEX loc_uid (mailbox_id=? AND uid=?)"],
    ),
    (
        "merge candidate",
        messages::MERGE_CANDIDATE,
        &[
            "SEARCH m USING COVERING INDEX message_msgid (account_id=? AND message_id_hdr=?)",
            "SEARCH l USING PRIMARY KEY (mailbox_id=? AND message_id=?)",
        ],
    ),
    (
        "free id",
        messages::IDS_FROM,
        &["SEARCH message USING INTEGER PRIMARY KEY (rowid>?)"],
    ),
    (
        "remove UIDs",
        messages::REMOVE_UID_RANGE,
        &["SEARCH message_location USING INDEX loc_uid (mailbox_id=? AND uid>? AND uid<?)"],
    ),
    (
        "known UIDs",
        messages::KNOWN_UIDS,
        &["SEARCH message_location USING COVERING INDEX loc_uid (mailbox_id=?)"],
    ),
    (
        "orphans",
        messages::DELETE_ORPHANS,
        &[
            "SEARCH message USING COVERING INDEX message_msgid (account_id=?)",
            "SEARCH l USING COVERING INDEX loc_message (message_id=?)",
        ],
    ),
];

fn check_plans(conn: &Connection, stage: &str) {
    let mut problems = Vec::new();
    for (name, sql, required) in EXPECTED {
        let lines = plan(conn, sql);
        let scans = lines
            .iter()
            .any(|l| l.starts_with("SCAN") || l.contains("TEMP B-TREE"));
        let missing: Vec<&str> = required
            .iter()
            .copied()
            .filter(|want| !lines.iter().any(|l| l == want))
            .collect();
        if scans || !missing.is_empty() {
            problems.push(format!("{name}: plan {lines:#?}, missing {missing:#?}"));
        }
    }
    assert!(
        problems.is_empty(),
        "{stage}: full scan, temporary B-tree or unexpected index:\n{}",
        problems.join("\n")
    );
}

/// Checks on a reader and on the writer (which holds fresh statistics right after `ANALYZE`).
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
async fn every_query_uses_its_index() {
    let f = fixture().await;
    check_all(&f.store, "fresh database").await;

    // Data with the shape of a real account, then statistics as `PRAGMA optimize` writes them.
    for (mailbox, n, offset) in [(f.inbox, 3_000u32, 0i64), (f.archive, 1_000, 500)] {
        let headers: Vec<_> = (1..=n)
            .map(|uid| {
                let h = header(
                    uid,
                    1_700_000_000 + (i64::from(uid) + offset) * 600,
                    Some(&format!("{}@x", i64::from(uid) + offset)),
                );
                if uid % 7 == 0 { h } else { seen(h) }
            })
            .collect();
        for window in headers.chunks(500) {
            f.store
                .store_headers(mailbox, UV, window.to_vec(), None)
                .await
                .expect("store");
        }
    }
    f.store
        .remove_uids(f.inbox, UV, UidSet::range(1, 200))
        .await
        .expect("remove");
    f.store
        .write(|tx| Ok(tx.execute_batch("ANALYZE")?))
        .await
        .expect("analyze");
    check_all(&f.store, "after ANALYZE").await;

    // Readers load the statistics with the schema; a new store has them on every connection.
    let Fixture {
        _dir: dir, store, ..
    } = f;
    drop(store);
    let store = Store::open(&db_path(&dir), StoreOptions::default()).expect("reopen");
    check_all(&store, "reopened with statistics").await;
}
