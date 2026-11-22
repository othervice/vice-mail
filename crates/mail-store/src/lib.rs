//! SQLite storage: the only crate with SQL.
//!
//! - One write connection on its own thread, a small pool of `query_only` reader threads,
//!   WAL mode and the pragmas from decision 0003. No call blocks a Tokio worker.
//! - Forward-only migrations with `rusqlite_migration` (schema version in `user_version`).
//! - A backup (`VACUUM INTO`) before every migration of an existing database; three are kept.
//! - A database newer than this build knows is rejected and left untouched.
//! - Accounts, mailboxes and message headers after decision 0003: `message.id` is the sort date
//!   (allocated on the writer thread), lists are keyset-paged, every list query has a plan test.

mod accounts;
pub mod backup;
mod error;
mod mail_api;
mod mailboxes;
mod messages;
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
mod bench;
#[cfg(test)]
mod mail_tests;
#[cfg(test)]
mod plan_tests;
#[cfg(test)]
mod test_support;
#[cfg(test)]
mod tests;
