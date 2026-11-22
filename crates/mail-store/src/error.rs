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
            StoreError::JobPanicked(_) | StoreError::WorkerGone(_) => {
                CoreError::Internal(err.to_string())
            }
            other => CoreError::Database(other.to_string()),
        }
    }
}
