//! Mailbox and message list commands.

use mail_engine::AppCore;
use mail_model::{AccountId, IpcError, ListCursor, Mailbox, MailboxId, MailboxRole, MessagePage};
use tauri::State;

use super::to_ipc;

/// Mailboxes of an account, ordered by path.
#[tauri::command]
#[specta::specta]
pub async fn list_mailboxes(
    core: State<'_, AppCore>,
    account_id: AccountId,
) -> Result<Vec<Mailbox>, IpcError> {
    core.list_mailboxes(account_id)
        .await
        .map_err(to_ipc("list_mailboxes"))
}

/// Sets the role of a mailbox (source `user`) and returns all mailboxes of its account.
#[tauri::command]
#[specta::specta]
pub async fn set_mailbox_role(
    core: State<'_, AppCore>,
    mailbox_id: MailboxId,
    role: MailboxRole,
) -> Result<Vec<Mailbox>, IpcError> {
    core.set_mailbox_role(mailbox_id, role)
        .await
        .map_err(to_ipc("set_mailbox_role"))
}

/// One keyset page of a mailbox, newest first. `limit` 1..500.
#[tauri::command]
#[specta::specta]
pub async fn list_messages(
    core: State<'_, AppCore>,
    mailbox_id: MailboxId,
    cursor: Option<ListCursor>,
    limit: u32,
) -> Result<MessagePage, IpcError> {
    core.list_messages(mailbox_id, cursor, limit)
        .await
        .map_err(to_ipc("list_messages"))
}
