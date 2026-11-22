use serde::{Deserialize, Serialize};

/// Stable, machine-readable error kinds. The UI picks its (translated) text by this code.
///
/// Every code is exported to TypeScript, and the frontend requires an English and a German
/// text for each one (`ERROR_CODES` in `app/src/services/ipc.ts` fails to compile otherwise):
/// a new code needs its texts in `app/src/i18n/` in the same change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    Internal,
    InvalidArgument,
    NotFound,
    Database,
    DatabaseTooNew,
    /// Server rejected user name or password.
    AuthFailed,
    /// Certificate chain not trusted by the system (self-signed, unknown CA).
    CertUntrusted,
    /// Certificate is valid, but for another host name.
    CertNameMismatch,
    /// Certificate expired or not yet valid (check the computer's clock).
    CertExpired,
    /// TLS handshake failed for another reason (protocol version, cipher, broken connection).
    TlsFailed,
    /// The server does not offer STARTTLS on this port; it is never used unencrypted.
    StarttlsUnavailable,
    /// Nothing listens on host and port.
    ConnectionRefused,
    /// Connecting took too long, or the server stopped answering.
    ConnectionTimeout,
    /// The connection broke (network change, server restart).
    ConnectionLost,
    /// Host name could not be resolved (typo, or offline).
    DnsFailed,
    /// The server sent something the app does not understand.
    Protocol,
    /// The server refused a command (IMAP NO/BAD).
    ServerRejected,
    /// Writing is turned off for this account.
    ReadOnly,
    /// The server lacks something the app requires (IMAP4rev1).
    UnsupportedServer,
    /// The system keychain refused to store, read or delete the password.
    Keychain,
    /// An account with this email address is already set up.
    AccountExists,
}

/// Error of a mail backend (IMAP adapter). Variants hold plain text so the type stays free of
/// IO dependencies; texts never contain secrets.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum BackendError {
    #[error("host name could not be resolved: {0}")]
    Dns(String),

    #[error("connection refused")]
    ConnectionRefused,

    #[error("connection attempt timed out")]
    ConnectTimeout,

    /// Read or write limit passed without progress (silent connection).
    #[error("no progress within the time limit: {0}")]
    Timeout(String),

    /// EOF, reset, BYE, or the connection was left unusable by an aborted command.
    #[error("connection lost: {0}")]
    ConnectionLost(String),

    #[error("server certificate is not trusted: {0}")]
    CertUntrusted(String),

    #[error("server certificate does not match the host name")]
    CertNameMismatch,

    #[error("server certificate has expired or is not yet valid")]
    CertExpired,

    #[error("TLS failed: {0}")]
    Tls(String),

    #[error("server does not offer STARTTLS")]
    StarttlsUnavailable,

    /// LOGIN/AUTHENTICATE answered NO (except `[UNAVAILABLE]`, which is [`Self::Rejected`]).
    /// Holds the server's text, which never contains the password.
    #[error("login rejected: {0}")]
    AuthFailed(String),

    /// The server lacks a required capability or the operation needs one it does not have.
    #[error("server does not support {0}")]
    Unsupported(String),

    /// Unparseable response. The connection is closed afterwards.
    #[error("could not understand the server response: {0}")]
    Protocol(String),

    /// A command answered NO or BAD.
    #[error("server refused the command: {0}")]
    Rejected(String),

    /// EXAMINE/SELECT of a mailbox that does not exist (any more). Re-list the mailboxes.
    #[error("mailbox not found: {0}")]
    MailboxNotFound(String),

    /// A writing command was refused before sending, because the connection is read-only.
    /// Holds the command name.
    #[error("writes are turned off for this account (refused {0})")]
    ReadOnly(String),

    /// The backend was used wrongly (for example a mailbox-scoped call for a mailbox that is not
    /// open). A bug in the caller.
    #[error("backend used incorrectly: {0}")]
    InvalidState(String),
}

impl BackendError {
    pub fn code(&self) -> ErrorCode {
        match self {
            Self::Dns(_) => ErrorCode::DnsFailed,
            Self::ConnectionRefused => ErrorCode::ConnectionRefused,
            Self::ConnectTimeout | Self::Timeout(_) => ErrorCode::ConnectionTimeout,
            Self::ConnectionLost(_) => ErrorCode::ConnectionLost,
            Self::CertUntrusted(_) => ErrorCode::CertUntrusted,
            Self::CertNameMismatch => ErrorCode::CertNameMismatch,
            Self::CertExpired => ErrorCode::CertExpired,
            Self::Tls(_) => ErrorCode::TlsFailed,
            Self::StarttlsUnavailable => ErrorCode::StarttlsUnavailable,
            Self::AuthFailed(_) => ErrorCode::AuthFailed,
            Self::Unsupported(_) => ErrorCode::UnsupportedServer,
            Self::Protocol(_) => ErrorCode::Protocol,
            Self::Rejected(_) => ErrorCode::ServerRejected,
            Self::MailboxNotFound(_) => ErrorCode::NotFound,
            Self::ReadOnly(_) => ErrorCode::ReadOnly,
            Self::InvalidState(_) => ErrorCode::Internal,
        }
    }

    /// Worth retrying with backoff (network trouble, a hiccup of the server), as opposed to
    /// errors only the user can fix (password, certificate, settings) or bugs.
    pub fn is_transient(&self) -> bool {
        matches!(
            self,
            Self::Dns(_)
                | Self::ConnectionRefused
                | Self::ConnectTimeout
                | Self::Timeout(_)
                | Self::ConnectionLost(_)
                | Self::Protocol(_)
                | Self::Rejected(_)
                | Self::MailboxNotFound(_)
        )
    }

    /// Short English text for people (fallback for the UI's translation by code).
    pub fn message(&self) -> &'static str {
        match self {
            Self::Dns(_) => "The server name could not be found.",
            Self::ConnectionRefused => "The server refused the connection.",
            Self::ConnectTimeout | Self::Timeout(_) => "The server did not respond in time.",
            Self::ConnectionLost(_) => "The connection to the server was lost.",
            Self::CertUntrusted(_) => "The server's certificate is not trusted.",
            Self::CertNameMismatch => "The server's certificate is for a different name.",
            Self::CertExpired => "The server's certificate has expired or is not yet valid.",
            Self::Tls(_) => "A secure connection to the server could not be established.",
            Self::StarttlsUnavailable => "The server does not offer encryption on this port.",
            Self::AuthFailed(_) => "The user name or password was not accepted.",
            Self::Unsupported(_) => "The server does not support a required feature.",
            Self::Protocol(_) => "The server sent a response the app does not understand.",
            Self::Rejected(_) => "The server refused the request.",
            Self::MailboxNotFound(_) => "The folder no longer exists on the server.",
            Self::ReadOnly(_) => "Changes are turned off for this account.",
            Self::InvalidState(_) => "An unexpected error occurred.",
        }
    }

    /// Technical detail for the log and an expandable section in the UI. Never contains secrets.
    pub fn detail(&self) -> Option<String> {
        match self {
            Self::ConnectionRefused
            | Self::ConnectTimeout
            | Self::CertNameMismatch
            | Self::CertExpired
            | Self::StarttlsUnavailable => None,
            Self::Dns(d)
            | Self::Timeout(d)
            | Self::ConnectionLost(d)
            | Self::CertUntrusted(d)
            | Self::Tls(d)
            | Self::AuthFailed(d)
            | Self::Unsupported(d)
            | Self::Protocol(d)
            | Self::Rejected(d)
            | Self::MailboxNotFound(d)
            | Self::ReadOnly(d)
            | Self::InvalidState(d) => (!d.is_empty()).then(|| d.clone()),
        }
    }
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

    /// A mail server operation failed.
    #[error(transparent)]
    Mail(#[from] BackendError),

    /// The system keychain failed; holds the platform's error text.
    #[error("keychain error: {0}")]
    Keychain(String),

    /// An account with this email address exists already; holds the address.
    #[error("an account for {0} exists already")]
    AccountExists(String),
}

impl CoreError {
    pub fn code(&self) -> ErrorCode {
        match self {
            Self::Internal(_) => ErrorCode::Internal,
            Self::InvalidArgument(_) => ErrorCode::InvalidArgument,
            Self::NotFound(_) => ErrorCode::NotFound,
            Self::Database(_) => ErrorCode::Database,
            Self::DatabaseTooNew { .. } => ErrorCode::DatabaseTooNew,
            Self::Mail(err) => err.code(),
            Self::Keychain(_) => ErrorCode::Keychain,
            Self::AccountExists(_) => ErrorCode::AccountExists,
        }
    }

    /// Worth retrying automatically with backoff. Only some mail errors are.
    pub fn is_transient(&self) -> bool {
        match self {
            Self::Mail(err) => err.is_transient(),
            _ => false,
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
            Self::Mail(err) => err.message(),
            Self::Keychain(_) => "The password could not be stored in or read from the keychain.",
            Self::AccountExists(_) => "An account with this email address is already set up.",
        }
    }

    /// Technical detail for the log and an expandable section in the UI. Never contains secrets.
    pub fn detail(&self) -> Option<String> {
        match self {
            Self::Internal(d)
            | Self::InvalidArgument(d)
            | Self::NotFound(d)
            | Self::Database(d)
            | Self::Keychain(d)
            | Self::AccountExists(d) => (!d.is_empty()).then(|| d.clone()),
            Self::DatabaseTooNew { found, supported } => Some(format!(
                "database schema version {found}, this app supports up to {supported}"
            )),
            Self::Mail(err) => err.detail(),
        }
    }
}

/// The error as it crosses the IPC bridge: `{ code, message, detail }`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
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
            (ErrorCode::AuthFailed, "auth_failed"),
            (ErrorCode::CertUntrusted, "cert_untrusted"),
            (ErrorCode::CertNameMismatch, "cert_name_mismatch"),
            (ErrorCode::CertExpired, "cert_expired"),
            (ErrorCode::TlsFailed, "tls_failed"),
            (ErrorCode::StarttlsUnavailable, "starttls_unavailable"),
            (ErrorCode::ConnectionRefused, "connection_refused"),
            (ErrorCode::ConnectionTimeout, "connection_timeout"),
            (ErrorCode::ConnectionLost, "connection_lost"),
            (ErrorCode::DnsFailed, "dns_failed"),
            (ErrorCode::Protocol, "protocol"),
            (ErrorCode::ServerRejected, "server_rejected"),
            (ErrorCode::ReadOnly, "read_only"),
            (ErrorCode::UnsupportedServer, "unsupported_server"),
            (ErrorCode::Keychain, "keychain"),
            (ErrorCode::AccountExists, "account_exists"),
        ];
        for (code, wire) in codes {
            assert_eq!(serde_json::to_value(code).expect("serialize"), wire);
        }
    }

    #[test]
    fn backend_errors_map_to_codes_and_retry_classes() {
        use BackendError as B;
        let cases = [
            (B::Dns("x".into()), ErrorCode::DnsFailed, true),
            (B::ConnectionRefused, ErrorCode::ConnectionRefused, true),
            (B::ConnectTimeout, ErrorCode::ConnectionTimeout, true),
            (
                B::Timeout("read".into()),
                ErrorCode::ConnectionTimeout,
                true,
            ),
            (
                B::ConnectionLost("eof".into()),
                ErrorCode::ConnectionLost,
                true,
            ),
            (B::Protocol("x".into()), ErrorCode::Protocol, true),
            (B::Rejected("NO".into()), ErrorCode::ServerRejected, true),
            (B::MailboxNotFound("X".into()), ErrorCode::NotFound, true),
            (
                B::CertUntrusted("UnknownIssuer".into()),
                ErrorCode::CertUntrusted,
                false,
            ),
            (B::CertNameMismatch, ErrorCode::CertNameMismatch, false),
            (B::CertExpired, ErrorCode::CertExpired, false),
            (B::Tls("x".into()), ErrorCode::TlsFailed, false),
            (
                B::StarttlsUnavailable,
                ErrorCode::StarttlsUnavailable,
                false,
            ),
            (B::AuthFailed(String::new()), ErrorCode::AuthFailed, false),
            (
                B::Unsupported("IMAP4rev1".into()),
                ErrorCode::UnsupportedServer,
                false,
            ),
            (B::ReadOnly("STORE".into()), ErrorCode::ReadOnly, false),
            (B::InvalidState("x".into()), ErrorCode::Internal, false),
        ];
        for (err, code, transient) in cases {
            assert_eq!(err.code(), code, "{err}");
            assert_eq!(err.is_transient(), transient, "{err}");
            let core = CoreError::from(err.clone());
            assert_eq!(core.code(), code);
            assert_eq!(core.is_transient(), transient);
            assert_eq!(core.message(), err.message());
            assert_eq!(core.to_string(), err.to_string());
        }
        assert!(!CoreError::Database("x".into()).is_transient());

        let ipc = IpcError::from(CoreError::from(B::ReadOnly("STORE".into())));
        let json = serde_json::to_value(ipc).expect("serialize");
        assert_eq!(json["code"], "read_only");
        assert_eq!(json["detail"], "STORE");
        assert!(
            IpcError::from(CoreError::from(B::CertExpired))
                .detail
                .is_none()
        );
    }
}
