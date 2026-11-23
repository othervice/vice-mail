use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

use mail_model::{CoreError, ErrorCode};
use rusqlite::{Connection, OpenFlags};
use rusqlite_migration::M;
use tempfile::TempDir;

use crate::store::Schema;
use crate::{GLOBAL_SCOPE, SCHEMA_VERSION, Store, StoreError, StoreOptions, backup};

fn db_path(dir: &TempDir) -> PathBuf {
    dir.path().join("data").join("mail.sqlite3")
}

/// Reads `user_version` through an independent read-only connection.
fn user_version(path: &Path) -> usize {
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .expect("open read-only");
    let v: i64 = conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .expect("read user_version");
    usize::try_from(v).expect("non-negative")
}

fn table_exists(path: &Path, table: &str) -> bool {
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .expect("open read-only");
    conn.query_row(
        "SELECT count(*) FROM sqlite_schema WHERE type = 'table' AND name = ?1",
        [table],
        |r| r.get::<_, i64>(0),
    )
    .expect("query schema")
        == 1
}

/// `n` throwaway migrations t1..tn, standing in for future schema changes.
fn test_schema(n: usize) -> Schema {
    const SQL: [&str; 5] = [
        "CREATE TABLE t1 (x INTEGER) STRICT;",
        "CREATE TABLE t2 (x INTEGER) STRICT;",
        "CREATE TABLE t3 (x INTEGER) STRICT;",
        "CREATE TABLE t4 (x INTEGER) STRICT;",
        "CREATE TABLE t5 (x INTEGER) STRICT;",
    ];
    Schema::from_vec(SQL[..n].iter().map(|sql| M::up(sql)).collect())
}

// ---------------------------------------------------------------------------------------------
// Acceptance: migration from empty, second start, too-new database, backups
// ---------------------------------------------------------------------------------------------

#[tokio::test]
async fn migrates_an_empty_database() {
    let dir = TempDir::new().expect("temp dir");
    let path = db_path(&dir);

    let store = Store::open(&path, StoreOptions::default()).expect("open");
    let report = store.open_report().clone();
    drop(store);

    assert_eq!(report.previous_version, 0);
    assert_eq!(report.version, SCHEMA_VERSION);
    assert_eq!(report.backup, None, "a new database needs no backup");
    assert_eq!(user_version(&path), SCHEMA_VERSION);
    assert!(table_exists(&path, "setting"));
    assert!(backup::list(&path).is_empty());

    let conn = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY).expect("open");
    let mode: String = conn
        .query_row("PRAGMA journal_mode", [], |r| r.get(0))
        .expect("journal_mode");
    assert_eq!(mode, "wal", "WAL is stored in the file");
}

#[tokio::test]
async fn second_start_changes_nothing() {
    let dir = TempDir::new().expect("temp dir");
    let path = db_path(&dir);

    let store = Store::open(&path, StoreOptions::default()).expect("first open");
    store
        .put_settings(GLOBAL_SCOPE, vec![("ui.theme".into(), "\"dark\"".into())])
        .await
        .expect("write");
    drop(store);
    let version_before = user_version(&path);

    let store = Store::open(&path, StoreOptions::default()).expect("second open");
    let report = store.open_report().clone();
    let settings = store.settings(GLOBAL_SCOPE).await.expect("read");
    drop(store);

    assert_eq!(report.previous_version, SCHEMA_VERSION);
    assert_eq!(report.version, SCHEMA_VERSION);
    assert_eq!(report.backup, None, "no migration, no backup");
    assert!(backup::list(&path).is_empty());
    assert!(!backup::backup_dir(&path).exists());
    assert_eq!(user_version(&path), version_before);
    assert_eq!(settings.len(), 1);
    assert_eq!(settings[0].value, "\"dark\"");
}

#[tokio::test]
async fn rejects_a_newer_database_and_leaves_it_untouched() {
    let dir = TempDir::new().expect("temp dir");
    let path = db_path(&dir);
    fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
    let newer = SCHEMA_VERSION + 5;
    {
        let conn = Connection::open(&path).expect("create");
        conn.execute_batch(&format!(
            "PRAGMA journal_mode = WAL;
             CREATE TABLE from_the_future (x INTEGER);
             INSERT INTO from_the_future VALUES (42);
             PRAGMA user_version = {newer};"
        ))
        .expect("prepare newer database");
    }
    let bytes_before = fs::read(&path).expect("read file");

    let err = Store::open(&path, StoreOptions::default())
        .err()
        .expect("must be rejected");
    assert!(
        matches!(err, StoreError::TooNew { found, supported } if found == newer && supported == SCHEMA_VERSION),
        "{err:?}"
    );

    let core = CoreError::from(err);
    assert_eq!(core.code(), ErrorCode::DatabaseTooNew);
    assert!(core.message().contains("newer version"));

    assert_eq!(
        fs::read(&path).expect("read file"),
        bytes_before,
        "file unchanged"
    );
    assert_eq!(user_version(&path), newer);
    assert!(
        !backup::backup_dir(&path).exists(),
        "no backup of a rejected file"
    );
}

#[tokio::test]
async fn backs_up_before_each_migration_and_keeps_three() {
    let dir = TempDir::new().expect("temp dir");
    let path = db_path(&dir);

    // Version 1, with data that the backups must contain.
    let store = Store::open_with(&path, StoreOptions::default(), &test_schema(1)).expect("v1");
    assert_eq!(store.open_report().backup, None);
    store
        .write(|tx| Ok(tx.execute("INSERT INTO t1 (x) VALUES (7)", [])?))
        .await
        .expect("insert");
    drop(store);

    let mut written = Vec::new();
    for n in 2..=5 {
        let store = Store::open_with(&path, StoreOptions::default(), &test_schema(n))
            .unwrap_or_else(|e| panic!("open v{n}: {e}"));
        let report = store.open_report().clone();
        drop(store);

        assert_eq!(report.previous_version, n - 1);
        assert_eq!(report.version, n);
        let backup = report.backup.expect("backup before migration");
        assert!(
            backup
                .to_string_lossy()
                .ends_with(&format!("-v{}.sqlite3", n - 1))
        );
        // The backup is the database as it was before this migration.
        assert_eq!(user_version(&backup), n - 1);
        assert!(!table_exists(&backup, &format!("t{n}")));
        let conn = Connection::open_with_flags(&backup, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .expect("open backup");
        let x: i64 = conn
            .query_row("SELECT x FROM t1", [], |r| r.get(0))
            .expect("data in backup");
        assert_eq!(x, 7);
        written.push(backup);
    }

    assert_eq!(user_version(&path), 5);
    let kept = backup::list(&path);
    assert_eq!(kept.len(), backup::BACKUPS_KEPT);
    assert_eq!(kept, written[1..], "the three newest are kept");
    assert!(!written[0].exists(), "the oldest was removed");
}

#[tokio::test]
async fn partial_backups_left_behind_are_removed_and_never_listed() {
    let dir = TempDir::new().expect("temp dir");
    let path = db_path(&dir);
    drop(Store::open_with(&path, StoreOptions::default(), &test_schema(1)).expect("v1"));
    // What an interrupted or failed backup leaves behind (e.g. the flush that Windows refused
    // for a read-only handle before 23.11.2026).
    let backups = backup::backup_dir(&path);
    fs::create_dir_all(&backups).expect("backup dir");
    let stem = path
        .file_stem()
        .expect("stem")
        .to_string_lossy()
        .into_owned();
    let leftover = backups.join(format!("{stem}-000000000000001-v1.sqlite3.partial"));
    fs::write(&leftover, b"half a database").expect("leftover");
    let foreign = backups.join("other-000000000000001-v1.sqlite3.partial");
    fs::write(&foreign, b"another database's leftover").expect("foreign");

    let store = Store::open_with(&path, StoreOptions::default(), &test_schema(2)).expect("v2");
    let backup = store
        .open_report()
        .backup
        .clone()
        .expect("backup before migration");
    drop(store);

    assert!(!leftover.exists(), "the leftover partial is removed");
    assert!(foreign.exists(), "another database's files stay");
    assert_eq!(
        backup::list(&path),
        vec![backup.clone()],
        "only complete backups are listed"
    );
    assert_eq!(user_version(&backup), 1);
}

#[tokio::test]
async fn failed_backup_stops_the_migration() {
    let dir = TempDir::new().expect("temp dir");
    let path = db_path(&dir);
    drop(Store::open_with(&path, StoreOptions::default(), &test_schema(1)).expect("v1"));
    // A file where the backup directory should be makes the backup fail.
    fs::write(backup::backup_dir(&path), b"not a directory").expect("block backup dir");

    let err = Store::open_with(&path, StoreOptions::default(), &test_schema(2))
        .err()
        .expect("must fail");
    assert!(matches!(err, StoreError::Io { .. }), "{err:?}");
    assert_eq!(user_version(&path), 1, "not migrated without a backup");
    assert!(!table_exists(&path, "t2"));
}

// ---------------------------------------------------------------------------------------------
// Writer thread and reader pool
// ---------------------------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn readers_do_not_wait_for_the_writer() {
    let dir = TempDir::new().expect("temp dir");
    let store = Store::open(&db_path(&dir), StoreOptions::default()).expect("open");

    let (started_tx, started_rx) = tokio::sync::oneshot::channel::<()>();
    let (release_tx, release_rx) = mpsc::channel::<()>();
    let writer = {
        let store = store.clone();
        tokio::spawn(async move {
            store
                .write(move |tx| {
                    tx.execute(
                        "INSERT INTO setting (scope, key, value, updated_at) VALUES ('global', 'k', '1', 0)",
                        [],
                    )?;
                    let _ = started_tx.send(());
                    // Keep the write transaction open until the test has read.
                    release_rx
                        .recv_timeout(Duration::from_secs(10))
                        .expect("release signal");
                    Ok(())
                })
                .await
        })
    };

    started_rx.await.expect("write started");
    // The write transaction is open and uncommitted: a reader still answers, from its snapshot.
    let during = tokio::time::timeout(Duration::from_secs(5), store.settings(GLOBAL_SCOPE))
        .await
        .expect("read was blocked by the writer")
        .expect("read");
    assert!(during.is_empty());

    release_tx.send(()).expect("release");
    writer.await.expect("join").expect("write");
    let after = store.settings(GLOBAL_SCOPE).await.expect("read");
    assert_eq!(after.len(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_reads_and_writes_all_complete() {
    let dir = TempDir::new().expect("temp dir");
    let store = Store::open(&db_path(&dir), StoreOptions { readers: 3 }).expect("open");

    let mut tasks = tokio::task::JoinSet::new();
    for i in 0..50 {
        let s = store.clone();
        tasks.spawn(async move {
            s.put_settings(GLOBAL_SCOPE, vec![(format!("k{i:02}"), i.to_string())])
                .await
                .map(|()| 0)
        });
        let s = store.clone();
        tasks.spawn(async move { s.settings(GLOBAL_SCOPE).await.map(|v| v.len()) });
    }
    while let Some(result) = tasks.join_next().await {
        let seen = result.expect("join").expect("db call");
        assert!(seen <= 50);
    }
    let all = store.settings(GLOBAL_SCOPE).await.expect("read");
    assert_eq!(all.len(), 50);
    assert_eq!(all[0].key, "k00");
}

#[tokio::test]
async fn connections_use_the_decided_pragmas() {
    let dir = TempDir::new().expect("temp dir");
    let store = Store::open(&db_path(&dir), StoreOptions::default()).expect("open");

    let writer = store
        .write(|tx| {
            let q = |sql: &str| tx.query_row(sql, [], |r| r.get::<_, rusqlite::types::Value>(0));
            Ok([
                q("PRAGMA journal_mode")?,
                q("PRAGMA foreign_keys")?,
                q("PRAGMA synchronous")?,
                q("PRAGMA temp_store")?,
                q("PRAGMA cache_size")?,
                q("PRAGMA journal_size_limit")?,
                q("PRAGMA query_only")?,
            ])
        })
        .await
        .expect("writer pragmas");
    use rusqlite::types::Value::{Integer, Text};
    assert_eq!(
        writer,
        [
            Text("wal".into()),
            Integer(1),        // foreign_keys ON
            Integer(1),        // synchronous NORMAL
            Integer(2),        // temp_store MEMORY
            Integer(-65536),   // 64 MB
            Integer(67108864), // 64 MB WAL limit
            Integer(0),        // writer is not query_only
        ]
    );

    let (query_only, cache, write_attempt) = store
        .read(|conn| {
            let query_only: i64 = conn.query_row("PRAGMA query_only", [], |r| r.get(0))?;
            let cache: i64 = conn.query_row("PRAGMA cache_size", [], |r| r.get(0))?;
            let write_attempt = conn
                .execute(
                    "INSERT INTO setting (scope, key, value, updated_at) VALUES ('global', 'x', '1', 0)",
                    [],
                )
                .is_err();
            Ok((query_only, cache, write_attempt))
        })
        .await
        .expect("reader pragmas");
    assert_eq!(query_only, 1);
    assert_eq!(cache, -8192);
    assert!(write_attempt, "readers must not write");
}

#[tokio::test]
async fn panicking_job_does_not_stop_the_writer() {
    let dir = TempDir::new().expect("temp dir");
    let store = Store::open(&db_path(&dir), StoreOptions::default()).expect("open");

    let err = store
        .write::<(), _>(|_| panic!("boom in a job"))
        .await
        .expect_err("panic becomes an error");
    assert!(
        matches!(err, StoreError::JobPanicked(ref m) if m.contains("boom")),
        "{err:?}"
    );

    store
        .put_settings(GLOBAL_SCOPE, vec![("k".into(), "1".into())])
        .await
        .expect("writer still works");
}

// ---------------------------------------------------------------------------------------------
// Settings
// ---------------------------------------------------------------------------------------------

#[tokio::test]
async fn settings_keep_their_version_unless_the_value_changes() {
    let dir = TempDir::new().expect("temp dir");
    let store = Store::open(&db_path(&dir), StoreOptions::default()).expect("open");
    let put = |k: &str, v: &str| store.put_settings(GLOBAL_SCOPE, vec![(k.into(), v.into())]);

    put("a", "\"x\"").await.expect("write");
    put("a", "\"x\"").await.expect("same value");
    let first = store.settings(GLOBAL_SCOPE).await.expect("read");
    assert_eq!(first[0].version, 1);

    put("a", "\"y\"").await.expect("new value");
    let second = store.settings(GLOBAL_SCOPE).await.expect("read");
    assert_eq!(second[0].value, "\"y\"");
    assert_eq!(second[0].version, 2);

    assert!(
        store
            .settings("other-scope")
            .await
            .expect("read")
            .is_empty()
    );
}

#[tokio::test]
async fn settings_reject_invalid_json() {
    let dir = TempDir::new().expect("temp dir");
    let store = Store::open(&db_path(&dir), StoreOptions::default()).expect("open");
    let err = store
        .put_settings(GLOBAL_SCOPE, vec![("a".into(), "not json".into())])
        .await
        .expect_err("CHECK (json_valid(value))");
    assert_eq!(CoreError::from(err).code(), ErrorCode::Database);
    assert!(store.settings(GLOBAL_SCOPE).await.expect("read").is_empty());
}

/// Guards the assumptions of decision 0003 about the bundled SQLite.
#[test]
fn bundled_sqlite_has_required_features() {
    assert!(
        rusqlite::version_number() >= 3_045_000,
        "SQLite {} is older than 3.45",
        rusqlite::version()
    );
    let conn = Connection::open_in_memory().expect("open");
    conn.execute_batch(
        "CREATE VIRTUAL TABLE f USING fts5(a, content='', contentless_delete=1,
                                          tokenize=\"unicode61 remove_diacritics 2\");
         INSERT INTO f (rowid, a) VALUES (1, 'Grüße aus Köln');",
    )
    .expect("FTS5 with contentless_delete and unicode61");
    let hit: i64 = conn
        .query_row("SELECT rowid FROM f WHERE f MATCH 'koln'", [], |r| r.get(0))
        .expect("diacritics removed");
    assert_eq!(hit, 1);
}

#[tokio::test]
async fn closing_the_store_checkpoints_and_removes_the_wal() {
    // Repeated because the failure mode (connections closing at the same moment) is a race.
    for round in 0..20 {
        let dir = TempDir::new().expect("temp dir");
        let path = db_path(&dir);
        let store = Store::open(&path, StoreOptions { readers: 3 }).expect("open");
        if round % 2 == 0 {
            store
                .put_settings(GLOBAL_SCOPE, vec![("k".into(), "1".into())])
                .await
                .expect("write");
            store.settings(GLOBAL_SCOPE).await.expect("read");
        }
        drop(store);
        for suffix in ["-wal", "-shm"] {
            let leftover = path.with_file_name(format!("mail.sqlite3{suffix}"));
            assert!(
                !leftover.exists(),
                "round {round}: {suffix} left behind after close"
            );
        }
    }
}
