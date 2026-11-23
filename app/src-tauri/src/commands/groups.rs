//! Account groups, hidden accounts and identities (stage 7). Each one forwards to [`AppCore`].

use mail_engine::AppCore;
use mail_model::{
    AccountGroup, AccountGroupId, AccountId, IdentityId, IdentityInfo, IdentityInput, IpcError,
    MessageId,
};
use tauri::State;

use super::to_ipc;

/// All account groups in sidebar order.
#[tauri::command]
#[specta::specta]
pub async fn list_account_groups(core: State<'_, AppCore>) -> Result<Vec<AccountGroup>, IpcError> {
    core.list_account_groups()
        .await
        .map_err(to_ipc("list_account_groups"))
}

#[tauri::command]
#[specta::specta]
pub async fn create_account_group(
    core: State<'_, AppCore>,
    name: String,
    account_ids: Vec<AccountId>,
) -> Result<AccountGroup, IpcError> {
    core.create_account_group(name, account_ids)
        .await
        .map_err(to_ipc("create_account_group"))
}

/// Renames a group and replaces its members.
#[tauri::command]
#[specta::specta]
pub async fn update_account_group(
    core: State<'_, AppCore>,
    group_id: AccountGroupId,
    name: String,
    account_ids: Vec<AccountId>,
) -> Result<AccountGroup, IpcError> {
    core.update_account_group(group_id, name, account_ids)
        .await
        .map_err(to_ipc("update_account_group"))
}

#[tauri::command]
#[specta::specta]
pub async fn delete_account_group(
    core: State<'_, AppCore>,
    group_id: AccountGroupId,
) -> Result<(), IpcError> {
    core.delete_account_group(group_id)
        .await
        .map_err(to_ipc("delete_account_group"))
}

/// Accounts hidden on this device.
#[tauri::command]
#[specta::specta]
pub async fn list_hidden_accounts(core: State<'_, AppCore>) -> Result<Vec<AccountId>, IpcError> {
    core.list_hidden_accounts()
        .await
        .map_err(to_ipc("list_hidden_accounts"))
}

/// Hides an account from the unified views and "search everywhere", or shows it again.
#[tauri::command]
#[specta::specta]
pub async fn set_account_hidden(
    core: State<'_, AppCore>,
    account_id: AccountId,
    hidden: bool,
) -> Result<(), IpcError> {
    core.set_account_hidden(account_id, hidden)
        .await
        .map_err(to_ipc("set_account_hidden"))
}

/// Identities of an account, the default first.
#[tauri::command]
#[specta::specta]
pub async fn list_identities(
    core: State<'_, AppCore>,
    account_id: AccountId,
) -> Result<Vec<IdentityInfo>, IpcError> {
    core.list_identities(account_id)
        .await
        .map_err(to_ipc("list_identities"))
}

#[tauri::command]
#[specta::specta]
pub async fn add_identity(
    core: State<'_, AppCore>,
    account_id: AccountId,
    identity: IdentityInput,
) -> Result<IdentityInfo, IpcError> {
    core.add_identity(account_id, identity)
        .await
        .map_err(to_ipc("add_identity"))
}

#[tauri::command]
#[specta::specta]
pub async fn update_identity(
    core: State<'_, AppCore>,
    identity_id: IdentityId,
    identity: IdentityInput,
) -> Result<IdentityInfo, IpcError> {
    core.update_identity(identity_id, identity)
        .await
        .map_err(to_ipc("update_identity"))
}

/// Makes an identity the default of its account; returns the account's identities.
#[tauri::command]
#[specta::specta]
pub async fn set_default_identity(
    core: State<'_, AppCore>,
    identity_id: IdentityId,
) -> Result<Vec<IdentityInfo>, IpcError> {
    core.set_default_identity(identity_id)
        .await
        .map_err(to_ipc("set_default_identity"))
}

/// Deletes an identity (not the default one).
#[tauri::command]
#[specta::specta]
pub async fn delete_identity(
    core: State<'_, AppCore>,
    identity_id: IdentityId,
) -> Result<(), IpcError> {
    core.delete_identity(identity_id)
        .await
        .map_err(to_ipc("delete_identity"))
}

/// The identity a reply to the message is sent from.
#[tauri::command]
#[specta::specta]
pub async fn get_reply_identity(
    core: State<'_, AppCore>,
    message_id: MessageId,
) -> Result<IdentityInfo, IpcError> {
    core.reply_identity(message_id)
        .await
        .map_err(to_ipc("get_reply_identity"))
}
