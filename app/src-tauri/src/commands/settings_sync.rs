//! Settings sync (stage 11, decisions 0014 and 0022): turning it on with a sync account and a
//! passphrase, joining from a second device (preview first), the status, "sync now",
//! questions about accounts removed elsewhere, the passphrase, the reset, turning it off and
//! the device name. Each one forwards to [`AppCore`]. Event: `settingsSyncChanged`.
//!
//! Passphrases cross the bridge as plain strings and become a [`Secret`] at once; they are
//! never stored (the keychain holds the sync key, decision 0014) and never logged.

use mail_engine::AppCore;
use mail_model::{AccountId, IpcError, Secret, SettingsSyncPreview, SettingsSyncStatus};
use tauri::State;

use super::to_ipc;

/// State, sync account and folder, last pass, devices, open questions, accounts that still
/// need their password, accounts whose mailbox holds a sync folder (while off).
#[tauri::command]
#[specta::specta]
pub async fn get_settings_sync_status(
    core: State<'_, AppCore>,
) -> Result<SettingsSyncStatus, IpcError> {
    core.get_settings_sync_status()
        .await
        .map_err(to_ipc("get_settings_sync_status"))
}

/// Turns the sync on with `account_id`: creates the folder (unsubscribed), protects a new key
/// with the passphrase and uploads this device's data. Allows writing to that one folder only.
/// `sync_already_set_up`: the account holds a sync already (join it instead).
#[tauri::command]
#[specta::specta]
pub async fn enable_settings_sync(
    core: State<'_, AppCore>,
    account_id: AccountId,
    passphrase: String,
) -> Result<SettingsSyncStatus, IpcError> {
    core.enable_settings_sync(account_id, Secret::new(passphrase))
        .await
        .map_err(to_ipc("enable_settings_sync"))
}

/// Second device (or after a reset elsewhere): checks the passphrase and shows what joining
/// would take over. Writes nothing; `sync_wrong_passphrase` changes nothing.
#[tauri::command]
#[specta::specta]
pub async fn preview_settings_sync_join(
    core: State<'_, AppCore>,
    account_id: AccountId,
    passphrase: String,
) -> Result<SettingsSyncPreview, IpcError> {
    core.preview_settings_sync_join(account_id, Secret::new(passphrase))
        .await
        .map_err(to_ipc("preview_settings_sync_join"))
}

/// Confirms the previewed join: merges, takes accounts over (they need their password here).
#[tauri::command]
#[specta::specta]
pub async fn join_settings_sync(core: State<'_, AppCore>) -> Result<SettingsSyncStatus, IpcError> {
    core.join_settings_sync()
        .await
        .map_err(to_ipc("join_settings_sync"))
}

/// A pass now.
#[tauri::command]
#[specta::specta]
pub async fn sync_settings_now(core: State<'_, AppCore>) -> Result<SettingsSyncStatus, IpcError> {
    core.sync_settings_now()
        .await
        .map_err(to_ipc("sync_settings_now"))
}

/// Another device removed `account_id`: `remove` removes it here too, otherwise it is kept and
/// comes back on every device.
#[tauri::command]
#[specta::specta]
pub async fn resolve_sync_question(
    core: State<'_, AppCore>,
    account_id: AccountId,
    remove: bool,
) -> Result<SettingsSyncStatus, IpcError> {
    core.resolve_sync_question(account_id, remove)
        .await
        .map_err(to_ipc("resolve_sync_question"))
}

/// Protects the key with a new passphrase; other devices keep working.
#[tauri::command]
#[specta::specta]
pub async fn change_settings_sync_passphrase(
    core: State<'_, AppCore>,
    passphrase: String,
) -> Result<(), IpcError> {
    core.change_settings_sync_passphrase(Secret::new(passphrase))
        .await
        .map_err(to_ipc("change_settings_sync_passphrase"))
}

/// Forgotten passphrase or "reset sync": empties the folder, new key, uploads this device's
/// data. Other devices then need the new passphrase.
#[tauri::command]
#[specta::specta]
pub async fn reset_settings_sync(
    core: State<'_, AppCore>,
    passphrase: String,
) -> Result<SettingsSyncStatus, IpcError> {
    core.reset_settings_sync(Secret::new(passphrase))
        .await
        .map_err(to_ipc("reset_settings_sync"))
}

/// Turns the sync off on this device (the key leaves the keychain; the folder stays).
#[tauri::command]
#[specta::specta]
pub async fn disable_settings_sync(
    core: State<'_, AppCore>,
) -> Result<SettingsSyncStatus, IpcError> {
    core.disable_settings_sync()
        .await
        .map_err(to_ipc("disable_settings_sync"))
}

/// The name of this device in the device list (1 to 64 characters).
#[tauri::command]
#[specta::specta]
pub async fn set_sync_device_name(
    core: State<'_, AppCore>,
    name: String,
) -> Result<SettingsSyncStatus, IpcError> {
    core.set_sync_device_name(name)
        .await
        .map_err(to_ipc("set_sync_device_name"))
}
