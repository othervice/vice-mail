//! Writing actions and the outbox (stage 4): apply and undo actions, create folders, the
//! activity view (list, retry, discard), counts, and the per-account write switch. Each one
//! forwards to [`AppCore`].

use mail_engine::AppCore;
use mail_model::{
    Account, AccountId, ActionResult, BatchId, IpcError, MailAction, MailboxRole, MessageRef,
    OperationId, OperationInfo, OutboxCounts,
};
use tauri::State;

use super::to_ipc;

/// Applies an action to messages of one account: local at once, then sent to the server. `requestId` is a UUID per user action.
#[tauri::command]
#[specta::specta]
pub async fn apply_action(
    core: State<'_, AppCore>,
    action: MailAction,
    messages: Vec<MessageRef>,
    request_id: String,
) -> Result<ActionResult, IpcError> {
    core.apply_action(action, messages, request_id)
        .await
        .map_err(to_ipc("apply_action"))
}

/// Undoes the batch of an earlier action (the notice's "Undo").
#[tauri::command]
#[specta::specta]
pub async fn undo_action(
    core: State<'_, AppCore>,
    batch: BatchId,
) -> Result<ActionResult, IpcError> {
    core.undo_action(batch).await.map_err(to_ipc("undo_action"))
}

/// Queues the creation of a folder (for example the archive after `missing_mailbox`).
#[tauri::command]
#[specta::specta]
pub async fn create_mailbox(
    core: State<'_, AppCore>,
    account_id: AccountId,
    path: String,
    role: Option<MailboxRole>,
) -> Result<ActionResult, IpcError> {
    core.create_mailbox(account_id, path, role)
        .await
        .map_err(to_ipc("create_mailbox"))
}

/// The account's operations, newest first, below `before` (activity view). `limit` 1..500.
#[tauri::command]
#[specta::specta]
pub async fn list_operations(
    core: State<'_, AppCore>,
    account_id: AccountId,
    before: Option<OperationId>,
    limit: u32,
) -> Result<Vec<OperationInfo>, IpcError> {
    core.list_operations(account_id, before, limit)
        .await
        .map_err(to_ipc("list_operations"))
}

/// Queues a failed or conflicting operation again.
#[tauri::command]
#[specta::specta]
pub async fn retry_operation(
    core: State<'_, AppCore>,
    operation_id: OperationId,
) -> Result<ActionResult, IpcError> {
    core.retry_operation(operation_id)
        .await
        .map_err(to_ipc("retry_operation"))
}

/// Removes a finished operation from the activity view.
#[tauri::command]
#[specta::specta]
pub async fn discard_operation(
    core: State<'_, AppCore>,
    operation_id: OperationId,
) -> Result<(), IpcError> {
    core.discard_operation(operation_id)
        .await
        .map_err(to_ipc("discard_operation"))
}

/// Pending, failed and conflicting operations of an account.
#[tauri::command]
#[specta::specta]
pub async fn get_outbox_counts(
    core: State<'_, AppCore>,
    account_id: AccountId,
) -> Result<OutboxCounts, IpcError> {
    core.outbox_counts(account_id)
        .await
        .map_err(to_ipc("get_outbox_counts"))
}

/// Turns writing on or off for an account (the user's switch); returns the account.
#[tauri::command]
#[specta::specta]
pub async fn set_writes_enabled(
    core: State<'_, AppCore>,
    account_id: AccountId,
    enabled: bool,
) -> Result<Account, IpcError> {
    core.set_writes_enabled(account_id, enabled)
        .await
        .map_err(to_ipc("set_writes_enabled"))
}
