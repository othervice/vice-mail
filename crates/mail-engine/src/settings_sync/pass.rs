//! Running a pass over a [`SettingsTransport`], and setting up, joining, re-protecting and
//! resetting the sync folder. All IO goes through the transport; the local records are an
//! in-memory [`LocalState`] that the integration loads from and writes back to its tables.

use std::collections::BTreeMap;

use mail_model::Secret;
use serde_json::Value;

use super::crypto::{RecordHeader, SyncKey, WorkFactor};
use super::error::{SettingsSyncError, TransportError};
use super::hlc::{Hlc, HlcClock};
use super::plan::{
    FetchRequest, LocalChange, LocalRecord, Notice, PlanContext, Planner, Question, SyncPlan,
};
use super::record::{FieldValue, MergeRule, RecordKey, SyncContent, SyncRecord};
use super::transport::{EntryHandle, RemoteEntry, SettingsTransport};

/// The local side of the sync: every synchronised record of this device and its sync state.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LocalState {
    records: BTreeMap<RecordKey, LocalRecord>,
    /// When this device last listed the complete folder in a successful pass.
    pub last_complete_sync_ms: Option<i64>,
}

impl LocalState {
    pub fn new() -> Self {
        Self::default()
    }

    pub const fn records(&self) -> &BTreeMap<RecordKey, LocalRecord> {
        &self.records
    }

    pub fn get(&self, key: &RecordKey) -> Option<&LocalRecord> {
        self.records.get(key)
    }

    /// The live content of a record (`None` if absent or deleted).
    pub fn content(&self, key: &RecordKey) -> Option<&SyncContent> {
        self.records.get(key).and_then(|r| r.record.content_ref())
    }

    /// Live records with their content.
    pub fn live(&self) -> impl Iterator<Item = (&RecordKey, &SyncContent)> {
        self.records
            .iter()
            .filter_map(|(k, r)| r.record.content_ref().map(|c| (k, c)))
    }

    /// Loads a record as stored by the integration.
    pub fn insert(&mut self, record: LocalRecord) {
        self.records.insert(record.record.key().clone(), record);
    }

    /// A local change: stores `content` under a new version. Unchanged content keeps its
    /// version (setting a value to what it is already is not a change).
    pub fn put(
        &mut self,
        key: RecordKey,
        content: SyncContent,
        hlc: &mut HlcClock,
    ) -> Result<Hlc, SettingsSyncError> {
        if let Some(existing) = self.records.get(&key)
            && existing.record.content_ref() == Some(&content)
        {
            return Ok(existing.record.version());
        }
        let version = hlc.now()?;
        let record = SyncRecord::content(key.clone(), version, content)?;
        self.store(key, record);
        Ok(version)
    }

    /// A local change of a per-field record (accounts): fields whose value differs from the
    /// current content get a new version, the others keep theirs. Fields not in `values`
    /// keep their value.
    pub fn put_fields(
        &mut self,
        key: RecordKey,
        values: BTreeMap<String, Value>,
        hlc: &mut HlcClock,
    ) -> Result<Hlc, SettingsSyncError> {
        let kind = key.kind();
        if kind.spec().merge != MergeRule::PerField {
            return Err(super::error::RecordError::InvalidContent {
                kind: kind.name(),
                reason: "kind has no per-field versions".to_owned(),
            }
            .into());
        }
        let mut fields: BTreeMap<String, FieldValue> = self
            .content(&key)
            .and_then(SyncContent::fields)
            .unwrap_or_default();
        let changed: Vec<(String, Value)> = values
            .into_iter()
            .filter(|(name, value)| fields.get(name).is_none_or(|f| &f.value != value))
            .collect();
        if changed.is_empty()
            && let Some(existing) = self.records.get(&key)
            && !existing.record.is_tombstone()
        {
            return Ok(existing.record.version());
        }
        let version = hlc.now()?;
        for (name, value) in changed {
            fields.insert(name, FieldValue { version, value });
        }
        let content = SyncContent::from_fields(kind, &fields)?;
        let record = SyncRecord::content(key.clone(), version, content)?;
        self.store(key, record);
        Ok(version)
    }

    /// A local delete: replaces live content by a tombstone. `None` if there was nothing live.
    pub fn delete(
        &mut self,
        key: &RecordKey,
        hlc: &mut HlcClock,
    ) -> Result<Option<Hlc>, SettingsSyncError> {
        let Some(existing) = self.records.get(key) else {
            return Ok(None);
        };
        if existing.record.is_tombstone() {
            return Ok(None);
        }
        let deleted = existing.record.version();
        let version = hlc.now()?;
        let record = SyncRecord::tombstone(key.clone(), version, deleted)?;
        self.store(key.clone(), record);
        Ok(Some(version))
    }

    fn store(&mut self, key: RecordKey, record: SyncRecord) {
        let synced = self.records.get(&key).and_then(|r| r.synced);
        self.records.insert(key, LocalRecord { record, synced });
    }

    /// The user agreed to a removal: apply it here.
    pub fn accept_removal(&mut self, question: &Question) {
        match question {
            Question::Removed {
                tombstone: Some(tombstone),
                key,
            } => {
                self.records.insert(
                    key.clone(),
                    LocalRecord {
                        record: tombstone.clone(),
                        synced: Some(tombstone.version()),
                    },
                );
            }
            Question::Removed {
                tombstone: None,
                key,
            } => {
                self.records.remove(key);
            }
        }
    }

    /// The user wants to keep a removed record: it is stored again under a new version, which
    /// beats the removal on every device.
    pub fn keep(
        &mut self,
        question: &Question,
        hlc: &mut HlcClock,
    ) -> Result<Option<Hlc>, SettingsSyncError> {
        let key = question.key();
        let Some(content) = self.content(key).cloned() else {
            return Ok(None);
        };
        let version = hlc.now()?;
        let content = match content.fields() {
            // Per-field: the fields carry the new version too, so the kept values win.
            Some(fields) => {
                let renewed = fields
                    .into_iter()
                    .map(|(name, f)| {
                        (
                            name,
                            FieldValue {
                                version,
                                value: f.value,
                            },
                        )
                    })
                    .collect();
                SyncContent::from_fields(key.kind(), &renewed)?
            }
            None => content,
        };
        let record = SyncRecord::content(key.clone(), version, content)?;
        self.store(key.clone(), record);
        Ok(Some(version))
    }

    /// Forgets what was synced (after joining a reset folder): every record counts as new and
    /// is merged with the folder as on a new device.
    pub fn forget_sync(&mut self) {
        for record in self.records.values_mut() {
            record.synced = None;
        }
        self.last_complete_sync_ms = None;
    }

    fn apply(&mut self, key: &RecordKey, change: &LocalChange) {
        match change {
            LocalChange::Apply(record) => {
                self.records.insert(
                    key.clone(),
                    LocalRecord {
                        record: record.clone(),
                        synced: Some(record.version()),
                    },
                );
            }
            LocalChange::MarkSynced(version) => {
                if let Some(record) = self.records.get_mut(key) {
                    record.synced = Some(*version);
                }
            }
            LocalChange::Forget => {
                self.records.remove(key);
            }
        }
    }
}

/// What a pass did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PassReport {
    pub uploaded: usize,
    /// Entries deleted (losers, expired tombstones, old key records).
    pub deleted: usize,
    /// Records whose local state changed (stored, deleted or forgotten).
    pub changed: Vec<RecordKey>,
    pub questions: Vec<Question>,
    pub notices: Vec<Notice>,
    /// Uploads and deletions that failed; retried by the next pass.
    pub failed: Vec<Failure>,
}

/// An upload or deletion that failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    /// The record, `None` for the cleanup of old key records.
    pub key: Option<RecordKey>,
    pub error: TransportError,
}

impl PassReport {
    /// Nothing was written, deleted or changed.
    pub fn is_quiet(&self) -> bool {
        self.uploaded == 0 && self.deleted == 0 && self.changed.is_empty() && self.failed.is_empty()
    }
}

/// A planned pass, before anything is written ([`prepare_pass`]); also the preview a new
/// device shows before merging.
#[derive(Debug, Clone)]
pub struct PreparedPass {
    pub plan: SyncPlan,
    /// Older copies of this key's key record (an interrupted passphrase change).
    old_key_records: Vec<EntryHandle>,
    listed_at_ms: i64,
}

/// How much a pass downloads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PassMode {
    /// Only entries that are new to this device (the normal pass, as part of every sync).
    #[default]
    Changes,
    /// Every entry, to find copies damaged on the server ([`PlanContext::full_check`]).
    FullCheck,
}

/// One pass: list, download what is new, merge, upload, delete, apply locally.
pub async fn run_pass<T: SettingsTransport>(
    transport: &mut T,
    key: &SyncKey,
    state: &mut LocalState,
    hlc: &mut HlcClock,
    mode: PassMode,
) -> Result<PassReport, SettingsSyncError> {
    let prepared = prepare_pass(transport, key, state, hlc, mode).await?;
    Ok(execute_pass(transport, key, state, prepared).await)
}

/// Lists, downloads, opens and plans. Writes nothing.
///
/// Fails with [`SettingsSyncError::NotSetUp`] if the folder has no key record and with
/// [`SettingsSyncError::KeyChanged`] if it only has key records of another key.
pub async fn prepare_pass<T: SettingsTransport>(
    transport: &mut T,
    key: &SyncKey,
    state: &LocalState,
    hlc: &mut HlcClock,
    mode: PassMode,
) -> Result<PreparedPass, SettingsSyncError> {
    let listed_at_ms = hlc.wall_ms();
    let listing = transport.list().await?;
    let mut ours = key_records(&listing)
        .into_iter()
        .filter(|(_, header)| header.opaque_id == key.key_record_id())
        .collect::<Vec<_>>();
    if ours.is_empty() {
        return Err(if key_records(&listing).is_empty() {
            SettingsSyncError::NotSetUp
        } else {
            SettingsSyncError::KeyChanged
        });
    }
    ours.sort_by_key(|(handle, header)| (header.version, *handle));
    ours.pop();
    let old_key_records = ours.into_iter().map(|(handle, _)| handle).collect();

    let ctx = PlanContext {
        now_ms: listed_at_ms,
        last_complete_sync_ms: state.last_complete_sync_ms,
        full_check: mode == PassMode::FullCheck,
    };
    let planner = Planner::new(key, &state.records, &listing, ctx);
    let requests = planner.to_fetch();
    let opened = if requests.is_empty() {
        Vec::new()
    } else {
        let handles: Vec<EntryHandle> = requests.iter().map(|r| r.handle).collect();
        let bodies: BTreeMap<EntryHandle, Vec<u8>> =
            transport.read(&handles).await?.into_iter().collect();
        requests
            .iter()
            .filter_map(|FetchRequest { handle, header }| {
                bodies
                    .get(handle)
                    .map(|body| (*handle, key.open(header, body)))
            })
            .collect()
    };
    let plan = planner.finish(opened, hlc)?;
    Ok(PreparedPass {
        plan,
        old_key_records,
        listed_at_ms,
    })
}

/// Executes a prepared plan. Failures of single uploads or deletions are collected in the
/// report; the group is retried by the next pass.
pub async fn execute_pass<T: SettingsTransport>(
    transport: &mut T,
    key: &SyncKey,
    state: &mut LocalState,
    prepared: PreparedPass,
) -> PassReport {
    let PreparedPass {
        plan,
        old_key_records,
        listed_at_ms,
    } = prepared;
    let mut report = PassReport {
        questions: plan.questions,
        notices: plan.notices,
        ..PassReport::default()
    };
    for group in plan.groups {
        if let Some(record) = &group.upload {
            let sealed = match key.seal(record) {
                Ok(sealed) => sealed,
                Err(err) => {
                    // Content is validated on construction, so this is not expected; skip the
                    // record rather than stopping the pass.
                    report.failed.push(Failure {
                        key: Some(group.key),
                        error: TransportError::Refused(err.to_string()),
                    });
                    continue;
                }
            };
            match transport
                .write(&sealed.header.to_raw(), sealed.ciphertext)
                .await
            {
                Ok(_) => report.uploaded += 1,
                Err(error) => {
                    report.failed.push(Failure {
                        key: Some(group.key),
                        error,
                    });
                    continue;
                }
            }
        }
        if !group.delete.is_empty() {
            match transport.delete(&group.delete).await {
                Ok(()) => report.deleted += group.delete.len(),
                // The winner is stored; the losers are deleted next time.
                Err(error) => report.failed.push(Failure {
                    key: Some(group.key.clone()),
                    error,
                }),
            }
        }
        if let Some(change) = &group.local {
            if !matches!(change, LocalChange::MarkSynced(_)) {
                report.changed.push(group.key.clone());
            }
            state.apply(&group.key, change);
        }
    }
    if !old_key_records.is_empty() {
        match transport.delete(&old_key_records).await {
            Ok(()) => report.deleted += old_key_records.len(),
            Err(error) => report.failed.push(Failure { key: None, error }),
        }
    }
    state.last_complete_sync_ms = Some(listed_at_ms);
    report
}

/// Key records in a listing with valid headers.
fn key_records(listing: &[RemoteEntry]) -> Vec<(EntryHandle, RecordHeader)> {
    listing
        .iter()
        .filter_map(|e| RecordHeader::parse(&e.header).map(|h| (e.handle, h)))
        .filter(|(_, h)| h.opaque_id.is_key_record())
        .collect()
}

/// Sets up the sync in an empty folder: a new key, protected with `passphrase`.
pub async fn setup<T: SettingsTransport>(
    transport: &mut T,
    passphrase: &Secret<String>,
    hlc: &mut HlcClock,
    work_factor: WorkFactor,
) -> Result<SyncKey, SettingsSyncError> {
    if !key_records(&transport.list().await?).is_empty() {
        return Err(SettingsSyncError::AlreadySetUp);
    }
    let key = SyncKey::generate();
    let sealed = key.wrap(passphrase, hlc.now()?, work_factor)?;
    transport
        .write(&sealed.header.to_raw(), sealed.ciphertext)
        .await?;
    Ok(key)
}

/// Unlocks the folder's key with the passphrase (a new device, or after a reset elsewhere).
/// Tries the key records newest first. Writes nothing, so a wrong passphrase changes nothing
/// on the server or locally.
pub async fn join<T: SettingsTransport>(
    transport: &mut T,
    passphrase: &Secret<String>,
) -> Result<SyncKey, SettingsSyncError> {
    let mut records = key_records(&transport.list().await?);
    if records.is_empty() {
        return Err(SettingsSyncError::NotSetUp);
    }
    records.sort_by_key(|(handle, header)| std::cmp::Reverse((header.version, *handle)));
    let handles: Vec<EntryHandle> = records.iter().map(|(h, _)| *h).collect();
    let bodies: BTreeMap<EntryHandle, Vec<u8>> =
        transport.read(&handles).await?.into_iter().collect();
    let mut invalid = None;
    for (handle, header) in &records {
        let Some(body) = bodies.get(handle) else {
            continue;
        };
        match SyncKey::unwrap(header, body, passphrase) {
            Ok(key) => return Ok(key),
            Err(SettingsSyncError::WrongPassphrase) => {}
            Err(err) => invalid = Some(err),
        }
    }
    // A wrong passphrase is the likely cause; a damaged record only if nothing else is there.
    Err(match invalid {
        Some(err) if records.len() == 1 => err,
        _ => SettingsSyncError::WrongPassphrase,
    })
}

/// Protects the key with a new passphrase: the new key record is written first, then the
/// old ones are deleted. Other devices keep working with the key they hold.
///
/// If the deletion fails, the error is returned although the new passphrase already works:
/// until the next pass of any device removes the old key record, the old passphrase still
/// unlocks the key too. The user should know that when the old passphrase has leaked.
pub async fn change_passphrase<T: SettingsTransport>(
    transport: &mut T,
    key: &SyncKey,
    new_passphrase: &Secret<String>,
    hlc: &mut HlcClock,
    work_factor: WorkFactor,
) -> Result<(), SettingsSyncError> {
    let old: Vec<EntryHandle> = key_records(&transport.list().await?)
        .into_iter()
        .filter(|(_, h)| h.opaque_id == key.key_record_id())
        .map(|(handle, _)| handle)
        .collect();
    if old.is_empty() {
        return Err(SettingsSyncError::KeyChanged);
    }
    let sealed = key.wrap(new_passphrase, hlc.now()?, work_factor)?;
    transport
        .write(&sealed.header.to_raw(), sealed.ciphertext)
        .await?;
    transport.delete(&old).await?;
    Ok(())
}

/// Starts over (forgotten passphrase, or "reset sync"): deletes everything in the folder,
/// sets up a new key and marks every local record as not synced, so the next pass uploads
/// this device's data. Other devices get [`SettingsSyncError::KeyChanged`] and join again.
pub async fn reset<T: SettingsTransport>(
    transport: &mut T,
    passphrase: &Secret<String>,
    state: &mut LocalState,
    hlc: &mut HlcClock,
    work_factor: WorkFactor,
) -> Result<SyncKey, SettingsSyncError> {
    if passphrase.expose_secret().is_empty() {
        return Err(SettingsSyncError::EmptyPassphrase);
    }
    let all: Vec<EntryHandle> = transport
        .list()
        .await?
        .into_iter()
        .map(|e| e.handle)
        .collect();
    if !all.is_empty() {
        transport.delete(&all).await?;
    }
    let key = setup(transport, passphrase, hlc, work_factor).await?;
    state.forget_sync();
    Ok(key)
}
