//! Stage 11, settings sync (`docs/SYNC.md`): the folder in the user's mailbox that holds the
//! encrypted records, the adapter traits for it, and the types the settings page sees.
//!
//! **The folder is the only place a settings-sync connection may write.** Turning the sync on
//! for an account allows appending to and expunging from exactly this one folder (decision
//! 0014); every other folder of the account stays read-only unless the user turns writing on
//! for the whole account. The adapter enforces that on the connection it hands out
//! ([`SyncFolderConnector::connect_sync_folder`]): any writing command outside the folder fails
//! with [`crate::BackendError::ReadOnly`] before it is sent, also for accounts with
//! `writes_enabled = false`.

use std::future::Future;

use serde::{Deserialize, Serialize};
use specta_typescript::Number;

use crate::{
    AccountId, BackendError, ConnectConfig, IpcError, ModSeq, ServerSettings, Uid, UidSet,
    UidValidity,
};

/// Name of the settings sync folder (decision 0022). Plain ASCII without hierarchy
/// delimiters, spaces or a leading dot, so it needs no modified UTF-7, never splits into a
/// hierarchy and is valid on every common server; neutral (no product name) and descriptive
/// for people who see it in another mail client. Created at the top level, or below `INBOX`
/// on servers that keep every folder there.
pub const SETTINGS_SYNC_FOLDER: &str = "Settings-Sync";

/// Header field with the record's opaque id (`docs/SYNC.md`).
pub const SYNC_ID_HEADER: &str = "X-Settings-Sync-Id";

/// Header field with the record's version (`docs/SYNC.md`).
pub const SYNC_VERSION_HEADER: &str = "X-Settings-Sync-Version";

/// Shortest passphrase the core accepts. The UI shows a strength hint on top of this.
pub const MIN_SYNC_PASSPHRASE_CHARS: u32 = 8;

/// Whether a mailbox (wire name, as LIST reports it) is a settings sync folder: the mail sync
/// leaves it out of the folder list, and its presence tells a new device that the account
/// holds a settings sync.
pub fn is_settings_sync_folder(name: &str, delimiter: Option<char>) -> bool {
    if name == SETTINGS_SYNC_FOLDER {
        return true;
    }
    let Some(delimiter) = delimiter else {
        return false;
    };
    name.len() > 6
        && name
            .get(..5)
            .is_some_and(|p| p.eq_ignore_ascii_case("INBOX"))
        && name[5..].strip_prefix(delimiter) == Some(SETTINGS_SYNC_FOLDER)
}

/// State of the sync folder after it was opened for writing (SELECT).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SyncFolderState {
    pub uid_validity: UidValidity,
    pub uid_next: Uid,
    pub exists: u32,
    /// `None` without CONDSTORE.
    pub highest_modseq: Option<ModSeq>,
}

/// One message of the sync folder, without its body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncFolderEntry {
    pub uid: Uid,
    /// RFC822.SIZE.
    pub size: u32,
    /// The raw `HEADER.FIELDS` block of the requested fields (may be empty).
    pub header: Vec<u8>,
}

/// One authenticated connection that may write to the settings sync folder only. Every other
/// mailbox of the account can only be read (and this type offers no way to read them).
pub trait SyncFolder: Send {
    /// Opens the folder with SELECT. With `create`, a missing folder is created first,
    /// without subscribing to it (most mail clients then do not show it). `None` if it does
    /// not exist and `create` is false.
    fn open(
        &mut self,
        create: bool,
    ) -> impl Future<Output = Result<Option<SyncFolderState>, BackendError>> + Send;

    /// UID, size and the given header fields of every message in the open folder.
    fn list(
        &mut self,
        fields: &[&str],
    ) -> impl Future<Output = Result<Vec<SyncFolderEntry>, BackendError>> + Send;

    /// Bodies (`BODY.PEEK[TEXT]`) of `uids`; messages that are gone are left out.
    fn read(
        &mut self,
        uids: &UidSet,
    ) -> impl Future<Output = Result<Vec<(Uid, Vec<u8>)>, BackendError>> + Send;

    /// Appends `message` to the folder. The UID from APPENDUID when the server has UIDPLUS.
    fn append(
        &mut self,
        message: &[u8],
    ) -> impl Future<Output = Result<Option<Uid>, BackendError>> + Send;

    /// Removes exactly these messages of the open folder: `\Deleted`, then `UID EXPUNGE`
    /// (UIDPLUS), or a plain EXPUNGE only when no other message there is marked deleted.
    fn delete(&mut self, uids: &UidSet) -> impl Future<Output = Result<(), BackendError>> + Send;

    fn logout(self) -> impl Future<Output = Result<(), BackendError>> + Send
    where
        Self: Sized;
}

/// Opens [`SyncFolder`] connections.
pub trait SyncFolderConnector: Send + Sync {
    type Folder: SyncFolder + 'static;

    /// Logs in with `config` (its access mode is ignored: the connection is read-only except
    /// for `folder`, a wire name) and returns the folder connection. Nothing is selected yet.
    fn connect_sync_folder(
        &self,
        config: &ConnectConfig,
        folder: &str,
    ) -> impl Future<Output = Result<Self::Folder, BackendError>> + Send;
}

// ---------------------------------------------------------------------------------------------
// IPC

/// Overall state of the settings sync on this device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum SettingsSyncState {
    /// Not turned on. `detectedAccounts` lists accounts whose mailbox holds a sync folder.
    Off,
    /// On, the last pass worked (or none ran yet).
    Active,
    /// The sync key is missing from the keychain (`sync_key_missing`), or the folder was reset
    /// on another device (`sync_key_changed`): enter the passphrase again
    /// (`previewSettingsSyncJoin`, then `joinSettingsSync`). Nothing is written.
    NeedsPassphrase,
    /// On, but the last pass failed (`lastError`); retried with the account's sync. Reading
    /// and sending mail are not affected.
    Error,
}

/// A device that takes part (each one writes a small record).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SyncDevice {
    /// Random id of the device in the sync (not tied to the hardware).
    pub id: String,
    pub name: String,
    pub system: String,
    /// Unix ms; updated at most once a day.
    #[specta(type = Number)]
    pub last_seen_at: i64,
    /// This device.
    pub current: bool,
}

/// A decision only the user can make.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SyncQuestion {
    /// Another device removed this account (or it vanished from the folder long ago). It stays
    /// here until the user decides (`resolveSyncQuestion`): remove it here too, or keep it,
    /// which brings it back on every device.
    AccountRemoved {
        account_id: AccountId,
        email: String,
    },
}

/// Everything the settings page shows about the sync.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SettingsSyncStatus {
    pub state: SettingsSyncState,
    /// The sync account; null while off.
    pub account_id: Option<AccountId>,
    /// Wire name of the sync folder; null while off.
    pub folder: Option<String>,
    /// Unix ms of the last complete pass; null before the first.
    #[specta(type = Option<Number>)]
    pub last_sync_at: Option<i64>,
    /// Why the last pass failed (`error`, `needsPassphrase`).
    pub last_error: Option<IpcError>,
    /// Local changes not yet in the folder.
    pub pending_changes: u32,
    /// Entries of the folder the last pass left out and reported (modified, damaged, from
    /// another key, too large, or waiting for the next pass); the rest synced as usual.
    pub rejected_entries: u32,
    pub devices: Vec<SyncDevice>,
    pub questions: Vec<SyncQuestion>,
    /// Accounts taken over from another device that still need their password on this one
    /// (`updatePassword`).
    pub accounts_needing_password: Vec<AccountId>,
    /// Accounts whose mailbox holds a sync folder while the sync is off here: offer to join
    /// with the passphrase.
    pub detected_accounts: Vec<AccountId>,
    /// Server settings another device changed for an account of this device. Not applied:
    /// the account keeps its servers (and the password stays with them) until the user
    /// confirms or dismisses the change (`resolveServerChange`, decision 0040).
    pub server_changes: Vec<SyncServerChange>,
    /// Stage 22 (decision 0076): this device's request for passwords from another device
    /// (`startPasswordTransfer`), or how the last one ended; null if there is none.
    pub password_transfer: Option<PasswordTransfer>,
    /// Stage 22: requests of other devices for passwords this device may have
    /// (`openPasswordRequest`, `givePasswords`, `declinePasswordRequest`).
    pub password_requests: Vec<PasswordRequest>,
}

/// Stage 22 (decision 0076): where this device's request for passwords stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum PasswordTransferState {
    /// The request is in the sync folder; no device answered yet.
    Waiting,
    /// A device answered: compare `code` with the one it shows and confirm there.
    Confirm,
    /// The passwords arrived (`stored`; `skipped` came without one).
    Done,
    /// The other device declined.
    Declined,
    /// Nobody answered within 15 minutes.
    Expired,
    /// The answer could not be read (not sealed to this request) or the request vanished.
    Failed,
}

/// Stage 22: this device's request for passwords from another device.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PasswordTransfer {
    pub state: PasswordTransferState,
    /// The code both devices show, `123 456`; null until a device answered.
    pub code: Option<String>,
    /// The name of the device that answered.
    pub other_device: Option<String>,
    /// Unix ms when the request runs out.
    #[specta(type = Number)]
    pub expires_at: i64,
    /// The accounts asked for.
    pub accounts: Vec<AccountId>,
    /// Accounts whose password arrived and is stored.
    pub stored: Vec<AccountId>,
    /// Accounts that came back without a password here (servers differ, none on the other
    /// device, or no longer waiting).
    pub skipped: Vec<AccountId>,
}

/// Stage 22: where a request of another device stands on this device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum PasswordRequestState {
    /// Seen in the folder; not answered here.
    Open,
    /// Answered here; waiting for the new device to send its key.
    Connecting,
    /// Compare `code` with the one the new device shows, then choose the accounts.
    Confirm,
    /// The new device answered another device.
    OtherDevice,
    /// The key the new device sent does not fit its request (tampered with): nothing is sent.
    Failed,
    /// The passwords were sent.
    Given,
    /// Declined here.
    Declined,
}

/// Stage 22: whether this device can give the password of an account a request asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum PasswordRequestAccountState {
    /// The account has a password here and the same servers on both devices.
    Ready,
    /// Host, port, security or user name of IMAP or SMTP differ (decision 0040): never sent.
    ServersDiffer,
    /// No password here (it signs in with its provider, or waits for its password too).
    NoPassword,
    /// The account does not exist here.
    Unknown,
}

/// Stage 22: an account a request asks for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PasswordRequestAccount {
    pub account_id: AccountId,
    pub state: PasswordRequestAccountState,
}

/// Stage 22: another device asks for passwords.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PasswordRequest {
    pub request_id: String,
    /// The name the new device gave itself (not proof of anything; the code is).
    pub device_name: String,
    #[specta(type = Number)]
    pub expires_at: i64,
    pub state: PasswordRequestState,
    /// The code to compare (`confirm`).
    pub code: Option<String>,
    pub accounts: Vec<PasswordRequestAccount>,
}

/// Server settings that arrived through the settings sync for an existing account and differ
/// from the ones this device uses (finding F1, decision 0040). The account's own `imap`/`smtp`
/// are the settings in use.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SyncServerChange {
    pub account_id: AccountId,
    pub imap: ServerSettings,
    pub smtp: ServerSettings,
}

/// What joining would take over, per kind of data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SyncKindCount {
    /// Kind name from `docs/SYNC.md` (`setting`, `signature`, `snooze`, …).
    pub kind: String,
    /// New here.
    pub added: u32,
    /// Exists here and changes.
    pub changed: u32,
    /// Exists here and is removed.
    pub removed: u32,
}

/// The preview a new device shows before it merges (`previewSettingsSyncJoin`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SettingsSyncPreview {
    pub kinds: Vec<SyncKindCount>,
    /// Addresses of accounts that will be added; each asks for its password afterwards.
    pub new_accounts: Vec<String>,
    /// Records of this device that will be uploaded.
    pub uploads: u32,
    /// Devices that already take part.
    pub devices: Vec<SyncDevice>,
}

/// A failed settings sync request (`CoreError::SettingsSync`).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SettingsSyncFailure {
    /// The passphrase does not unlock the folder's key. Nothing was written.
    #[error("the passphrase does not unlock the settings sync")]
    WrongPassphrase,
    /// The account has no sync folder (or it holds no key record): set the sync up instead.
    #[error("the account holds no settings sync")]
    NotSetUp,
    /// The account's folder holds a settings sync already: join it with the passphrase.
    #[error("the account holds a settings sync already")]
    AlreadySetUp,
    /// The folder was reset on another device; enter the (new) passphrase again.
    #[error("the settings sync was reset on another device")]
    KeyChanged,
    /// The folder's key record is damaged or from a newer app version.
    #[error("the settings sync key record is invalid: {0}")]
    KeyInvalid(String),
    /// The sync is on, but this device's keychain has no sync key (restored from a backup,
    /// removed by hand): enter the passphrase again.
    #[error("the settings sync key is missing on this device")]
    KeyMissing,
    /// The request needs the sync to be on.
    #[error("the settings sync is off")]
    Off,
}

impl SettingsSyncFailure {
    pub const fn code(&self) -> crate::ErrorCode {
        match self {
            Self::WrongPassphrase => crate::ErrorCode::SyncWrongPassphrase,
            Self::NotSetUp => crate::ErrorCode::SyncNotSetUp,
            Self::AlreadySetUp => crate::ErrorCode::SyncAlreadySetUp,
            Self::KeyChanged => crate::ErrorCode::SyncKeyChanged,
            Self::KeyInvalid(_) => crate::ErrorCode::SyncKeyInvalid,
            Self::KeyMissing => crate::ErrorCode::SyncKeyMissing,
            Self::Off => crate::ErrorCode::SyncOff,
        }
    }

    pub const fn message(&self) -> &'static str {
        match self {
            Self::WrongPassphrase => "The passphrase is not correct. Nothing was changed.",
            Self::NotSetUp => "This account holds no settings sync yet.",
            Self::AlreadySetUp => {
                "This account holds a settings sync already. Join it with its passphrase."
            }
            Self::KeyChanged => {
                "The settings sync was reset on another device. Enter the new passphrase."
            }
            Self::KeyInvalid(_) => "The settings sync data in the mailbox cannot be read.",
            Self::KeyMissing => {
                "The settings sync key is missing on this device. Enter the passphrase again."
            }
            Self::Off => "The settings sync is turned off.",
        }
    }

    pub fn detail(&self) -> Option<String> {
        match self {
            Self::KeyInvalid(detail) => Some(detail.clone()),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_the_folder_at_the_top_and_below_inbox() {
        assert!(is_settings_sync_folder("Settings-Sync", Some('/')));
        assert!(is_settings_sync_folder("Settings-Sync", None));
        assert!(is_settings_sync_folder("INBOX.Settings-Sync", Some('.')));
        assert!(is_settings_sync_folder("inbox/Settings-Sync", Some('/')));
        assert!(!is_settings_sync_folder("INBOX.Settings-Sync", Some('/')));
        assert!(!is_settings_sync_folder("Archive/Settings-Sync", Some('/')));
        assert!(!is_settings_sync_folder("settings-sync", Some('/')));
        assert!(!is_settings_sync_folder("INBOX", Some('/')));
    }

    #[test]
    fn status_wire_format() {
        let status = SettingsSyncStatus {
            state: SettingsSyncState::NeedsPassphrase,
            account_id: None,
            folder: None,
            last_sync_at: Some(1_764_000_000_000),
            last_error: None,
            pending_changes: 2,
            rejected_entries: 0,
            devices: vec![],
            questions: vec![SyncQuestion::AccountRemoved {
                account_id: "6f9619ff-8b86-4d11-b42d-00c04fc964ff".parse().expect("id"),
                email: "a@example.org".into(),
            }],
            accounts_needing_password: vec![],
            detected_accounts: vec![],
            server_changes: vec![],
            password_transfer: Some(PasswordTransfer {
                state: PasswordTransferState::Confirm,
                code: Some("123 456".into()),
                other_device: Some("Mac".into()),
                expires_at: 1_764_000_900_000,
                accounts: vec![],
                stored: vec![],
                skipped: vec![],
            }),
            password_requests: vec![],
        };
        let json = serde_json::to_value(&status).expect("serialize");
        assert_eq!(json["state"], "needsPassphrase");
        assert_eq!(json["lastSyncAt"], 1_764_000_000_000_i64);
        assert_eq!(json["questions"][0]["type"], "accountRemoved");
        assert_eq!(json["passwordTransfer"]["state"], "confirm");
        assert_eq!(json["passwordTransfer"]["otherDevice"], "Mac");
        assert_eq!(
            json["questions"][0]["accountId"],
            "6f9619ff-8b86-4d11-b42d-00c04fc964ff"
        );
    }
}
