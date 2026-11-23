//! AI access over MCP (stage 12, decisions 0010, 0015, 0021): the switch (also the emergency
//! stop), what AI clients may do with each account, registering and revoking clients with the
//! ready-made configurations, the access log, the endpoint's state and "test connection". Each
//! one forwards to [`AppCore`]. Events: `mcpAccess`, `mcpStatusChanged`.

use mail_engine::AppCore;
use mail_model::{
    AccountId, IpcError, McpAccessId, McpAccessPage, McpAccountAccess, McpClientCreated,
    McpClientInfo, McpConnectionTest, McpSettingsInfo, McpSetup, McpStatus,
};
use tauri::State;

use super::to_ipc;

/// Switch, accounts with their access level, clients and the endpoint's state.
#[tauri::command]
#[specta::specta]
pub async fn get_mcp_settings(core: State<'_, AppCore>) -> Result<McpSettingsInfo, IpcError> {
    core.get_mcp_settings()
        .await
        .map_err(to_ipc("get_mcp_settings"))
}

/// On or off. Off is the emergency stop: every open AI connection closes at once.
#[tauri::command]
#[specta::specta]
pub async fn set_mcp_enabled(
    core: State<'_, AppCore>,
    enabled: bool,
) -> Result<McpSettingsInfo, IpcError> {
    core.set_mcp_enabled(enabled)
        .await
        .map_err(to_ipc("set_mcp_enabled"))
}

/// `hidden`, `read` or `read_and_drafts` for one account.
#[tauri::command]
#[specta::specta]
pub async fn set_mcp_account_access(
    core: State<'_, AppCore>,
    account_id: AccountId,
    access: McpAccountAccess,
) -> Result<McpSettingsInfo, IpcError> {
    core.set_mcp_account_access(account_id, access)
        .await
        .map_err(to_ipc("set_mcp_account_access"))
}

/// Registers a client; returns its token and the configurations with it, once.
#[tauri::command]
#[specta::specta]
pub async fn create_mcp_client(
    core: State<'_, AppCore>,
    name: String,
) -> Result<McpClientCreated, IpcError> {
    core.create_mcp_client(name)
        .await
        .map_err(to_ipc("create_mcp_client"))
}

/// Revokes a client: its token stops working, its connections close.
#[tauri::command]
#[specta::specta]
pub async fn revoke_mcp_client(
    core: State<'_, AppCore>,
    client_id: String,
) -> Result<McpSettingsInfo, IpcError> {
    core.revoke_mcp_client(client_id)
        .await
        .map_err(to_ipc("revoke_mcp_client"))
}

/// Registered and revoked clients, newest first.
#[tauri::command]
#[specta::specta]
pub async fn list_mcp_clients(core: State<'_, AppCore>) -> Result<Vec<McpClientInfo>, IpcError> {
    core.list_mcp_clients()
        .await
        .map_err(to_ipc("list_mcp_clients"))
}

/// Helper path, endpoint and the configurations with the placeholder `<token>`.
#[tauri::command]
#[specta::specta]
pub fn get_mcp_setup(core: State<'_, AppCore>) -> McpSetup {
    core.get_mcp_setup()
}

/// The access log ("Zugriffe"), newest first, below `before`. `limit` 1..=500.
#[tauri::command]
#[specta::specta]
pub async fn list_mcp_access(
    core: State<'_, AppCore>,
    before: Option<McpAccessId>,
    limit: u32,
) -> Result<McpAccessPage, IpcError> {
    core.list_mcp_access(before, limit)
        .await
        .map_err(to_ipc("list_mcp_access"))
}

/// Whether AI access is on, the endpoint's state and the helper.
#[tauri::command]
#[specta::specta]
pub fn get_mcp_status(core: State<'_, AppCore>) -> McpStatus {
    core.get_mcp_status()
}

/// Starts the helper like an AI client would and runs the handshake through it.
#[tauri::command]
#[specta::specta]
pub async fn test_mcp_connection(core: State<'_, AppCore>) -> Result<McpConnectionTest, IpcError> {
    core.test_mcp_connection()
        .await
        .map_err(to_ipc("test_mcp_connection"))
}
