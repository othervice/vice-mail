//! Stage 20, task E (decision 0060): how full an account's mailbox on the server is.

use mail_engine::AppCore;
use mail_model::{AccountId, IpcError, MailboxQuota};
use tauri::State;

use super::to_ipc;

/// IMAP QUOTA (RFC 9208) or JMAP quotas (RFC 9425) of the account, over its read-only reading
/// connection; kept for 15 minutes unless `refresh`.
#[tauri::command]
#[specta::specta]
pub async fn get_account_quota(
    core: State<'_, AppCore>,
    account_id: AccountId,
    refresh: bool,
) -> Result<MailboxQuota, IpcError> {
    core.get_account_quota(account_id, refresh)
        .await
        .map_err(to_ipc("get_account_quota"))
}
