//! Settings sync (stage 11): merges what only the app knows (settings, signatures, templates,
//! rules, views, snoozes, pins, accounts without secrets, identities, AI settings, devices)
//! between devices through an encrypted folder in the user's own mailbox. The format, the
//! rules and what the mail server can see are documented in `docs/SYNC.md`.
//!
//! This module is the logic without IO: records and their registry ([`record`]), the hybrid
//! logical clock ([`hlc`]), merge rules ([`merge`]), planning a pass ([`plan`]), encryption
//! with age ([`crypto`]), the transport trait with an in-memory folder ([`transport`]) and the
//! pass over a transport ([`pass`]). Database table (`sync_record`), IMAP transport and UI
//! follow with the rest of stage 11.

pub mod crypto;
pub mod error;
pub mod hlc;
pub mod merge;
pub mod pass;
pub mod plan;
pub mod record;
pub mod transport;

#[cfg(test)]
mod tests;

pub use crypto::{
    FORMAT_VERSION, KEY_RECORD_PREFIX, OpaqueId, OpenError, RecordHeader, SealedRecord, SyncKey,
    WorkFactor,
};
pub use error::{HlcError, RecordError, SettingsSyncError, TransportError};
pub use hlc::{DeviceId, Hlc, HlcClock, MAX_CLOCK_DRIFT_MS};
pub use merge::{Resolution, resolve};
pub use pass::{
    Failure, LocalState, PassMode, PassReport, PreparedPass, change_passphrase, execute_pass, join,
    prepare_pass, reset, run_pass, setup,
};
pub use plan::{
    FetchRequest, GroupPlan, LocalChange, LocalRecord, Notice, PlanContext, Planner, Question,
    SyncPlan, TOMBSTONE_RETENTION_MS,
};
pub use record::{
    ACCOUNT_FIELDS, DeviceInfo, FieldValue, IdShape, KindSpec, MergeRule, REGISTRY, RecordBody,
    RecordKey, RecordKind, Removal, SYNCED_SETTINGS, SettingScope, SnoozeInfo, SyncContent,
    SyncRecord, SyncedAccount,
};
pub use transport::{
    EntryHandle, MemoryTransport, RawHeader, RemoteEntry, SettingsTransport, StoredEntry,
};
