//! SQLite storage: the only crate with SQL.
//!
//! - One write connection on its own thread, a small pool of `query_only` reader threads,
//!   WAL mode and the pragmas from decision 0003. No call blocks a Tokio worker.
//! - Forward-only migrations with `rusqlite_migration` (schema version in `user_version`).
//! - A backup (`VACUUM INTO`) before every migration of an existing database; three are kept.
//! - A database newer than this build knows is rejected and left untouched.

pub mod backup;
mod error;
mod mail_api;
mod migrations;
mod settings;
mod store;
mod workers;

pub use error::StoreError;
pub use mail_api::{
    MAX_PAGE_SIZE, MailboxBaseline, MailboxListReport, MailboxSyncState, NewAccount, StoreReport,
    StoredAccount,
};
pub use migrations::SCHEMA_VERSION;
pub use settings::{GLOBAL_SCOPE, Setting};
pub use store::{OpenReport, Store, StoreOptions};

#[cfg(test)]
mod tests;
