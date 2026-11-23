//! Stage 11 acceptance: the settings sync with two engines (two `AppCore` instances with their
//! own databases and keychains) against the local test server, alice's account as the sync
//! account. Each test starts from an empty sync folder.
//!
//! Ignored by default. Take the shared lock, start the server (any seed):
//!
//! ```text
//! scripts/testmail.sh lock sync && scripts/testmail.sh start
//! cargo test -p mail-engine --test settings_sync -- --include-ignored --nocapture
//! scripts/testmail.sh stop && scripts/testmail.sh unlock sync
//! ```

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, AtomicU16, Ordering};
use std::time::Duration;

use mail_engine::credentials::{Credentials, MemorySecretStore};
use mail_engine::sync_state::BackoffPolicy;
use mail_engine::{AppCore, BuildInfo, Clock, CoreConfig, CoreServices, SyncConfig, SystemClock};
use mail_imap::test_support::{Proxy, TestEnv};
use mail_imap::{ImapBackend, ImapConnector, ImapSyncFolder, ImapWriter, TlsTransport, WireTap};
use mail_model::{
    Access, AccountId, AccountSettings, AccountState, BackendError, ConnectConfig, CoreEvent,
    ErrorCode, IpcError, LanguagePreference, MailBackend, MailConnector, MessageCategory,
    Preferences, Secret, Security, SenderKey, ServerSettings, SettingsSyncState,
    SettingsSyncStatus, SyncFolder, SyncFolderConnector, ThemePreference, UidSet, WriteConnector,
};
use tempfile::TempDir;

/// The tests share alice's account and the server's connection limit: one at a time.
static SERVER: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
use tokio::sync::broadcast;
use tokio::time::Instant;

const WAIT: Duration = Duration::from_secs(60);
const DAY_MS: i64 = 24 * 60 * 60 * 1000;
const FOLDER: &str = "Settings-Sync";
const FIELDS: [&str; 2] = ["X-Settings-Sync-Id", "X-Settings-Sync-Version"];

fn passphrase() -> Secret<String> {
    Secret::new("correct horse battery staple".to_owned())
}

/// Wall clock plus an offset the test moves (a device that was away, a later day).
#[derive(Debug, Default)]
struct OffsetClock(AtomicI64);

impl Clock for OffsetClock {
    fn now_ms(&self) -> i64 {
        SystemClock.now_ms() + self.0.load(Ordering::SeqCst)
    }
}

impl OffsetClock {
    fn set(&self, offset_ms: i64) {
        self.0.store(offset_ms, Ordering::SeqCst);
    }
}

/// Mail connections direct; the settings sync connection through its own tap, and through a
/// proxy when the test sets its port (faults, an outage of the sync account).
#[derive(Clone)]
struct Connector {
    mail: ImapConnector,
    folder: ImapConnector,
    proxy_port: Arc<AtomicU16>,
}

impl MailConnector for Connector {
    type Backend = ImapBackend<TlsTransport>;

    async fn connect(&self, config: &ConnectConfig) -> Result<Self::Backend, BackendError> {
        self.mail.connect(config).await
    }
}

impl WriteConnector for Connector {
    type Writer = ImapWriter<TlsTransport>;

    async fn connect_writer(&self, config: &ConnectConfig) -> Result<Self::Writer, BackendError> {
        self.mail.connect_writer(config).await
    }
}

impl SyncFolderConnector for Connector {
    type Folder = ImapSyncFolder<TlsTransport>;

    async fn connect_sync_folder(
        &self,
        config: &ConnectConfig,
        folder: &str,
    ) -> Result<Self::Folder, BackendError> {
        let mut config = config.clone();
        let port = self.proxy_port.load(Ordering::SeqCst);
        if port != 0 {
            config.port = port;
        }
        self.folder.connect_sync_folder(&config, folder).await
    }
}

fn sync_config(env: &TestEnv) -> SyncConfig {
    let mut config = SyncConfig {
        backoff: BackoffPolicy {
            base: Duration::from_millis(100),
            max: Duration::from_secs(1),
            jitter: 0.5,
        },
        poll_interval: Duration::from_secs(600),
        extra_root_ca: Some(env.extra_root()),
        settings_sync_auto: false,
        settings_sync_debounce: Duration::from_millis(50),
        settings_sync_work_factor: 10,
        ..SyncConfig::default()
    };
    config.timeouts.read = Duration::from_secs(10);
    config
}

fn account_settings(env: &TestEnv, who: &str) -> (AccountSettings, Secret<String>) {
    let account = if who == "alice" { &env.alice } else { &env.bob };
    let server = |port, security| ServerSettings {
        host: env.host.clone(),
        port,
        security,
        username: account.address.clone(),
    };
    (
        AccountSettings {
            email: account.address.clone(),
            display_name: who.to_owned(),
            imap: server(env.imaps_port, Security::Tls),
            smtp: server(env.submission_port, Security::Starttls),
        },
        account.password.clone(),
    )
}

/// One device: an engine with its own data directory and keychain.
struct Device {
    dir: TempDir,
    keychain: Arc<MemorySecretStore>,
    core: AppCore<Connector>,
    events: broadcast::Receiver<CoreEvent>,
    clock: Arc<OffsetClock>,
    tap: WireTap,
    proxy_port: Arc<AtomicU16>,
    alice: AccountId,
}

impl Device {
    async fn start(env: &TestEnv, configure: impl FnOnce(&mut SyncConfig)) -> Self {
        Self::start_at(env, 0, configure).await
    }

    /// A device whose clock runs `offset_ms` from the wall clock from its first moment on.
    async fn start_at(
        env: &TestEnv,
        offset_ms: i64,
        configure: impl FnOnce(&mut SyncConfig),
    ) -> Self {
        let dir = TempDir::new().expect("temp dir");
        let keychain = Arc::new(MemorySecretStore::new());
        let clock = Arc::new(OffsetClock::default());
        clock.set(offset_ms);
        let mut config = sync_config(env);
        configure(&mut config);
        let mut device = Self::open(dir, keychain, clock, config).await;
        device.alice = device.add(env, "alice").await;
        device
    }

    async fn open(
        dir: TempDir,
        keychain: Arc<MemorySecretStore>,
        clock: Arc<OffsetClock>,
        config: SyncConfig,
    ) -> Self {
        let tap = WireTap::new();
        let connector = Connector {
            mail: ImapConnector::new(),
            folder: ImapConnector::new().with_wire_tap(tap.clone()),
            proxy_port: Arc::new(AtomicU16::new(0)),
        };
        let proxy_port = Arc::clone(&connector.proxy_port);
        let mut services =
            CoreServices::with_connector(Credentials::new(Arc::clone(&keychain)), connector);
        services.sync = config;
        services.clock = Arc::clone(&clock) as Arc<dyn Clock>;
        let core = AppCore::start(
            CoreConfig {
                data_dir: dir.path().join("data"),
                build: BuildInfo {
                    version: "0.0.0".into(),
                    commit: None,
                    build_date: None,
                },
            },
            services,
        )
        .await;
        let events = core.subscribe();
        let alice = core
            .list_accounts()
            .await
            .ok()
            .and_then(|a| a.first().map(|a| a.id))
            .unwrap_or_else(|| AccountId::from_uuid(uuid::Uuid::nil()));
        Self {
            dir,
            keychain,
            core,
            events,
            clock,
            tap,
            proxy_port,
            alice,
        }
    }

    /// The app closed and opened again: same data directory, keychain and clock.
    async fn restart(self, env: &TestEnv) -> Self {
        let Self {
            dir,
            keychain,
            core,
            clock,
            ..
        } = self;
        core.shutdown().await;
        drop(core);
        let mut device = Self::open(dir, keychain, clock, sync_config(env)).await;
        let alice = device.alice;
        device.idle(alice).await;
        device
    }

    async fn add(&mut self, env: &TestEnv, who: &str) -> AccountId {
        let (settings, password) = account_settings(env, who);
        let id = self
            .core
            .add_account(settings, password)
            .await
            .unwrap_or_else(|err| panic!("add {who}: {err}"))
            .id;
        self.idle(id).await;
        id
    }

    async fn idle(&mut self, account: AccountId) {
        let deadline = Instant::now() + WAIT;
        loop {
            let accounts = self.core.list_accounts().await.expect("accounts");
            if accounts
                .iter()
                .any(|a| a.id == account && a.status.state == AccountState::Idle)
            {
                return;
            }
            assert!(Instant::now() < deadline, "{account} never idle");
            let _ = tokio::time::timeout(Duration::from_millis(200), self.events.recv()).await;
        }
    }

    /// The first device: turns the sync on with alice.
    async fn first(env: &TestEnv) -> Self {
        Self::first_at(env, 0).await
    }

    async fn first_at(env: &TestEnv, offset_ms: i64) -> Self {
        let device = Self::start_at(env, offset_ms, |_| {}).await;
        let status = device
            .core
            .enable_settings_sync(device.alice, passphrase())
            .await
            .expect("enable");
        assert_eq!(status.state, SettingsSyncState::Active, "{status:?}");
        device
    }

    /// A second device: sets alice up, sees the folder, joins with the passphrase.
    async fn second(env: &TestEnv) -> Self {
        Self::second_at(env, 0).await
    }

    async fn second_at(env: &TestEnv, offset_ms: i64) -> Self {
        let mut device = Self::start_at(env, offset_ms, |_| {}).await;
        device.join().await;
        device
    }

    /// Joins with the passphrase. The local alice takes the folder's id for the same account.
    async fn join(&mut self) {
        let status = self.status().await;
        assert!(
            status.detected_accounts.contains(&self.alice),
            "the folder was detected: {status:?}"
        );
        self.core
            .preview_settings_sync_join(self.alice, passphrase())
            .await
            .expect("preview");
        let status = self.core.join_settings_sync().await.expect("join");
        assert_eq!(status.state, SettingsSyncState::Active, "{status:?}");
        self.alice = status.account_id.expect("sync account");
    }

    /// Joins again (after a reset elsewhere, or with another passphrase).
    async fn rejoin(&mut self, passphrase: Secret<String>) {
        self.core
            .preview_settings_sync_join(self.alice, passphrase)
            .await
            .expect("preview");
        let status = self.core.join_settings_sync().await.expect("join");
        self.alice = status.account_id.expect("sync account");
    }

    async fn pass(&self) -> SettingsSyncStatus {
        self.core.sync_settings_now().await.expect("pass")
    }

    async fn status(&self) -> SettingsSyncStatus {
        self.core.get_settings_sync_status().await.expect("status")
    }

    async fn prefs(&self) -> Preferences {
        self.core.get_preferences().await.expect("preferences")
    }

    async fn set_prefs(&self, theme: ThemePreference, language: LanguagePreference) {
        self.core
            .set_preferences(Preferences { theme, language })
            .await
            .expect("set preferences");
    }

    async fn signature(&self) -> Option<String> {
        self.core
            .list_compose_identities(self.alice)
            .await
            .expect("identities")
            .into_iter()
            .find(|i| i.is_default)
            .and_then(|i| i.signature)
    }

    async fn set_signature(&self, text: Option<&str>) {
        let identity = self
            .core
            .list_compose_identities(self.alice)
            .await
            .expect("identities")
            .into_iter()
            .find(|i| i.is_default)
            .expect("default identity");
        self.core
            .set_identity_signature(identity.id, text.map(str::to_owned))
            .await
            .expect("signature");
    }

    async fn sender_rules(&self) -> Vec<String> {
        let mut keys: Vec<String> = self
            .core
            .list_sender_rules()
            .await
            .expect("sender rules")
            .into_iter()
            .map(|r| r.key.value)
            .collect();
        keys.sort();
        keys
    }

    async fn newsletter(&self, address: &str) {
        self.core
            .set_sender_category(
                SenderKey::address(address),
                Some(MessageCategory::Newsletter),
            )
            .await
            .expect("sender rule");
    }

    async fn forget_sender(&self, address: &str) {
        self.core
            .set_sender_category(SenderKey::address(address), None)
            .await
            .expect("sender rule removed");
    }

    async fn shutdown(self) {
        self.core.shutdown().await;
    }
}

async fn until<T, F, Fut>(what: &str, mut f: F) -> T
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Option<T>>,
{
    let deadline = Instant::now() + WAIT;
    loop {
        if let Some(value) = f().await {
            return value;
        }
        assert!(Instant::now() < deadline, "never: {what}");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// The sync folder as another client sees it.
async fn admin_folder(env: &TestEnv) -> ImapSyncFolder<TlsTransport> {
    let config = env.config(&env.alice, Security::Tls, Access::ReadOnly);
    let mut folder = ImapConnector::new()
        .connect_sync_folder(&config, FOLDER)
        .await
        .expect("admin folder");
    folder.open(false).await.expect("select");
    folder
}

/// Entries of the folder: UID → (opaque id, version).
async fn entries(env: &TestEnv) -> BTreeMap<u32, (String, String)> {
    let mut folder = admin_folder(env).await;
    let listed = folder.list(&FIELDS).await.expect("list");
    folder.logout().await.ok();
    listed
        .into_iter()
        .filter_map(|e| {
            let text = String::from_utf8_lossy(&e.header).into_owned();
            let get = |name: &str| {
                text.lines()
                    .find_map(|l| l.strip_prefix(&format!("{name}: ")))
                    .map(str::trim)
                    .map(str::to_owned)
            };
            Some((e.uid, (get(FIELDS[0])?, get(FIELDS[1])?)))
        })
        .collect()
}

/// Entries per record (key records left out).
async fn per_record(env: &TestEnv) -> BTreeMap<String, usize> {
    let mut out = BTreeMap::new();
    for (id, _) in entries(env).await.into_values() {
        if !id.starts_with("key-") {
            *out.entry(id).or_insert(0) += 1;
        }
    }
    out
}

/// Starts every test from an empty folder.
async fn clean(env: &TestEnv) {
    let mut admin = ImapConnector::new()
        .connect(&env.config(&env.alice, Security::Tls, Access::ReadWrite))
        .await
        .expect("admin");
    admin
        .raw_command(&format!("DELETE \"{FOLDER}\""), |_| false)
        .await
        .ok();
    admin.logout().await.ok();
}

fn code(err: &mail_model::CoreError) -> ErrorCode {
    IpcError::from(err).code
}

// ---------------------------------------------------------------------------------------------

#[tokio::test]
#[ignore = "needs the test server"]
async fn different_settings_on_two_devices_both_stay() {
    let _serial = SERVER.lock().await;
    let env = TestEnv::require();
    clean(&env).await;
    let a = Device::first(&env).await;
    let b = Device::second(&env).await;
    // Concurrent changes of different settings: A when an opened message counts as read, B
    // theme and language.
    a.core
        .set_reading_preferences(mail_model::ReadingPreferences {
            mark_read_on_open: mail_model::MarkReadOnOpen::Never,
        })
        .await
        .expect("reading");
    b.set_prefs(ThemePreference::Dark, LanguagePreference::De)
        .await;
    a.pass().await;
    b.pass().await;
    a.pass().await;
    for device in [&a, &b] {
        assert_eq!(
            device.prefs().await,
            Preferences {
                theme: ThemePreference::Dark,
                language: LanguagePreference::De
            }
        );
        assert_eq!(
            device
                .core
                .get_reading_preferences()
                .await
                .expect("reading")
                .mark_read_on_open,
            mail_model::MarkReadOnOpen::Never
        );
    }
    // One entry per record in the folder.
    assert!(per_record(&env).await.values().all(|n| *n == 1));
    a.shutdown().await;
    b.shutdown().await;
    clean(&env).await;
}

#[tokio::test]
#[ignore = "needs the test server"]
async fn the_same_setting_on_two_devices_ends_equal() {
    let _serial = SERVER.lock().await;
    let env = TestEnv::require();
    clean(&env).await;
    let a = Device::first(&env).await;
    let b = Device::second(&env).await;
    a.set_prefs(ThemePreference::Light, LanguagePreference::En)
        .await;
    a.pass().await;
    b.pass().await;
    assert_eq!(b.prefs().await.theme, ThemePreference::Light);
    // Both change the theme before either syncs; B's change is the later one.
    a.set_prefs(ThemePreference::Dark, LanguagePreference::En)
        .await;
    tokio::time::sleep(Duration::from_millis(5)).await;
    b.set_prefs(ThemePreference::System, LanguagePreference::En)
        .await;
    a.pass().await;
    b.pass().await;
    a.pass().await;
    assert_eq!(a.prefs().await, b.prefs().await);
    assert_eq!(a.prefs().await.theme, ThemePreference::System);
    assert!(per_record(&env).await.values().all(|n| *n == 1));
    a.shutdown().await;
    b.shutdown().await;
    clean(&env).await;
}

#[tokio::test]
#[ignore = "needs the test server"]
async fn a_deleted_signature_edited_elsewhere_keeps_the_edit() {
    let _serial = SERVER.lock().await;
    let env = TestEnv::require();
    clean(&env).await;
    let a = Device::first(&env).await;
    let b = Device::second(&env).await;
    a.set_signature(Some("Regards")).await;
    a.pass().await;
    b.pass().await;
    assert_eq!(b.signature().await.as_deref(), Some("Regards"));
    // B edits, A deletes later in time (without having seen the edit).
    b.set_signature(Some("Best regards")).await;
    tokio::time::sleep(Duration::from_millis(5)).await;
    a.set_signature(None).await;
    a.pass().await;
    b.pass().await;
    a.pass().await;
    assert_eq!(a.signature().await.as_deref(), Some("Best regards"));
    assert_eq!(b.signature().await.as_deref(), Some("Best regards"));
    // A delete that saw the edit removes it everywhere.
    a.set_signature(None).await;
    a.pass().await;
    b.pass().await;
    assert_eq!(b.signature().await, None);
    assert!(per_record(&env).await.values().all(|n| *n == 1));
    a.shutdown().await;
    b.shutdown().await;
    clean(&env).await;
}

#[tokio::test]
#[ignore = "needs the test server"]
async fn a_device_back_after_long_offline_keeps_newer_states_and_applies_tombstones() {
    let _serial = SERVER.lock().await;
    let env = TestEnv::require();
    clean(&env).await;
    let start = -10 * DAY_MS;
    let a = Device::first_at(&env, start).await;
    a.newsletter("old@shop.example").await;
    // Both devices start from the same settings, so a change touches only what it changes.
    a.set_prefs(ThemePreference::System, LanguagePreference::System)
        .await;
    a.pass().await;
    let b = Device::second_at(&env, start).await;
    assert_eq!(b.sender_rules().await, ["old@shop.example"]);
    // B goes offline and changes things an hour later.
    b.clock.set(start + DAY_MS / 24);
    b.set_prefs(ThemePreference::Light, LanguagePreference::De)
        .await;
    // Five days later A changes the theme and deletes the sender rule.
    a.clock.set(start + 5 * DAY_MS);
    a.set_prefs(ThemePreference::Dark, LanguagePreference::System)
        .await;
    a.forget_sender("old@shop.example").await;
    a.pass().await;
    // Today B is back.
    a.clock.set(0);
    b.clock.set(0);
    b.pass().await;
    a.pass().await;
    b.pass().await;
    for device in [&a, &b] {
        let prefs = device.prefs().await;
        assert_eq!(prefs.theme, ThemePreference::Dark, "A's newer theme");
        assert_eq!(
            prefs.language,
            LanguagePreference::De,
            "B's language, nobody else's"
        );
        assert!(
            device.sender_rules().await.is_empty(),
            "the tombstone applied"
        );
    }
    a.shutdown().await;
    b.shutdown().await;
    clean(&env).await;
}

#[tokio::test]
#[ignore = "needs the test server"]
async fn expired_tombstones_do_not_bring_deleted_records_back() {
    let _serial = SERVER.lock().await;
    let env = TestEnv::require();
    clean(&env).await;
    let start = -200 * DAY_MS;
    let a = Device::first_at(&env, start).await;
    a.newsletter("gone@shop.example").await;
    a.newsletter("kept@shop.example").await;
    a.pass().await;
    let b = Device::second_at(&env, start).await;
    assert_eq!(b.sender_rules().await.len(), 2);
    // B is away from here on. A deletes one rule a day later.
    a.clock.set(start + DAY_MS);
    a.forget_sender("gone@shop.example").await;
    a.pass().await;
    let with_tombstone = entries(&env).await.len();
    // 199 days later the tombstone has expired: A's next pass removes it from the folder.
    a.clock.set(0);
    a.pass().await;
    assert_eq!(
        entries(&env).await.len(),
        with_tombstone - 1,
        "tombstone gone"
    );
    // B comes back after more than 180 days: its old copy counts as deleted, not as new.
    b.clock.set(0);
    b.pass().await;
    a.pass().await;
    assert_eq!(b.sender_rules().await, ["kept@shop.example"]);
    assert_eq!(a.sender_rules().await, ["kept@shop.example"]);
    assert_eq!(
        entries(&env).await.len(),
        with_tombstone - 1,
        "nothing uploaded again"
    );
    a.shutdown().await;
    b.shutdown().await;
    clean(&env).await;
}

/// The newest entry's UID, header values and body.
async fn newest_entry(env: &TestEnv) -> (u32, String, String, Vec<u8>) {
    let listed = entries(env).await;
    let (uid, (id, version)) = listed
        .iter()
        .filter(|(_, (id, _))| !id.starts_with("key-"))
        .max_by_key(|(uid, _)| **uid)
        .map(|(u, h)| (*u, h.clone()))
        .expect("an entry");
    let mut folder = admin_folder(env).await;
    let body = folder
        .read(&UidSet::from_uids([uid]))
        .await
        .expect("read")
        .pop()
        .expect("body")
        .1;
    folder.logout().await.ok();
    (uid, id, version, body)
}

fn message(id: &str, version: &str, body: &[u8]) -> Vec<u8> {
    let mut message = format!(
        "Date: Mon, 23 Nov 2026 10:00:00 +0000\r\nSubject: Settings sync data\r\n\
         X-Settings-Sync-Id: {id}\r\nX-Settings-Sync-Version: {version}\r\n\
         Content-Type: text/plain; charset=us-ascii\r\n\r\n"
    )
    .into_bytes();
    message.extend_from_slice(body);
    message
}

#[tokio::test]
#[ignore = "needs the test server"]
async fn a_modified_entry_is_rejected_and_reported_while_the_rest_syncs() {
    let _serial = SERVER.lock().await;
    let env = TestEnv::require();
    clean(&env).await;
    let a = Device::first(&env).await;
    let b = Device::second(&env).await;
    a.set_prefs(ThemePreference::Dark, LanguagePreference::System)
        .await;
    a.pass().await;
    // The server (or anyone with access to the mailbox) changes the newest entry, keeping
    // its header: one character of the ciphertext.
    let (uid, id, version, body) = newest_entry(&env).await;
    let mut text = String::from_utf8(body).expect("armored");
    // A line near the end: the encrypted payload, not age's header.
    let line = text
        .lines()
        .rev()
        .nth(2)
        .expect("a line of ciphertext")
        .to_owned();
    let mut changed = line.clone().into_bytes();
    changed[10] = if changed[10] == b'A' { b'B' } else { b'A' };
    text = text.replacen(&line, &String::from_utf8(changed).expect("ascii"), 1);
    let mut admin = admin_folder(&env).await;
    admin
        .append(&message(&id, &version, text.as_bytes()))
        .await
        .expect("append the modified entry");
    admin
        .delete(&UidSet::from_uids([uid]))
        .await
        .expect("delete");
    admin.logout().await.ok();

    b.core
        .set_reading_preferences(mail_model::ReadingPreferences {
            mark_read_on_open: mail_model::MarkReadOnOpen::Immediately,
        })
        .await
        .expect("reading");
    let status = b.pass().await;
    assert_eq!(status.rejected_entries, 1, "reported: {status:?}");
    assert_eq!(status.state, SettingsSyncState::Active);
    assert_eq!(
        b.prefs().await.theme,
        ThemePreference::System,
        "not taken over"
    );
    a.pass().await;
    assert_eq!(
        a.core
            .get_reading_preferences()
            .await
            .expect("reading")
            .mark_read_on_open,
        mail_model::MarkReadOnOpen::Immediately,
        "the rest synced"
    );
    // A's daily full check finds the damaged copy and heals the folder from its own.
    a.clock.set(DAY_MS + 60_000);
    b.clock.set(DAY_MS + 60_000);
    a.pass().await;
    let status = b.pass().await;
    assert_eq!(b.prefs().await.theme, ThemePreference::Dark, "healed");
    assert_eq!(status.rejected_entries, 0, "{status:?}");
    assert!(per_record(&env).await.values().all(|n| *n == 1));
    a.shutdown().await;
    b.shutdown().await;
    clean(&env).await;
}

#[tokio::test]
#[ignore = "needs the test server"]
async fn an_old_version_played_back_is_ignored() {
    let _serial = SERVER.lock().await;
    let env = TestEnv::require();
    clean(&env).await;
    let a = Device::first(&env).await;
    let b = Device::second(&env).await;
    a.set_prefs(ThemePreference::Dark, LanguagePreference::System)
        .await;
    a.pass().await;
    let (_, id, version, body) = newest_entry(&env).await;
    a.set_prefs(ThemePreference::Light, LanguagePreference::System)
        .await;
    a.pass().await;
    let mut admin = admin_folder(&env).await;
    admin
        .append(&message(&id, &version, &body))
        .await
        .expect("replay the old version");
    admin.logout().await.ok();
    assert_eq!(per_record(&env).await.get(&id), Some(&2));
    b.pass().await;
    a.pass().await;
    assert_eq!(b.prefs().await.theme, ThemePreference::Light);
    assert_eq!(a.prefs().await.theme, ThemePreference::Light);
    assert_eq!(
        per_record(&env).await.get(&id),
        Some(&1),
        "the replay was removed"
    );
    a.shutdown().await;
    b.shutdown().await;
    clean(&env).await;
}

#[tokio::test]
#[ignore = "needs the test server"]
async fn an_interruption_between_append_and_delete_is_cleaned_up_next_time() {
    let _serial = SERVER.lock().await;
    let env = TestEnv::require();
    clean(&env).await;
    let upstream: SocketAddr = format!("{}:{}", env.host, env.imaps_port)
        .parse()
        .expect("addr");
    let proxy = Proxy::start(upstream).await.expect("proxy");
    let mut a = Device::first(&env).await;
    let b = Device::second(&env).await;
    a.set_prefs(ThemePreference::Dark, LanguagePreference::System)
        .await;
    a.pass().await;
    // From now on A's settings sync connects through the proxy, which cuts the connection
    // the moment the old version is to be deleted (after the new one was appended).
    a.core.disable_settings_sync().await.ok();
    a.proxy_port.store(proxy.port(), Ordering::SeqCst);
    a.rejoin(passphrase()).await;
    a.set_prefs(ThemePreference::Light, LanguagePreference::System)
        .await;
    let cut = Arc::clone(&proxy);
    a.tap.on_write("+FLAGS.SILENT (\\Deleted)", move || {
        cut.cut_responses(Duration::ZERO)
    });
    let err = a.core.sync_settings_now().await.expect_err("cut");
    assert!(
        matches!(
            code(&err),
            ErrorCode::ConnectionLost | ErrorCode::ConnectionTimeout
        ),
        "{err}"
    );
    let doubled = per_record(&env).await;
    assert!(
        doubled.values().any(|n| *n == 2),
        "two versions of the theme: {doubled:?}"
    );
    assert_eq!(a.status().await.state, SettingsSyncState::Error);
    // The next pass of any device removes the loser.
    b.pass().await;
    a.pass().await;
    assert!(per_record(&env).await.values().all(|n| *n == 1));
    assert_eq!(b.prefs().await.theme, ThemePreference::Light);
    assert_eq!(a.status().await.state, SettingsSyncState::Active);
    a.shutdown().await;
    b.shutdown().await;
    clean(&env).await;
}

#[tokio::test]
#[ignore = "needs the test server"]
async fn a_wrong_passphrase_is_reported_and_changes_nothing() {
    let _serial = SERVER.lock().await;
    let env = TestEnv::require();
    clean(&env).await;
    let a = Device::first(&env).await;
    a.set_prefs(ThemePreference::Dark, LanguagePreference::De)
        .await;
    a.pass().await;
    let before = entries(&env).await;
    let b = Device::start(&env, |_| {}).await;
    b.set_prefs(ThemePreference::Light, LanguagePreference::En)
        .await;
    let err = b
        .core
        .preview_settings_sync_join(b.alice, Secret::new("wrong horse battery".into()))
        .await
        .expect_err("wrong passphrase");
    assert_eq!(code(&err), ErrorCode::SyncWrongPassphrase);
    let err = b
        .core
        .join_settings_sync()
        .await
        .expect_err("nothing to join");
    assert_eq!(code(&err), ErrorCode::InvalidArgument);
    assert_eq!(entries(&env).await, before, "nothing written to the folder");
    assert_eq!(b.status().await.state, SettingsSyncState::Off);
    assert_eq!(
        b.prefs().await.theme,
        ThemePreference::Light,
        "nothing changed here"
    );
    // Setting up again over an existing sync is refused too.
    let err = b
        .core
        .enable_settings_sync(b.alice, passphrase())
        .await
        .expect_err("already set up");
    assert_eq!(code(&err), ErrorCode::SyncAlreadySetUp);
    assert_eq!(entries(&env).await, before);
    a.shutdown().await;
    b.shutdown().await;
    clean(&env).await;
}

#[tokio::test]
#[ignore = "needs the test server"]
async fn a_thousand_records_first_pass_is_measured() {
    let _serial = SERVER.lock().await;
    let env = TestEnv::require();
    clean(&env).await;
    let a = Device::start(&env, |_| {}).await;
    let created = std::time::Instant::now();
    for n in 0..1_000 {
        a.newsletter(&format!("sender{n:04}@bulk.example")).await;
    }
    let created = created.elapsed();
    let upload = std::time::Instant::now();
    a.core
        .enable_settings_sync(a.alice, passphrase())
        .await
        .expect("enable");
    let upload = upload.elapsed();
    let records = per_record(&env).await.len();
    assert!(records >= 1_000, "{records} records in the folder");
    let mut b = Device::start(&env, |_| {}).await;
    let join = std::time::Instant::now();
    b.join().await;
    let join = join.elapsed();
    assert_eq!(b.sender_rules().await.len(), 1_000);
    let quiet = std::time::Instant::now();
    b.core.sync_settings_now().await.expect("pass");
    let quiet = quiet.elapsed();
    let full = std::time::Instant::now();
    b.clock.set(DAY_MS + 60_000);
    b.core.sync_settings_now().await.expect("full check");
    let full = full.elapsed();
    println!(
        "1,000 records ({records} in the folder): creating {created:?}, first pass with upload \
         {upload:?}, joining (download, merge, apply) {join:?}, pass without changes \
         {quiet:?}, daily full check {full:?}"
    );
    assert!(join < Duration::from_secs(60), "joining took {join:?}");
    a.shutdown().await;
    b.shutdown().await;
    clean(&env).await;
}

#[tokio::test]
#[ignore = "needs the test server"]
async fn an_outage_of_the_sync_account_leaves_reading_and_sending_alone() {
    let _serial = SERVER.lock().await;
    let env = TestEnv::require();
    clean(&env).await;
    let upstream: SocketAddr = format!("{}:{}", env.host, env.imaps_port)
        .parse()
        .expect("addr");
    let proxy = Proxy::start(upstream).await.expect("proxy");
    // The settings sync reaches the sync account through the proxy; mail goes direct.
    let mut a = Device::start(&env, |_| {}).await;
    a.proxy_port.store(proxy.port(), Ordering::SeqCst);
    a.core
        .enable_settings_sync(a.alice, passphrase())
        .await
        .expect("enable");
    let bob = a.add(&env, "bob").await;
    a.core.set_writes_enabled(bob, true).await.expect("writes");
    a.pass().await;
    // The sync account becomes unreachable for the settings sync: the open connection breaks,
    // new ones are refused.
    proxy.refuse_next(usize::MAX);
    proxy.cut_responses(Duration::ZERO);
    a.set_prefs(ThemePreference::Dark, LanguagePreference::De)
        .await;
    let err = a.core.sync_settings_now().await.expect_err("unreachable");
    assert!(
        matches!(
            code(&err),
            ErrorCode::ConnectionLost | ErrorCode::ConnectionRefused | ErrorCode::ConnectionTimeout
        ),
        "{err}"
    );
    let status = a.status().await;
    assert_eq!(status.state, SettingsSyncState::Error, "{status:?}");
    assert!(status.pending_changes > 0, "the change waits");

    // Mail keeps working in both accounts: arriving and sending.
    let run = std::process::id();
    let to_bob = format!("Outage {run} to bob");
    mail_imap::test_support::send_mail(&env, &env.alice, &env.bob.address, &to_bob, "hi")
        .await
        .expect("smtp");
    let to_alice = format!("Outage {run} to alice");
    mail_imap::test_support::send_mail(&env, &env.bob, &env.alice.address, &to_alice, "hi")
        .await
        .expect("smtp");
    for (account, subject) in [(bob, &to_bob), (a.alice, &to_alice)] {
        until("the message arrives", || async {
            let inbox = a
                .core
                .list_mailboxes(account)
                .await
                .ok()?
                .into_iter()
                .find(|m| m.path == "INBOX")?;
            let page = a.core.list_messages(inbox.id, None, 50).await.ok()?;
            page.items
                .iter()
                .any(|i| &i.subject == subject)
                .then_some(())
        })
        .await;
    }
    let draft = a
        .core
        .create_draft(mail_model::NewDraft::New {
            account_id: bob,
            identity_id: None,
            to: Vec::new(),
        })
        .await
        .expect("draft");
    let subject = format!("Outage {run} sent by bob");
    a.core
        .save_draft(
            draft.id.clone(),
            mail_model::DraftUpdate {
                identity_id: draft.identity_id,
                to: vec![mail_model::Address {
                    name: None,
                    email: env.alice.address.clone(),
                }],
                cc: Vec::new(),
                bcc: Vec::new(),
                subject: subject.clone(),
                format: mail_model::ComposeFormat::Html,
                body_html: "<p>still works</p>".into(),
                body_text: String::new(),
                include_quote: false,
                quote_position: mail_model::QuotePosition::Above,
                include_signature: false,
                signature_id: None,
            },
        )
        .await
        .expect("save");
    let outgoing = a
        .core
        .send_draft(draft.id.clone(), format!("outage-{run}"))
        .await
        .expect("send");
    until("the message is sent", || async {
        a.core
            .list_outgoing(bob, None, 20)
            .await
            .ok()?
            .into_iter()
            .find(|m| {
                m.operation_id == outgoing.operation_id && m.state == mail_model::SendState::Sent
            })
    })
    .await;
    a.shutdown().await;
    clean(&env).await;
}

#[tokio::test]
#[ignore = "needs the test server"]
async fn an_account_removed_on_one_device_is_asked_about_on_the_other() {
    let _serial = SERVER.lock().await;
    let env = TestEnv::require();
    clean(&env).await;
    let mut a = Device::first(&env).await;
    let bob = a.add(&env, "bob").await;
    a.pass().await;
    let b = Device::second(&env).await;
    // The second device took both accounts over: alice under A's id, bob without a password.
    let accounts = b.core.list_accounts().await.expect("accounts");
    let ids: Vec<AccountId> = accounts.iter().map(|x| x.id).collect();
    assert_eq!(accounts.len(), 2, "{accounts:?}");
    assert!(
        ids.contains(&a.alice) && ids.contains(&bob),
        "same ids on both devices"
    );
    assert!(
        accounts.iter().all(|x| !x.writes_enabled),
        "taken over read-only"
    );
    assert_eq!(b.status().await.accounts_needing_password, [bob]);
    let (_, password) = account_settings(&env, "bob");
    b.core
        .update_password(bob, password)
        .await
        .expect("password");
    assert!(b.status().await.accounts_needing_password.is_empty());

    // A removes bob: B asks instead of removing it.
    a.core.remove_account(bob).await.expect("remove");
    a.pass().await;
    let status = b.pass().await;
    assert_eq!(status.questions.len(), 1, "{status:?}");
    assert!(
        b.core
            .list_accounts()
            .await
            .expect("accounts")
            .iter()
            .any(|x| x.id == bob)
    );
    // Keeping it brings it back on A.
    b.core
        .resolve_sync_question(bob, false)
        .await
        .expect("keep");
    b.pass().await;
    a.pass().await;
    assert!(
        a.core
            .list_accounts()
            .await
            .expect("accounts")
            .iter()
            .any(|x| x.id == bob)
    );
    assert_eq!(a.status().await.accounts_needing_password, [bob]);
    // Removed again, and this time B agrees.
    a.core.remove_account(bob).await.expect("remove again");
    a.pass().await;
    b.pass().await;
    b.core
        .resolve_sync_question(bob, true)
        .await
        .expect("remove here too");
    b.pass().await;
    a.pass().await;
    assert!(
        !b.core
            .list_accounts()
            .await
            .expect("accounts")
            .iter()
            .any(|x| x.id == bob)
    );
    assert!(
        !a.core
            .list_accounts()
            .await
            .expect("accounts")
            .iter()
            .any(|x| x.id == bob)
    );
    assert!(b.status().await.questions.is_empty());
    a.shutdown().await;
    b.shutdown().await;
    clean(&env).await;
}

#[tokio::test]
#[ignore = "needs the test server"]
async fn the_second_device_follows_without_being_asked() {
    let _serial = SERVER.lock().await;
    let env = TestEnv::require();
    clean(&env).await;
    let a = Device::first(&env).await;
    let mut b = Device::start(&env, |config| {
        config.settings_sync_auto = true;
        config.poll_interval = Duration::from_secs(2);
    })
    .await;
    b.join().await;
    a.set_prefs(ThemePreference::Dark, LanguagePreference::De)
        .await;
    a.pass().await;
    until("B takes the theme over by itself", || async {
        (b.prefs().await.theme == ThemePreference::Dark).then_some(())
    })
    .await;
    // And B's own changes go up by themselves.
    b.set_prefs(ThemePreference::Light, LanguagePreference::De)
        .await;
    until("B uploads by itself", || async {
        a.pass().await;
        (a.prefs().await.theme == ThemePreference::Light).then_some(())
    })
    .await;
    a.shutdown().await;
    b.shutdown().await;
    clean(&env).await;
}

#[tokio::test]
#[ignore = "needs the test server"]
async fn passphrase_change_and_reset() {
    let _serial = SERVER.lock().await;
    let env = TestEnv::require();
    clean(&env).await;
    let a = Device::first(&env).await;
    let mut b = Device::second(&env).await;
    let new = Secret::new("a new and longer passphrase".to_owned());
    a.core
        .change_settings_sync_passphrase(new.clone())
        .await
        .expect("change");
    // B keeps working with the key it holds; a third device needs the new passphrase.
    a.set_prefs(ThemePreference::Dark, LanguagePreference::De)
        .await;
    a.pass().await;
    b.pass().await;
    assert_eq!(b.prefs().await.theme, ThemePreference::Dark);
    let mut c = Device::start(&env, |_| {}).await;
    let err = c
        .core
        .preview_settings_sync_join(c.alice, passphrase())
        .await
        .expect_err("old passphrase");
    assert_eq!(code(&err), ErrorCode::SyncWrongPassphrase);
    c.rejoin(new.clone()).await;
    assert_eq!(c.prefs().await.theme, ThemePreference::Dark);

    // Forgotten: A resets with a third passphrase and uploads its data again.
    let third = Secret::new("third passphrase for the reset".to_owned());
    b.set_prefs(ThemePreference::Light, LanguagePreference::De)
        .await;
    a.core
        .reset_settings_sync(third.clone())
        .await
        .expect("reset");
    let err = b.core.sync_settings_now().await.expect_err("key changed");
    assert_eq!(code(&err), ErrorCode::SyncKeyChanged);
    assert_eq!(b.status().await.state, SettingsSyncState::NeedsPassphrase);
    b.rejoin(third.clone()).await;
    a.pass().await;
    // B's change made while A reset is merged like a new device's.
    assert_eq!(a.prefs().await.theme, ThemePreference::Light);
    assert_eq!(b.status().await.state, SettingsSyncState::Active);
    a.shutdown().await;
    b.shutdown().await;
    c.shutdown().await;
    clean(&env).await;
}

#[tokio::test]
#[ignore = "needs the test server"]
async fn after_a_restart_the_sync_goes_on_with_the_stored_key_and_clock() {
    let _serial = SERVER.lock().await;
    let env = TestEnv::require();
    clean(&env).await;
    let a = Device::first(&env).await;
    let b = Device::second(&env).await;
    a.set_prefs(ThemePreference::Dark, LanguagePreference::De)
        .await;
    a.pass().await;
    let a = a.restart(&env).await;
    let status = a.status().await;
    assert_eq!(status.state, SettingsSyncState::Active, "{status:?}");
    assert_eq!(status.devices.len(), 2, "{status:?}");
    // Later than everything before the restart, so it wins.
    a.set_prefs(ThemePreference::Light, LanguagePreference::De)
        .await;
    a.pass().await;
    b.pass().await;
    assert_eq!(b.prefs().await.theme, ThemePreference::Light);
    assert!(per_record(&env).await.values().all(|n| *n == 1));
    a.shutdown().await;
    b.shutdown().await;
    clean(&env).await;
}

#[tokio::test]
#[ignore = "needs the test server"]
async fn shortcuts_and_text_size_reach_the_other_device_history_and_layouts_stay() {
    use mail_model::{
        Density, PlatformShortcuts, Shortcut, ShortcutOverride, UiPreferences, WindowLayout,
    };
    let _serial = SERVER.lock().await;
    let env = TestEnv::require();
    clean(&env).await;
    let a = Device::first(&env).await;
    let mut b = Device::second(&env).await;
    let prefs = UiPreferences {
        shortcuts: PlatformShortcuts {
            macos: vec![ShortcutOverride {
                command: "archive".into(),
                shortcuts: vec![Shortcut {
                    key: "y".into(),
                    shift: false,
                    modifier: true,
                }],
            }],
            windows: vec![ShortcutOverride {
                command: "delete".into(),
                shortcuts: Vec::new(),
            }],
        },
        text_size: 130,
        recent_commands: vec!["action:archive".into()],
        ai_panel: mail_model::AiPanelState::Folded,
        accent_theme: mail_model::AccentTheme::Orange,
    };
    a.core
        .set_ui_preferences(prefs.clone())
        .await
        .expect("ui preferences");
    let compact = WindowLayout {
        density: Density::Compact,
        ..WindowLayout::default()
    };
    a.core
        .set_window_layout("main", compact)
        .await
        .expect("layout");
    a.pass().await;
    while b.events.try_recv().is_ok() {}
    b.pass().await;

    let taken = b.core.get_ui_preferences().await.expect("ui preferences");
    assert_eq!(taken.shortcuts, prefs.shortcuts);
    assert_eq!(taken.text_size, 130);
    // Stage 16 (decision 0036): the accent theme goes along like the text size.
    assert_eq!(taken.accent_theme, mail_model::AccentTheme::Orange);
    assert!(
        taken.recent_commands.is_empty(),
        "the palette history stays on its device"
    );
    assert_eq!(
        taken.ai_panel,
        mail_model::AiPanelState::Open,
        "the AI panel's state stays on its device (decision 0028)"
    );
    assert_eq!(
        b.core.get_window_layout("main").await.expect("layout"),
        WindowLayout::default(),
        "layouts stay on their device"
    );
    let announced = until("B announces the taken-over UI preferences", || {
        let event = b.events.try_recv().ok();
        async move {
            match event {
                Some(CoreEvent::UiPreferencesChanged { preferences }) => Some(preferences),
                _ => None,
            }
        }
    })
    .await;
    assert_eq!(announced, taken);
    a.shutdown().await;
    b.shutdown().await;
    clean(&env).await;
}

/// Stage 7 (decision 0026): a saved search reaches the other device (with
/// `savedSearchesChanged`), a rename and a delete follow; recent searches stay on the device
/// where they were typed.
#[tokio::test]
#[ignore = "needs the test server"]
async fn saved_searches_reach_the_other_device_recent_ones_stay() {
    let _serial = SERVER.lock().await;
    let env = TestEnv::require();
    clean(&env).await;
    let a = Device::first(&env).await;
    let mut b = Device::second(&env).await;
    let saved = a
        .core
        .save_search("Rechnungen".into(), "rechnung has:attachment".into())
        .await
        .expect("save");
    a.core
        .add_recent_search("from:alice".into())
        .await
        .expect("recent");
    b.events = b.core.subscribe();
    a.pass().await;
    b.pass().await;
    let on_b = b.core.list_saved_searches().await.expect("list");
    assert_eq!(on_b.len(), 1, "{on_b:?}");
    assert_eq!(on_b[0].id, saved.id);
    assert_eq!(on_b[0].name, "Rechnungen");
    assert_eq!(on_b[0].query, "rechnung has:attachment");
    let deadline = Instant::now() + WAIT;
    loop {
        match tokio::time::timeout_at(deadline, b.events.recv()).await {
            Ok(Ok(CoreEvent::SavedSearchesChanged)) => break,
            Ok(Ok(_) | Err(broadcast::error::RecvError::Lagged(_))) => {}
            Ok(Err(err)) => panic!("events: {err}"),
            Err(_) => panic!("no savedSearchesChanged on B"),
        }
    }
    assert!(
        b.core
            .list_recent_searches()
            .await
            .expect("recent")
            .is_empty(),
        "recent searches stay on the device"
    );

    // Renamed on B, deleted on A after seeing the rename.
    b.core
        .rename_saved_search(saved.id.clone(), "Belege".into())
        .await
        .expect("rename");
    b.pass().await;
    a.pass().await;
    let on_a = a.core.list_saved_searches().await.expect("list");
    assert_eq!(on_a.len(), 1);
    assert_eq!(on_a[0].name, "Belege");
    a.core
        .delete_saved_search(saved.id.clone())
        .await
        .expect("delete");
    a.pass().await;
    b.pass().await;
    assert!(b.core.list_saved_searches().await.expect("list").is_empty());
    assert!(per_record(&env).await.values().all(|n| *n == 1));
    a.shutdown().await;
    b.shutdown().await;
    clean(&env).await;
}

/// Stage 16 (decisions 0033, 0036): the reading list, folder favourites and colours, blocked
/// senders and pinned attachments reach the other device, and their removal does too.
#[tokio::test]
#[ignore = "needs the test server"]
async fn reading_list_favourites_colours_and_blocked_senders_reach_the_other_device() {
    let _serial = SERVER.lock().await;
    let env = TestEnv::require();
    clean(&env).await;
    let a = Device::first(&env).await;
    let b = Device::second(&env).await;
    let inbox_of = |device: &Device| {
        let core = device.core.clone();
        let alice = device.alice;
        async move {
            core.list_mailboxes(alice)
                .await
                .expect("mailboxes")
                .into_iter()
                .find(|m| m.path == "INBOX")
                .expect("inbox")
        }
    };
    // A message of the test's own in alice's inbox; both devices see it arrive.
    let subject = format!("Stage 16 reading list {}", std::process::id());
    let mut admin = ImapConnector::new()
        .connect(&env.config(&env.alice, Security::Tls, Access::ReadWrite))
        .await
        .expect("admin");
    let body = format!(
        "From: Reader <reader@stage16.test>\r\nTo: alice@vicemail.test\r\nSubject: {subject}\r\n\
         Message-ID: <reading-{}@stage16.test>\r\nMIME-Version: 1.0\r\n\
         Content-Type: multipart/mixed; boundary=\"b\"\r\n\r\n\
         --b\r\nContent-Type: text/plain\r\n\r\nLater.\r\n\
         --b\r\nContent-Type: text/plain\r\nContent-Disposition: attachment; filename=\"note.txt\"\r\n\r\n\
         Keep this.\r\n--b--\r\n",
        std::process::id()
    );
    let reply = admin
        .raw_command(
            &format!("APPEND \"INBOX\" () {{{}+}}\r\n{body}", body.len()),
            |_| false,
        )
        .await
        .expect("append");
    assert!(reply.ok, "{}", reply.text);
    let inbox_a = inbox_of(&a).await;
    let find = |device: &Device| {
        let core = device.core.clone();
        let alice = device.alice;
        let subject = subject.clone();
        async move {
            let inbox = core
                .list_mailboxes(alice)
                .await
                .ok()?
                .into_iter()
                .find(|m| m.path == "INBOX")?;
            core.list_messages(inbox.id, None, 50)
                .await
                .ok()?
                .items
                .into_iter()
                .find(|i| i.subject == subject)
        }
    };
    let message = until("the message on a", || find(&a)).await;
    until("the message on b", || find(&b)).await;
    a.core
        .set_reading_list(vec![message.id], true)
        .await
        .expect("reading list");
    a.core
        .set_mailbox_favorite(inbox_a.id, true)
        .await
        .expect("favourite");
    let colour = mail_model::AccountColor::new(5).expect("colour");
    a.core
        .set_mailbox_color(inbox_a.id, Some(colour))
        .await
        .expect("colour");
    a.core
        .block_sender("Offers@Spam.Example".into())
        .await
        .expect("block");
    // Decision 0036: pinning needs the attachment list of the stored body.
    until("the body on a", || async {
        let view = a
            .core
            .get_message(message.id, mail_model::BodyFormat::Auto)
            .await
            .ok()?;
        (!view.attachments.is_empty()).then_some(view)
    })
    .await;
    a.core
        .set_attachment_pinned(message.id, 0, true)
        .await
        .expect("pin the attachment");
    a.pass().await;
    b.pass().await;

    let inbox_b = inbox_of(&b).await;
    assert_eq!(inbox_b.favorite_position, Some(0));
    assert_eq!(inbox_b.color, Some(colour));
    assert_eq!(
        b.core
            .list_favorite_mailboxes()
            .await
            .expect("favourites")
            .into_iter()
            .map(|m| m.id)
            .collect::<Vec<_>>(),
        vec![inbox_b.id]
    );
    let blocked: Vec<String> = b
        .core
        .list_blocked_senders()
        .await
        .expect("blocked")
        .into_iter()
        .map(|s| s.address)
        .collect();
    assert_eq!(blocked, ["offers@spam.example"]);
    let reading = until("the reading list on b", || async {
        let items = b
            .core
            .list_reading_list(mail_model::MarkScope::All)
            .await
            .expect("reading list");
        (!items.is_empty()).then_some(items)
    })
    .await;
    assert_eq!(reading.len(), 1);
    assert_eq!(reading[0].subject, message.subject);
    let pinned = b
        .core
        .list_pinned_attachments(mail_model::MarkScope::All)
        .await
        .expect("pinned attachments");
    assert_eq!(pinned.len(), 1);
    assert_eq!(
        (pinned[0].name.as_str(), pinned[0].index, pinned[0].size > 0),
        ("note.txt", 0, true),
        "the pin keeps name and size, without the body on b"
    );
    assert_eq!(pinned[0].message.subject, message.subject);

    // Removed on b: gone on a.
    b.core
        .unblock_sender("offers@spam.example".into())
        .await
        .expect("unblock");
    b.core
        .set_reading_list(vec![reading[0].id], false)
        .await
        .expect("off the list");
    b.core
        .set_mailbox_color(inbox_b.id, None)
        .await
        .expect("no colour");
    b.core
        .set_attachment_pinned(pinned[0].message.id, 0, false)
        .await
        .expect("unpin");
    b.pass().await;
    a.pass().await;
    assert!(
        a.core
            .list_pinned_attachments(mail_model::MarkScope::All)
            .await
            .expect("pinned attachments")
            .is_empty()
    );
    assert!(
        a.core
            .list_blocked_senders()
            .await
            .expect("blocked")
            .is_empty()
    );
    assert!(
        a.core
            .list_reading_list(mail_model::MarkScope::All)
            .await
            .expect("reading list")
            .is_empty()
    );
    let inbox_a = inbox_of(&a).await;
    assert_eq!((inbox_a.favorite_position, inbox_a.color), (Some(0), None));
    assert!(per_record(&env).await.values().all(|n| *n == 1));
    a.shutdown().await;
    b.shutdown().await;
    clean(&env).await;
    let opened = admin.open_mailbox("INBOX").await.expect("select");
    let all = admin.search_uids(&opened).await.expect("search");
    let ours: Vec<u32> = admin
        .fetch_headers(&opened, &all)
        .await
        .expect("headers")
        .into_iter()
        .filter(|h| h.subject == subject)
        .map(|h| h.uid)
        .collect();
    if !ours.is_empty() {
        let set = UidSet::from_uids(ours);
        admin
            .raw_command(
                &format!("UID STORE {set} +FLAGS.SILENT (\\Deleted)"),
                |_| false,
            )
            .await
            .ok();
        admin
            .raw_command(&format!("UID EXPUNGE {set}"), |_| false)
            .await
            .ok();
    }
    admin.logout().await.ok();
}
