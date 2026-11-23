//! Mailbox and message list commands.

use mail_engine::AppCore;
use mail_model::{
    AccountId, ActionResult, IpcError, ListCursor, Mailbox, MailboxId, MailboxRole, MessageId,
    MessageListItem, MessagePage,
};
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

/// Stage 13 (decision 0027): the list rows of these messages in the order asked, each in its
/// first folder, to name and open the sources of an AI answer. Unknown ids are left out; more
/// than `MAX_MESSAGES_BY_IDS` ids is `invalid_argument`.
#[tauri::command]
#[specta::specta]
pub async fn get_messages_by_ids(
    core: State<'_, AppCore>,
    message_ids: Vec<MessageId>,
) -> Result<Vec<MessageListItem>, IpcError> {
    core.get_messages_by_ids(message_ids)
        .await
        .map_err(to_ipc("get_messages_by_ids"))
}

/// Stage 14 (decision 0034): the Gmail labels of a message (`\Inbox`, `\Important`, user
/// labels by their folder path); null for messages of other servers. They change through
/// `apply_action` with `addLabel` / `removeLabel`.
#[tauri::command]
#[specta::specta]
pub async fn get_message_labels(
    core: State<'_, AppCore>,
    message_id: MessageId,
) -> Result<Option<Vec<String>>, IpcError> {
    core.get_message_labels(message_id)
        .await
        .map_err(to_ipc("get_message_labels"))
}

/// Stage 14: adds (`add`) or removes a Gmail label of one message through the outbox; the core
/// picks the folder the change is sent from. `requestId` is a fresh UUID per user action.
#[tauri::command]
#[specta::specta]
pub async fn set_message_label(
    core: State<'_, AppCore>,
    message_id: MessageId,
    label: String,
    add: bool,
    request_id: String,
) -> Result<ActionResult, IpcError> {
    core.set_message_label(message_id, label, add, request_id)
        .await
        .map_err(to_ipc("set_message_label"))
}
