//! Typed errors of the settings sync. Messages name fields and kinds, never values: a rejected
//! value may be exactly the secret the check is there to keep out.

use super::hlc::Hlc;

/// A pass, the setup or a key operation failed as a whole. Problems with single remote
/// entries are not errors; they come back as [`super::Notice`]s and the rest goes on.
#[derive(Debug, thiserror::Error)]
pub enum SettingsSyncError {
    /// The passphrase does not unlock any key record in the folder. Nothing was written.
    #[error("the passphrase does not unlock the settings sync key")]
    WrongPassphrase,

    /// Setting up or changing the passphrase with an empty one.
    #[error("the passphrase is empty")]
    EmptyPassphrase,

    /// The folder holds no key record: settings sync was never set up here, or the folder was
    /// emptied. Nothing is uploaded until the user sets it up again.
    #[error("the settings sync folder has no key record")]
    NotSetUp,

    /// Setup found a key record already; join with the passphrase instead.
    #[error("settings sync is already set up in this folder")]
    AlreadySetUp,

    /// The folder's key record belongs to another key: sync was reset on another device. The
    /// user enters the passphrase again ([`super::join`]); nothing was written.
    #[error("settings sync was reset on another device; enter the passphrase again")]
    KeyChanged,

    /// A key record decrypted but is damaged, from a newer app version, or does not match its
    /// header.
    #[error("the settings sync key record is invalid: {0}")]
    InvalidKeyRecord(String),

    #[error(transparent)]
    Record(#[from] RecordError),

    #[error(transparent)]
    Clock(#[from] HlcError),

    #[error(transparent)]
    Transport(#[from] TransportError),

    /// age refused to encrypt (only on invalid input; holds age's message).
    #[error("encryption failed: {0}")]
    Encrypt(String),
}

/// A record or its content is not acceptable for the sync.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RecordError {
    /// The kind name is not in the registry (for example from a newer app version).
    #[error("record kind {0:?} is not registered for the settings sync")]
    UnknownKind(String),

    /// The id does not have the shape the kind requires.
    #[error("invalid {kind} record id: {reason}")]
    InvalidId { kind: &'static str, reason: String },

    /// A setting key that is not registered as synchronised (device settings stay local).
    #[error("setting {0:?} is not registered for the settings sync")]
    SettingNotSynced(String),

    /// The content holds a field whose name says it is a password, token or key. Holds the
    /// path of the field, never its value.
    #[error("{kind} content must not hold secrets: field {path}")]
    SecretField { kind: &'static str, path: String },

    /// A string value looks like key material (an age secret key or a PEM private key).
    /// Holds the path, never the value.
    #[error("{kind} content must not hold key material: value at {path}")]
    SecretValue { kind: &'static str, path: String },

    /// The serialised content is larger than the kind allows.
    #[error("{kind} content is {size} bytes, more than the {max} allowed")]
    TooLarge {
        kind: &'static str,
        size: usize,
        max: usize,
    },

    /// The content does not have the structure the kind requires.
    #[error("invalid {kind} content: {reason}")]
    InvalidContent { kind: &'static str, reason: String },

    /// A tombstone must be newer than the version it deletes.
    #[error("a tombstone at {version} cannot delete the newer version {deleted}")]
    TombstoneOrder { version: Hlc, deleted: Hlc },
}

/// The hybrid logical clock refused a timestamp.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum HlcError {
    /// A remote version lies further in the future than [`super::MAX_CLOCK_DRIFT_MS`]. The
    /// record is deferred, not dropped: it is merged once the local clock gets close enough.
    #[error("remote version is {ahead_ms} ms ahead of this device's clock")]
    ClockAhead { ahead_ms: i64 },

    /// More than 2^32 events within one millisecond, or a time beyond the year 2286.
    #[error("hybrid logical clock overflow")]
    Overflow,

    /// The text form of a version or device id does not parse.
    #[error("invalid version text {0:?}")]
    InvalidText(String),
}

/// The transport could not list, read, write or delete.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TransportError {
    /// The server is not reachable right now; retry later.
    #[error("settings sync folder unavailable: {0}")]
    Unavailable(String),

    /// The server refused the operation (quota, permissions, folder missing).
    #[error("settings sync folder refused the operation: {0}")]
    Refused(String),
}
