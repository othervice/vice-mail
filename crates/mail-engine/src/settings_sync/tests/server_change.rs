//! Finding F1 of the stage 15 security review (decision 0040): whoever holds the sync key can
//! write account records. A record that names other servers for an account must never make
//! this device send the account's stored password there.
//!
//! Through `AppCore` with the fake mail server behind a connector that records every login
//! (host and password), and an in-memory sync folder behind the `SyncFolder` trait, so the
//! real pass, the real store and the real account tasks run. The attacker is a device of its
//! own that holds the sync key (taken from the victim's keychain, as a compromised device or a
//! cracked passphrase would give it) and writes into the same folder.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use mail_model::{
    AccountId, AccountSettings, BackendError, ConnectConfig, MailConnector, ModSeq, Secret,
    Security, ServerSettings, SyncFolder, SyncFolderConnector, SyncFolderEntry, SyncFolderState,
    Uid, UidSet, UidValidity, WriteConnector,
};
use serde_json::{Value, json};
use tempfile::TempDir;

use super::*;
use crate::credentials::{CredentialRef, Credentials, MemorySecretStore, SecretStore};
use crate::settings_sync::imap::ImapTransport;
use crate::sync::SyncConfig;
use crate::sync::fake::{FakeBackend, FakeConnector, FakeServer, PASSWORD};
use crate::{AppCore, BuildInfo, CoreConfig, CoreServices};

const EVIL_IMAP: &str = "imap.attacker.example";
const EVIL_SMTP: &str = "smtp.attacker.example";

// ---------------------------------------------------------------------------------------------
// The sync folder in memory

#[derive(Debug, Default)]
struct FolderState {
    exists: bool,
    next_uid: Uid,
    modseq: u64,
    messages: BTreeMap<Uid, Vec<u8>>,
    /// Every UID whose body was read (finding F2).
    reads: Vec<Uid>,
}

/// One connection to the shared in-memory folder.
#[derive(Debug, Clone)]
struct MemFolder(Arc<Mutex<FolderState>>);

impl MemFolder {
    fn lock(&self) -> std::sync::MutexGuard<'_, FolderState> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

fn split_message(raw: &[u8]) -> (&[u8], &[u8]) {
    match raw.windows(4).position(|w| w == b"\r\n\r\n") {
        Some(at) => (&raw[..at + 2], &raw[at + 4..]),
        None => (raw, &[]),
    }
}

impl SyncFolder for MemFolder {
    async fn open(&mut self, create: bool) -> Result<Option<SyncFolderState>, BackendError> {
        let mut state = self.lock();
        if !state.exists && !create {
            return Ok(None);
        }
        if !state.exists {
            state.exists = true;
            state.next_uid = 1;
            state.modseq = 1;
        }
        Ok(Some(SyncFolderState {
            uid_validity: UidValidity(7),
            uid_next: state.next_uid,
            exists: u32::try_from(state.messages.len()).unwrap_or(u32::MAX),
            highest_modseq: Some(ModSeq(state.modseq)),
        }))
    }

    async fn list(&mut self, fields: &[&str]) -> Result<Vec<SyncFolderEntry>, BackendError> {
        let state = self.lock();
        Ok(state
            .messages
            .iter()
            .map(|(uid, raw)| {
                let (header, _) = split_message(raw);
                let mut block = Vec::new();
                for line in header.split(|b| *b == b'\n') {
                    let text = String::from_utf8_lossy(line);
                    let wanted = text.split_once(':').is_some_and(|(name, _)| {
                        fields.iter().any(|f| f.eq_ignore_ascii_case(name.trim()))
                    });
                    if wanted {
                        block.extend_from_slice(line);
                        block.push(b'\n');
                    }
                }
                block.extend_from_slice(b"\r\n");
                SyncFolderEntry {
                    uid: *uid,
                    size: u32::try_from(raw.len()).unwrap_or(u32::MAX),
                    header: block,
                }
            })
            .collect())
    }

    async fn read(&mut self, uids: &UidSet) -> Result<Vec<(Uid, Vec<u8>)>, BackendError> {
        let mut state = self.lock();
        let bodies: Vec<(Uid, Vec<u8>)> = state
            .messages
            .iter()
            .filter(|(uid, _)| uids.contains(**uid))
            .map(|(uid, raw)| (*uid, split_message(raw).1.to_vec()))
            .collect();
        state.reads.extend(bodies.iter().map(|(uid, _)| *uid));
        Ok(bodies)
    }

    async fn append(&mut self, message: &[u8]) -> Result<Option<Uid>, BackendError> {
        let mut state = self.lock();
        let uid = state.next_uid;
        state.next_uid += 1;
        state.modseq += 1;
        state.messages.insert(uid, message.to_vec());
        Ok(Some(uid))
    }

    async fn delete(&mut self, uids: &UidSet) -> Result<(), BackendError> {
        let mut state = self.lock();
        state.messages.retain(|uid, _| !uids.contains(*uid));
        state.modseq += 1;
        Ok(())
    }

    async fn logout(self) -> Result<(), BackendError> {
        Ok(())
    }
}

// ---------------------------------------------------------------------------------------------
// A connector that records every login

/// Every login attempt: the host it went to and the password it carried.
type Logins = Arc<Mutex<Vec<(String, String)>>>;

#[derive(Debug, Clone)]
struct Recording {
    fake: FakeConnector,
    folder: Arc<Mutex<FolderState>>,
    logins: Logins,
}

impl Recording {
    fn record(&self, config: &ConnectConfig) {
        self.logins
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push((config.host.clone(), config.password.expose_secret().clone()));
    }
}

impl MailConnector for Recording {
    type Backend = FakeBackend;

    async fn connect(&self, config: &ConnectConfig) -> Result<FakeBackend, BackendError> {
        self.record(config);
        self.fake.connect(config).await
    }
}

impl WriteConnector for Recording {
    type Writer = FakeBackend;

    async fn connect_writer(&self, config: &ConnectConfig) -> Result<FakeBackend, BackendError> {
        self.record(config);
        self.fake.connect_writer(config).await
    }
}

impl SyncFolderConnector for Recording {
    type Folder = MemFolder;

    async fn connect_sync_folder(
        &self,
        config: &ConnectConfig,
        _folder: &str,
    ) -> Result<MemFolder, BackendError> {
        self.record(config);
        if config.password.expose_secret() != PASSWORD {
            return Err(BackendError::AuthFailed("wrong password".into()));
        }
        Ok(MemFolder(Arc::clone(&self.folder)))
    }
}

// ---------------------------------------------------------------------------------------------
// Devices

struct Victim {
    _dir: TempDir,
    core: AppCore<Recording>,
    keychain: Arc<MemorySecretStore>,
    logins: Logins,
    account: AccountId,
}

fn server(host: &str, port: u16) -> ServerSettings {
    ServerSettings {
        host: host.into(),
        port,
        security: Security::Tls,
        username: "alice".into(),
    }
}

async fn start_core(
    server: &Arc<FakeServer>,
    folder: &Arc<Mutex<FolderState>>,
) -> (TempDir, AppCore<Recording>, Arc<MemorySecretStore>, Logins) {
    let dir = TempDir::new().expect("dir");
    let keychain = Arc::new(MemorySecretStore::new());
    let logins = Logins::default();
    let connector = Recording {
        fake: FakeConnector(Arc::clone(server)),
        folder: Arc::clone(folder),
        logins: Arc::clone(&logins),
    };
    let mut services =
        CoreServices::with_connector(Credentials::new(Arc::clone(&keychain)), connector);
    services.sync = SyncConfig {
        poll_interval: Duration::from_secs(600),
        settings_sync_auto: false,
        settings_sync_work_factor: 10,
        ..SyncConfig::default()
    };
    let core = AppCore::start(
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
    .await;
    (dir, core, keychain, logins)
}

/// A device with alice's account (its password in the keychain, writing on) and the settings
/// sync turned on with that account.
async fn victim(server: &Arc<FakeServer>, folder: &Arc<Mutex<FolderState>>) -> Victim {
    let (dir, core, keychain, logins) = start_core(server, folder).await;
    let account = core
        .add_account(
            AccountSettings {
                email: "alice@fake.test".into(),
                display_name: "Alice".into(),
                imap: self::server("imap.fake.test", 993),
                smtp: self::server("smtp.fake.test", 465),
                protocol: Default::default(),
            },
            Secret::new(PASSWORD.into()),
        )
        .await
        .expect("account")
        .id;
    core.set_writes_enabled(account, true)
        .await
        .expect("writes on");
    core.enable_settings_sync(account, passphrase())
        .await
        .expect("sync on");
    Victim {
        _dir: dir,
        core,
        keychain,
        logins,
        account,
    }
}

impl Victim {
    fn logins(&self) -> Vec<(String, String)> {
        self.logins
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// No login ever went to a server named only by the folder.
    fn assert_nothing_sent_to_the_attacker(&self) {
        let logins = self.logins();
        assert!(!logins.is_empty(), "the device did log in somewhere");
        for (host, password) in &logins {
            assert!(
                !host.contains("attacker"),
                "a login went to {host} (with the stored password: {})",
                password == PASSWORD
            );
        }
    }

    async fn stored(&self, id: AccountId) -> mail_store::StoredAccount {
        self.core
            .store_for_tests()
            .account(id)
            .await
            .expect("read")
            .expect("account")
    }
}

/// Holds the sync key and writes into the same folder.
struct Attacker {
    key: SyncKey,
    state: LocalState,
    hlc: HlcClock,
    folder: MemFolder,
}

impl Attacker {
    async fn with_key_of(victim: &Victim, folder: &Arc<Mutex<FolderState>>) -> Self {
        let text = victim
            .keychain
            .get(&CredentialRef::for_settings_sync())
            .expect("keychain")
            .expect("sync key");
        let mut attacker = Self {
            key: SyncKey::import(&text).expect("key"),
            state: LocalState::new(),
            hlc: HlcClock::new(
                Arc::new(crate::clock::SystemClock),
                hlc::DeviceId::from_uuid(uuid::Uuid::from_u128(0xbad)),
            ),
            folder: MemFolder(Arc::clone(folder)),
        };
        attacker.pass().await;
        attacker
    }

    async fn pass(&mut self) {
        let now = self.hlc.wall_ms();
        let mut transport = ImapTransport::new(&mut self.folder, now);
        let report = run_pass(
            &mut transport,
            &self.key,
            &mut self.state,
            &mut self.hlc,
            PassMode::Changes,
        )
        .await
        .expect("attacker pass");
        assert!(report.failed.is_empty(), "{:?}", report.failed);
    }

    /// Writes new fields of the account record `id`.
    async fn write_account(&mut self, id: AccountId, fields: Value) {
        let values: BTreeMap<String, Value> = fields
            .as_object()
            .expect("object")
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        self.state
            .put_fields(RecordKey::account(id), values, &mut self.hlc)
            .expect("put");
        self.pass().await;
    }
}

fn wire(host: &str, port: u16) -> Value {
    json!({ "host": host, "port": port, "security": "tls", "username": "alice" })
}

/// Lets the account tasks that a pass started try to connect.
async fn settle() {
    tokio::time::sleep(Duration::from_millis(300)).await;
}

// ---------------------------------------------------------------------------------------------
// Tests

#[tokio::test]
async fn a_server_change_from_the_folder_waits_and_never_gets_the_stored_password() {
    let server = FakeServer::new();
    let folder = Arc::new(Mutex::new(FolderState::default()));
    let victim = victim(&server, &folder).await;
    let mut attacker = Attacker::with_key_of(&victim, &folder).await;
    attacker
        .write_account(
            victim.account,
            json!({ "imap": wire(EVIL_IMAP, 993), "smtp": wire(EVIL_SMTP, 465),
                    "displayName": "Alice (new)" }),
        )
        .await;

    let status = victim.core.sync_settings_now().await.expect("pass");
    settle().await;
    // The account keeps its servers and its password; the change waits.
    let stored = victim.stored(victim.account).await;
    assert_eq!(stored.imap.host, "imap.fake.test");
    assert_eq!(stored.smtp.host, "smtp.fake.test");
    assert_eq!(stored.display_name, "Alice (new)", "other fields apply");
    assert!(stored.writes_enabled);
    assert_eq!(status.server_changes.len(), 1);
    assert_eq!(status.server_changes[0].account_id, victim.account);
    assert_eq!(status.server_changes[0].imap.host, EVIL_IMAP);
    assert_eq!(status.server_changes[0].smtp.host, EVIL_SMTP);
    assert!(status.accounts_needing_password.is_empty());
    victim.assert_nothing_sent_to_the_attacker();

    // Another pass and a reconnect of the account: still only the old servers.
    victim.core.sync_settings_now().await.expect("pass");
    victim
        .core
        .retry_account(victim.account)
        .await
        .expect("retry");
    settle().await;
    victim.assert_nothing_sent_to_the_attacker();

    // Confirmed: the new servers, writing off, no password left to send anywhere.
    let status = victim
        .core
        .resolve_server_change(victim.account, true)
        .await
        .expect("confirm");
    settle().await;
    let stored = victim.stored(victim.account).await;
    assert_eq!(stored.imap.host, EVIL_IMAP);
    assert_eq!(stored.smtp.host, EVIL_SMTP);
    assert!(!stored.writes_enabled);
    let key = CredentialRef::parse(&stored.credential_ref).expect("ref");
    assert!(!victim.keychain.contains(&key), "the password is gone");
    assert!(status.server_changes.is_empty());
    assert_eq!(status.accounts_needing_password, [victim.account]);
    victim.core.sync_settings_now().await.ok();
    victim.core.retry_account(victim.account).await.ok();
    settle().await;
    victim.assert_nothing_sent_to_the_attacker();
    // Nothing waits any more, and a second answer finds nothing.
    let err = victim
        .core
        .resolve_server_change(victim.account, true)
        .await
        .expect_err("nothing waits");
    assert_eq!(err.code(), mail_model::ErrorCode::NotFound);
}

#[tokio::test]
async fn a_dismissed_server_change_keeps_the_servers_and_is_not_offered_again() {
    let server = FakeServer::new();
    let folder = Arc::new(Mutex::new(FolderState::default()));
    let victim = victim(&server, &folder).await;
    let mut attacker = Attacker::with_key_of(&victim, &folder).await;
    attacker
        .write_account(victim.account, json!({ "imap": wire(EVIL_IMAP, 993) }))
        .await;
    let status = victim.core.sync_settings_now().await.expect("pass");
    assert_eq!(status.server_changes.len(), 1);

    let status = victim
        .core
        .resolve_server_change(victim.account, false)
        .await
        .expect("dismiss");
    assert!(status.server_changes.is_empty());
    let stored = victim.stored(victim.account).await;
    assert_eq!(stored.imap.host, "imap.fake.test");
    assert!(stored.writes_enabled);
    let key = CredentialRef::parse(&stored.credential_ref).expect("ref");
    assert!(victim.keychain.contains(&key), "the password stays");

    // A local change of the account does not send this device's servers to the folder, and
    // later passes do not offer the same change again.
    victim
        .core
        .set_writes_enabled(victim.account, false)
        .await
        .expect("writes off");
    let status = victim.core.sync_settings_now().await.expect("pass");
    assert!(status.server_changes.is_empty());
    attacker.pass().await;
    let theirs = attacker
        .state
        .content(&RecordKey::account(victim.account))
        .and_then(SyncContent::field_values)
        .expect("account");
    assert_eq!(theirs["imap"]["host"], EVIL_IMAP);

    // A different change is offered again.
    attacker
        .write_account(victim.account, json!({ "imap": wire(EVIL_IMAP, 143) }))
        .await;
    let status = victim.core.sync_settings_now().await.expect("pass");
    assert_eq!(status.server_changes.len(), 1);
    assert_eq!(status.server_changes[0].imap.port, 143);
    settle().await;
    victim.assert_nothing_sent_to_the_attacker();
}

#[tokio::test]
async fn a_new_account_from_the_folder_never_uses_a_password_of_this_keychain() {
    let server = FakeServer::new();
    let folder = Arc::new(Mutex::new(FolderState::default()));
    let victim = victim(&server, &folder).await;
    // This device holds a password under the UUID the record will use (a re-keyed local
    // account keeps its old UUID as the entry name; a leftover entry looks the same).
    let planted: AccountId = uuid::Uuid::from_u128(0x5eed)
        .to_string()
        .parse()
        .expect("id");
    victim
        .keychain
        .set(
            &CredentialRef::for_account(planted),
            &Secret::new(PASSWORD.into()),
        )
        .expect("plant");
    let mut attacker = Attacker::with_key_of(&victim, &folder).await;
    attacker
        .write_account(
            planted,
            json!({ "email": "bob@fake.test", "displayName": "Bob", "color": 2,
                    "imap": wire(EVIL_IMAP, 993), "smtp": wire(EVIL_SMTP, 465) }),
        )
        .await;

    let status = victim.core.sync_settings_now().await.expect("pass");
    settle().await;
    let adopted = victim.stored(planted).await;
    assert_eq!(
        adopted.imap.host, EVIL_IMAP,
        "a new account takes the folder's servers"
    );
    assert!(!adopted.writes_enabled);
    assert_ne!(adopted.credential_ref, planted.to_string());
    let key = CredentialRef::parse(&adopted.credential_ref).expect("ref");
    assert!(!victim.keychain.contains(&key));
    assert_eq!(status.accounts_needing_password, [planted]);
    assert!(status.server_changes.is_empty());
    victim.assert_nothing_sent_to_the_attacker();
}

/// The same account set up on a second device before it joins takes the folder's id
/// (decision 0022); the folder's other servers wait there too, and the password this device
/// stored stays with the servers it was entered for.
#[tokio::test]
async fn an_account_matched_by_address_on_joining_keeps_its_servers() {
    let server = FakeServer::new();
    let folder = Arc::new(Mutex::new(FolderState::default()));
    let victim = victim(&server, &folder).await;
    let mut attacker = Attacker::with_key_of(&victim, &folder).await;
    attacker
        .write_account(
            victim.account,
            json!({ "imap": wire(EVIL_IMAP, 993), "smtp": wire(EVIL_SMTP, 465) }),
        )
        .await;

    let (_dir, core, keychain, logins) = start_core(&server, &folder).await;
    let mine = core
        .add_account(
            AccountSettings {
                email: "alice@fake.test".into(),
                display_name: "Alice".into(),
                imap: self::server("imap.fake.test", 993),
                smtp: self::server("smtp.fake.test", 465),
                protocol: Default::default(),
            },
            Secret::new(PASSWORD.into()),
        )
        .await
        .expect("account");
    core.preview_settings_sync_join(mine.id, passphrase())
        .await
        .expect("preview");
    let status = core.join_settings_sync().await.expect("join");
    settle().await;
    let accounts = core.store_for_tests().accounts().await.expect("accounts");
    assert_eq!(accounts.len(), 1);
    assert_eq!(accounts[0].id, victim.account, "matched by address");
    assert_eq!(accounts[0].imap.host, "imap.fake.test");
    assert_eq!(accounts[0].smtp.host, "smtp.fake.test");
    assert_eq!(status.server_changes.len(), 1);
    assert_eq!(status.server_changes[0].account_id, victim.account);
    let key = CredentialRef::parse(&accounts[0].credential_ref).expect("ref");
    assert!(keychain.contains(&key), "the password stays");
    let logins = logins
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    assert!(!logins.is_empty());
    assert!(
        logins.iter().all(|(host, _)| !host.contains("attacker")),
        "{logins:?}"
    );
}

// ---------------------------------------------------------------------------------------------
// Finding F2, with the same harness: the size the folder lists (RFC822.SIZE) decides before
// anything is read.

#[tokio::test]
async fn an_entry_listed_too_large_is_not_read_kept_and_counted_in_the_status() {
    let server = FakeServer::new();
    let folder = Arc::new(Mutex::new(FolderState::default()));
    let victim = victim(&server, &folder).await;
    let status = victim.core.sync_settings_now().await.expect("pass");
    assert_eq!(status.rejected_entries, 0);
    // Valid sync headers (a version of now) and a body far larger than a record can be.
    let now = crate::clock::Clock::now_ms(&crate::clock::SystemClock);
    let device = hlc::DeviceId::from_uuid(uuid::Uuid::from_u128(0xf2));
    let version = Hlc::new(now, 0, device).expect("version");
    let mut message = format!(
        "{}: 00112233445566778899aabbccddeeff\r\n{}: {version}\r\n\r\n",
        mail_model::SYNC_ID_HEADER,
        mail_model::SYNC_VERSION_HEADER,
    )
    .into_bytes();
    message.resize(message.len() + 3 * 1024 * 1024, b'x');
    let uid = MemFolder(Arc::clone(&folder))
        .append(&message)
        .await
        .expect("append")
        .expect("uid");

    for _ in 0..2 {
        let status = victim.core.sync_settings_now().await.expect("pass");
        assert_eq!(status.rejected_entries, 1, "{status:?}");
        let state = folder.lock().unwrap_or_else(PoisonError::into_inner);
        assert!(state.messages.contains_key(&uid), "kept");
        // Without the limit it would be read: its id is new to this device.
        assert!(!state.reads.contains(&uid), "never read");
    }
}
