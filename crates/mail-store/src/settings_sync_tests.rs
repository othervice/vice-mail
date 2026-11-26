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

/// Stage 22: [`account_apply`] for the other test modules, each call a newer version.
pub(crate) async fn account_apply_for_tests(store: &Store, id: &str, value: Value) -> SyncApply {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1_000);
    let mut n = NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    account_apply(store, id, &mut n, value).await
}

/// A new version of the account record `id` from the folder, planned against the stored row.
async fn account_apply(store: &Store, id: &str, n: &mut u64, value: Value) -> SyncApply {
    let expected = store
        .sync_record("account", id)
        .await
        .expect("read")
        .map(|r| r.version);
    *n += 1;
    let version = format!("{}-00000000-x", 1_764_000_000_000 + *n);
    SyncApply {
        kind: "account".into(),
        id: id.to_owned(),
        expected,
        row: Some(SyncRecordRow {
            kind: "account".into(),
            id: id.to_owned(),
            version: version.clone(),
            synced_version: Some(version),
            content: Some(value.clone()),
            deleted_version: None,
        }),
        table: TableWrite::Put(value),
        rekey_from: None,
    }
}

/// Decision 0040 (finding F1): a record that names other servers for an existing account does
/// not change the servers this device uses; the change waits for the user, and the sync keeps
/// seeing the folder's servers so no local change sends this device's back.
#[tokio::test]
async fn other_servers_from_the_folder_wait_for_the_user() {
    let (_dir, store, recorder) = open().await;
    let local = store
        .insert_account(new_account(1, "ann@example.org"))
        .await
        .expect("account");
    let id = local.id.to_string();
    recorder.take();
    let server = |host: &str, port: u16| json!({ "host": host, "port": port, "security": "tls", "username": "ann@example.org" });
    let content = |imap: Value, name: &str| {
        json!({ "email": "ann@example.org", "displayName": name, "color": 3,
                "imap": imap, "smtp": server("smtp.example.org", 465) })
    };
    let mut n = 0;
    let evil = server("imap.attacker.example", 993);
    let outcome = store
        .apply_sync(
            vec![account_apply(&store, &id, &mut n, content(evil.clone(), "Ann")).await],
            None,
        )
        .await
        .expect("apply");
    assert_eq!(outcome.accounts_server_changed, [local.id]);
    let here = store
        .account(local.id)
        .await
        .expect("read")
        .expect("account");
    assert_eq!(here.imap.host, "imap.example.org", "servers kept");
    assert_eq!(here.display_name, "Ann", "the other fields apply");
    let waiting = store.account_server_changes().await.expect("changes");
    assert_eq!(waiting.len(), 1);
    assert_eq!(waiting[0].imap.host, "imap.attacker.example");
    // The sync's view: the folder's servers, so the record and the table agree.
    let seen = store
        .sync_table_content("account", &id)
        .await
        .expect("content")
        .expect("account");
    assert_eq!(seen["imap"], evil);
    assert!(recorder.take().is_empty(), "applying is not a local change");

    // A local change of the account (writing switched on) keeps the record's version: this
    // device's servers do not go back to the folder.
    let before = store
        .sync_record("account", &id)
        .await
        .expect("read")
        .expect("row");
    store
        .set_writes_enabled(local.id, true)
        .await
        .expect("writes on");
    let after = store
        .sync_record("account", &id)
        .await
        .expect("read")
        .expect("row");
    assert_eq!(after.version, before.version);

    // Dismissed: not offered again for the same change, offered for a different one.
    store
        .dismiss_account_server_change(local.id)
        .await
        .expect("dismiss");
    assert!(
        store
            .account_server_changes()
            .await
            .expect("changes")
            .is_empty()
    );
    assert!(matches!(
        store.dismiss_account_server_change(local.id).await,
        Err(crate::StoreError::NotFound(_))
    ));
    store
        .apply_sync(
            vec![account_apply(&store, &id, &mut n, content(evil.clone(), "Ann B.")).await],
            None,
        )
        .await
        .expect("apply");
    assert!(
        store
            .account_server_changes()
            .await
            .expect("changes")
            .is_empty(),
        "the same change stays dismissed"
    );
    let other = server("imap.attacker.example", 143);
    store
        .apply_sync(
            vec![account_apply(&store, &id, &mut n, content(other.clone(), "Ann B.")).await],
            None,
        )
        .await
        .expect("apply");
    let waiting = store.account_server_changes().await.expect("changes");
    assert_eq!(waiting.len(), 1, "a different change is offered");
    assert_eq!(waiting[0].imap.port, 143);

    // Confirmed: the folder's servers, writing off, the account needs its password again; the
    // record keeps its version (table and folder agree now).
    let before = store
        .sync_record("account", &id)
        .await
        .expect("read")
        .expect("row");
    let confirmed = store
        .confirm_account_server_change(local.id)
        .await
        .expect("confirm");
    assert_eq!(confirmed.imap.host, "imap.attacker.example");
    assert_eq!(confirmed.imap.port, 143);
    assert!(!confirmed.writes_enabled);
    assert_eq!(confirmed.credential_ref, local.credential_ref);
    assert_eq!(
        store.sync_adopted_accounts().await.expect("adopted"),
        [local.id]
    );
    assert!(
        store
            .account_server_changes()
            .await
            .expect("changes")
            .is_empty()
    );
    let after = store
        .sync_record("account", &id)
        .await
        .expect("read")
        .expect("row");
    assert_eq!(after.version, before.version);
    assert!(matches!(
        store.confirm_account_server_change(local.id).await,
        Err(crate::StoreError::NotFound(_))
    ));

    // The folder agrees with this device again: nothing waits.
    store
        .apply_sync(
            vec![account_apply(&store, &id, &mut n, content(evil.clone(), "Ann")).await],
            None,
        )
        .await
        .expect("apply");
    assert_eq!(
        store.account_server_changes().await.expect("changes").len(),
        1
    );
    store
        .apply_sync(
            vec![account_apply(&store, &id, &mut n, content(other, "Ann")).await],
            None,
        )
        .await
        .expect("apply");
    assert!(
        store
            .account_server_changes()
            .await
            .expect("changes")
            .is_empty()
    );
}

/// Decision 0040: an account taken over from the folder gets a keychain entry name of its own,
/// never one this device may already hold (its UUID).
#[tokio::test]
async fn adopted_accounts_get_a_fresh_keychain_reference() {
    let (_dir, store, _recorder) = open().await;
    let id = account_id(7).to_string();
    let server = json!({ "host": "imap.example.org", "port": 993, "security": "tls",
                         "username": "bob@example.org" });
    let content = json!({ "email": "bob@example.org", "displayName": "Bob", "color": 1,
                          "imap": server, "smtp": server });
    let outcome = store
        .apply_sync(
            vec![SyncApply {
                kind: "account".into(),
                id: id.clone(),
                expected: None,
                row: Some(SyncRecordRow {
                    kind: "account".into(),
                    id: id.clone(),
                    version: "1764000000000-00000000-x".into(),
                    synced_version: Some("1764000000000-00000000-x".into()),
                    content: Some(content.clone()),
                    deleted_version: None,
                }),
                table: TableWrite::Put(content),
                rekey_from: None,
            }],
            None,
        )
        .await
        .expect("apply");
    assert_eq!(outcome.accounts_added, [account_id(7)]);
    let adopted = store
        .account(account_id(7))
        .await
        .expect("read")
        .expect("account");
    assert_ne!(adopted.credential_ref, id);
    assert!(
        adopted.credential_ref.starts_with("synced:") && adopted.credential_ref.len() == 39,
        "{}",
        adopted.credential_ref
    );
    assert!(!adopted.writes_enabled);
    assert_eq!(
        store.sync_adopted_accounts().await.expect("adopted"),
        [account_id(7)]
    );
}

/// Stage 14 (decision 0042): an account that signs in with its provider names it in its record
/// (never a token), and a device that takes it over keeps the sign-in kind under an entry name
/// of its own, without a token.
#[tokio::test]
async fn accounts_that_sign_in_with_a_provider_say_so_and_are_taken_over_as_such() {
    let (_dir, store, _recorder) = open().await;
    let apply = |id: String, content: Value| SyncApply {
        kind: "account".into(),
        id: id.clone(),
        expected: None,
        row: Some(SyncRecordRow {
            kind: "account".into(),
            id,
            version: "1764000000000-00000000-x".into(),
            synced_version: Some("1764000000000-00000000-x".into()),
            content: Some(content.clone()),
            deleted_version: None,
        }),
        table: TableWrite::Put(content),
        rekey_from: None,
    };
    let server = json!({ "host": "outlook.office365.com", "port": 993, "security": "tls",
                         "username": "ada@outlook.com" });
    let content = json!({ "email": "ada@outlook.com", "displayName": "Ada", "color": 2,
                          "imap": server, "smtp": server, "oauth": "microsoft" });
    let outcome = store
        .apply_sync(vec![apply(account_id(8).to_string(), content)], None)
        .await
        .expect("apply");
    assert_eq!(outcome.accounts_added, [account_id(8)]);
    let adopted = store
        .account(account_id(8))
        .await
        .expect("read")
        .expect("account");
    assert!(
        adopted
            .credential_ref
            .starts_with("oauth:microsoft:synced:"),
        "{}",
        adopted.credential_ref
    );
    assert!(!adopted.writes_enabled);
    assert_eq!(
        adopted
            .into_account(mail_model::AccountStatus::connecting())
            .oauth,
        Some(mail_model::OAuthProvider::Microsoft)
    );
    // Its record names the provider, and only the provider.
    let record = store
        .sync_table_content("account", &account_id(8).to_string())
        .await
        .expect("content")
        .expect("record");
    assert_eq!(record["oauth"], "microsoft");

    // Unknown providers are refused; nothing is added.
    let bad = json!({ "email": "x@example.org", "displayName": "", "color": 0,
                      "imap": server, "smtp": server, "oauth": "yahoo" });
    let outcome = store
        .apply_sync(vec![apply(account_id(9).to_string(), bad)], None)
        .await
        .expect("apply");
    assert!(outcome.accounts_added.is_empty());
    assert!(store.account(account_id(9)).await.expect("read").is_none());

    // A password account's record has no such field.
    let password = store
        .insert_account(new_account(3, "carol@example.org"))
        .await
        .expect("insert");
    let record = store
        .sync_table_content("account", &password.id.to_string())
        .await
        .expect("content")
        .expect("record");
    assert!(record.get("oauth").is_none(), "{record}");
}

/// Stage 14: a JMAP account that signs in with an API token says so in its record (never the
/// token) and is taken over as such, under a `token:` reference of its own; the kind needs JMAP.
#[tokio::test]
async fn api_token_accounts_say_so_and_are_taken_over_as_such() {
    let (_dir, store, _recorder) = open().await;
    let apply = |id: String, content: Value| SyncApply {
        kind: "account".into(),
        id: id.clone(),
        expected: None,
        row: Some(SyncRecordRow {
            kind: "account".into(),
            id,
            version: "1764000000000-00000000-x".into(),
            synced_version: Some("1764000000000-00000000-x".into()),
            content: Some(content.clone()),
            deleted_version: None,
        }),
        table: TableWrite::Put(content),
        rekey_from: None,
    };
    let server = json!({ "host": "api.fastmail.com", "port": 443, "security": "tls",
                         "username": "ada@fastmail.com" });
    let content = json!({ "email": "ada@fastmail.com", "displayName": "Ada", "color": 2,
                          "imap": server, "smtp": server, "protocol": "jmap",
                          "signIn": "apiToken" });
    let outcome = store
        .apply_sync(vec![apply(account_id(8).to_string(), content)], None)
        .await
        .expect("apply");
    assert_eq!(outcome.accounts_added, [account_id(8)]);
    let adopted = store
        .account(account_id(8))
        .await
        .expect("read")
        .expect("account");
    assert!(
        adopted.credential_ref.starts_with("token:synced:"),
        "{}",
        adopted.credential_ref
    );
    assert!(!adopted.writes_enabled);
    let account = adopted.into_account(mail_model::AccountStatus::connecting());
    assert_eq!(account.credential, mail_model::CredentialKind::ApiToken);
    assert_eq!(account.protocol, mail_model::Protocol::Jmap);
    assert_eq!(account.oauth, None);
    let record = store
        .sync_table_content("account", &account_id(8).to_string())
        .await
        .expect("content")
        .expect("record");
    assert_eq!(record["signIn"], "apiToken");
    assert_eq!(record["protocol"], "jmap");

    // Not for IMAP accounts, not with a provider sign-in, no unknown kinds: nothing is added.
    for (n, extra) in [
        (9, json!({ "signIn": "apiToken" })),
        (
            10,
            json!({ "signIn": "apiToken", "protocol": "jmap", "oauth": "google" }),
        ),
        (
            11,
            json!({ "signIn": "clientCertificate", "protocol": "jmap" }),
        ),
    ] {
        let mut bad = json!({ "email": format!("x{n}@example.org"), "displayName": "",
                              "color": 0, "imap": server, "smtp": server });
        for (key, value) in extra.as_object().expect("object") {
            bad[key] = value.clone();
        }
        let outcome = store
            .apply_sync(vec![apply(account_id(n).to_string(), bad)], None)
            .await
            .expect("apply");
        assert!(outcome.accounts_added.is_empty(), "{n}");
        assert!(store.account(account_id(n)).await.expect("read").is_none());
    }

    // A password account's record has no such field.
    let password = store
        .insert_account(new_account(3, "carol@example.org"))
        .await
        .expect("insert");
    let record = store
        .sync_table_content("account", &password.id.to_string())
        .await
        .expect("content")
        .expect("record");
    assert!(record.get("signIn").is_none(), "{record}");
}
