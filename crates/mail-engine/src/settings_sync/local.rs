//! The local side of the integration: `sync_record` rows of the store as [`LocalRecord`]s, and
//! the capture hook that versions every change of a synchronised table in the transaction
//! that made it (decision 0022).
//!
//! The store's triggers mark changed records; before the transaction commits, the store hands
//! the content its table holds now and the stored row to [`EngineCapture`], which runs the
//! change through [`LocalState::put`], [`LocalState::put_fields`] or [`LocalState::delete`]
//! with the device's clock. Content the registry refuses (a device setting, a field that
//! looks like a secret, a record larger than its kind allows) is not synchronised: the change
//! stays on this device and the reason is logged without the value.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, PoisonError};

use mail_store::{CaptureOutcome, CapturedChange, SyncCapture, SyncRecordRow};
use serde_json::Value;

use super::error::RecordError;
use super::hlc::{Hlc, HlcClock};
use super::pass::LocalState;
use super::plan::LocalRecord;
use super::record::{MergeRule, RecordBody, RecordKey, RecordKind, SyncContent, SyncRecord};

/// The device's clock, shared by the capture hook (writer thread) and the passes. Held only for
/// synchronous steps, never across IO.
pub(crate) type SharedClock = Arc<Mutex<HlcClock>>;

pub(crate) fn lock_clock(clock: &SharedClock) -> std::sync::MutexGuard<'_, HlcClock> {
    clock.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A stored row as a local record. Rows this version cannot read (a kind or setting of a newer
/// version, damaged JSON) are `Err` and left alone.
pub(crate) fn record_from_row(row: &SyncRecordRow) -> Result<LocalRecord, String> {
    let kind = RecordKind::from_name(&row.kind).ok_or_else(|| format!("kind {}", row.kind))?;
    let key = RecordKey::parse(kind, &row.id).map_err(|err| err.to_string())?;
    let version: Hlc = row.version.parse().map_err(|_| "version".to_owned())?;
    let synced = row
        .synced_version
        .as_deref()
        .map(str::parse::<Hlc>)
        .transpose()
        .map_err(|_| "synced version".to_owned())?;
    let record = match (&row.content, &row.deleted_version) {
        (Some(content), None) => {
            let content = SyncContent::new(kind, content.clone()).map_err(|err| err.to_string())?;
            SyncRecord::content(key, version, content).map_err(|err| err.to_string())?
        }
        (None, Some(deleted)) => {
            let deleted: Hlc = deleted.parse().map_err(|_| "deleted version".to_owned())?;
            SyncRecord::tombstone(key, version, deleted).map_err(|err| err.to_string())?
        }
        _ => return Err("content and tombstone".to_owned()),
    };
    Ok(LocalRecord { record, synced })
}

pub(crate) fn row_from_record(local: &LocalRecord) -> SyncRecordRow {
    let record = &local.record;
    let (content, deleted_version) = match record.body() {
        RecordBody::Content(content) => (Some(content.value().clone()), None),
        RecordBody::Tombstone { deleted } => (None, Some(deleted.to_string())),
    };
    SyncRecordRow {
        kind: record.kind().name().to_owned(),
        id: record.key().id().to_owned(),
        version: record.version().to_string(),
        synced_version: local.synced.map(|v| v.to_string()),
        content,
        deleted_version,
    }
}

/// Every readable row as the local state of a pass.
pub(crate) fn load_state(rows: &[SyncRecordRow], last_complete_sync_ms: Option<i64>) -> LocalState {
    let mut state = LocalState::new();
    for row in rows {
        match record_from_row(row) {
            Ok(record) => state.insert(record),
            Err(reason) => {
                tracing::debug!(kind = %row.kind, %reason, "sync record left alone");
            }
        }
    }
    state.last_complete_sync_ms = last_complete_sync_ms;
    state
}

/// The plain values of a record's content for its table: accounts without field versions.
pub(crate) fn table_value(content: &SyncContent) -> Value {
    match content.field_values() {
        Some(fields) => Value::Object(fields.into_iter().collect()),
        None => content.value().clone(),
    }
}

/// The store's capture hook (see the module docs).
pub(crate) struct EngineCapture {
    clock: SharedClock,
    /// Woken when a change got a new version (the sync uploads it soon).
    changed: Arc<super::service::SyncSignals>,
}

impl EngineCapture {
    pub(crate) fn new(clock: SharedClock, changed: Arc<super::service::SyncSignals>) -> Self {
        Self { clock, changed }
    }
}

/// One captured change through the local state; `Ok(None)` if nothing changed.
pub(crate) fn capture_one(
    change: &CapturedChange,
    clock: &mut HlcClock,
) -> Result<Option<SyncRecordRow>, String> {
    let Some(kind) = RecordKind::from_name(&change.kind) else {
        return Ok(None);
    };
    let key = match RecordKey::parse(kind, &change.id) {
        Ok(key) => key,
        // A device setting, or an id this version cannot address.
        Err(RecordError::SettingNotSynced(_)) => return Ok(None),
        Err(err) => return Err(err.to_string()),
    };
    let mut state = LocalState::new();
    let before = match &change.stored {
        Some(row) => match record_from_row(row) {
            Ok(record) => {
                state.insert(record.clone());
                Some(record)
            }
            // A row this version cannot read is replaced only by a real change.
            Err(_) => None,
        },
        None => None,
    };
    let result = match (&change.current, kind.spec().merge) {
        (Some(Value::Object(values)), MergeRule::PerField) => {
            let values: BTreeMap<String, Value> =
                values.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
            state.put_fields(key.clone(), values, clock).map(drop)
        }
        (Some(value), MergeRule::PerField) => {
            return Err(format!(
                "{kind} content is not an object: {}",
                value_kind(value)
            ));
        }
        (Some(value), _) => SyncContent::new(kind, value.clone())
            .map_err(super::error::SettingsSyncError::from)
            .and_then(|content| state.put(key.clone(), content, clock).map(drop)),
        (None, _) => state.delete(&key, clock).map(drop),
    };
    result.map_err(|err| err.to_string())?;
    let after = state.get(&key);
    Ok(match after {
        Some(after) if Some(after) != before.as_ref() => Some(row_from_record(after)),
        _ => None,
    })
}

fn value_kind(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "a boolean",
        Value::Number(_) => "a number",
        Value::String(_) => "a string",
        Value::Array(_) => "an array",
        Value::Object(_) => "an object",
    }
}

impl SyncCapture for EngineCapture {
    fn capture(&self, changes: Vec<CapturedChange>) -> CaptureOutcome {
        let mut clock = lock_clock(&self.clock);
        let mut rows = Vec::new();
        for change in &changes {
            match capture_one(change, &mut clock) {
                Ok(Some(row)) => rows.push(row),
                Ok(None) => {}
                // Never the value: it may be what the check keeps out.
                Err(reason) => tracing::warn!(
                    kind = %change.kind,
                    %reason,
                    "change not synchronised; it stays on this device"
                ),
            }
        }
        if !rows.is_empty() {
            self.changed.wake();
        }
        CaptureOutcome {
            rows,
            hlc_last: clock.last().map(|v| v.to_string()),
        }
    }
}
