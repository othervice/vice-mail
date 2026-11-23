//! Scenario, convergence and performance tests of the settings sync: simulated devices with
//! their own clocks and local state over one shared in-memory folder.

mod capture;
mod convergence;
mod perf;
mod scenarios;

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};

use mail_model::Secret;
use serde_json::{Value, json};

use super::*;
use crate::clock::Clock;

pub(super) const DAY_MS: i64 = 24 * 60 * 60 * 1000;
/// 2025-11-24, a plausible "now".
pub(super) const START_MS: i64 = 1_764_000_000_000;

#[derive(Debug)]
pub(super) struct TestClock(AtomicI64);

impl Clock for TestClock {
    fn now_ms(&self) -> i64 {
        self.0.load(Ordering::SeqCst)
    }
}

pub(super) fn passphrase() -> Secret<String> {
    Secret::new("correct horse battery staple".to_owned())
}

/// A low scrypt factor so tests stay fast; the default is measured in `perf`.
pub(super) fn factor() -> WorkFactor {
    WorkFactor::new(10).expect("factor")
}

/// One simulated device.
pub(super) struct Device {
    pub clock: Arc<TestClock>,
    pub hlc: HlcClock,
    pub state: LocalState,
    pub key: SyncKey,
    pub transport: MemoryTransport,
}

impl Device {
    fn new(transport: &MemoryTransport, key: SyncKey, id: u128, at_ms: i64) -> Self {
        let clock = Arc::new(TestClock(AtomicI64::new(at_ms)));
        let hlc = HlcClock::new(
            Arc::clone(&clock) as Arc<dyn Clock>,
            DeviceId::from_uuid(uuid::Uuid::from_u128(id)),
        );
        Self {
            clock,
            hlc,
            state: LocalState::new(),
            key,
            transport: transport.clone(),
        }
    }

    /// Sets up the sync in an empty folder.
    pub async fn first(transport: &MemoryTransport, id: u128) -> Self {
        let mut t = transport.clone();
        let mut device = Self::new(transport, SyncKey::generate(), id, START_MS);
        device.key = setup(&mut t, &passphrase(), &mut device.hlc, factor())
            .await
            .expect("setup");
        device
    }

    /// Joins an existing folder with the passphrase.
    pub async fn joining(transport: &MemoryTransport, id: u128, at_ms: i64) -> Self {
        Self::joining_with(transport, id, at_ms, &passphrase()).await
    }

    pub async fn joining_with(
        transport: &MemoryTransport,
        id: u128,
        at_ms: i64,
        passphrase: &Secret<String>,
    ) -> Self {
        let mut t = transport.clone();
        let key = join(&mut t, passphrase).await.expect("join");
        Self::new(transport, key, id, at_ms)
    }

    pub fn now(&self) -> i64 {
        self.clock.now_ms()
    }

    pub fn set_time(&self, ms: i64) {
        self.clock.0.store(ms, Ordering::SeqCst);
    }

    pub fn advance(&self, ms: i64) {
        self.clock.0.fetch_add(ms, Ordering::SeqCst);
    }

    pub async fn try_pass(&mut self) -> Result<PassReport, SettingsSyncError> {
        self.pass_with(PassMode::Changes).await
    }

    pub async fn pass_with(&mut self, mode: PassMode) -> Result<PassReport, SettingsSyncError> {
        run_pass(
            &mut self.transport,
            &self.key,
            &mut self.state,
            &mut self.hlc,
            mode,
        )
        .await
    }

    pub async fn pass(&mut self) -> PassReport {
        self.try_pass().await.expect("pass")
    }

    pub fn put(&mut self, key: &RecordKey, value: Value) {
        let content = SyncContent::new(key.kind(), value).expect("content");
        self.state
            .put(key.clone(), content, &mut self.hlc)
            .expect("put");
    }

    pub fn put_fields(&mut self, key: &RecordKey, values: Value) {
        let Value::Object(map) = values else {
            panic!("fields must be an object");
        };
        self.state
            .put_fields(key.clone(), map.into_iter().collect(), &mut self.hlc)
            .expect("put fields");
    }

    pub fn delete(&mut self, key: &RecordKey) {
        self.state.delete(key, &mut self.hlc).expect("delete");
    }

    /// The live value of a record; per-field records as their plain field values.
    pub fn value(&self, key: &RecordKey) -> Option<Value> {
        let content = self.state.content(key)?;
        Some(match content.field_values() {
            Some(fields) => Value::Object(fields.into_iter().collect()),
            None => content.value().clone(),
        })
    }

    /// Every live record with its version: equal on all devices once they have converged.
    pub fn live(&self) -> BTreeMap<RecordKey, (Hlc, Value)> {
        self.state
            .records()
            .iter()
            .filter_map(|(k, r)| {
                r.record
                    .content_ref()
                    .map(|c| (k.clone(), (r.record.version(), c.value().clone())))
            })
            .collect()
    }
}

/// Runs passes on every device, in order, until a whole round changes nothing. Answers
/// removal questions with "remove here too". Returns the number of rounds.
pub(super) async fn settle(devices: &mut [&mut Device]) -> usize {
    for round in 1..=12 {
        let mut quiet = true;
        for device in devices.iter_mut() {
            let report = device.pass().await;
            for question in &report.questions {
                device.state.accept_removal(question);
                quiet = false;
            }
            quiet &= report.is_quiet();
        }
        if quiet {
            return round;
        }
    }
    panic!("devices did not settle within 12 rounds");
}

/// Entries per opaque id in the folder (key records excluded).
pub(super) fn entries_per_record(transport: &MemoryTransport) -> BTreeMap<String, usize> {
    let mut out = BTreeMap::new();
    for (_, entry) in transport.entries() {
        if !entry.header.opaque_id.starts_with(KEY_RECORD_PREFIX) {
            *out.entry(entry.header.opaque_id).or_insert(0) += 1;
        }
    }
    out
}

pub(super) fn theme() -> RecordKey {
    RecordKey::setting(SettingScope::Global, "ui.theme").expect("theme")
}

pub(super) fn language() -> RecordKey {
    RecordKey::setting(SettingScope::Global, "ui.language").expect("language")
}

pub(super) fn signature(n: u128) -> RecordKey {
    RecordKey::uuid(RecordKind::Signature, uuid::Uuid::from_u128(0x5100 + n)).expect("signature")
}

pub(super) fn account_id(n: u128) -> mail_model::AccountId {
    mail_model::AccountId::from_uuid(uuid::Uuid::from_u128(0xacc0 + n))
}

pub(super) fn account(n: u128) -> RecordKey {
    RecordKey::account(account_id(n))
}

pub(super) fn snooze(n: u128) -> RecordKey {
    RecordKey::mail(
        RecordKind::Snooze,
        account_id(1),
        &format!("<msg-{n}@example.org>"),
    )
    .expect("snooze")
}

/// The content of a local rule that archives mail from `from`.
pub(super) fn rule_content(from: &str, position: i64) -> Value {
    json!({
        "name": format!("from {from}"),
        "position": position,
        "enabled": true,
        "definition": {
            "matchAll": true,
            "conditions": [{ "type": "from", "op": "contains", "value": from }],
            "actions": [{ "type": "markRead" }],
            "stop": false,
        },
    })
}

pub(super) fn account_fields(email: &str, name: &str) -> Value {
    let server =
        |host: &str| json!({ "host": host, "port": 993, "security": "tls", "username": email });
    json!({
        "email": email,
        "displayName": name,
        "color": 1,
        "imap": server("imap.example.org"),
        "smtp": server("smtp.example.org"),
    })
}
