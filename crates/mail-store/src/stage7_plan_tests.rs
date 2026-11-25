//! Query plans of stage 7 (decision 0003): every search, thread, list and index query, on a
//! fresh database and after `ANALYZE` with data, on a reader and on the writer. No full scan of
//! `message`, `message_location`, `thread` or `thread_ref`, the expected index for each, and a
//! temporary B-tree only where several mailbox branches are merged (n × (limit + 1) rows).

use mail_model::search::{RowidRange, TextMatch};
use mail_model::{MailboxId, MessageId};
use rusqlite::Connection;

use crate::search::{SearchFilter, build};
use crate::test_support::{Fixture, UV, db_path, fixture, header, seen};
use crate::{Store, StoreOptions, fts, groups, lists, threads};

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

/// A query, the plan lines it must contain, and whether merging branches may sort.
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
    let text = |f: SearchFilter| build(&f, Some(&[1, 2]), Some(1), 2_000_000_000, 51).sql;
    let m = |ids: &[i64]| ids.iter().map(|i| MailboxId(*i)).collect::<Vec<_>>();
    let all_filters = SearchFilter {
        text: TextMatch::Match("\"rechnung\"".into()),
        dates: RowidRange {
            from: Some(1),
            until: Some(2),
        },
        accounts: None,
        mailbox_sets: vec![m(&[1, 2]), m(&[3])],
        excluded_mailboxes: m(&[4]),
        unread: Some(true),
        flagged: Some(false),
        has_attachment: Some(true),
        only: None,
        without_complete_body: false,
        snoozed: Some(true),
        size_at_least: Some(1_000_000),
        size_below: None,
    };
    let fts_only = SearchFilter {
        text: TextMatch::Match("\"rechnung\"".into()),
        ..SearchFilter::default()
    };
    let in_one_unread = SearchFilter {
        mailbox_sets: vec![m(&[1])],
        unread: Some(true),
        ..SearchFilter::default()
    };
    let in_three = SearchFilter {
        mailbox_sets: vec![m(&[1, 2, 3])],
        has_attachment: Some(true),
        ..SearchFilter::default()
    };
    let exclude_only = SearchFilter {
        text: TextMatch::Exclude("\"spam\"".into()),
        unread: Some(false),
        ..SearchFilter::default()
    };
    let mut out = vec![
        // Index maintenance and the job
        e(
            "index job",
            fts::JOB,
            &["SEARCH search_index_job USING INTEGER PRIMARY KEY (rowid=?)"],
        ),
        e(
            "message for the index",
            fts::MESSAGE_FOR_INDEX,
            &[
                "SEARCH m USING INTEGER PRIMARY KEY (rowid=?)",
                "SEARCH b USING INTEGER PRIMARY KEY (rowid=?)",
            ],
        ),
        e(
            "index job batch",
            fts::JOB_BATCH,
            &[
                "SEARCH m USING INTEGER PRIMARY KEY (rowid>?)",
                "SEARCH b USING INTEGER PRIMARY KEY (rowid=?)",
            ],
        ),
        e("message id range", fts::ID_RANGE, &["SEARCH message"]),
        // Threading
        e(
            "thread of an id",
            threads::REF_THREAD,
            &["SEARCH thread_ref USING PRIMARY KEY (account_id=? AND ref_id=?)"],
        ),
        e(
            "subjects of a Message-ID",
            threads::SUBJECTS_BY_MESSAGE_ID,
            &["SEARCH message USING INDEX message_msgid (account_id=? AND message_id_hdr=?)"],
        ),
        e(
            "subject candidates",
            threads::SUBJECT_CANDIDATES,
            &[
                "SEARCH message USING INDEX message_subject (account_id=? AND subject_key=? AND id>? AND id<?)",
            ],
        ),
        e(
            "merge thread messages",
            threads::MOVE_MESSAGES,
            &["SEARCH message USING INDEX message_thread (thread_id=?)"],
        ),
        e(
            "merge thread ids",
            threads::MOVE_REFS,
            &["SEARCH thread_ref USING COVERING INDEX thread_ref_thread (thread_id=?)"],
        ),
        e(
            "thread counts",
            threads::THREAD_COUNTS,
            &["SEARCH thread USING INTEGER PRIMARY KEY (rowid=?)"],
        ),
        e(
            "delete thread",
            threads::DELETE_THREAD,
            &["SEARCH thread USING INTEGER PRIMARY KEY (rowid=?)"],
        ),
        e(
            "count thread message",
            threads::ADD_TO_THREAD,
            &["SEARCH thread USING INTEGER PRIMARY KEY (rowid=?)"],
        ),
        e(
            "own addresses",
            threads::OWN_ADDRESSES,
            &[
                "SEARCH account USING INTEGER PRIMARY KEY (rowid=?)",
                "SEARCH identity USING COVERING INDEX identity_email (account_id=?)",
            ],
        ),
        e(
            "unthreaded messages",
            threads::UNTHREADED,
            &["SEARCH message USING INDEX message_thread (thread_id=?)"],
        ),
        e(
            "any unthreaded message",
            threads::ANY_UNTHREADED,
            &["SEARCH message USING COVERING INDEX message_thread (thread_id=?)"],
        ),
        e(
            "set thread",
            threads::SET_THREAD,
            &["SEARCH message USING INTEGER PRIMARY KEY (rowid=?)"],
        ),
        // The trigger's query when a thread's newest message goes.
        e(
            "newest message of a thread",
            "SELECT max(id) FROM message WHERE thread_id = ?1",
            &["SEARCH message USING COVERING INDEX message_thread (thread_id=?)"],
        ),
        // Lists
        e(
            "thread list of a mailbox",
            &lists::threads_sql(1),
            &[
                "SEARCH l USING PRIMARY KEY (mailbox_id=? AND message_id<?)",
                "SEARCH m USING INTEGER PRIMARY KEY (rowid=?)",
                "SEARCH n USING COVERING INDEX message_thread (thread_id=? AND rowid>?)",
                "SEARCH nl USING COVERING INDEX loc_message (message_id=? AND mailbox_id=?)",
                "SEARCH o USING INDEX operation_location (mailbox_id=? AND uid=?)",
            ],
        ),
        e(
            "thread of a message",
            lists::THREAD_OF,
            &["SEARCH message USING INTEGER PRIMARY KEY (rowid=?)"],
        ),
        e(
            "messages of a thread",
            lists::THREAD_MESSAGES,
            &["SEARCH message USING COVERING INDEX message_thread (thread_id=?)"],
        ),
        e(
            "message row",
            lists::MESSAGE_ROW,
            &[
                "SEARCH m USING INTEGER PRIMARY KEY (rowid=?)",
                "SEARCH a USING INTEGER PRIMARY KEY (rowid=?)",
                "SEARCH b USING INTEGER PRIMARY KEY (rowid=?)",
            ],
        ),
        e(
            "locations of a message",
            &lists::locations_sql(),
            &[
                "SEARCH l USING INDEX loc_message (message_id=?)",
                "SEARCH o USING INDEX operation_location (mailbox_id=? AND uid=?)",
            ],
        ),
        e(
            "message at a UID",
            lists::MESSAGE_BY_UID,
            &["SEARCH message_location USING COVERING INDEX loc_uid (mailbox_id=? AND uid=?)"],
        ),
        e(
            "unified inbox, one account",
            &lists::unified_sql(1),
            &[
                "SEARCH l USING PRIMARY KEY (mailbox_id=? AND message_id<?)",
                "SEARCH m USING INTEGER PRIMARY KEY (rowid=?)",
            ],
        ),
        // Groups
        e(
            "group by id",
            groups::GROUP_BY_UUID,
            &[
                "SEARCH account_group USING COVERING INDEX sqlite_autoindex_account_group_1 (uuid=?)",
            ],
        ),
        e(
            "group members",
            groups::GROUP_MEMBERS,
            &["SEARCH account_group_member USING PRIMARY KEY (group_id=?)"],
        ),
        e(
            "inbox of an account",
            groups::INBOX_OF,
            &["SEARCH mailbox USING INDEX mailbox_role (account_id=? AND role=?)"],
        ),
        e(
            "mailbox names",
            groups::MAILBOX_NAMES,
            &["SEARCH mailbox USING INDEX mailbox_path (account_id=?)"],
        ),
        // Search
        e(
            "search: text with every filter",
            &text(all_filters),
            &[
                // FTS5 walks the rowid range descending (`<>`: both bounds) and stops early.
                "SCAN message_fts VIRTUAL TABLE INDEX 192:M6<>",
                "SEARCH m USING INTEGER PRIMARY KEY (rowid=?)",
                // The location filters: a key or a partial index, both by mailbox and message.
                "SEARCH fl ",
                "SEARCH il EXISTS USING COVERING INDEX loc_message (message_id=? AND mailbox_id=?)",
                "SEARCH xl USING PRIMARY KEY (mailbox_id=? AND message_id=?)",
            ],
        ),
        e(
            "search: text",
            &text(fts_only),
            &[
                "SCAN message_fts VIRTUAL TABLE INDEX 192:M6<>",
                "SEARCH m USING INTEGER PRIMARY KEY (rowid=?)",
            ],
        ),
        e(
            "search: unread in one mailbox",
            &text(in_one_unread),
            &[
                "SEARCH l USING COVERING INDEX loc_unseen (mailbox_id=? AND message_id>? AND message_id<?)",
                "SEARCH m USING INTEGER PRIMARY KEY (rowid=?)",
            ],
        ),
        e(
            "search: only exclusions",
            &text(exclude_only),
            &[
                "SEARCH m USING INTEGER PRIMARY KEY (rowid>? AND rowid<?)",
                "SCAN message_fts VIRTUAL TABLE INDEX 0:M6",
                "SEARCH fl USING INDEX loc_message (message_id=?)",
            ],
        ),
    ];
    out.push(Expected {
        name: "thread list of the unified inbox, three accounts",
        sql: lists::threads_sql(3),
        required: vec![
            "SEARCH l USING PRIMARY KEY (mailbox_id=? AND message_id<?)",
            "SEARCH n USING COVERING INDEX message_thread (thread_id=? AND rowid>?)",
        ],
        merge: true,
    });
    out.push(Expected {
        name: "unified inbox, three accounts",
        sql: lists::unified_sql(3),
        required: vec![
            "SEARCH l USING PRIMARY KEY (mailbox_id=? AND message_id<?)",
            "SEARCH m USING INTEGER PRIMARY KEY (rowid=?)",
        ],
        merge: true,
    });
    out.push(Expected {
        name: "search: three mailboxes",
        sql: text(in_three),
        required: vec![
            "SEARCH l USING PRIMARY KEY (mailbox_id=? AND message_id>? AND message_id<?)",
            "SEARCH m USING INTEGER PRIMARY KEY (rowid=?)",
        ],
        merge: true,
    });
    out
}

fn check_plans(conn: &Connection, stage: &str) {
    let mut problems = Vec::new();
    for q in expected() {
        let lines = plan(conn, &q.sql);
        let bad = lines.iter().any(|l| {
            let merge_scan = q.merge && l.starts_with("SCAN (subquery-");
            (l.starts_with("SCAN")
                && !l.starts_with("SCAN message_fts VIRTUAL TABLE")
                && l != "SCAN CONSTANT ROW"
                && !merge_scan)
                || (l.contains("TEMP B-TREE") && !q.merge)
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
async fn every_stage_7_query_uses_its_index() {
    let f = fixture().await;
    check_all(&f.store, "fresh database").await;

    for (mailbox, n, offset) in [
        (f.inbox, 3_000u32, 0i64),
        (f.archive, 1_000, 500),
        (f.sent, 500, 900),
    ] {
        let headers: Vec<_> = (1..=n)
            .map(|uid| {
                let k = i64::from(uid) + offset;
                let mut h = header(uid, 1_700_000_000 + k * 600, Some(&format!("{k}@x")));
                h.subject = format!("Projekt {} Besprechung", k / 3);
                if k % 3 != 0 {
                    h.in_reply_to = Some(format!("{}@x", k - 1));
                    h.references = vec![format!("{}@x", k - k % 3), format!("{}@x", k - 1)];
                }
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
        .store_body(
            MessageId(1_700_000_600),
            b"Subject: x\r\n\r\nhallo".to_vec(),
        )
        .await
        .expect("body");
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
