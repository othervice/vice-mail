//! The vault against a fake keychain that behaves like macOS with ad-hoc signed builds: every
//! item remembers the build that created it and the builds the user allowed; a read by any
//! other build shows a dialog (counted, answered as the test says), and depending on the model
//! a build may or may not change or delete an item another build created.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex, PoisonError};

use mail_model::{CoreError, ErrorCode, IpcError, Secret};

use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Answer {
    AlwaysAllow,
    AllowOnce,
    Deny,
}

#[derive(Debug, Clone)]
struct Item {
    value: String,
    creator: u32,
    trusted: BTreeSet<u32>,
}

#[derive(Debug)]
struct Keychain {
    items: BTreeMap<String, Item>,
    /// The build that runs now.
    build: u32,
    answer: Answer,
    /// Names whose dialog is answered with "Deny" whatever `answer` says.
    deny: BTreeSet<String>,
    /// Every dialog, by item name.
    prompts: Vec<String>,
    /// Whether a build may change or delete an item another build created (after the user
    /// allowed it to read the item). `false` is the pessimistic model ("Invalid attempt to
    /// change the owner of this item").
    foreign_edits: bool,
    /// The next this many writes store a damaged value (verification must notice).
    damage: usize,
    /// Every write fails while set.
    writes_fail: bool,
}

#[derive(Debug, Clone)]
struct FakeKeychain(Arc<Mutex<Keychain>>);

impl FakeKeychain {
    fn new(foreign_edits: bool) -> Self {
        Self(Arc::new(Mutex::new(Keychain {
            items: BTreeMap::new(),
            build: 1,
            answer: Answer::AlwaysAllow,
            deny: BTreeSet::new(),
            prompts: Vec::new(),
            foreign_edits,
            damage: 0,
            writes_fail: false,
        })))
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Keychain> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// An update: another code signature.
    fn update_to(&self, build: u32) {
        self.lock().build = build;
    }

    fn prompts(&self) -> Vec<String> {
        self.lock().prompts.clone()
    }

    fn clear_prompts(&self) {
        self.lock().prompts.clear();
    }

    fn names_now(&self) -> Vec<String> {
        self.lock().items.keys().cloned().collect()
    }

    /// The vault items in the keychain.
    fn vaults(&self) -> Vec<String> {
        self.names_now()
            .into_iter()
            .filter(|n| generation_of(n).is_some())
            .collect()
    }

    fn raw(&self, name: &str) -> Option<String> {
        self.lock().items.get(name).map(|i| i.value.clone())
    }

    fn put_raw(&self, name: &str, value: &str) {
        let mut k = self.lock();
        let build = k.build;
        k.items.insert(
            name.to_owned(),
            Item {
                value: value.to_owned(),
                creator: build,
                trusted: BTreeSet::from([build]),
            },
        );
    }

    /// The dialog for `name` if the running build is not trusted for it.
    fn check_read(k: &mut Keychain, name: &str) -> Result<(), CoreError> {
        let build = k.build;
        let answer = if k.deny.contains(name) {
            Answer::Deny
        } else {
            k.answer
        };
        let Some(item) = k.items.get_mut(name) else {
            return Ok(());
        };
        if item.trusted.contains(&build) {
            return Ok(());
        }
        k.prompts.push(name.to_owned());
        match answer {
            Answer::AlwaysAllow => {
                item.trusted.insert(build);
                Ok(())
            }
            Answer::AllowOnce => Ok(()),
            Answer::Deny => Err(CoreError::Keychain("the user denied access".into())),
        }
    }

    fn may_edit(k: &Keychain, name: &str) -> Result<(), CoreError> {
        match k.items.get(name) {
            Some(item) if item.creator != k.build && !k.foreign_edits => Err(CoreError::Keychain(
                "no access to the keychain: Invalid attempt to change the owner of this item"
                    .into(),
            )),
            _ => Ok(()),
        }
    }
}

impl ItemStore for FakeKeychain {
    fn read(&self, name: &str) -> Result<Option<Secret<String>>, CoreError> {
        let mut k = self.lock();
        Self::check_read(&mut k, name)?;
        Ok(k.items.get(name).map(|i| Secret::new(i.value.clone())))
    }

    fn write(&self, name: &str, value: &Secret<String>) -> Result<(), CoreError> {
        let mut k = self.lock();
        if k.writes_fail {
            return Err(CoreError::Keychain("injected write failure".into()));
        }
        // Like `set_generic_password`: find the item with its data first, then change it.
        Self::check_read(&mut k, name)?;
        Self::may_edit(&k, name)?;
        let stored = if k.damage > 0 {
            k.damage -= 1;
            value.expose_secret().chars().take(3).collect()
        } else {
            value.expose_secret().clone()
        };
        let build = k.build;
        match k.items.get_mut(name) {
            Some(item) => item.value = stored,
            None => {
                k.items.insert(
                    name.to_owned(),
                    Item {
                        value: stored,
                        creator: build,
                        trusted: BTreeSet::from([build]),
                    },
                );
            }
        }
        Ok(())
    }

    fn remove(&self, name: &str) -> Result<bool, CoreError> {
        let mut k = self.lock();
        if !k.items.contains_key(name) {
            return Ok(false);
        }
        Self::may_edit(&k, name)?;
        k.items.remove(name);
        Ok(true)
    }

    fn names(&self) -> Result<Vec<String>, CoreError> {
        Ok(self.names_now())
    }
}

const SECRETS: [(&str, &str); 6] = [
    ("6f9619ff-8b86-4d11-b42d-00c04fc964ff", "first password 1%"),
    (
        "synced:00112233445566778899aabbccddeeff",
        "second \"quoted\" password",
    ),
    (
        "oauth:google:7c9e6679-7425-40de-944b-e07fc1f90ae7",
        "{\"v\":1}",
    ),
    ("mcp-client:abc", "token with ünïcödé"),
    ("ai-profile:p1", "sk-not-a-real-key"),
    ("settings-sync:key", "AGE-SECRET-KEY-1NOTAREALKEY"),
];

fn key(name: &str) -> CredentialRef {
    CredentialRef::parse(name).expect("ref")
}

fn names() -> Vec<CredentialRef> {
    SECRETS.iter().map(|(n, _)| key(n)).collect()
}

fn secret(value: &str) -> Secret<String> {
    Secret::new(value.to_owned())
}

fn read(store: &VaultStore<FakeKeychain>, name: &str) -> Option<String> {
    store
        .get(&key(name))
        .expect("get")
        .map(Secret::into_exposed)
}

/// A store with every secret in it, written by build 1.
fn filled(foreign_edits: bool) -> (FakeKeychain, VaultStore<FakeKeychain>) {
    let keychain = FakeKeychain::new(foreign_edits);
    let store = VaultStore::new(keychain.clone());
    for (name, value) in SECRETS {
        store.set(&key(name), &secret(value)).expect("set");
    }
    (keychain, store)
}

#[test]
fn every_secret_lives_in_one_item() {
    let (keychain, store) = filled(true);
    assert_eq!(keychain.names_now(), keychain.vaults());
    assert_eq!(keychain.vaults().len(), 1);
    for (name, value) in SECRETS {
        assert_eq!(read(&store, name).as_deref(), Some(value));
    }
    // Replace, delete, and what a restart reads back.
    store
        .set(&key(SECRETS[0].0), &secret("changed"))
        .expect("replace");
    assert!(store.delete(&key(SECRETS[1].0)).expect("delete"));
    assert!(!store.delete(&key(SECRETS[1].0)).expect("delete again"));
    let again = VaultStore::new(keychain.clone());
    assert_eq!(read(&again, SECRETS[0].0).as_deref(), Some("changed"));
    assert_eq!(read(&again, SECRETS[1].0), None);
    assert_eq!(read(&again, SECRETS[2].0).as_deref(), Some(SECRETS[2].1));
    assert_eq!(keychain.names_now(), keychain.vaults());
    assert_eq!(keychain.vaults().len(), 1, "older generations are gone");
    assert!(keychain.prompts().is_empty(), "the same build never asks");

    // Empty again: nothing of the app stays in the keychain.
    for (name, _) in SECRETS {
        again.delete(&key(name)).expect("delete");
    }
    assert!(keychain.names_now().is_empty());
    assert_eq!(read(&again, SECRETS[2].0), None);

    // Empty secrets and the vault's own names are refused.
    assert_eq!(
        again.set(&key("x"), &secret("")).expect_err("empty").code(),
        ErrorCode::InvalidArgument
    );
    for reserved in ["vault", "vault.2"] {
        assert_eq!(
            again
                .set(&key(reserved), &secret("x"))
                .expect_err("reserved")
                .code(),
            ErrorCode::InvalidArgument
        );
        assert!(again.get(&key(reserved)).expect("get").is_none());
    }
}

#[test]
fn an_update_asks_once_whatever_macos_allows_a_new_build() {
    for foreign_edits in [true, false] {
        let (keychain, store) = filled(foreign_edits);
        drop(store);
        for build in 2..=4 {
            keychain.update_to(build);
            keychain.clear_prompts();
            let store = VaultStore::new(keychain.clone());
            for (name, value) in SECRETS {
                assert_eq!(read(&store, name).as_deref(), Some(value), "build {build}");
            }
            // A change after the update (a new account, a renewed sign-in).
            let name = format!("account-of-build-{build}");
            store
                .set(&key(&name), &secret("new"))
                .expect("set after update");
            store.delete(&key(SECRETS[0].0)).expect("delete");
            store
                .set(&key(SECRETS[0].0), &secret(SECRETS[0].1))
                .expect("set again");
            assert_eq!(
                keychain.prompts().len(),
                1,
                "build {build}, foreign edits {foreign_edits}: {:?}",
                keychain.prompts()
            );
            let restarted = VaultStore::new(keychain.clone());
            assert_eq!(read(&restarted, &name).as_deref(), Some("new"));
            assert_eq!(keychain.prompts().len(), 1, "a restart asks nothing more");
        }
        let vaults = keychain.vaults();
        if foreign_edits {
            assert_eq!(vaults.len(), 1, "{vaults:?}");
        } else {
            // The item each earlier build wrote last stays (macOS keeps it for its build) but is
            // never read again.
            assert_eq!(vaults.len(), 4, "{vaults:?}");
        }
    }
}

#[test]
fn the_migration_moves_old_items_and_deletes_them_after_the_vault_is_verified() {
    for foreign_edits in [true, false] {
        let keychain = FakeKeychain::new(foreign_edits);
        // 0.1.2: one item per secret, plus a leftover nobody knows of any more.
        for (name, value) in SECRETS {
            keychain.put_raw(name, value);
        }
        keychain.put_raw("leftover-of-a-removed-account", "old");
        keychain.update_to(2);
        let store = VaultStore::new(keychain.clone());
        let mut known = names();
        known.push(key("oauth:google:7c9e6679-7425-40de-944b-e07fc1f90ae7#1")); // absent piece
        let report = store.migrate(&known).expect("migrate");
        assert_eq!(report.moved, SECRETS.len());
        assert_eq!(report.failed, 0);
        assert_eq!(
            keychain.prompts().len(),
            SECRETS.len(),
            "one last dialog per old item, none for absent names or leftovers"
        );
        let old: Vec<String> = keychain
            .names_now()
            .into_iter()
            .filter(|n| generation_of(n).is_none())
            .collect();
        assert_eq!(keychain.vaults().len(), 1);
        if foreign_edits {
            assert_eq!(report.kept_old, 0);
            assert_eq!(old, ["leftover-of-a-removed-account"]);
        } else {
            assert_eq!(report.kept_old, SECRETS.len());
            assert_eq!(old.len(), SECRETS.len() + 1);
        }
        // Everything reads from the vault now; a second migration has nothing to do.
        keychain.clear_prompts();
        for (name, value) in SECRETS {
            assert_eq!(read(&store, name).as_deref(), Some(value));
        }
        assert_eq!(
            store.migrate(&known).expect("again"),
            MigrationReport::default()
        );
        assert!(keychain.prompts().is_empty());

        // A secret deleted through the vault never comes back from an old item that stayed.
        assert!(store.delete(&key(SECRETS[1].0)).expect("delete"));
        let restarted = VaultStore::new(keychain.clone());
        assert_eq!(read(&restarted, SECRETS[1].0), None);
        assert_eq!(
            read(&restarted, SECRETS[0].0).as_deref(),
            Some(SECRETS[0].1)
        );
        // Nor does a changed one show its earlier value.
        restarted
            .set(&key(SECRETS[0].0), &secret("changed"))
            .expect("set");
        assert_eq!(
            read(&VaultStore::new(keychain.clone()), SECRETS[0].0).as_deref(),
            Some("changed")
        );
    }
}

#[test]
fn a_denied_dialog_or_a_failed_write_never_loses_a_secret() {
    let keychain = FakeKeychain::new(false);
    for (name, value) in SECRETS {
        keychain.put_raw(name, value);
    }
    keychain.update_to(2);

    // The user denies one item: the others move, that one stays where it was.
    keychain.lock().deny.insert(SECRETS[2].0.to_owned());
    let store = VaultStore::new(keychain.clone());
    let report = store.migrate(&names()).expect("migrate");
    assert_eq!((report.moved, report.failed), (SECRETS.len() - 1, 1));
    assert_eq!(keychain.raw(SECRETS[2].0).as_deref(), Some(SECRETS[2].1));
    let err = store.get(&key(SECRETS[2].0)).expect_err("still denied");
    assert_eq!(err.code(), ErrorCode::Keychain);
    // Allowed later: moved on the next read.
    keychain.lock().deny.clear();
    assert_eq!(read(&store, SECRETS[2].0).as_deref(), Some(SECRETS[2].1));
    let restarted = VaultStore::new(keychain.clone());
    keychain.clear_prompts();
    for (name, value) in SECRETS {
        assert_eq!(read(&restarted, name).as_deref(), Some(value));
    }
    assert!(keychain.prompts().is_empty());

    // A crash before the vault is written (here: every write fails): the old items stay and
    // are read as before, by the next start too.
    let keychain = FakeKeychain::new(true);
    for (name, value) in SECRETS {
        keychain.put_raw(name, value);
    }
    keychain.update_to(2);
    keychain.lock().writes_fail = true;
    let store = VaultStore::new(keychain.clone());
    assert_eq!(
        store.migrate(&names()).expect_err("write fails").code(),
        ErrorCode::Keychain
    );
    for (name, value) in SECRETS {
        assert_eq!(keychain.raw(name).as_deref(), Some(value), "kept");
        assert_eq!(read(&store, name).as_deref(), Some(value), "still readable");
    }
    keychain.lock().writes_fail = false;
    let restarted = VaultStore::new(keychain.clone());
    let report = restarted.migrate(&names()).expect("migrate");
    assert_eq!(report.moved, SECRETS.len());
    assert_eq!(keychain.names_now(), keychain.vaults());
    assert_eq!(keychain.vaults().len(), 1);
}

#[test]
fn a_write_counts_only_when_it_reads_back_identical() {
    let (keychain, store) = filled(true);
    let before = keychain.vaults();
    // The keychain keeps a damaged value: the write fails, the damaged item is removed, and
    // what was stored before stays the newest.
    keychain.lock().damage = 1;
    let err = store
        .set(&key("new"), &secret("value"))
        .expect_err("damaged");
    assert_eq!(err.code(), ErrorCode::Keychain);
    assert_eq!(keychain.vaults(), before);
    assert_eq!(read(&store, "new"), None);
    let restarted = VaultStore::new(keychain.clone());
    assert_eq!(read(&restarted, "new"), None);
    for (name, value) in SECRETS {
        assert_eq!(read(&restarted, name).as_deref(), Some(value));
    }
    // The next write works.
    restarted.set(&key("new"), &secret("value")).expect("set");
    assert_eq!(
        read(&VaultStore::new(keychain), "new").as_deref(),
        Some("value")
    );
}

#[test]
fn an_unreadable_or_newer_vault_is_never_overwritten() {
    for (content, newer) in [
        ("not json at all, first password 1%", false),
        (r#"{"format":1,"entries":{"a":5}}"#, false),
        (r#"{"format":2,"entries":{},"more":true}"#, true),
    ] {
        let keychain = FakeKeychain::new(true);
        keychain.put_raw(VAULT_ITEM, content);
        let store = VaultStore::new(keychain.clone());
        let err = store.get(&key("a")).expect_err("unreadable");
        assert_eq!(err.code(), ErrorCode::Keychain);
        assert_eq!(err.to_string().contains("newer"), newer, "{err}");
        assert!(!err.to_string().contains("first password"));
        store
            .set(&key("a"), &secret("b"))
            .expect_err("no overwrite");
        store.delete(&key("a")).expect_err("no overwrite");
        assert_eq!(keychain.raw(VAULT_ITEM).as_deref(), Some(content));
    }
}

#[test]
fn concurrent_writers_lose_nothing() {
    let keychain = FakeKeychain::new(true);
    let store = Arc::new(VaultStore::new(keychain.clone()));
    let threads: Vec<_> = (0..8)
        .map(|t| {
            let store = Arc::clone(&store);
            std::thread::spawn(move || {
                for n in 0..25 {
                    store
                        .set(&key(&format!("t{t}-{n}")), &secret(&format!("v{t}-{n}")))
                        .expect("set");
                }
            })
        })
        .collect();
    for thread in threads {
        thread.join().expect("thread");
    }
    let restarted = VaultStore::new(keychain);
    for t in 0..8 {
        for n in 0..25 {
            assert_eq!(
                read(&restarted, &format!("t{t}-{n}")),
                Some(format!("v{t}-{n}"))
            );
        }
    }
}

#[test]
fn emptying_a_vault_an_older_build_wrote_leaves_no_old_secret_readable() {
    let (keychain, store) = filled(false);
    drop(store);
    keychain.update_to(2);
    let store = VaultStore::new(keychain.clone());
    store.set(&key("new"), &secret("v")).expect("set");
    assert_eq!(keychain.vaults().len(), 2, "build 1's item stays");
    for (name, _) in SECRETS {
        store.delete(&key(name)).expect("delete");
    }
    store.delete(&key("new")).expect("delete");
    // Build 1's item cannot be deleted, so an empty vault stays as the newest one.
    let restarted = VaultStore::new(keychain.clone());
    for (name, _) in SECRETS {
        assert_eq!(read(&restarted, name), None);
    }
    assert_eq!(read(&restarted, "new"), None);
    assert_eq!(keychain.vaults().len(), 2);
}

#[test]
fn errors_and_debug_output_never_carry_a_secret() {
    let keychain = FakeKeychain::new(true);
    keychain.put_raw(
        VAULT_ITEM,
        r#"{"format":1,"entries":{"a":"first password 1%"},"x":"#,
    );
    let store = VaultStore::new(keychain);
    let err = store.get(&key("a")).expect_err("unreadable");
    let ipc = IpcError::from(&err);
    for text in [
        err.to_string(),
        format!("{err:?}"),
        format!("{ipc:?}"),
        format!("{store:?}"),
    ] {
        assert!(!text.contains("first password"), "{text}");
    }
}

#[test]
fn generation_names() {
    assert_eq!(generation_of("vault"), Some(1));
    assert_eq!(generation_of("vault.2"), Some(2));
    assert_eq!(generation_of("vault.17"), Some(17));
    for no in [
        "vault.1", "vault.02", "vault.", "vault.x", "vaults", "a-vault", "vault.-3",
    ] {
        assert_eq!(generation_of(no), None, "{no}");
    }
    assert_eq!(item_name(1), "vault");
    assert_eq!(item_name(3), "vault.3");
}

/// The fake as the store of 0.1.2: one item per secret.
impl SecretStore for FakeKeychain {
    fn set(&self, key: &CredentialRef, secret: &Secret<String>) -> Result<(), CoreError> {
        self.write(key.as_str(), secret)
    }

    fn get(&self, key: &CredentialRef) -> Result<Option<Secret<String>>, CoreError> {
        self.read(key.as_str())
    }

    fn delete(&self, key: &CredentialRef) -> Result<bool, CoreError> {
        self.remove(key.as_str())
    }
}

/// Through the core: 0.1.2 stored the password in an item of its own; 0.1.3 (another build)
/// moves it into the vault at start, deletes the old item, and the account logs in with it.
#[tokio::test]
async fn the_core_moves_old_items_into_the_vault_at_start() {
    use std::time::Duration;

    use mail_model::{AccountSettings, AccountState, Security, ServerSettings};

    use crate::credentials::Credentials;
    use crate::sync::SyncConfig;
    use crate::sync::fake::{FakeConnector, FakeServer, PASSWORD};
    use crate::{AppCore, BuildInfo, CoreConfig, CoreServices};

    let dir = tempfile::TempDir::new().expect("dir");
    let server = FakeServer::new();
    let start = |credentials: Credentials| {
        let mut services =
            CoreServices::with_connector(credentials, FakeConnector(Arc::clone(&server)));
        services.sync = SyncConfig {
            poll_interval: Duration::from_secs(600),
            settings_sync_auto: false,
            ..SyncConfig::default()
        };
        AppCore::start(
            CoreConfig {
                data_dir: dir.path().join("data"),
                build: BuildInfo {
                    version: "0".into(),
                    commit: None,
                    build_date: None,
                },
            },
            services,
        )
    };
    let keychain = FakeKeychain::new(true);
    let core = start(Credentials::new(keychain.clone())).await;
    let server_settings = |host: &str, port| ServerSettings {
        host: host.into(),
        port,
        security: Security::Tls,
        username: "alice".into(),
    };
    let account = core
        .add_account(
            AccountSettings {
                credential: Default::default(),
                protocol: Default::default(),
                email: "alice@fake.test".into(),
                display_name: "Alice".into(),
                imap: server_settings("imap.fake.test", 993),
                smtp: server_settings("smtp.fake.test", 465),
            },
            Secret::new(PASSWORD.into()),
        )
        .await
        .expect("account")
        .id;
    core.shutdown().await;
    drop(core);
    assert_eq!(keychain.names_now(), [account.to_string()]);

    keychain.update_to(2);
    let core = start(Credentials::new(VaultStore::new(keychain.clone()))).await;
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        let state = core.list_accounts().await.expect("accounts")[0]
            .status
            .state;
        let moved = keychain.vaults().len() == 1 && keychain.names_now() == keychain.vaults();
        if moved && state == AccountState::Idle {
            break;
        }
        assert_ne!(state, AccountState::Error, "the account lost its password");
        assert!(
            std::time::Instant::now() < deadline,
            "not moved: {:?}",
            keychain.names_now()
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(keychain.prompts(), [account.to_string()], "one last dialog");
    core.shutdown().await;
}

/// "Allow" instead of "Always Allow": the vault is read once per run and kept in memory, and
/// the first write after the update puts it into an item of this build, so later runs of the
/// same build ask nothing.
#[test]
fn allowing_once_asks_once_per_run_until_the_first_write() {
    let (keychain, store) = filled(false);
    drop(store);
    keychain.update_to(2);
    keychain.lock().answer = Answer::AllowOnce;
    let store = VaultStore::new(keychain.clone());
    for (name, value) in SECRETS {
        assert_eq!(read(&store, name).as_deref(), Some(value));
    }
    assert_eq!(keychain.prompts().len(), 1);
    drop(store);
    let store = VaultStore::new(keychain.clone());
    assert_eq!(read(&store, SECRETS[0].0).as_deref(), Some(SECRETS[0].1));
    assert_eq!(keychain.prompts().len(), 2, "not trusted: the next run asks again");
    store.set(&key("new"), &secret("v")).expect("set");
    drop(store);
    let store = VaultStore::new(keychain.clone());
    assert_eq!(read(&store, "new").as_deref(), Some("v"));
    assert_eq!(keychain.prompts().len(), 2, "the newest item is this build's own");
}
