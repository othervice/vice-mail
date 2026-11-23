//! Query plans of stage 9 (decision 0020): the inbox lists (rows, a category, the important
//! senders) over one and three mailboxes, the marks of list rows, the category jobs, the
//! snooze timer, the counts and the arrivals. No full scan of `message` or `message_location`,
//! the expected index for each; a temporary B-tree only where several mailbox branches are
//! merged. The small tables of the user's marks and rules (`pin`, `task`, `snooze`,
//! `sender_rule`, `rule`) may be scanned.

use mail_model::{MailboxId, MessageCategory, TaskFilter};
use rusqlite::Connection;

use crate::inbox::{self, InboxListKind};
use crate::search::{SearchFilter, build};
use crate::test_support::{Fixture, UV, db_path, fixture, header, seen};
use crate::{Store, StoreOptions, categories, marks};

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

struct Expected {
    name: &'static str,
    sql: String,
    required: Vec<&'static str>,
    merge: bool,
}

fn expected() -> Vec<Expected> {
    let e = |name, sql: &str, required: &[&'static str]| Expected {
        name,
        sql: sql.to_owned(),
        required: required.to_vec(),
        merge: false,
    };
    let merged = |name, sql: String, required: &[&'static str]| Expected {
        name,
        sql,
        required: required.to_vec(),
        merge: true,
    };
    let mut out = vec![
        // The inbox lists, one mailbox.
        e(
            "inbox rows",
            &inbox::inbox_sql(InboxListKind::Rows, 1),
            &[
                "SEARCH l USING PRIMARY KEY (mailbox_id=? AND message_id<?)",
                "SEARCH m USING INTEGER PRIMARY KEY (rowid=?)",
                "SEARCH s USING COVERING INDEX sqlite_autoindex_snooze_1 (account_uuid=? AND message_id_hdr=?)",
                "SEARCH p USING COVERING INDEX sqlite_autoindex_pin_1 (account_uuid=? AND message_id_hdr=?)",
            ],
        ),
        e(
            "inbox category",
            &inbox::inbox_sql(InboxListKind::Category(MessageCategory::Person), 1),
            &[
                "SEARCH l USING INDEX loc_category (mailbox_id=? AND category=? AND message_id<?)",
                "SEARCH m USING INTEGER PRIMARY KEY (rowid=?)",
            ],
        ),
        e(
            "inbox important",
            &inbox::inbox_sql(InboxListKind::Important, 1),
            &[
                "SEARCH l USING INDEX loc_important (mailbox_id=? AND message_id<?)",
                "SEARCH m USING INTEGER PRIMARY KEY (rowid=?)",
            ],
        ),
        // Marks of list rows and the snooze timer.
        e(
            "row marks",
            marks::ROW_MARKS,
            &["SEARCH m USING INTEGER PRIMARY KEY (rowid=?)"],
        ),
        e(
            "sync identity",
            marks::KEY_OF,
            &["SEARCH m USING INTEGER PRIMARY KEY (rowid=?)"],
        ),
        e(
            "snooze of a message",
            marks::SNOOZE_OF,
            &[
                "SEARCH snooze USING INDEX sqlite_autoindex_snooze_1 (account_uuid=? AND message_id_hdr=?)",
            ],
        ),
        e(
            "task of a message",
            marks::TASK_OF,
            &[
                "SEARCH task USING INDEX sqlite_autoindex_task_1 (account_uuid=? AND message_id_hdr=?)",
            ],
        ),
        e(
            "messages of an identity",
            marks::MESSAGES_OF,
            &["SEARCH m USING COVERING INDEX message_msgid (account_id=? AND message_id_hdr=?)"],
        ),
        e(
            "due snoozes",
            marks::DUE,
            &["SEARCH snooze USING INDEX snooze_due (until<?)"],
        ),
        e(
            "next due snooze",
            "SELECT min(until) FROM snooze WHERE returned_at IS NULL",
            &["SEARCH snooze USING INDEX snooze_due"],
        ),
        e(
            "snoozed view",
            &marks::view_sql(marks::MarkView::Snoozed, 2),
            &[],
        ),
        e(
            "pinned view",
            &marks::view_sql(marks::MarkView::Pinned, 2),
            &[],
        ),
        e(
            "task view",
            &marks::view_sql(marks::MarkView::Tasks(TaskFilter::Open), 2),
            &[],
        ),
        // Categories.
        e(
            "unclassified messages",
            categories::UNCLASSIFIED,
            &["SCAN message USING INDEX message_category_todo"],
        ),
        e(
            "rules pass",
            categories::RULE_ROWS,
            &["SEARCH message USING INTEGER PRIMARY KEY (rowid<?)"],
        ),
        e(
            "set the effective category",
            categories::SET_EFFECTIVE,
            &["SEARCH message USING INTEGER PRIMARY KEY (rowid=?)"],
        ),
        e(
            "category of a message",
            categories::MESSAGE_CATEGORY,
            &["SEARCH message USING INTEGER PRIMARY KEY (rowid=?)"],
        ),
        e(
            "written to the sender",
            categories::SENT_TO,
            &["SEARCH contact USING INDEX sqlite_autoindex_contact_1 (address_key=?)"],
        ),
        e(
            "senders of answered messages",
            categories::REFERENCED_FROM,
            &["SEARCH message USING INDEX message_msgid (account_id=? AND message_id_hdr=?)"],
        ),
        e(
            "the copies on the locations (trigger)",
            "UPDATE message_location SET category = ?2, important = ?3 WHERE message_id = ?1",
            &["SEARCH message_location USING INDEX loc_message (message_id=?)"],
        ),
        // Top block, counts, arrivals.
        e(
            "pinned on top",
            inbox::PINNED_IN,
            &["SEARCH p USING INDEX sqlite_autoindex_pin_1 (account_uuid=?)"],
        ),
        e(
            "returned on top",
            inbox::RETURNED_IN,
            &["SEARCH s USING INDEX sqlite_autoindex_snooze_1 (account_uuid=?)"],
        ),
        e(
            "important counts",
            inbox::IMPORTANT_COUNTS,
            &["SEARCH message_location USING INDEX loc_important (mailbox_id=?)"],
        ),
        e(
            "unread for the badge",
            inbox::UNREAD_ALL,
            &["SEARCH message_location USING COVERING INDEX loc_unseen (mailbox_id=?)"],
        ),
        e(
            "unread people for the badge",
            inbox::UNREAD_PEOPLE,
            &["SEARCH message_location USING INDEX loc_unseen (mailbox_id=?)"],
        ),
        e(
            "arrived messages",
            inbox::ARRIVED,
            &[
                "SEARCH l USING INDEX loc_uid (mailbox_id=? AND uid>? AND uid<?)",
                "SEARCH m USING INTEGER PRIMARY KEY (rowid=?)",
            ],
        ),
    ];
    for (name, kind, required) in [
        (
            "inbox rows, three mailboxes",
            InboxListKind::Rows,
            "SEARCH l USING PRIMARY KEY (mailbox_id=? AND message_id<?)",
        ),
        (
            "inbox category, three mailboxes",
            InboxListKind::Category(MessageCategory::Newsletter),
            "SEARCH l USING INDEX loc_category (mailbox_id=? AND category=? AND message_id<?)",
        ),
        (
            "inbox important, three mailboxes",
            InboxListKind::Important,
            "SEARCH l USING INDEX loc_important (mailbox_id=? AND message_id<?)",
        ),
    ] {
        out.push(merged(name, inbox::inbox_sql(kind, 3), &[required]));
    }
    let m = |ids: &[i64]| ids.iter().map(|i| MailboxId(*i)).collect::<Vec<_>>();
    out.push(e(
        "search is:snoozed in a mailbox",
        &build(
            &SearchFilter {
                mailbox_sets: vec![m(&[1])],
                snoozed: Some(true),
                ..SearchFilter::default()
            },
            None,
            None,
            2_000_000_000,
            51,
        )
        .sql,
        &[
            "SEARCH l USING PRIMARY KEY (mailbox_id=? AND message_id<?)",
            "SEARCH sz EXISTS USING INDEX sqlite_autoindex_snooze_1 (account_uuid=? AND message_id_hdr=?)",
        ],
    ));
    out
}

/// Tables that only hold the user's marks and rules (dozens to a few thousand rows).
const SMALL: [&str; 5] = ["pin", "task", "snooze", "sender_rule", "rule"];

fn check_plans(conn: &Connection, stage: &str) {
    let mut problems = Vec::new();
    for q in expected() {
        let lines = plan(conn, &q.sql);
        let bad = lines.iter().any(|l| {
            let merge_scan = q.merge && l.starts_with("SCAN (subquery-");
            let small = SMALL.iter().any(|t| l.starts_with(&format!("SCAN {t}")));
            let todo = l == "SCAN message USING INDEX message_category_todo";
            (l.starts_with("SCAN") && l != "SCAN CONSTANT ROW" && !merge_scan && !small && !todo)
                || (l.contains("TEMP B-TREE") && !q.merge && !small_sort(&q, l))
        });
        let missing: Vec<&str> = q
            .required
            .iter()
            .copied()
            .filter(|want| !lines.iter().any(|l| l.starts_with(want)))
            .collect();
        if bad || !missing.is_empty() {
            problems.push(format!("{}: plan {lines:#?}, missing {missing:#?}", q.name));
        }
    }
    assert!(
        problems.is_empty(),
        "{stage}: full scan, temporary B-tree or unexpected index:\n{}",
        problems.join("\n")
    );
}

/// The views of pins and tasks sort their few rows.
fn small_sort(q: &Expected, line: &str) -> bool {
    matches!(
        q.name,
        "snoozed view" | "pinned view" | "task view" | "pinned on top" | "returned on top"
    ) && line.contains("ORDER BY")
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
async fn every_stage_9_query_uses_its_index() {
    let f = fixture().await;
    check_all(&f.store, "fresh database").await;

    for (mailbox, n, offset) in [(f.inbox, 3_000u32, 0i64), (f.archive, 1_000, 5_000)] {
        let headers: Vec<_> = (1..=n)
            .map(|uid| {
                let k = i64::from(uid) + offset;
                let h = header(uid, 1_700_000_000 + k * 600, Some(&format!("{k}@x")));
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
    // Some categories, marks and rules, so the statistics see them.
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
    for (i, id) in [1_700_000_600i64, 1_700_001_200, 1_700_001_800]
        .into_iter()
        .enumerate()
    {
        let message = mail_model::MessageId(id);
        f.store.pin_message(message, 1).await.expect("pin");
        f.store
            .snooze_message(message, 10_000 + i64::try_from(i).expect("i"), 1)
            .await
            .expect("snooze");
        f.store
            .set_task(message, Some(false), 1)
            .await
            .expect("task");
    }
    f.store
        .write(|tx| Ok(tx.execute_batch("ANALYZE")?))
        .await
        .expect("analyze");
    check_all(&f.store, "after ANALYZE").await;

    let Fixture {
        _dir: dir, store, ..
    } = f;
    drop(store);
    let store = Store::open(&db_path(&dir), StoreOptions::default()).expect("reopen");
    check_all(&store, "reopened with statistics").await;
}
