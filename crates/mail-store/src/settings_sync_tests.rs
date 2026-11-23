//! Stage 11: change capture and applying records, with a recording hook instead of the
//! engine's clock (the engine tests the real hook with every kind).

use std::sync::{Arc, Mutex, PoisonError};

use serde_json::{Value, json};
use tempfile::TempDir;

use crate::test_support::{account_id, db_path, new_account};
use crate::{
    CaptureOutcome, CapturedChange, GLOBAL_SCOPE, Store, StoreOptions, SyncApply, SyncCapture,
    SyncRecordRow, TableWrite,
};

/// Versions every change with a counter, like the engine but without merge logic: content
/// that equals the stored content keeps its version, a gone row becomes a tombstone.
#[derive(Default)]
struct Recorder {
    seen: Mutex<Vec<CapturedChange>>,
    counter: Mutex<u64>,
}

impl SyncCapture for Recorder {
    fn capture(&self, changes: Vec<CapturedChange>) -> CaptureOutcome {
        let mut counter = self.counter.lock().unwrap_or_else(PoisonError::into_inner);
        let mut rows = Vec::new();
        for change in &changes {
            let unchanged = change
                .stored
                .as_ref()
                .is_some_and(|s| s.content == change.current);
            if unchanged || (change.current.is_none() && change.stored.is_none()) {
                continue;
            }
            *counter += 1;
            let version = format!("{:013}", *counter);
            rows.push(SyncRecordRow {
                kind: change.kind.clone(),
                id: change.id.clone(),
                deleted_version: change
                    .current
                    .is_none()
                    .then(|| change.stored.as_ref().map(|s| s.version.clone()))
                    .flatten(),
                content: change.current.clone(),
                version,
                synced_version: change
                    .stored
                    .as_ref()
                    .and_then(|s| s.synced_version.clone()),
            });
        }
        self.seen
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .extend(changes);
        CaptureOutcome {
            rows,
            hlc_last: Some(format!("{:013}", *counter)),
        }
    }
}

impl Recorder {
    fn take(&self) -> Vec<(String, String)> {
        let mut seen = self.seen.lock().unwrap_or_else(PoisonError::into_inner);
        let mut out: Vec<(String, String)> = seen.drain(..).map(|c| (c.kind, c.id)).collect();
        out.sort();
        out
    }
}

async fn open() -> (TempDir, Store, Arc<Recorder>) {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = Store::open(&db_path(&dir), StoreOptions::default()).expect("open");
    let recorder = Arc::new(Recorder::default());
    store.set_sync_capture(Arc::clone(&recorder) as Arc<dyn SyncCapture>);
    store.flush_sync_capture().await.expect("flush");
    recorder.take();
    (dir, store, recorder)
}

fn key(kind: &str, id: &str) -> (String, String) {
    (kind.to_owned(), id.to_owned())
}

#[tokio::test]
async fn every_write_path_marks_its_record_in_the_same_transaction() {
    let (_dir, store, recorder) = open().await;
    let account = store
        .insert_account(new_account(1, "ann@example.org"))
        .await
        .expect("account");
    let a = account.id.to_string();
    let marked = recorder.take();
    assert!(marked.contains(&key("account", &a)), "{marked:?}");
    assert!(
        marked.iter().any(|(k, _)| k == "identity"),
        "the default identity: {marked:?}"
    );
    let record = store
        .sync_record("account", &a)
        .await
        .expect("read")
        .expect("captured");
    assert_eq!(
        record.content.as_ref().map(|c| &c["email"]),
        Some(&json!("ann@example.org"))
    );

    store
        .put_settings(GLOBAL_SCOPE, vec![("ui.theme".into(), "\"dark\"".into())])
        .await
        .expect("setting");
    assert_eq!(recorder.take(), [key("setting", "global/ui.theme")]);
    let setting = store
        .sync_record("setting", "global/ui.theme")
        .await
        .expect("read")
        .expect("row");
    assert_eq!(setting.content, Some(json!("dark")));

    // writes_enabled is no synchronised field: marked, but the version stays.
    store
        .write(move |tx| {
            tx.execute("UPDATE account SET writes_enabled = 1", [])?;
            Ok(())
        })
        .await
        .expect("writes on");
    assert_eq!(recorder.take(), [key("account", &a)]);
    let after = store
        .sync_record("account", &a)
        .await
        .expect("read")
        .expect("row");
    assert_eq!(after.version, record.version);

    // Deleting the account cascades to its identity: both become tombstones.
    let identity = store.sync_records().await.expect("rows");
    let identity = identity
        .iter()
        .find(|r| r.kind == "identity")
        .expect("identity")
        .id
        .clone();
    store.delete_account(account.id).await.expect("delete");
    let marked = recorder.take();
    assert!(marked.contains(&key("account", &a)), "{marked:?}");
    assert!(
        marked.contains(&key("identity", &identity)),
        "cascade: {marked:?}"
    );
    let gone = store
        .sync_record("identity", &identity)
        .await
        .expect("read")
        .expect("tombstone");
    assert!(gone.content.is_none());
    assert!(gone.deleted_version.is_some());
}

#[tokio::test]
async fn applied_records_leave_no_mark_and_respect_local_changes() {
    let (_dir, store, recorder) = open().await;
    let row = |version: &str, value: Value| SyncRecordRow {
        kind: "setting".into(),
        id: "global/ui.language".into(),
        version: version.into(),
        synced_version: Some(version.into()),
        content: Some(value),
        deleted_version: None,
    };
    let outcome = store
        .apply_sync(
            vec![SyncApply {
                kind: "setting".into(),
                id: "global/ui.language".into(),
                expected: None,
                row: Some(row("1764000000000-00000000-x", json!("de"))),
                table: TableWrite::Put(json!("de")),
                rekey_from: None,
            }],
            None,
        )
        .await
        .expect("apply");
    assert_eq!(outcome.applied.len(), 1);
    assert!(recorder.take().is_empty(), "applying is not a local change");
    let settings = store.settings(GLOBAL_SCOPE).await.expect("settings");
    assert_eq!(settings[0].value, "\"de\"");

    // A local change after the pass read the record: the pass's version is not applied.
    store
        .put_settings(GLOBAL_SCOPE, vec![("ui.language".into(), "\"en\"".into())])
        .await
        .expect("local");
    let outcome = store
        .apply_sync(
            vec![SyncApply {
                kind: "setting".into(),
                id: "global/ui.language".into(),
                expected: Some("1764000000000-00000000-x".into()),
                row: Some(row("1764000000001-00000000-x", json!("fr"))),
                table: TableWrite::Put(json!("fr")),
                rekey_from: None,
            }],
            None,
        )
        .await
        .expect("apply");
    assert!(outcome.applied.is_empty());
    assert_eq!(outcome.skipped.len(), 1);
    let settings = store.settings(GLOBAL_SCOPE).await.expect("settings");
    assert_eq!(settings[0].value, "\"en\"");
}

#[tokio::test]
async fn records_of_unknown_accounts_are_skipped_and_accounts_rekeyed() {
    let (_dir, store, _recorder) = open().await;
    let remote = account_id(9).to_string();
    let snooze = format!("{remote}/m1@example.org");
    let outcome = store
        .apply_sync(
            vec![SyncApply {
                kind: "snooze".into(),
                id: snooze.clone(),
                expected: None,
                row: None,
                table: TableWrite::Put(json!({ "untilMs": 5 })),
                rekey_from: None,
            }],
            None,
        )
        .await
        .expect("apply");
    assert_eq!(outcome.skipped.len(), 1, "no such account here");

    // The same address set up here before joining: the local account takes the remote id.
    let local = store
        .insert_account(new_account(1, "ann@example.org"))
        .await
        .expect("account");
    let server = json!({ "host": "imap.example.org", "port": 993, "security": "tls",
                         "username": "ann@example.org" });
    let content = json!({ "email": "ann@example.org", "displayName": "Ann", "color": 3,
                          "imap": server, "smtp": server });
    let outcome = store
        .apply_sync(
            vec![SyncApply {
                kind: "account".into(),
                id: remote.clone(),
                expected: None,
                row: Some(SyncRecordRow {
                    kind: "account".into(),
                    id: remote.clone(),
                    version: "1764000000000-00000000-x".into(),
                    synced_version: Some("1764000000000-00000000-x".into()),
                    content: Some(content.clone()),
                    deleted_version: None,
                }),
                table: TableWrite::Put(content),
                rekey_from: Some(local.id.to_string()),
            }],
            None,
        )
        .await
        .expect("apply");
    assert_eq!(outcome.accounts_rekeyed, [(local.id, account_id(9))]);
    let accounts = store.accounts().await.expect("accounts");
    assert_eq!(accounts.len(), 1);
    assert_eq!(accounts[0].id, account_id(9));
    assert_eq!(accounts[0].display_name, "Ann");
    assert_eq!(
        accounts[0].credential_ref, local.credential_ref,
        "keychain entry kept"
    );
    assert!(
        store
            .sync_record("account", &local.id.to_string())
            .await
            .expect("read")
            .is_none(),
        "the never-synced local record is gone"
    );
}
