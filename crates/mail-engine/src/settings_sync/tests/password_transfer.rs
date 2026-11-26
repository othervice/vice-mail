//! Stage 22 (decision 0076): passwords from another device, through `AppCore` on two devices
//! over one in-memory sync folder (the harness of `server_change`), with the fake mail server.
//!
//! Device A has alice (the sync account) and bob, both with passwords. Device B has alice too,
//! joins, and takes bob over without a password. An attacker holds the sync key and writes into
//! the same folder.

use std::path::Path;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use mail_model::{
    AccountId, AccountSettings, ErrorCode, PasswordRequestAccountState, PasswordRequestState,
    PasswordTransferState, Secret, SettingsSyncStatus,
};
use serde_json::json;
use tempfile::TempDir;

use super::server_change::{FolderState, MemFolder, Recording, server, settle};
use super::*;
use crate::clock::Clock;
use crate::credentials::{CredentialRef, Credentials, MemorySecretStore, SecretStore};
use crate::settings_sync::imap::ImapTransport;
use crate::settings_sync::transfer::{
    GivenItem, RequestId, TransferEntry, TransferKind, TransferMessage, seal_items,
};
use crate::settings_sync::transport::SettingsTransport;
use crate::sync::SyncConfig;
use crate::sync::fake::{FakeConnector, FakeServer, PASSWORD};
use crate::{AppCore, BuildInfo, CoreConfig, CoreServices};

/// A wall clock the test moves.
#[derive(Debug)]
struct Wall(AtomicI64);

impl Clock for Wall {
    fn now_ms(&self) -> i64 {
        self.0.load(Ordering::SeqCst)
    }
}

struct Device {
    core: AppCore<Recording>,
    keychain: Arc<MemorySecretStore>,
}

async fn start(
    server: &Arc<FakeServer>,
    folder: &Arc<Mutex<FolderState>>,
    data: &Path,
    keychain: &Arc<MemorySecretStore>,
    clock: &Arc<Wall>,
) -> Device {
    let connector = Recording {
        fake: FakeConnector(Arc::clone(server)),
        folder: Arc::clone(folder),
        logins: Arc::default(),
    };
    let mut services =
        CoreServices::with_connector(Credentials::new(Arc::clone(keychain)), connector);
    services.clock = Arc::clone(clock) as Arc<dyn Clock>;
    services.sync = SyncConfig {
        poll_interval: Duration::from_secs(600),
        settings_sync_auto: false,
        settings_sync_work_factor: 10,
        ..SyncConfig::default()
    };
    let core = AppCore::start(
        CoreConfig {
            data_dir: data.to_path_buf(),
            build: BuildInfo {
                version: "0".into(),
                commit: None,
                build_date: None,
            },
        },
        services,
    )
    .await;
    Device {
        core,
        keychain: Arc::clone(keychain),
    }
}

fn settings(email: &str, imap: &str, smtp: &str) -> AccountSettings {
    AccountSettings {
        credential: Default::default(),
        protocol: Default::default(),
        email: email.into(),
        display_name: email.into(),
        imap: server(imap, 993),
        smtp: server(smtp, 465),
    }
}

struct World {
    _dirs: (TempDir, TempDir),
    folder: Arc<Mutex<FolderState>>,
    clock: Arc<Wall>,
    a: Device,
    b: Device,
    b_data: std::path::PathBuf,
    bob: AccountId,
}

/// A with alice and bob, the sync on; B with alice, joined, bob taken over without password.
async fn world() -> World {
    let server = FakeServer::new();
    let folder = Arc::new(Mutex::new(FolderState::default()));
    let clock = Arc::new(Wall(AtomicI64::new(crate::clock::SystemClock.now_ms())));
    let (dir_a, dir_b) = (TempDir::new().expect("dir"), TempDir::new().expect("dir"));
    let a = start(
        &server,
        &folder,
        &dir_a.path().join("data"),
        &Arc::new(MemorySecretStore::new()),
        &clock,
    )
    .await;
    let alice = a
        .core
        .add_account(
            settings("alice@fake.test", "imap.fake.test", "smtp.fake.test"),
            Secret::new(PASSWORD.into()),
        )
        .await
        .expect("alice")
        .id;
    let bob = a
        .core
        .add_account(
            settings("bob@fake.test", "imap.bob.test", "smtp.bob.test"),
            Secret::new(PASSWORD.into()),
        )
        .await
        .expect("bob")
        .id;
    a.core
        .enable_settings_sync(alice, passphrase())
        .await
        .expect("sync on");
    a.core
        .set_sync_device_name("Old Mac".into())
        .await
        .expect("name");
    a.core.sync_settings_now().await.expect("pass");

    let b_data = dir_b.path().join("data");
    let b = start(
        &server,
        &folder,
        &b_data,
        &Arc::new(MemorySecretStore::new()),
        &clock,
    )
    .await;
    let mine = b
        .core
        .add_account(
            settings("alice@fake.test", "imap.fake.test", "smtp.fake.test"),
            Secret::new(PASSWORD.into()),
        )
        .await
        .expect("alice on b");
    b.core
        .preview_settings_sync_join(mine.id, passphrase())
        .await
        .expect("preview");
    b.core.join_settings_sync().await.expect("join");
    b.core
        .set_sync_device_name("New Mac".into())
        .await
        .expect("name");
    let status = b.core.sync_settings_now().await.expect("pass");
    assert_eq!(status.accounts_needing_password, [bob]);
    a.core.sync_settings_now().await.expect("pass");
    settle().await;
    World {
        _dirs: (dir_a, dir_b),
        folder,
        clock,
        a,
        b,
        b_data,
        bob,
    }
}

impl World {
    fn transfer_entries(&self) -> usize {
        let state = self.folder.lock().unwrap_or_else(PoisonError::into_inner);
        state
            .messages
            .values()
            .filter(|m| String::from_utf8_lossy(m).contains("X-Settings-Sync-Id: xfer-"))
            .count()
    }

    /// No message of the folder ever holds a password in plain text.
    fn assert_no_plain_password(&self) {
        let state = self.folder.lock().unwrap_or_else(PoisonError::into_inner);
        for message in state.messages.values() {
            assert!(!String::from_utf8_lossy(message).contains(PASSWORD));
        }
    }

    fn advance(&self, ms: i64) {
        self.clock.0.fetch_add(ms, Ordering::SeqCst);
    }

    async fn bob_on_b(&self) -> mail_store::StoredAccount {
        self.b
            .core
            .store_for_tests()
            .account(self.bob)
            .await
            .expect("read")
            .expect("bob")
    }

    async fn bob_password_on_b(&self) -> Option<String> {
        let key = CredentialRef::parse(&self.bob_on_b().await.credential_ref).expect("ref");
        self.b
            .keychain
            .get(&key)
            .expect("keychain")
            .map(Secret::into_exposed)
    }

    /// B asks; A sees the request and answers it; B reveals; both show the code.
    async fn up_to_the_code(&self) -> (String, SettingsSyncStatus, SettingsSyncStatus) {
        let status = self.b.core.start_password_transfer().await.expect("start");
        let own = status.password_transfer.expect("own request");
        assert_eq!(own.state, PasswordTransferState::Waiting);
        assert_eq!(own.accounts, [self.bob]);
        assert!(own.code.is_none());

        let status = self.a.core.sync_settings_now().await.expect("pass");
        assert_eq!(status.password_requests.len(), 1, "{status:?}");
        let request = &status.password_requests[0];
        assert_eq!(request.device_name, "New Mac");
        assert_eq!(request.state, PasswordRequestState::Open);
        assert_eq!(request.accounts.len(), 1);
        assert_eq!(request.accounts[0].account_id, self.bob);
        assert_eq!(
            request.accounts[0].state,
            PasswordRequestAccountState::Ready
        );
        let id = request.request_id.clone();
        let status = self
            .a
            .core
            .open_password_request(id.clone())
            .await
            .expect("open");
        assert_eq!(
            status.password_requests[0].state,
            PasswordRequestState::Connecting
        );

        let b_status = self.b.core.sync_settings_now().await.expect("pass");
        let own = b_status.password_transfer.clone().expect("own");
        assert_eq!(own.state, PasswordTransferState::Confirm);
        assert_eq!(own.other_device.as_deref(), Some("Old Mac"));
        let a_status = self.a.core.sync_settings_now().await.expect("pass");
        let request = &a_status.password_requests[0];
        assert_eq!(request.state, PasswordRequestState::Confirm);
        assert_eq!(request.code, own.code, "both devices show the same code");
        assert_eq!(request.code.as_deref().map(str::len), Some(7));
        (id, a_status, b_status)
    }
}

#[tokio::test]
async fn passwords_go_to_the_new_device_after_the_codes_match() {
    let w = world().await;
    assert!(w.bob_password_on_b().await.is_none());
    let (id, _, _) = w.up_to_the_code().await;

    // Giving before the codes are compared, or for an unknown request, is refused.
    let err =
        w.a.core
            .give_passwords(RequestId::random().to_string(), vec![w.bob])
            .await
            .expect_err("unknown");
    assert_eq!(err.code(), ErrorCode::NotFound);

    let status =
        w.a.core
            .give_passwords(id, vec![w.bob])
            .await
            .expect("give");
    assert_eq!(
        status.password_requests[0].state,
        PasswordRequestState::Given
    );
    w.assert_no_plain_password();

    let status = w.b.core.sync_settings_now().await.expect("pass");
    let own = status.password_transfer.expect("outcome");
    assert_eq!(own.state, PasswordTransferState::Done);
    assert_eq!(own.stored, [w.bob]);
    assert!(own.skipped.is_empty());
    assert!(status.accounts_needing_password.is_empty());
    assert_eq!(w.bob_password_on_b().await.as_deref(), Some(PASSWORD));
    assert!(!w.bob_on_b().await.writes_enabled, "writing stays off");
    assert_eq!(w.transfer_entries(), 0, "request and answers are gone");

    // A forgets the request; B closes the outcome.
    let status = w.a.core.sync_settings_now().await.expect("pass");
    assert!(status.password_requests.is_empty());
    let status = w.b.core.cancel_password_transfer().await.expect("close");
    assert!(status.password_transfer.is_none());
}

#[tokio::test]
async fn a_declined_request_stores_nothing_and_cleans_up() {
    let w = world().await;
    let (id, _, _) = w.up_to_the_code().await;
    let status =
        w.a.core
            .decline_password_request(id)
            .await
            .expect("decline");
    assert_eq!(
        status.password_requests[0].state,
        PasswordRequestState::Declined
    );
    let status = w.b.core.sync_settings_now().await.expect("pass");
    let own = status.password_transfer.expect("outcome");
    assert_eq!(own.state, PasswordTransferState::Declined);
    assert_eq!(own.other_device.as_deref(), Some("Old Mac"));
    assert!(w.bob_password_on_b().await.is_none());
    assert_eq!(status.accounts_needing_password, [w.bob]);
    assert_eq!(w.transfer_entries(), 0);
}

#[tokio::test]
async fn a_request_runs_out_after_fifteen_minutes() {
    let w = world().await;
    let status = w.b.core.start_password_transfer().await.expect("start");
    let id = status.password_transfer.expect("own").expires_at;
    assert!(id > w.clock.now_ms());
    let status = w.a.core.sync_settings_now().await.expect("pass");
    let request_id = status.password_requests[0].request_id.clone();

    w.advance(16 * 60 * 1000);
    let status = w.a.core.sync_settings_now().await.expect("pass");
    assert!(status.password_requests.is_empty(), "not offered any more");
    let err =
        w.a.core
            .open_password_request(request_id)
            .await
            .expect_err("ran out");
    assert_eq!(err.code(), ErrorCode::NotFound);
    let status = w.b.core.sync_settings_now().await.expect("pass");
    assert_eq!(
        status.password_transfer.expect("outcome").state,
        PasswordTransferState::Expired
    );
    assert_eq!(
        w.transfer_entries(),
        0,
        "the new device deleted its request"
    );
    assert!(w.bob_password_on_b().await.is_none());
}

#[tokio::test]
async fn a_restart_drops_the_request() {
    let w = world().await;
    w.b.core.start_password_transfer().await.expect("start");
    assert_eq!(w.transfer_entries(), 1);
    w.b.core.shutdown().await;
    let World {
        _dirs,
        folder,
        clock,
        a,
        b,
        b_data,
        ..
    } = w;
    let keychain = Arc::clone(&b.keychain);
    drop(b);
    let server = FakeServer::new();
    let b = start(&server, &folder, &b_data, &keychain, &clock).await;
    let status = b.core.get_settings_sync_status().await.expect("status");
    assert!(status.password_transfer.is_none(), "the key is gone");
    b.core.sync_settings_now().await.expect("pass");
    let left = folder
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .messages
        .values()
        .filter(|m| String::from_utf8_lossy(m).contains("xfer-"))
        .count();
    assert_eq!(left, 0, "the old request is deleted");
    let status = a.core.sync_settings_now().await.expect("pass");
    assert!(status.password_requests.is_empty());
}

/// Holds the sync key and writes into the folder.
struct Intruder {
    key: SyncKey,
    folder: MemFolder,
    hlc: HlcClock,
    device: DeviceId,
}

impl Intruder {
    fn new(world: &World) -> Self {
        let text = world
            .a
            .keychain
            .get(&CredentialRef::for_settings_sync())
            .expect("keychain")
            .expect("sync key");
        Self {
            key: SyncKey::import(&text).expect("key"),
            folder: MemFolder(Arc::clone(&world.folder)),
            hlc: HlcClock::new(
                Arc::clone(&world.clock) as Arc<dyn Clock>,
                DeviceId::from_uuid(uuid::Uuid::from_u128(0xbad)),
            ),
            device: DeviceId::from_uuid(uuid::Uuid::from_u128(0xbad)),
        }
    }

    /// Every transfer entry of the folder, opened.
    async fn entries(&mut self) -> Vec<TransferEntry> {
        let mut transport = ImapTransport::new(&mut self.folder, 0);
        let listing = transport.list().await.expect("list");
        let mut out = Vec::new();
        for entry in listing {
            let Some(header) = RecordHeader::parse(&entry.header) else {
                continue;
            };
            if !header.opaque_id.is_transfer() {
                continue;
            }
            let body = transport.read(&[entry.handle]).await.expect("read");
            out.push(
                self.key
                    .open_transfer(&header, &body[0].1)
                    .expect("open transfer"),
            );
        }
        out
    }

    async fn write(&mut self, entry: &TransferEntry) {
        let version = self.hlc.now().expect("version");
        let sealed = self.key.seal_transfer(entry, version).expect("seal");
        let mut transport = ImapTransport::new(&mut self.folder, 0);
        transport
            .write(&sealed.header.to_raw(), sealed.ciphertext)
            .await
            .expect("write");
    }
}

/// Someone with the sync key answers the request itself: with passwords for other servers,
/// the new device stores nothing.
#[tokio::test]
async fn passwords_for_other_servers_are_never_stored() {
    let w = world().await;
    w.b.core.start_password_transfer().await.expect("start");
    let mut intruder = Intruder::new(&w);
    let request = intruder
        .entries()
        .await
        .into_iter()
        .find(|e| e.message.kind() == TransferKind::Request)
        .expect("request");
    let nonce = crate::settings_sync::transfer::nonce();
    intruder
        .write(&TransferEntry {
            request_id: request.request_id.clone(),
            device: intruder.device,
            expires_ms: request.expires_ms,
            message: TransferMessage::Challenge {
                device_name: "Old Mac".into(),
                nonce,
            },
        })
        .await;
    let status = w.b.core.sync_settings_now().await.expect("pass");
    assert_eq!(
        status.password_transfer.expect("own").state,
        PasswordTransferState::Confirm
    );
    let public_key = intruder
        .entries()
        .await
        .into_iter()
        .find_map(|e| match e.message {
            TransferMessage::Reveal { public_key, .. } => Some(public_key),
            _ => None,
        })
        .expect("reveal");
    let evil = GivenItem::new(
        w.bob,
        mail_model::CredentialKind::Password,
        server("imap.attacker.example", 993),
        server("smtp.bob.test", 465),
        &Secret::new("planted".into()),
    );
    let sealed = seal_items(&public_key, &request.request_id, vec![evil]).expect("seal");
    intruder
        .write(&TransferEntry {
            request_id: request.request_id.clone(),
            device: intruder.device,
            expires_ms: request.expires_ms,
            message: TransferMessage::Response {
                device_name: "Old Mac".into(),
                declined: false,
                sealed: Some(sealed),
            },
        })
        .await;
    let status = w.b.core.sync_settings_now().await.expect("pass");
    let own = status.password_transfer.expect("outcome");
    assert_eq!(own.state, PasswordTransferState::Done);
    assert!(own.stored.is_empty());
    assert_eq!(own.skipped, [w.bob]);
    assert!(w.bob_password_on_b().await.is_none());
    assert_eq!(status.accounts_needing_password, [w.bob]);
}

/// A response sealed to another key than the one the new device revealed fails cleanly.
#[tokio::test]
async fn a_response_for_another_key_fails_cleanly() {
    let w = world().await;
    let (id, _, _) = w.up_to_the_code().await;
    let mut intruder = Intruder::new(&w);
    let entries = intruder.entries().await;
    let challenge = entries
        .iter()
        .find(|e| e.message.kind() == TransferKind::Challenge)
        .expect("challenge");
    let other_key = crate::settings_sync::transfer::RequestKey::generate();
    let item = GivenItem::new(
        w.bob,
        mail_model::CredentialKind::Password,
        server("imap.bob.test", 993),
        server("smtp.bob.test", 465),
        &Secret::new(PASSWORD.into()),
    );
    let request_id = RequestId::parse(&id).expect("id");
    let sealed = seal_items(other_key.public_key(), &request_id, vec![item]).expect("seal");
    // Written as if by A (the device the request chose).
    intruder
        .write(&TransferEntry {
            request_id,
            device: challenge.device,
            expires_ms: challenge.expires_ms,
            message: TransferMessage::Response {
                device_name: "Old Mac".into(),
                declined: false,
                sealed: Some(sealed),
            },
        })
        .await;
    let status = w.b.core.sync_settings_now().await.expect("pass");
    let own = status.password_transfer.expect("outcome");
    assert_eq!(own.state, PasswordTransferState::Failed);
    assert!(own.stored.is_empty());
    assert!(w.bob_password_on_b().await.is_none());
    assert_eq!(w.transfer_entries(), 0);
}

/// Accounts whose servers differ between the devices are not offered and cannot be given.
#[tokio::test]
async fn an_account_with_other_servers_here_is_not_given() {
    let w = world().await;
    // Another device moved bob to other servers: B's user accepted them, A keeps its own until
    // its user decides (decision 0040).
    let mut attacker =
        super::server_change::Attacker::with_key_of_core(&w.a.core, &w.a.keychain, &w.folder).await;
    attacker
        .write_account(
            w.bob,
            json!({ "imap": { "host": "imap.elsewhere.example", "port": 993, "security": "tls", "username": "alice" } }),
        )
        .await;
    w.a.core.sync_settings_now().await.expect("pass");
    let status = w.b.core.sync_settings_now().await.expect("pass");
    assert_eq!(status.server_changes.len(), 1);
    w.b.core
        .resolve_server_change(w.bob, true)
        .await
        .expect("take the new servers");
    assert_eq!(w.bob_on_b().await.imap.host, "imap.elsewhere.example");
    assert_eq!(
        w.a.core
            .store_for_tests()
            .account(w.bob)
            .await
            .expect("a")
            .expect("bob")
            .imap
            .host,
        "imap.bob.test"
    );

    let (id, a_status, _) = w.up_to_the_code_differing().await;
    assert_eq!(
        a_status.password_requests[0].accounts[0].state,
        PasswordRequestAccountState::ServersDiffer
    );
    let err =
        w.a.core
            .give_passwords(id, vec![w.bob])
            .await
            .expect_err("nothing to give");
    assert_eq!(err.code(), ErrorCode::InvalidArgument);
    assert!(w.bob_password_on_b().await.is_none());
}

impl World {
    /// As [`World::up_to_the_code`], for a request whose account is not ready on A.
    async fn up_to_the_code_differing(&self) -> (String, SettingsSyncStatus, SettingsSyncStatus) {
        self.b.core.start_password_transfer().await.expect("start");
        let status = self.a.core.sync_settings_now().await.expect("pass");
        let id = status.password_requests[0].request_id.clone();
        self.a
            .core
            .open_password_request(id.clone())
            .await
            .expect("open");
        let b_status = self.b.core.sync_settings_now().await.expect("pass");
        let a_status = self.a.core.sync_settings_now().await.expect("pass");
        assert_eq!(
            a_status.password_requests[0].state,
            PasswordRequestState::Confirm
        );
        (id, a_status, b_status)
    }
}
