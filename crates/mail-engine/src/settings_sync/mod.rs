//! Settings sync (stage 11): merges what only the app knows (settings, signatures, templates,
//! rules, views, snoozes, pins, accounts without secrets, identities, AI settings, devices)
//! between devices through an encrypted folder in the user's own mailbox. The format, the
//! rules and what the mail server can see are documented in `docs/SYNC.md`.
//!
//! The logic without IO: records and their registry ([`record`]), the hybrid logical clock
//! ([`hlc`]), merge rules ([`merge`]), planning a pass ([`plan`]), encryption with age
//! ([`crypto`]), the transport trait with an in-memory folder ([`transport`]) and the pass over
//! a transport ([`pass`]).
//!
//! The integration (decision 0022): the store's capture hook and the `sync_record` rows
//! (`local`), the IMAP folder as transport (`imap`), and the flows in the core (`service`:
//! turning it on, joining, passes along with the mail sync, questions, status, passphrase,
//! reset).

pub mod crypto;
pub mod error;
pub mod hlc;
mod imap;
pub(crate) mod local;
pub mod merge;
pub mod pass;
pub mod plan;
pub mod record;
pub(crate) mod service;
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
    Failure, FetchedPass, LocalState, PassMode, PassReport, PreparedPass, change_passphrase,
    execute_pass, fetch_pass, join, prepare_pass, reset, run_pass, setup,
};
pub use plan::{
    FetchRequest, GroupPlan, LocalChange, LocalRecord, Notice, PlanContext, Planner, Question,
    SyncPlan, TOMBSTONE_RETENTION_MS,
};
pub use record::{
    ACCOUNT_FIELDS, AccountGroupContent, AttachmentPinContent, ContactContent,
    ContactPictureContent, DeviceInfo, EmptyContent, FieldValue, IdShape, IdentityContent,
    KindSpec, MailboxPrefContent, MergeRule, PinInfo, REGISTRY, ReadingListInfo, RecordBody,
    RecordKey, RecordKind, Removal, RuleContent, SYNCED_SETTINGS, SenderRuleContent, SettingScope,
    SignatureContent, SnoozeInfo, SyncContent, SyncRecord, SyncedAccount, TaskInfo,
};
pub use transport::{
    EntryHandle, FetchLimits, MemoryTransport, RawHeader, RemoteEntry, SettingsTransport,
    StoredEntry,
};
