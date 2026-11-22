//! Types shared by every layer: ids, accounts, mailboxes, message headers, flags,
//! capabilities, errors, the [`Secret`] wrapper, the [`MailBackend`] trait, role detection and
//! the types that cross the IPC bridge. Pure data and logic: no IO, no dependency on other
//! workspace crates.
//!
//! IPC types derive `serde` and `specta::Type`; the shell exports them to TypeScript with
//! tauri-specta (decision 0004). Structs use camelCase on the wire, error codes snake_case.
//! Types documented as internal (adapter results, sync state) never cross the bridge.

mod account;
mod backend;
mod capabilities;
mod error;
mod ids;
mod ipc;
mod mailbox;
mod message;
mod role;
mod secret;
mod uid_set;

pub use account::{
    Account, AccountColor, AccountSettings, AccountState, AccountStatus, ConnectionTestResult,
    DiscoveryResult, DiscoverySource, Identity, ProviderHint, Security, ServerSettings,
    SyncProgress,
};
pub use backend::{
    Access, ConnectConfig, ExtraRootCa, IdleOutcome, MailBackend, MailConnector, SelectedMailbox,
    Timeouts,
};
pub use capabilities::Capabilities;
pub use error::{BackendError, CoreError, ErrorCode, IpcError};
pub use ids::{AccountId, IdentityId, ListCursor, MailboxId, MessageId, ModSeq, Uid, UidValidity};
pub use ipc::{AppInfo, CoreEvent, LanguagePreference, Platform, Preferences, ThemePreference};
pub use mailbox::{
    Mailbox, MailboxRole, MailboxStatus, RemoteMailbox, RoleSource, is_selectable, leaf_name,
};
pub use message::{
    Address, DATE_HEADER_TOLERANCE, FlagUpdate, Flags, MAX_SORT_DATE, MessageHeader,
    MessageListItem, MessagePage, sort_date,
};
pub use role::{RoleAssignment, RoleInput, assign_roles};
pub use secret::Secret;
pub use uid_set::UidSet;
