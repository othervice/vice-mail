//! Account commands: discovery, connection test, add, remove, retry, new password.

use mail_engine::AppCore;
use mail_model::{
    Account, AccountId, AccountSettings, ConnectionTestResult, DiscoveryResult, IpcError, Secret,
};
use tauri::State;

use super::to_ipc;

/// All accounts, oldest first, with their live sync status.
#[tauri::command]
#[specta::specta]
pub async fn list_accounts(core: State<'_, AppCore>) -> Result<Vec<Account>, IpcError> {
    core.list_accounts().await.map_err(to_ipc("list_accounts"))
}

/// Server settings from the built-in table of well-known providers. No network.
#[tauri::command]
#[specta::specta]
pub async fn discover_known_provider(
    core: State<'_, AppCore>,
    email: String,
) -> Result<DiscoveryResult, IpcError> {
    core.discover_known_provider(&email)
        .map_err(to_ipc("discover_known_provider"))
}

/// DNS SRV (RFC 6186), then `autoconfig.<domain>`. Only after the user asked for it.
#[tauri::command]
#[specta::specta]
pub async fn discover_online(
    core: State<'_, AppCore>,
    email: String,
) -> Result<DiscoveryResult, IpcError> {
    core.discover_online(&email)
        .await
        .map_err(to_ipc("discover_online"))
}

/// Connects to IMAP read-only, logs in and out. Stores nothing.
#[tauri::command]
#[specta::specta]
pub async fn test_connection(
    core: State<'_, AppCore>,
    settings: AccountSettings,
    password: String,
) -> Result<ConnectionTestResult, IpcError> {
    let password = Secret::new(password);
    core.test_connection(settings, password)
        .await
        .map_err(to_ipc("test_connection"))
}

/// Checks the login, stores the password in the keychain, adds the account read-only and starts its sync.
#[tauri::command]
#[specta::specta]
pub async fn add_account(
    core: State<'_, AppCore>,
    settings: AccountSettings,
    password: String,
) -> Result<Account, IpcError> {
    let password = Secret::new(password);
    core.add_account(settings, password)
        .await
        .map_err(to_ipc("add_account"))
}

/// Stops the sync, deletes the keychain entry and all local data of the account.
#[tauri::command]
#[specta::specta]
pub async fn remove_account(
    core: State<'_, AppCore>,
    account_id: AccountId,
) -> Result<(), IpcError> {
    core.remove_account(account_id)
        .await
        .map_err(to_ipc("remove_account"))
}

/// Reconnects now: cuts a backoff short and leaves the `error` state.
#[tauri::command]
#[specta::specta]
pub async fn retry_account(
    core: State<'_, AppCore>,
    account_id: AccountId,
) -> Result<(), IpcError> {
    core.retry_account(account_id)
        .await
        .map_err(to_ipc("retry_account"))
}

/// Checks the login with the new password, replaces the keychain entry and reconnects.
#[tauri::command]
#[specta::specta]
pub async fn update_password(
    core: State<'_, AppCore>,
    account_id: AccountId,
    password: String,
) -> Result<(), IpcError> {
    let password = Secret::new(password);
    core.update_password(account_id, password)
        .await
        .map_err(to_ipc("update_password"))
}
