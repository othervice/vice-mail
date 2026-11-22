use serde::Serialize;

/// Stable, machine-readable error kinds. The UI picks its (translated) text by this code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    Internal,
    InvalidArgument,
    NotFound,
    Database,
    DatabaseTooNew,
}

/// Error type of the core (`mail-engine` and everything below it, as seen from the shell).
///
/// Variants hold plain text so the type stays free of IO dependencies. Never put a secret into
/// a variant in plain form; wrap it in [`crate::Secret`], whose output is redacted.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum CoreError {
    #[error("internal error: {0}")]
    Internal(String),

    #[error("invalid argument: {0}")]
    InvalidArgument(String),

    #[error("not found: {0}")]
    NotFound(String),

    #[error("database error: {0}")]
    Database(String),

    #[error(
        "database schema version {found} is newer than the newest version this app knows ({supported})"
    )]
    DatabaseTooNew { found: usize, supported: usize },
}

impl CoreError {
    pub fn code(&self) -> ErrorCode {
        match self {
            Self::Internal(_) => ErrorCode::Internal,
            Self::InvalidArgument(_) => ErrorCode::InvalidArgument,
            Self::NotFound(_) => ErrorCode::NotFound,
            Self::Database(_) => ErrorCode::Database,
            Self::DatabaseTooNew { .. } => ErrorCode::DatabaseTooNew,
        }
    }

    /// Short English text for people. The UI normally shows its own translation chosen by
    /// [`CoreError::code`] and uses this only as a fallback.
    pub fn message(&self) -> &'static str {
        match self {
            Self::Internal(_) => "An unexpected error occurred.",
            Self::InvalidArgument(_) => "The request contained an invalid value.",
            Self::NotFound(_) => "The requested item was not found.",
            Self::Database(_) => "The local database could not be read or written.",
            Self::DatabaseTooNew { .. } => {
                "The local database was created by a newer version of this app. \
                 Install the newer version to open it; the database was not changed."
            }
        }
    }

    /// Technical detail for the log and an expandable section in the UI. Never contains secrets.
    pub fn detail(&self) -> Option<String> {
        match self {
            Self::Internal(d)
            | Self::InvalidArgument(d)
            | Self::NotFound(d)
            | Self::Database(d) => (!d.is_empty()).then(|| d.clone()),
            Self::DatabaseTooNew { found, supported } => Some(format!(
                "database schema version {found}, this app supports up to {supported}"
            )),
        }
    }
}

/// The error as it crosses the IPC bridge: `{ code, message, detail }`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct IpcError {
    pub code: ErrorCode,
    /// English fallback text; the UI shows an i18n text chosen by `code`.
    pub message: String,
    /// Technical detail, never secrets.
    pub detail: Option<String>,
}

impl From<&CoreError> for IpcError {
    fn from(err: &CoreError) -> Self {
        Self {
            code: err.code(),
            message: err.message().to_owned(),
            detail: err.detail(),
        }
    }
}

impl From<CoreError> for IpcError {
    fn from(err: CoreError) -> Self {
        Self::from(&err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_crosses_the_bridge_as_code_message_detail() {
        let err = CoreError::DatabaseTooNew {
            found: 7,
            supported: 1,
        };
        let json = serde_json::to_value(IpcError::from(&err)).expect("serialize");
        assert_eq!(json["code"], "database_too_new");
        assert!(
            json["message"]
                .as_str()
                .is_some_and(|m| m.contains("newer version"))
        );
        assert_eq!(
            json["detail"],
            "database schema version 7, this app supports up to 1"
        );

        let json = serde_json::to_value(IpcError::from(CoreError::Internal(String::new())))
            .expect("serialize");
        assert_eq!(json["code"], "internal");
        assert!(json["detail"].is_null());
    }

    #[test]
    fn error_codes_match_the_contract() {
        let codes = [
            (ErrorCode::Internal, "internal"),
            (ErrorCode::InvalidArgument, "invalid_argument"),
            (ErrorCode::NotFound, "not_found"),
            (ErrorCode::Database, "database"),
            (ErrorCode::DatabaseTooNew, "database_too_new"),
        ];
        for (code, wire) in codes {
            assert_eq!(serde_json::to_value(code).expect("serialize"), wire);
        }
    }
}
