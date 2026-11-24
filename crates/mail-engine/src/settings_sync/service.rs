//! The settings sync in the core (stage 11, decision 0022): turning it on, joining from a
//! second device, the passes that run along with the sync account's mail sync, open
//! questions, the status, passphrase changes and the reset.
//!
//! - **Capture** happens in the store's write transactions ([`super::local`]); every change
//!   wakes the worker, which runs a pass after a short pause.
//! - **Passes** run on the worker: after a local change, after each pass of the sync
//!   account's mail sync, and at the mail sync's poll interval. The folder's state
//!   (UIDVALIDITY, UIDNEXT, EXISTS, HIGHESTMODSEQ) tells whether anything changed; with
//!   nothing local to upload and an unchanged folder a pass ends after one SELECT. Once a day
//!   a pass downloads and checks every entry (`PassMode::FullCheck`).
//! - **One connection** of its own, restricted to the folder (decision 0014), kept between
//!   passes. Its failure only shows in the status: reading and sending mail never wait for it.
//! - **Records that exist on both sides under different ids** (the sync account added on the
//!   second device before joining, its default identity) are matched by address: a local
//!   account or identity that was never synced takes the folder's id first (decision 0022).
//! - **Removed accounts** are asked about. Until the user decides, the removals of the
//!   account's other records (identities, snoozes, …) wait too; keeping the account renews
//!   it and them, so they come back on every device.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use mail_model::{
    AccountId, BackendError, CoreError, CoreEvent, IpcError, MIN_SYNC_PASSPHRASE_CHARS,
    SETTINGS_SYNC_FOLDER, Secret, SettingsSyncFailure, SettingsSyncPreview, SettingsSyncState,
    SettingsSyncStatus, SyncDevice, SyncFolder, SyncFolderConnector, SyncFolderState,
    SyncKindCount, SyncQuestion, SyncServerChange,
};
use mail_store::{
    ApplyOutcome, Store, StoredSyncDevice, SyncApply, SyncProgress, SyncStateRow, TableWrite,
};
use tokio::sync::Notify;
use tokio::task::JoinHandle;

use super::crypto::{SyncKey, WorkFactor};
use super::error::{SettingsSyncError, TransportError};
use super::hlc::{DeviceId, Hlc, HlcClock};
use super::imap::ImapTransport;
use super::local::{
    EngineCapture, SharedClock, load_state, lock_clock, row_from_record, table_value,
};
use super::pass::{LocalState, PassMode, PassReport, change_passphrase, execute_pass, fetch_pass};
use super::plan::{LocalChange, LocalRecord, Notice, Question};
use super::record::{IdentityContent, RecordKey, RecordKind, SyncRecord};
use crate::clock::Clock;
use crate::core::{AppCore, Connector, WeakCore};
use crate::credentials::CredentialRef;

const DAY_MS: i64 = 24 * 60 * 60 * 1000;

/// Signals the mail sync and the capture hook send to the settings sync. Not generic, so the
/// account tasks can hold it.
#[derive(Default)]
pub(crate) struct SyncSignals {
    /// Run a pass soon.
    wake: Notify,
    /// Accounts whose folder list holds a sync folder, with its wire name.
    detected: Mutex<BTreeMap<AccountId, String>>,
    /// The sync account: its mail sync's passes wake the settings sync.
    sync_account: Mutex<Option<AccountId>>,
    /// The detection changed: the worker emits a new status.
    detection_changed: AtomicBool,
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl SyncSignals {
    /// The mail sync of `account` listed its folders; `folder` is the sync folder it saw.
    pub(crate) fn folder_seen(&self, account: AccountId, folder: Option<String>) {
        let mut detected = lock(&self.detected);
        let changed = match folder {
            Some(name) => detected.insert(account, name.clone()).as_ref() != Some(&name),
            None => detected.remove(&account).is_some(),
        };
        if changed {
            self.detection_changed.store(true, Ordering::SeqCst);
            self.wake.notify_one();
        }
    }

    /// A mail pass of `account` completed: if it is the sync account, a settings pass follows.
    pub(crate) fn pass_completed(&self, account: AccountId) {
        if *lock(&self.sync_account) == Some(account) {
            self.wake.notify_one();
        }
    }

    pub(crate) fn wake(&self) {
        self.wake.notify_one();
    }

    fn detected(&self) -> BTreeMap<AccountId, String> {
        lock(&self.detected).clone()
    }
}

/// What the status shows besides the stored state.
#[derive(Debug, Default, Clone)]
struct Live {
    /// `needsPassphrase` or `error`, with the reason; `None` while it works.
    problem: Option<(SettingsSyncState, IpcError)>,
    rejected: u32,
    questions: Vec<Question>,
    /// Some entry waited for this device's clock or for the next pass (the download limits of
    /// a pass): the next check must not skip.
    deferred: bool,
    /// A pass ran since the core started: questions and rejected entries are known (they are
    /// not stored, so the first check after a start never skips).
    ran: bool,
}

/// What a call of the pass did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PassRun {
    /// The sync is off.
    Off,
    /// Nothing changed locally or in the folder.
    Skipped,
    Ran,
}

/// A join checked with the passphrase, waiting for the user's confirmation after the preview.
struct PendingJoin {
    account: AccountId,
    folder: String,
    key: Arc<SyncKey>,
}

/// The settings sync's part of the core.
pub(crate) struct SettingsSync<C: Connector> {
    pub(crate) signals: Arc<SyncSignals>,
    clock: SharedClock,
    device: DeviceId,
    key: Mutex<Option<Arc<SyncKey>>>,
    live: Mutex<Live>,
    pending: Mutex<Option<PendingJoin>>,
    /// Held for every pass and folder operation; keeps the folder connection between passes.
    runner: tokio::sync::Mutex<Option<<C as SyncFolderConnector>::Folder>>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

impl<C: Connector> SettingsSync<C> {
    /// Sets the store's capture hook with this device's clock. Called right after the store
    /// opened, before anything writes.
    pub(crate) async fn new(store: Option<&Store>, wall: Arc<dyn Clock>) -> Self {
        let signals = Arc::new(SyncSignals::default());
        let mut device = DeviceId::random();
        let mut last = None;
        if let Some(store) = store {
            match store.sync_state().await {
                Ok(row) => {
                    if let Ok(id) = row.device_id.parse() {
                        device = id;
                    }
                    last = row.hlc_last.as_deref().and_then(|t| t.parse::<Hlc>().ok());
                    if row.enabled {
                        *lock(&signals.sync_account) = row.account;
                    }
                }
                Err(err) => tracing::error!(error = %err, "settings sync state unreadable"),
            }
        }
        let clock: SharedClock = Arc::new(Mutex::new(HlcClock::resume(wall, device, last)));
        if let Some(store) = store {
            let hook = EngineCapture::new(Arc::clone(&clock), Arc::clone(&signals));
            store.set_sync_capture(Arc::new(hook));
            // Data stored before the settings sync existed (migration 0009 marked it).
            if let Err(err) = store.flush_sync_capture().await {
                tracing::error!(error = %err, "earlier data could not be captured for the sync");
            }
        }
        Self {
            signals,
            clock,
            device,
            key: Mutex::new(None),
            live: Mutex::new(Live::default()),
            pending: Mutex::new(None),
            runner: tokio::sync::Mutex::new(None),
            worker: Mutex::new(None),
        }
    }

    pub(crate) fn stop(&self) {
        if let Some(worker) = lock(&self.worker).take() {
            worker.abort();
        }
    }

    /// The kept folder connection (the core logs it out when it shuts down).
    pub(crate) async fn take_connection(&self) -> Option<<C as SyncFolderConnector>::Folder> {
        self.runner.lock().await.take()
    }

    fn live(&self) -> Live {
        lock(&self.live).clone()
    }

    fn set_problem(&self, problem: Option<(SettingsSyncState, IpcError)>) {
        lock(&self.live).problem = problem;
    }

    /// A clock for one key operation (setup, passphrase, reset) that spans IO: it continues
    /// after the shared clock and is observed back into it afterwards.
    fn temp_clock(&self, wall: Arc<dyn Clock>) -> HlcClock {
        let last = lock_clock(&self.clock).last();
        HlcClock::resume(wall, self.device, last)
    }

    fn absorb(&self, temp: &HlcClock) {
        if let Some(last) = temp.last() {
            let mut clock = lock_clock(&self.clock);
            if let Err(err) = clock.observe(&last) {
                tracing::debug!(error = %err, "temporary clock not observed");
            }
        }
    }
}

/// Starts the worker (see the module docs).
pub(crate) fn start<C: Connector>(core: &AppCore<C>) {
    let state = core.settings_sync();
    let config = core.sync_config();
    let weak = core.downgrade();
    let signals = Arc::clone(&state.signals);
    let task = tokio::spawn(worker(weak, signals, config));
    *lock(&state.worker) = Some(task);
    state.signals.wake();
}

async fn worker<C: Connector>(
    core: WeakCore<C>,
    signals: Arc<SyncSignals>,
    config: Arc<crate::sync::SyncConfig>,
) {
    loop {
        tokio::select! {
            () = signals.wake.notified() => {}
            () = tokio::time::sleep(config.poll_interval) => {}
        }
        // Changes come in bursts (a rule reorder, a snooze of ten messages): one pass for all.
        tokio::time::sleep(config.settings_sync_debounce).await;
        let Some(core) = core.upgrade() else {
            return;
        };
        if signals.detection_changed.swap(false, Ordering::SeqCst) {
            core.emit_settings_status().await;
        }
        if config.settings_sync_auto
            && let Err(err) = core.settings_pass(false).await
        {
            tracing::warn!(code = ?err.code(), error = %err, "settings sync pass failed");
        }
    }
}

/// A [`SettingsSyncError`] as the UI sees it.
pub(crate) fn core_error(err: SettingsSyncError) -> CoreError {
    match err {
        SettingsSyncError::WrongPassphrase => SettingsSyncFailure::WrongPassphrase.into(),
        SettingsSyncError::NotSetUp => SettingsSyncFailure::NotSetUp.into(),
        SettingsSyncError::AlreadySetUp => SettingsSyncFailure::AlreadySetUp.into(),
        SettingsSyncError::KeyChanged => SettingsSyncFailure::KeyChanged.into(),
        SettingsSyncError::InvalidKeyRecord(detail) => {
            SettingsSyncFailure::KeyInvalid(detail).into()
        }
        SettingsSyncError::EmptyPassphrase => {
            CoreError::InvalidArgument("the passphrase is empty".into())
        }
        SettingsSyncError::Transport(TransportError::Unavailable(detail)) => {
            CoreError::Mail(BackendError::ConnectionLost(detail))
        }
        SettingsSyncError::Transport(TransportError::Refused(detail)) => {
            CoreError::Mail(BackendError::Rejected(detail))
        }
        other => CoreError::Internal(format!("settings sync: {other}")),
    }
}

fn check_passphrase(passphrase: &Secret<String>) -> Result<(), CoreError> {
    let chars = passphrase.expose_secret().chars().count();
    if chars < MIN_SYNC_PASSPHRASE_CHARS as usize {
        return Err(CoreError::InvalidArgument(format!(
            "the passphrase needs at least {MIN_SYNC_PASSPHRASE_CHARS} characters"
        )));
    }
    Ok(())
}

fn encode_state(state: &SyncFolderState) -> String {
    format!(
        "{} {} {} {}",
        state.uid_validity.0,
        state.uid_next,
        state.exists,
        state.highest_modseq.map_or(0, |m| m.0)
    )
}

fn default_device_name() -> String {
    match std::env::consts::OS {
        "macos" => "Mac".to_owned(),
        "windows" => "Windows PC".to_owned(),
        other => other.to_owned(),
    }
}

fn device_system() -> String {
    let os = match std::env::consts::OS {
        "macos" => "macOS",
        "windows" => "Windows",
        "linux" => "Linux",
        other => other,
    };
    format!("{os} ({})", std::env::consts::ARCH)
}

/// The account a record belongs to, for records that go with their account.
fn account_of(
    key: &RecordKey,
    record: Option<&LocalRecord>,
    state: &LocalState,
) -> Option<AccountId> {
    match key.kind() {
        RecordKind::Snooze
        | RecordKind::Pin
        | RecordKind::Task
        | RecordKind::ReadingList
        | RecordKind::AttachmentPin
        | RecordKind::MailboxPref
        | RecordKind::RemoteContent
        | RecordKind::Setting => key.id().split_once('/').and_then(|(a, _)| a.parse().ok()),
        RecordKind::Identity => identity_account(record?),
        RecordKind::Signature => {
            let identity = RecordKey::parse(RecordKind::Identity, key.id()).ok()?;
            identity_account(state.get(&identity)?)
        }
        _ => None,
    }
}

fn identity_account(record: &LocalRecord) -> Option<AccountId> {
    let content: IdentityContent = record.record.content_ref()?.decode().ok()?;
    Some(content.account)
}

/// Removals that wait for an open question about their account.
fn held_back(
    key: &RecordKey,
    before: Option<&LocalRecord>,
    after: Option<&LocalRecord>,
    state: &LocalState,
    questions: &BTreeSet<AccountId>,
) -> bool {
    let removes = match after {
        None => true,
        Some(after) => {
            after.record.is_tombstone() && before.is_some_and(|b| !b.record.is_tombstone())
        }
    };
    removes && account_of(key, before, state).is_some_and(|account| questions.contains(&account))
}

fn question_accounts(questions: &[Question]) -> BTreeSet<AccountId> {
    questions
        .iter()
        .filter_map(|q| q.key().id().parse().ok())
        .collect()
}

/// What a pass changed locally, as store writes.
fn diff_items(before: &LocalState, after: &LocalState, questions: &[Question]) -> Vec<SyncApply> {
    let held = question_accounts(questions);
    let keys: BTreeSet<&RecordKey> = before
        .records()
        .keys()
        .chain(after.records().keys())
        .collect();
    let mut items = Vec::new();
    for key in keys {
        let b = before.get(key);
        let a = after.get(key);
        if a == b || held_back(key, b, a, before, &held) {
            continue;
        }
        let mut table = match a {
            None => TableWrite::Delete,
            Some(a) if b.is_some_and(|b| b.record == a.record) => TableWrite::Keep,
            Some(a) => match a.record.content_ref() {
                Some(content) => TableWrite::Put(table_value(content)),
                None => TableWrite::Delete,
            },
        };
        // Accounts are removed only by the user.
        if key.kind() == RecordKind::Account && table == TableWrite::Delete {
            table = TableWrite::Keep;
        }
        items.push(SyncApply {
            kind: key.kind().name().to_owned(),
            id: key.id().to_owned(),
            expected: b.map(|r| r.record.version().to_string()),
            row: a.map(row_from_record),
            table,
            rekey_from: None,
        });
    }
    items
}

fn applied(record: &SyncRecord) -> LocalRecord {
    LocalRecord {
        record: record.clone(),
        synced: Some(record.version()),
    }
}

/// The records of a plan that exist here under another id: accounts by address, then (once
/// those carry the folder's ids) identities by account and address. Only local records that
/// were never synced take another id.
async fn collisions(
    store: &Store,
    plan: &super::plan::SyncPlan,
    local: &LocalState,
) -> Result<Vec<SyncApply>, CoreError> {
    let incoming = || {
        plan.groups.iter().filter_map(|group| match &group.local {
            Some(LocalChange::Apply(record))
                if local.get(&group.key).is_none() && !record.is_tombstone() =>
            {
                Some((&group.key, record))
            }
            _ => None,
        })
    };
    let unsynced = |key: &RecordKey| local.get(key).is_none_or(|r| r.synced.is_none());
    let accounts = store.accounts().await?;
    let mut items = Vec::new();
    for (key, record) in incoming().filter(|(k, _)| k.kind() == RecordKind::Account) {
        let Some(content) = record.content_ref() else {
            continue;
        };
        let Some(email) = content
            .field_values()
            .and_then(|v| v.get("email").and_then(|e| e.as_str().map(str::to_owned)))
        else {
            continue;
        };
        let Some(mine) = accounts
            .iter()
            .find(|a| a.email.eq_ignore_ascii_case(&email) && a.id.to_string() != key.id())
        else {
            continue;
        };
        if !unsynced(&RecordKey::account(mine.id)) {
            tracing::warn!(account = %mine.id, "an account of the folder has the address of a synced local account; left alone");
            continue;
        }
        items.push(SyncApply {
            kind: key.kind().name().to_owned(),
            id: key.id().to_owned(),
            expected: None,
            row: Some(row_from_record(&applied(record))),
            table: TableWrite::Put(table_value(content)),
            rekey_from: Some(mine.id.to_string()),
        });
    }
    if !items.is_empty() {
        return Ok(items);
    }
    let local_identities: Vec<(&RecordKey, IdentityContent)> = local
        .live()
        .filter(|(k, _)| k.kind() == RecordKind::Identity)
        .filter_map(|(k, c)| c.decode().ok().map(|c| (k, c)))
        .collect();
    for (key, record) in incoming().filter(|(k, _)| k.kind() == RecordKind::Identity) {
        let Some(content) = record.content_ref() else {
            continue;
        };
        let Ok(theirs) = content.decode::<IdentityContent>() else {
            continue;
        };
        let Some((mine, _)) = local_identities.iter().find(|(k, c)| {
            k.id() != key.id()
                && c.account == theirs.account
                && c.email.eq_ignore_ascii_case(&theirs.email)
        }) else {
            continue;
        };
        if !unsynced(mine) {
            continue;
        }
        items.push(SyncApply {
            kind: key.kind().name().to_owned(),
            id: key.id().to_owned(),
            expected: None,
            row: Some(row_from_record(&applied(record))),
            table: TableWrite::Put(table_value(content)),
            rekey_from: Some(mine.id().to_owned()),
        });
    }
    Ok(items)
}

/// Per kind, what joining would change (the preview).
fn preview_of(
    plan: &super::plan::SyncPlan,
    local: &LocalState,
    local_emails: &BTreeSet<String>,
) -> (Vec<SyncKindCount>, Vec<String>, u32, Vec<SyncDevice>) {
    let mut kinds: BTreeMap<&'static str, SyncKindCount> = BTreeMap::new();
    let mut new_accounts = Vec::new();
    let mut uploads = 0;
    let mut devices = Vec::new();
    for group in &plan.groups {
        if group
            .upload
            .as_ref()
            .is_some_and(|u| local.get(&group.key).is_some_and(|l| l.record == *u))
        {
            uploads += 1;
        }
        let Some(LocalChange::Apply(record)) = &group.local else {
            continue;
        };
        let kind = record.kind().name();
        let entry = kinds.entry(kind).or_insert_with(|| SyncKindCount {
            kind: kind.to_owned(),
            added: 0,
            changed: 0,
            removed: 0,
        });
        let here = local.content(&group.key).is_some();
        match (record.content_ref(), here) {
            (Some(_), false) => entry.added += 1,
            (Some(_), true) => entry.changed += 1,
            (None, true) => entry.removed += 1,
            (None, false) => {}
        }
        if let Some(content) = record.content_ref() {
            if record.kind() == RecordKind::Account
                && !here
                && let Some(email) = content
                    .field_values()
                    .and_then(|v| v.get("email").and_then(|e| e.as_str().map(str::to_owned)))
                && !local_emails.contains(&email.to_ascii_lowercase())
            {
                new_accounts.push(email);
            }
            if record.kind() == RecordKind::Device
                && let Ok(info) = content.decode::<super::record::DeviceInfo>()
            {
                devices.push(SyncDevice {
                    id: group.key.id().to_owned(),
                    name: info.name,
                    system: info.system,
                    last_seen_at: info.last_seen_ms,
                    current: false,
                });
            }
        }
    }
    (
        kinds
            .into_values()
            .filter(|k| k.added + k.changed + k.removed > 0)
            .collect(),
        new_accounts,
        uploads,
        devices,
    )
}

/// A record kept against a removal: the same content under a new version, which beats the
/// removal everywhere (for accounts every field gets the new version too).
fn renewed(record: &LocalRecord, clock: &mut HlcClock) -> Result<LocalRecord, SettingsSyncError> {
    let mut state = LocalState::new();
    state.insert(record.clone());
    let key = record.record.key().clone();
    let question = Question::Removed {
        key: key.clone(),
        tombstone: None,
    };
    state.keep(&question, clock)?;
    state
        .get(&key)
        .cloned()
        .ok_or_else(|| SettingsSyncError::Encrypt("renewed record vanished".into()))
}

impl<C: Connector> AppCore<C> {
    fn sync_work_factor(&self) -> WorkFactor {
        WorkFactor::new(self.sync_config().settings_sync_work_factor).unwrap_or(WorkFactor::DEFAULT)
    }

    /// The sync key: in memory, or from the keychain.
    async fn sync_key(&self) -> Result<Option<Arc<SyncKey>>, CoreError> {
        let state = self.settings_sync();
        if let Some(key) = lock(&state.key).clone() {
            return Ok(Some(key));
        }
        let Some(text) = self
            .credentials_ref()
            .get(&CredentialRef::for_settings_sync())
            .await?
        else {
            return Ok(None);
        };
        let key = Arc::new(SyncKey::import(&text).map_err(core_error)?);
        *lock(&state.key) = Some(Arc::clone(&key));
        Ok(Some(key))
    }

    async fn store_sync_key(&self, key: &Arc<SyncKey>) -> Result<(), CoreError> {
        self.credentials_ref()
            .set(&CredentialRef::for_settings_sync(), key.export())
            .await?;
        *lock(&self.settings_sync().key) = Some(Arc::clone(key));
        Ok(())
    }

    /// The folder's wire name in `account`: the one the mail sync saw, else at the top level,
    /// or below `INBOX` on servers that keep every folder there.
    async fn sync_folder_name(&self, account: AccountId) -> Result<String, CoreError> {
        if let Some(name) = self.settings_sync().signals.detected().get(&account) {
            return Ok(name.clone());
        }
        let mailboxes = self.store_ref()?.mailboxes(account).await?;
        let others: Vec<_> = mailboxes
            .iter()
            .filter(|m| !m.path.eq_ignore_ascii_case("INBOX"))
            .collect();
        let below_inbox = !others.is_empty()
            && others.iter().all(|m| {
                m.delimiter.as_deref().is_some_and(|d| {
                    m.path
                        .get(..5)
                        .is_some_and(|p| p.eq_ignore_ascii_case("INBOX"))
                        && m.path[5..].starts_with(d)
                })
            });
        Ok(match others.first().and_then(|m| m.delimiter.clone()) {
            Some(d) if below_inbox => format!("INBOX{d}{SETTINGS_SYNC_FOLDER}"),
            _ => SETTINGS_SYNC_FOLDER.to_owned(),
        })
    }

    async fn connect_sync_folder(
        &self,
        account: AccountId,
        folder: &str,
    ) -> Result<<C as SyncFolderConnector>::Folder, CoreError> {
        let store = self.store_ref()?;
        let stored = store
            .account(account)
            .await?
            .ok_or_else(|| CoreError::NotFound(format!("account {account}")))?;
        // Stage 14 (decision 0042): a password, or an access token for a sync account that
        // signs in with its provider.
        let login = self
            .credentials_ref()
            .login(&stored.credential_ref, &stored.imap, true)
            .await?;
        let (password, auth) = (login.secret, login.auth);
        // Read-only, like every connection of the account; the adapter lets it write to this
        // one folder only.
        let mut config = self.sync_config().connect_config(&stored.imap, password);
        crate::protocols::for_account(&mut config, &stored);
        Ok(self
            .connector_ref()
            .connect_sync_folder(&mail_model::ConnectConfig { auth, ..config }, folder)
            .await?)
    }

    /// The cached connection, or a new one, with the folder open. `create` creates a missing
    /// folder; otherwise a missing one is `NotSetUp`.
    async fn open_sync_folder(
        &self,
        runner: &mut Option<<C as SyncFolderConnector>::Folder>,
        account: AccountId,
        folder: &str,
        create: bool,
    ) -> Result<(<C as SyncFolderConnector>::Folder, SyncFolderState), CoreError> {
        let mut attempts = 0;
        loop {
            attempts += 1;
            let reused = runner.is_some();
            let mut connection = match runner.take() {
                Some(c) => c,
                None => self.connect_sync_folder(account, folder).await?,
            };
            match connection.open(create).await {
                Ok(Some(state)) => return Ok((connection, state)),
                Ok(None) => return Err(SettingsSyncFailure::NotSetUp.into()),
                // A kept connection the server closed meanwhile: once more with a new one.
                Err(err) if reused && attempts == 1 && err.is_transient() => {
                    tracing::debug!(error = %err, "settings sync connection renewed");
                }
                Err(err) => return Err(err.into()),
            }
        }
    }

    /// Writes this device's entry in the device list when it is missing or a day old.
    async fn touch_sync_device(&self, name: Option<String>) -> Result<(), CoreError> {
        let store = self.store_ref()?;
        let device = self.settings_sync().device.to_string();
        let now = self.now_ms();
        let own = store
            .sync_devices()
            .await?
            .into_iter()
            .find(|d| d.device_id == device);
        let due = own.as_ref().is_none_or(|d| now - d.last_seen_ms >= DAY_MS);
        if name.is_none() && !due {
            return Ok(());
        }
        let name = name
            .or_else(|| own.map(|d| d.name))
            .unwrap_or_else(default_device_name);
        store
            .put_sync_device(StoredSyncDevice {
                device_id: device,
                name,
                system: device_system(),
                last_seen_ms: now,
            })
            .await?;
        Ok(())
    }

    /// Runs a pass now (or checks the folder and finds nothing to do, unless `force`).
    /// Announces the status when the pass ran or its outcome changed.
    pub(crate) async fn settings_pass(&self, force: bool) -> Result<(), CoreError> {
        let state = self.settings_sync();
        let mut runner = state.runner.lock().await;
        let result = self.settings_pass_locked(&mut runner, force).await;
        drop(runner);
        let before = state.live().problem;
        let (ran, result) = match result {
            Ok(run) => {
                state.set_problem(None);
                (run == PassRun::Ran, Ok(()))
            }
            Err(err) => {
                let kind = match &err {
                    CoreError::SettingsSync(
                        SettingsSyncFailure::KeyChanged | SettingsSyncFailure::KeyMissing,
                    ) => SettingsSyncState::NeedsPassphrase,
                    _ => SettingsSyncState::Error,
                };
                state.set_problem(Some((kind, IpcError::from(&err))));
                (true, Err(err))
            }
        };
        if ran || state.live().problem != before {
            self.emit_settings_status().await;
        }
        result
    }

    async fn settings_pass_locked(
        &self,
        runner: &mut Option<<C as SyncFolderConnector>::Folder>,
        force: bool,
    ) -> Result<PassRun, CoreError> {
        let store = self.store_ref()?.clone();
        let row = store.sync_state().await?;
        let (true, Some(account), Some(folder_name)) =
            (row.enabled, row.account, row.folder.clone())
        else {
            return Ok(PassRun::Off);
        };
        let Some(key) = self.sync_key().await? else {
            // On, but the keychain has no key (restored from a backup, removed by hand).
            return Err(SettingsSyncFailure::KeyMissing.into());
        };
        self.touch_sync_device(None).await?;
        let (mut folder, opened) = self
            .open_sync_folder(runner, account, &folder_name, false)
            .await?;
        let now = self.now_ms();
        let full = row
            .last_full_check_ms
            .is_none_or(|t| now.saturating_sub(t) >= DAY_MS);
        let mode = if full {
            PassMode::FullCheck
        } else {
            PassMode::Changes
        };
        let encoded = encode_state(&opened);
        let pending = store.sync_pending().await?;
        let live = self.settings_sync().live();
        if !force
            && !full
            && live.ran
            && !live.deferred
            && pending == 0
            && row.folder_state.as_deref() == Some(encoded.as_str())
        {
            *runner = Some(folder);
            return Ok(PassRun::Skipped);
        }
        let result = self
            .run_settings_pass(&store, &row, &key, &mut folder, now, mode, encoded)
            .await;
        if result.is_ok() {
            *runner = Some(folder);
        } else if let Err(err) = folder.logout().await {
            tracing::debug!(error = %err, "LOGOUT of the settings sync connection failed");
        }
        result.map(|()| PassRun::Ran)
    }

    #[allow(clippy::too_many_arguments)]
    async fn run_settings_pass(
        &self,
        store: &Store,
        row: &SyncStateRow,
        key: &SyncKey,
        folder: &mut <C as SyncFolderConnector>::Folder,
        now: i64,
        mode: PassMode,
        encoded: String,
    ) -> Result<(), CoreError> {
        let state = self.settings_sync();
        let started = std::time::Instant::now();
        let mut transport = ImapTransport::new(folder, now);
        let mut local = load_state(&store.sync_records().await?, row.last_complete_sync_ms);
        let mut rounds = 0;
        let prepared = loop {
            rounds += 1;
            let fetched = fetch_pass(&mut transport, key, &local, now, mode)
                .await
                .map_err(core_error)?;
            let prepared = {
                let mut clock = lock_clock(&state.clock);
                fetched.plan(&mut clock).map_err(core_error)?
            };
            let rekeys = if rounds < 3 {
                collisions(store, &prepared.plan, &local).await?
            } else {
                Vec::new()
            };
            if rekeys.is_empty() {
                break prepared;
            }
            let outcome = store.apply_sync(rekeys, None).await?;
            self.after_apply(&outcome).await;
            local = load_state(&store.sync_records().await?, row.last_complete_sync_ms);
        };
        let questions = prepared.plan.questions.clone();
        let before = local.clone();
        let report: PassReport = execute_pass(&mut transport, key, &mut local, prepared).await;
        drop(transport);
        let items = diff_items(&before, &local, &questions);
        let hlc_last = lock_clock(&state.clock).last().map(|v| v.to_string());
        let progress = SyncProgress {
            last_complete_sync_ms: local.last_complete_sync_ms,
            last_full_check_ms: (mode == PassMode::FullCheck).then_some(now),
            folder_state: Some(encoded),
            hlc_last,
        };
        let outcome = store.apply_sync(items, Some(progress)).await?;
        self.after_apply(&outcome).await;
        // Entries left out of this pass: rejected, unreadable, too large, or postponed by the
        // download limits (finding F2).
        let rejected = report.notices.iter().filter(|n| n.is_left_out()).count();
        let postponed = report
            .notices
            .iter()
            .filter(|n| matches!(n, Notice::Postponed { .. }))
            .count();
        for notice in &report.notices {
            match notice {
                Notice::Rejected { handle, error } => {
                    tracing::warn!(uid = handle.0, %error, "settings sync entry rejected");
                }
                Notice::InvalidHeader { handle } => {
                    tracing::warn!(uid = handle.0, "settings sync entry with an invalid header");
                }
                Notice::Deferred { ahead_ms, .. } => {
                    tracing::info!(ahead_ms, "settings sync entry from a clock ahead deferred");
                }
                Notice::TooLarge { handle, size } => {
                    tracing::warn!(uid = handle.0, size, "settings sync entry too large");
                }
                // Counted in the summary below.
                Notice::Postponed { .. } => {}
            }
        }
        {
            let mut live = lock(&state.live);
            live.rejected = u32::try_from(rejected).unwrap_or(u32::MAX);
            live.questions = report.questions.clone();
            live.deferred = report.notices.iter().any(Notice::waits);
            live.ran = true;
        }
        tracing::info!(
            uploaded = report.uploaded,
            deleted = report.deleted,
            changed = report.changed.len(),
            applied = outcome.applied.len(),
            skipped = outcome.skipped.len(),
            questions = report.questions.len(),
            rejected,
            postponed,
            full_check = mode == PassMode::FullCheck,
            elapsed_ms = started.elapsed().as_millis(),
            "settings sync pass"
        );
        if let Some(failure) = report.failed.first() {
            return Err(core_error(SettingsSyncError::Transport(
                failure.error.clone(),
            )));
        }
        Ok(())
    }

    /// Starts, restarts and announces what a pass changed.
    async fn after_apply(&self, outcome: &ApplyOutcome) {
        for (old, new) in &outcome.accounts_rekeyed {
            self.rekeyed_account(*old, *new).await;
        }
        for added in &outcome.accounts_added {
            self.start_synced_account(*added).await;
        }
        // Decision 0040: a pass never changes the servers of an existing account (other
        // servers wait for the user), so the accounts it updated keep their connections.
        for changed in &outcome.accounts_server_changed {
            tracing::warn!(
                account = %changed,
                "server settings changed on another device; kept until the user decides"
            );
        }
        let kinds = &outcome.kinds;
        let any = |names: &[&str]| names.iter().any(|n| kinds.contains(*n));
        if any(&["account", "identity", "signature", "accountGroup"])
            || !outcome.accounts_rekeyed.is_empty()
        {
            self.emit_event(CoreEvent::AccountsChanged);
        }
        if any(&["setting"])
            && let Ok(preferences) = self.get_preferences().await
        {
            self.emit_event(CoreEvent::PreferencesChanged { preferences });
        }
        // Stage 8: shortcuts or text size from another device (decision 0025).
        if any(&["setting"])
            && let Ok(preferences) = self.get_ui_preferences().await
        {
            self.emit_event(CoreEvent::UiPreferencesChanged { preferences });
        }
        if any(&["signature"]) {
            self.emit_event(CoreEvent::SignaturesChanged);
        }
        if any(&["template"]) {
            self.emit_event(CoreEvent::TemplatesChanged);
        }
        // Stage 7 (decision 0026): saved searches from another device.
        if any(&["savedView"]) {
            self.emit_event(CoreEvent::SavedSearchesChanged);
        }
        // Stage 17 (decision 0050): names, notes, merges, pictures and spam marks of contacts.
        // Stage 19 (decision 0056): contact groups too.
        if any(&["contact", "contactPicture", "blockedSender", "contactList"]) {
            self.emit_event(CoreEvent::ContactsChanged);
        }
        if any(&["senderRule"]) {
            self.comfort().kick_categories();
        }
        if any(&["snooze"]) {
            self.comfort().wake_snoozes();
        }
        if any(&[
            "senderRule",
            "snooze",
            "pin",
            "task",
            "rule",
            "readingList",
            "blockedSender",
            "attachmentPin",
        ]) {
            self.emit_event(CoreEvent::InboxChanged { account_id: None });
        }
        // Stage 16: favourites and colours are part of the folder list.
        if any(&["mailboxPref"]) {
            self.mailbox_prefs_changed().await;
        }
        for (kind, id, reason) in &outcome.skipped {
            tracing::debug!(%kind, ?reason, record = %id.len(), "settings sync record not applied now");
        }
    }

    pub(crate) async fn emit_settings_status(&self) {
        match self.get_settings_sync_status().await {
            Ok(status) => self.emit_event(CoreEvent::SettingsSyncChanged { status }),
            Err(err) => tracing::debug!(error = %err, "settings sync status unavailable"),
        }
    }

    // -----------------------------------------------------------------------------------------
    // Commands

    /// The settings page's view of the sync.
    pub async fn get_settings_sync_status(&self) -> Result<SettingsSyncStatus, CoreError> {
        let store = self.store_ref()?;
        let row = store.sync_state().await?;
        let state = self.settings_sync();
        let live = state.live();
        let on = row.enabled && row.account.is_some();
        let accounts = store.accounts().await?;
        let email_of = |id: AccountId| {
            accounts
                .iter()
                .find(|a| a.id == id)
                .map(|a| a.email.clone())
                .unwrap_or_default()
        };
        let device = state.device.to_string();
        let devices = store
            .sync_devices()
            .await?
            .into_iter()
            .map(|d| SyncDevice {
                current: d.device_id == device,
                id: d.device_id,
                name: d.name,
                system: d.system,
                last_seen_at: d.last_seen_ms,
            })
            .collect();
        let questions = if on {
            live.questions
                .iter()
                .filter_map(|q| q.key().id().parse::<AccountId>().ok())
                .map(|account_id| SyncQuestion::AccountRemoved {
                    email: email_of(account_id),
                    account_id,
                })
                .collect()
        } else {
            Vec::new()
        };
        let (status, last_error) = match (&live.problem, on) {
            (_, false) => (SettingsSyncState::Off, None),
            (Some((kind, error)), true) => (*kind, Some(error.clone())),
            (None, true) => (SettingsSyncState::Active, None),
        };
        Ok(SettingsSyncStatus {
            state: status,
            account_id: row.account.filter(|_| on),
            folder: row.folder.clone().filter(|_| on),
            last_sync_at: row.last_complete_sync_ms,
            last_error,
            pending_changes: store.sync_pending().await?,
            rejected_entries: if on { live.rejected } else { 0 },
            devices,
            questions,
            accounts_needing_password: store.sync_adopted_accounts().await?,
            detected_accounts: if on {
                Vec::new()
            } else {
                state.signals.detected().into_keys().collect()
            },
            // Shown whether the sync is on or not: the change waits until the user decides.
            server_changes: store
                .account_server_changes()
                .await?
                .into_iter()
                .map(|change| SyncServerChange {
                    account_id: change.account,
                    imap: change.imap,
                    smtp: change.smtp,
                })
                .collect(),
        })
    }

    /// Turns the sync on with `account`: creates the folder (not subscribed), a new key
    /// protected by `passphrase`, and uploads this device's data. Allows writing to that one
    /// folder only (decision 0014). `sync_already_set_up` if the folder holds a sync: join it.
    pub async fn enable_settings_sync(
        &self,
        account: AccountId,
        passphrase: Secret<String>,
    ) -> Result<SettingsSyncStatus, CoreError> {
        check_passphrase(&passphrase)?;
        let store = self.store_ref()?.clone();
        let folder_name = self.sync_folder_name(account).await?;
        let state = self.settings_sync();
        {
            let mut runner = state.runner.lock().await;
            if let Some(old) = runner.take() {
                old.logout().await.ok();
            }
            let (mut folder, _) = self
                .open_sync_folder(&mut runner, account, &folder_name, true)
                .await?;
            let mut temp = state.temp_clock(self.wall_clock());
            let result = {
                let mut transport = ImapTransport::new(&mut folder, self.now_ms());
                super::pass::setup(
                    &mut transport,
                    &passphrase,
                    &mut temp,
                    self.sync_work_factor(),
                )
                .await
            };
            state.absorb(&temp);
            let key = Arc::new(result.map_err(core_error)?);
            self.store_sync_key(&key).await?;
            store.forget_sync().await?;
            store
                .set_sync_enabled(true, Some(account), Some(folder_name))
                .await?;
            *lock(&state.signals.sync_account) = Some(account);
            *runner = Some(folder);
        }
        tracing::info!(%account, "settings sync turned on");
        self.touch_sync_device(None).await?;
        self.settings_pass(true).await?;
        self.get_settings_sync_status().await
    }

    /// Second device: unlocks the folder's key with `passphrase` and shows what joining would
    /// take over. Writes nothing (a wrong passphrase is `sync_wrong_passphrase` and changes
    /// nothing). `joinSettingsSync` then merges.
    pub async fn preview_settings_sync_join(
        &self,
        account: AccountId,
        passphrase: Secret<String>,
    ) -> Result<SettingsSyncPreview, CoreError> {
        let store = self.store_ref()?.clone();
        let folder_name = self.sync_folder_name(account).await?;
        let state = self.settings_sync();
        let mut runner = state.runner.lock().await;
        // Another account's connection, or a stale one: a new one for this account.
        if let Some(old) = runner.take() {
            old.logout().await.ok();
        }
        let (mut folder, _) = self
            .open_sync_folder(&mut runner, account, &folder_name, false)
            .await?;
        let now = self.now_ms();
        let result = async {
            let mut transport = ImapTransport::new(&mut folder, now);
            let key = super::pass::join(&mut transport, &passphrase)
                .await
                .map_err(core_error)?;
            let mut local = load_state(&store.sync_records().await?, None);
            local.forget_sync();
            let fetched = fetch_pass(&mut transport, &key, &local, now, PassMode::Changes)
                .await
                .map_err(core_error)?;
            let mut temp = state.temp_clock(self.wall_clock());
            let prepared = fetched.plan(&mut temp).map_err(core_error)?;
            let emails: BTreeSet<String> = store
                .accounts()
                .await?
                .into_iter()
                .map(|a| a.email.to_ascii_lowercase())
                .collect();
            let (kinds, new_accounts, uploads, devices) =
                preview_of(&prepared.plan, &local, &emails);
            Ok::<_, CoreError>((key, kinds, new_accounts, uploads, devices))
        }
        .await;
        *runner = Some(folder);
        let (key, kinds, new_accounts, uploads, devices) = result?;
        *lock(&state.pending) = Some(PendingJoin {
            account,
            folder: folder_name,
            key: Arc::new(key),
        });
        Ok(SettingsSyncPreview {
            kinds,
            new_accounts,
            uploads,
            devices,
        })
    }

    /// Confirms the join the preview showed: the key goes to the keychain, the sync turns on
    /// and the first pass merges this device's data with the folder. Accounts taken over have
    /// no password here yet (`accountsNeedingPassword`).
    pub async fn join_settings_sync(&self) -> Result<SettingsSyncStatus, CoreError> {
        let state = self.settings_sync();
        let pending = lock(&state.pending).take().ok_or_else(|| {
            CoreError::InvalidArgument("preview the join with the passphrase first".into())
        })?;
        let store = self.store_ref()?;
        let same_key = lock(&state.key)
            .as_ref()
            .is_some_and(|k| k.key_record_id() == pending.key.key_record_id());
        self.store_sync_key(&pending.key).await?;
        if !same_key {
            // A new device, or the folder was reset elsewhere: merge like a new device.
            store.forget_sync().await?;
        }
        store
            .set_sync_enabled(true, Some(pending.account), Some(pending.folder))
            .await?;
        *lock(&state.signals.sync_account) = Some(pending.account);
        state.set_problem(None);
        tracing::info!(account = %pending.account, "settings sync joined");
        self.touch_sync_device(None).await?;
        self.settings_pass(true).await?;
        self.get_settings_sync_status().await
    }

    /// A pass now (the settings page's "sync now").
    pub async fn sync_settings_now(&self) -> Result<SettingsSyncStatus, CoreError> {
        if !self.store_ref()?.sync_state().await?.enabled {
            return Err(SettingsSyncFailure::Off.into());
        }
        self.settings_pass(true).await?;
        self.get_settings_sync_status().await
    }

    /// Protects the key with a new passphrase (the old one stops working; other devices keep
    /// working with the key they hold).
    pub async fn change_settings_sync_passphrase(
        &self,
        passphrase: Secret<String>,
    ) -> Result<(), CoreError> {
        check_passphrase(&passphrase)?;
        let store = self.store_ref()?;
        let row = store.sync_state().await?;
        let (true, Some(account), Some(folder_name)) = (row.enabled, row.account, row.folder)
        else {
            return Err(SettingsSyncFailure::Off.into());
        };
        let key = self
            .sync_key()
            .await?
            .ok_or(SettingsSyncFailure::KeyMissing)?;
        let state = self.settings_sync();
        let mut runner = state.runner.lock().await;
        let (mut folder, _) = self
            .open_sync_folder(&mut runner, account, &folder_name, false)
            .await?;
        let mut temp = state.temp_clock(self.wall_clock());
        let result = {
            let mut transport = ImapTransport::new(&mut folder, self.now_ms());
            change_passphrase(
                &mut transport,
                &key,
                &passphrase,
                &mut temp,
                self.sync_work_factor(),
            )
            .await
        };
        state.absorb(&temp);
        *runner = Some(folder);
        result.map_err(core_error)?;
        tracing::info!("settings sync passphrase changed");
        Ok(())
    }

    /// Starts over (forgotten passphrase, or "reset sync"): every entry of the folder is
    /// deleted, a new key is protected by `passphrase`, and this device uploads its data.
    /// Other devices see `needsPassphrase` and join again with the new passphrase.
    pub async fn reset_settings_sync(
        &self,
        passphrase: Secret<String>,
    ) -> Result<SettingsSyncStatus, CoreError> {
        check_passphrase(&passphrase)?;
        let store = self.store_ref()?.clone();
        let row = store.sync_state().await?;
        let (Some(account), Some(folder_name)) = (row.account, row.folder) else {
            return Err(SettingsSyncFailure::Off.into());
        };
        let state = self.settings_sync();
        {
            let mut runner = state.runner.lock().await;
            let (mut folder, _) = self
                .open_sync_folder(&mut runner, account, &folder_name, true)
                .await?;
            let mut temp = state.temp_clock(self.wall_clock());
            let mut local = LocalState::new();
            let result = {
                let mut transport = ImapTransport::new(&mut folder, self.now_ms());
                super::pass::reset(
                    &mut transport,
                    &passphrase,
                    &mut local,
                    &mut temp,
                    self.sync_work_factor(),
                )
                .await
            };
            state.absorb(&temp);
            *runner = Some(folder);
            let key = Arc::new(result.map_err(core_error)?);
            self.store_sync_key(&key).await?;
            store.forget_sync().await?;
            store
                .set_sync_enabled(true, Some(account), Some(folder_name))
                .await?;
            *lock(&state.signals.sync_account) = Some(account);
            *lock(&state.live) = Live::default();
        }
        tracing::info!("settings sync reset");
        self.settings_pass(true).await?;
        self.get_settings_sync_status().await
    }

    /// Turns the sync off on this device: the key leaves the keychain, the folder stays.
    pub async fn disable_settings_sync(&self) -> Result<SettingsSyncStatus, CoreError> {
        let store = self.store_ref()?;
        store.set_sync_enabled(false, None, None).await?;
        let state = self.settings_sync();
        *lock(&state.key) = None;
        *lock(&state.live) = Live::default();
        *lock(&state.signals.sync_account) = None;
        *lock(&state.pending) = None;
        if let Some(folder) = state.runner.lock().await.take() {
            folder.logout().await.ok();
        }
        self.credentials_ref()
            .delete(&CredentialRef::for_settings_sync())
            .await?;
        tracing::info!("settings sync turned off");
        let status = self.get_settings_sync_status().await?;
        self.emit_event(CoreEvent::SettingsSyncChanged {
            status: status.clone(),
        });
        Ok(status)
    }

    /// Answers "removed on another device": `remove` removes the account here too; otherwise
    /// it is kept and comes back on every device (with its identities, snoozes and the rest).
    pub async fn resolve_sync_question(
        &self,
        account: AccountId,
        remove: bool,
    ) -> Result<SettingsSyncStatus, CoreError> {
        let state = self.settings_sync();
        let key = RecordKey::account(account);
        let question = state
            .live()
            .questions
            .into_iter()
            .find(|q| q.key() == &key)
            .ok_or_else(|| CoreError::NotFound(format!("question about account {account}")))?;
        let store = self.store_ref()?.clone();
        {
            let _runner = state.runner.lock().await;
            let mut local = load_state(&store.sync_records().await?, None);
            if remove {
                let before = local.get(&key).map(|r| r.record.version().to_string());
                local.accept_removal(&question);
                store
                    .apply_sync(
                        vec![SyncApply {
                            kind: key.kind().name().to_owned(),
                            id: key.id().to_owned(),
                            expected: before,
                            row: local.get(&key).map(row_from_record),
                            table: TableWrite::Keep,
                            rekey_from: None,
                        }],
                        None,
                    )
                    .await?;
            } else {
                let (rows, last) = {
                    let mut clock = lock_clock(&state.clock);
                    let mut rows = Vec::new();
                    for (record_key, record) in local.records() {
                        let belongs = record_key == &key
                            || account_of(record_key, Some(record), &local) == Some(account);
                        if !belongs || record.record.is_tombstone() {
                            continue;
                        }
                        let renewed = renewed(record, &mut clock).map_err(core_error)?;
                        rows.push((
                            Some(record.record.version().to_string()),
                            row_from_record(&renewed),
                        ));
                    }
                    (rows, clock.last().map(|v| v.to_string()))
                };
                store.put_sync_records(rows).await?;
                store
                    .set_sync_progress(SyncProgress {
                        hlc_last: last,
                        ..SyncProgress::default()
                    })
                    .await?;
            }
            lock(&state.live).questions.retain(|q| q.key() != &key);
        }
        if remove {
            self.remove_account(account).await?;
        }
        state.signals.wake();
        let status = self.get_settings_sync_status().await?;
        self.emit_event(CoreEvent::SettingsSyncChanged {
            status: status.clone(),
        });
        Ok(status)
    }

    /// Answers "server settings changed on another device" (decision 0040). `apply` takes the
    /// new servers: writing is switched off and the stored password is removed, so it never
    /// reaches servers named only by the sync folder; the account then asks for its password.
    /// Otherwise this device keeps its servers and the same change is not offered again.
    /// `not_found` if no change waits for `account`.
    pub async fn resolve_server_change(
        &self,
        account: AccountId,
        apply: bool,
    ) -> Result<SettingsSyncStatus, CoreError> {
        if apply {
            self.apply_server_change(account).await?;
        } else {
            self.store_ref()?
                .dismiss_account_server_change(account)
                .await?;
        }
        tracing::info!(%account, apply, "server change from the settings sync resolved");
        let status = self.get_settings_sync_status().await?;
        self.emit_event(CoreEvent::SettingsSyncChanged {
            status: status.clone(),
        });
        Ok(status)
    }

    /// Renames this device in the device list.
    pub async fn set_sync_device_name(
        &self,
        name: String,
    ) -> Result<SettingsSyncStatus, CoreError> {
        let name = name.trim().to_owned();
        if name.is_empty() || name.chars().count() > 64 || name.chars().any(char::is_control) {
            return Err(CoreError::InvalidArgument(
                "a device name has 1 to 64 characters".into(),
            ));
        }
        self.touch_sync_device(Some(name)).await?;
        self.settings_sync().signals.wake();
        self.get_settings_sync_status().await
    }

    /// This device's random id in the sync (the `current` entry of the device list).
    pub fn settings_sync_device_id(&self) -> String {
        self.settings_sync().device.to_string()
    }

    /// The sync account was removed here: the sync is off.
    pub(crate) async fn sync_account_removed(&self, account: AccountId) {
        let Ok(store) = self.store_ref() else {
            return;
        };
        let Ok(row) = store.sync_state().await else {
            return;
        };
        if row.enabled
            && row.account == Some(account)
            && let Err(err) = self.disable_settings_sync().await
        {
            tracing::warn!(error = %err, "settings sync could not be turned off");
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::settings_sync::hlc::DeviceId;
    use crate::settings_sync::record::SyncContent;

    fn clock() -> HlcClock {
        HlcClock::new(
            Arc::new(crate::clock::SystemClock),
            DeviceId::from_uuid(uuid::Uuid::from_u128(5)),
        )
    }

    #[test]
    fn removals_of_an_accounts_records_wait_for_its_question() {
        let mut hlc = clock();
        let account: AccountId = "6f9619ff-8b86-4d11-b42d-00c04fc964ff".parse().expect("id");
        let mut before = LocalState::new();
        let snooze = RecordKey::mail(RecordKind::Snooze, account, "m@x").expect("key");
        before
            .put(
                snooze.clone(),
                SyncContent::new(RecordKind::Snooze, json!({ "untilMs": 1 })).expect("content"),
                &mut hlc,
            )
            .expect("put");
        let mut after = before.clone();
        after.delete(&snooze, &mut hlc).expect("delete");
        let question = Question::Removed {
            key: RecordKey::account(account),
            tombstone: None,
        };
        assert!(diff_items(&before, &after, std::slice::from_ref(&question)).is_empty());
        let items = diff_items(&before, &after, &[]);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].table, TableWrite::Delete);
    }
}
