use std::io;

use mail_model::CoreError;

/// Errors of the storage layer. Converted into [`CoreError`] at the crate boundary.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum StoreError {
    #[error("{context}: {source}")]
    Sqlite {
        context: &'static str,
        source: rusqlite::Error,
    },

    #[error("{context}: {source}")]
    Io {
        context: &'static str,
        source: io::Error,
    },

    #[error("migration failed: {0}")]
    Migration(rusqlite_migration::Error),

    /// The file was written by a newer app version. It is left untouched.
    #[error("database schema version {found} is newer than the newest known version {supported}")]
    TooNew { found: usize, supported: usize },

    #[error("database configuration failed: {0}")]
    Config(String),

    #[error("database {0} thread is not running")]
    WorkerGone(&'static str),

    #[error("database job panicked: {0}")]
    JobPanicked(String),

    /// An account with this email address exists (unique, case-insensitive).
    #[error("an account for {0} exists already")]
    AccountExists(String),

    /// The account, mailbox or message does not exist (any more).
    #[error("not found: {0}")]
    NotFound(String),

    /// A caller passed a value the store refuses (for example a list limit of 0).
    #[error("invalid argument: {0}")]
    InvalidArgument(String),

    /// A writing action or outbox request was refused (stage 4); nothing was written.
    #[error(transparent)]
    Action(mail_model::ActionError),

    /// A sync write named another UIDVALIDITY than the stored one: the mailbox was reset in
    /// between, and the caller's UIDs are void. Nothing was written.
    #[error("mailbox {mailbox}: UIDVALIDITY is {stored:?}, the write was for {given}")]
    UidValidityMismatch {
        mailbox: i64,
        stored: Option<u32>,
        given: u32,
    },
}

impl StoreError {
    pub(crate) fn io(context: &'static str) -> impl FnOnce(io::Error) -> Self {
        move |source| Self::Io { context, source }
    }

    pub(crate) fn sqlite(context: &'static str) -> impl FnOnce(rusqlite::Error) -> Self {
        move |source| Self::Sqlite { context, source }
    }
}

/// Lets job closures use `?` on rusqlite calls.
impl From<rusqlite::Error> for StoreError {
    fn from(source: rusqlite::Error) -> Self {
        Self::Sqlite {
            context: "query",
            source,
        }
    }
}

impl From<rusqlite_migration::Error> for StoreError {
    fn from(err: rusqlite_migration::Error) -> Self {
        Self::Migration(err)
    }
}

impl From<StoreError> for CoreError {
    fn from(err: StoreError) -> Self {
        match err {
            StoreError::TooNew { found, supported } => {
                CoreError::DatabaseTooNew { found, supported }
            }
            StoreError::JobPanicked(_)
            | StoreError::WorkerGone(_)
            | StoreError::UidValidityMismatch { .. } => CoreError::Internal(err.to_string()),
            StoreError::AccountExists(email) => CoreError::AccountExists(email),
            StoreError::NotFound(what) => CoreError::NotFound(what),
            StoreError::InvalidArgument(what) => CoreError::InvalidArgument(what),
            StoreError::Action(action) => CoreError::Action(action),
            other => CoreError::Database(other.to_string()),
        }
    }
}
