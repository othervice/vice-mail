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
    /// The account has no folder for this action: archive (`archive`), trash (`delete`), junk
    /// (`spam`) or inbox (`notSpam`). The UI asks which folder to use (`setMailboxRole`) or
    /// offers to create one (`createMailbox`).
    MissingMailbox,
    /// The messages lie in the trash, where deleting removes them for good. The UI asks and
    /// then sends the action `deletePermanently`.
    ConfirmationRequired,
    /// The action cannot be undone (a permanent delete, a created folder, a move still on its
    /// way or not yet visible in its target folder). Nothing was changed.
    NotUndoable,
    /// A tag contains spaces or characters IMAP does not allow in keywords.
    InvalidKeyword,
    /// The SMTP server refused the message or a recipient; nothing was delivered.
    SendRejected,
    /// The message went out, but the server's confirmation never arrived; it may have been
    /// delivered. Never sent again without the user's decision (`resolveUnclearSend`).
    SendUnclear,
    /// Stage 5: an address is not valid, or the message has no recipient.
    InvalidAddress,
    /// Stage 5: the attachments together are larger than the app allows.
    AttachmentTooLarge,
    /// Stage 5: the message is in the outbox (cancel sending to change it), or its submission
    /// started and can no longer be cancelled.
    SendInProgress,
    /// Stage 10: a message scheduled for later was due while the app was not running, and
    /// more than an hour has passed: nothing was sent. The user decides (`resolveMissedSend`:
    /// send now, schedule again, or keep it as a draft).
    SendMissed,
    /// Stage 10: the message is larger than the account's SMTP server accepts (its SIZE
    /// limit); the server would refuse it.
    MessageTooLarge,
    /// Stage 11: the passphrase does not unlock the settings sync; nothing was changed.
    SyncWrongPassphrase,
    /// Stage 11: the account holds no settings sync yet (set it up instead of joining).
    SyncNotSetUp,
    /// Stage 11: the account holds a settings sync already (join it instead of setting up).
    SyncAlreadySetUp,
    /// Stage 11: the settings sync was reset on another device; enter the new passphrase.
    SyncKeyChanged,
    /// Stage 11: the settings sync data in the mailbox is damaged or from a newer version.
    SyncKeyInvalid,
    /// Stage 11: the request needs the settings sync to be on.
    SyncOff,
    /// Stage 11: the settings sync is on, but this device has no sync key; enter the
    /// passphrase again.
    SyncKeyMissing,
    /// Stage 13: AI is off, or the function has no provider (or the provider no model).
    AiOff,
    /// Stage 13: a cloud provider needs the user's consent before the first request
    /// (`grantAiConsent`); nothing was sent.
    AiConsentRequired,
    /// Stage 13: an account involved may not send mail to a cloud provider; nothing was sent.
    AiNotPermitted,
    /// Stage 13: the request was cancelled.
    AiCancelled,
    /// Stage 13: the provider could not be reached or did not answer in time.
    AiUnreachable,
    /// Stage 13: the provider refused the key (missing, wrong, no access).
    AiUnauthorized,
    /// Stage 13: rate limit or overload at the provider; try later.
    AiBusy,
    /// Stage 13: the model declined the request.
    AiRefused,
    /// Stage 13: any other failure of an AI request (address, model, size, unreadable answer).
    AiFailed,
    /// Stage 9 (decision 0027): the `.eml` file is larger than the app opens.
    EmlTooLarge,
    /// Stage 9 (decision 0027): the file is empty or has no readable mail header.
    EmlInvalid,
    /// Stage 9 (decision 0027): the file is missing, a folder, or could not be read.
    EmlUnreadable,
    /// Stage 7 (decision 0026): another saved search has this name.
    SavedSearchExists,
    /// Stage 7 (decision 0026): the most saved searches are stored already.
    SavedSearchLimit,
    /// Stage 16 (decision 0036): the core does not preview this attachment (type, or bytes
    /// that are not what the type says); it can still be saved or opened in the system.
    PreviewUnsupported,
    /// Stage 16 (decision 0036): the attachment is larger than a preview decodes.
    PreviewTooLarge,
    /// Stage 16 (decision 0036): the image or PDF could not be decoded for a preview.
    PreviewDamaged,
    /// Stage 15 (decision 0037): the update manifest could not be fetched or read.
    UpdateCheckFailed,
    /// Stage 15 (decision 0037): the update file could not be downloaded.
    UpdateDownloadFailed,
    /// Stage 15 (decision 0037): the update's signature is not valid; it was discarded.
    UpdateSignatureInvalid,
    /// Stage 15 (decision 0037): installing failed; the installed version is unchanged.
    UpdateInstallFailed,
    /// Stage 15 (decision 0037): no update is available or ready for this step.
    UpdateNotReady,
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

    /// The SMTP server refused the message (sender, a recipient, DATA or the content); nothing
    /// was delivered. `permanent` for 5xx replies; 4xx may succeed later. Holds the replies.
    /// Built by the engine from [`crate::SendRejection`].
    #[error("server refused the message: {detail}")]
    SendRejected { permanent: bool, detail: String },

    /// The message was transmitted, but the connection ended before the server confirmed it.
    /// Never retried automatically. Built by the engine from [`crate::SendOutcome::Unclear`].
    #[error("unclear whether the message was sent: {0}")]
    SendUnclear(String),
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
            Self::SendRejected { .. } => ErrorCode::SendRejected,
            Self::SendUnclear(_) => ErrorCode::SendUnclear,
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
                | Self::SendRejected {
                    permanent: false,
                    ..
                }
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
            Self::SendRejected { .. } => "The server refused to send the message.",
            Self::SendUnclear(_) => {
                "The connection broke while sending; the message may have been sent."
            }
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
            | Self::InvalidState(d)
            | Self::SendRejected { detail: d, .. }
            | Self::SendUnclear(d) => (!d.is_empty()).then(|| d.clone()),
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

    /// A writing action was refused before anything was recorded (stage 4, outbox).
    #[error(transparent)]
    Action(#[from] crate::ActionError),

    /// A compose request was refused (stage 5); nothing changed.
    #[error(transparent)]
    Draft(#[from] crate::DraftError),

    /// A settings sync request failed (stage 11).
    #[error(transparent)]
    SettingsSync(#[from] crate::SettingsSyncFailure),

    /// A contact request was refused (stage 10); nothing changed.
    #[error(transparent)]
    Contact(#[from] crate::ContactRequestError),

    /// An AI request failed or was refused (stage 13).
    #[error(transparent)]
    Ai(#[from] crate::AiFailure),

    /// An `.eml` file could not be opened (stage 9, decision 0027); nothing changed.
    #[error(transparent)]
    Eml(#[from] crate::EmlError),

    /// A saved search was refused (stage 7, decision 0026); nothing changed.
    #[error(transparent)]
    SavedSearch(#[from] crate::SavedSearchError),

    /// An attachment could not be previewed (stage 16, decision 0036); nothing was shown.
    #[error(transparent)]
    Preview(#[from] crate::PreviewError),

    /// An update step failed (stage 15, decision 0037).
    #[error(transparent)]
    Update(#[from] crate::UpdateError),
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
            Self::Action(err) => err.code(),
            Self::Draft(err) => err.code(),
            Self::SettingsSync(err) => err.code(),
            Self::Contact(err) => err.code(),
            Self::Ai(err) => err.code(),
            Self::Eml(err) => err.code(),
            Self::SavedSearch(err) => err.code(),
            Self::Preview(err) => err.code(),
            Self::Update(err) => err.code(),
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
            Self::Action(err) => err.message(),
            Self::Draft(err) => err.message(),
            Self::SettingsSync(err) => err.message(),
            Self::Contact(err) => err.message(),
            Self::Ai(err) => err.message(),
            Self::Eml(err) => err.message(),
            Self::SavedSearch(err) => err.message(),
            Self::Preview(err) => err.message(),
            Self::Update(err) => err.message(),
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
            Self::Action(err) => err.detail(),
            Self::Draft(err) => err.detail(),
            Self::SettingsSync(err) => err.detail(),
            Self::Contact(err) => err.detail(),
            Self::Ai(err) => err.detail(),
            Self::Eml(err) => err.detail(),
            Self::SavedSearch(err) => err.detail(),
            Self::Preview(err) => err.detail(),
            Self::Update(err) => err.detail(),
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
            (ErrorCode::MissingMailbox, "missing_mailbox"),
            (ErrorCode::ConfirmationRequired, "confirmation_required"),
            (ErrorCode::NotUndoable, "not_undoable"),
            (ErrorCode::InvalidKeyword, "invalid_keyword"),
            (ErrorCode::SendRejected, "send_rejected"),
            (ErrorCode::SendUnclear, "send_unclear"),
            (ErrorCode::InvalidAddress, "invalid_address"),
            (ErrorCode::AttachmentTooLarge, "attachment_too_large"),
            (ErrorCode::SendInProgress, "send_in_progress"),
            (ErrorCode::SyncWrongPassphrase, "sync_wrong_passphrase"),
            (ErrorCode::SyncNotSetUp, "sync_not_set_up"),
            (ErrorCode::SyncAlreadySetUp, "sync_already_set_up"),
            (ErrorCode::SyncKeyChanged, "sync_key_changed"),
            (ErrorCode::SyncKeyInvalid, "sync_key_invalid"),
            (ErrorCode::SyncOff, "sync_off"),
            (ErrorCode::SyncKeyMissing, "sync_key_missing"),
            (ErrorCode::AiOff, "ai_off"),
            (ErrorCode::AiConsentRequired, "ai_consent_required"),
            (ErrorCode::AiNotPermitted, "ai_not_permitted"),
            (ErrorCode::AiCancelled, "ai_cancelled"),
            (ErrorCode::AiUnreachable, "ai_unreachable"),
            (ErrorCode::AiUnauthorized, "ai_unauthorized"),
            (ErrorCode::AiBusy, "ai_busy"),
            (ErrorCode::AiRefused, "ai_refused"),
            (ErrorCode::AiFailed, "ai_failed"),
            (ErrorCode::EmlTooLarge, "eml_too_large"),
            (ErrorCode::EmlInvalid, "eml_invalid"),
            (ErrorCode::EmlUnreadable, "eml_unreadable"),
            (ErrorCode::SavedSearchExists, "saved_search_exists"),
            (ErrorCode::SavedSearchLimit, "saved_search_limit"),
            (ErrorCode::PreviewUnsupported, "preview_unsupported"),
            (ErrorCode::PreviewTooLarge, "preview_too_large"),
            (ErrorCode::PreviewDamaged, "preview_damaged"),
            (ErrorCode::UpdateCheckFailed, "update_check_failed"),
            (ErrorCode::UpdateDownloadFailed, "update_download_failed"),
            (
                ErrorCode::UpdateSignatureInvalid,
                "update_signature_invalid",
            ),
            (ErrorCode::UpdateInstallFailed, "update_install_failed"),
            (ErrorCode::UpdateNotReady, "update_not_ready"),
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
            (
                B::SendRejected {
                    permanent: true,
                    detail: "550 no".into(),
                },
                ErrorCode::SendRejected,
                false,
            ),
            (
                B::SendRejected {
                    permanent: false,
                    detail: "451 later".into(),
                },
                ErrorCode::SendRejected,
                true,
            ),
            (B::SendUnclear("eof".into()), ErrorCode::SendUnclear, false),
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
