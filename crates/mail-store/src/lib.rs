//! SQLite storage: the only crate with SQL.
//!
//! - One write connection on its own thread, a small pool of `query_only` reader threads,
//!   WAL mode and the pragmas from decision 0003. No call blocks a Tokio worker.
//! - Forward-only migrations with `rusqlite_migration` (schema version in `user_version`).
//! - A backup (`VACUUM INTO`) before every migration of an existing database; three are kept.
//! - A database newer than this build knows is rejected and left untouched.
//! - Accounts, mailboxes and message headers after decision 0003: `message.id` is the sort date
//!   (allocated on the writer thread), lists are keyset-paged, every list query has a plan test.
//! - Message bodies after decision 0009: zstd-compressed files under `bodies/` next to the
//!   database, written atomically, referenced from `message_body`; previews, prefetch settings
//!   and the remote-content allowlist.

mod accounts;
pub mod backup;
mod bodies;
mod body_api;
mod body_files;
mod error;
mod mail_api;
mod mailboxes;
mod messages;
mod migrations;
mod reading;
mod settings;
mod store;
mod workers;

pub use bodies::BodyInfo;
pub use body_api::{BODY_PACK_LIMIT, SWEEP_MIN_AGE};
pub use body_files::SweepReport;
pub use error::StoreError;
pub use mail_api::{
    MAX_PAGE_SIZE, MailboxBaseline, MailboxListReport, MailboxSyncState, NewAccount, StoreReport,
    StoredAccount,
};
pub use migrations::SCHEMA_VERSION;
pub use reading::{
    MAX_PREFETCH_BATCH, MAX_PREVIEW_CHARS, MessageLocation, PrefetchCandidate, StoredMessage,
};
pub use settings::{GLOBAL_SCOPE, Setting};
pub use store::{BODIES_DIR, OpenReport, Store, StoreOptions};

#[cfg(test)]
mod bench;
#[cfg(test)]
mod body_bench;
#[cfg(test)]
mod body_tests;
#[cfg(test)]
mod mail_tests;
#[cfg(test)]
mod plan_tests;
#[cfg(test)]
mod test_support;
#[cfg(test)]
mod tests;
