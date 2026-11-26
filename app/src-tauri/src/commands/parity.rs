//! Stage 16 commands (decision 0033): the reading list, unread lists, folder favourites and
//! colours, blocked senders, and copying or moving between accounts. Each forwards to
//! [`AppCore`].

use mail_engine::AppCore;
use mail_model::{
    AccountColor, ActionResult, BlockedSender, DropResult, InboxScope, IpcError, Mailbox,
    MailboxId, MarkScope, MessageId, MessageListItem, MessageRef, PinnedAttachment, TransferMode,
    UnreadFirstCursor, UnreadFirstPage,
};
use tauri::State;

use super::to_ipc;

/// Puts messages on the reading list ("Zum Lesen", `on`) or takes them off. Local only;
/// emits `inboxChanged`.
#[tauri::command]
#[specta::specta]
pub async fn set_reading_list(
    core: State<'_, AppCore>,
    message_ids: Vec<MessageId>,
    on: bool,
) -> Result<(), IpcError> {
    core.set_reading_list(message_ids, on)
        .await
        .map_err(to_ipc("set_reading_list"))
}

/// "Zum Lesen": the reading list, the most recently added first.
#[tauri::command]
#[specta::specta]
pub async fn list_reading_list(
    core: State<'_, AppCore>,
    scope: MarkScope,
) -> Result<Vec<MessageListItem>, IpcError> {
    core.list_reading_list(scope)
        .await
        .map_err(to_ipc("list_reading_list"))
}

/// Stage 16 (decision 0036): pins attachment `index` of a message or takes the pin off. Local
/// only, synced; emits `inboxChanged`.
#[tauri::command]
#[specta::specta]
pub async fn set_attachment_pinned(
    core: State<'_, AppCore>,
    message_id: MessageId,
    index: u32,
    pinned: bool,
) -> Result<(), IpcError> {
    core.set_attachment_pinned(message_id, index, pinned)
        .await
        .map_err(to_ipc("set_attachment_pinned"))
}

/// The pinned attachment indexes of a message, for the reading pane.
#[tauri::command]
#[specta::specta]
pub async fn get_attachment_pins(
    core: State<'_, AppCore>,
    message_id: MessageId,
) -> Result<Vec<u32>, IpcError> {
    core.get_attachment_pins(message_id)
        .await
        .map_err(to_ipc("get_attachment_pins"))
}

/// "Angeheftete Anhänge": the pinned attachments of the scope, the newest pin first.
#[tauri::command]
#[specta::specta]
pub async fn list_pinned_attachments(
    core: State<'_, AppCore>,
    scope: MarkScope,
) -> Result<Vec<PinnedAttachment>, IpcError> {
    core.list_pinned_attachments(scope)
        .await
        .map_err(to_ipc("list_pinned_attachments"))
}

/// Stage 16 (decision 0036): how many messages of blocked senders the inbox lists of the scope
/// leave out, for the list's note.
#[tauri::command]
#[specta::specta]
pub async fn count_blocked_hidden(
    core: State<'_, AppCore>,
    scope: InboxScope,
) -> Result<u32, IpcError> {
    core.count_blocked_hidden(scope)
        .await
        .map_err(to_ipc("count_blocked_hidden"))
}

/// "Ungelesene oben": unread messages of the scope first, then the read ones. `limit` 1..500.
#[tauri::command]
#[specta::specta]
pub async fn list_unread_first(
    core: State<'_, AppCore>,
    scope: InboxScope,
    cursor: Option<UnreadFirstCursor>,
    limit: u32,
) -> Result<UnreadFirstPage, IpcError> {
    core.list_unread_first(scope, cursor, limit)
        .await
        .map_err(to_ipc("list_unread_first"))
}

/// The list views (keys the UI chooses) that group unread messages on top. Device setting.
#[tauri::command]
#[specta::specta]
pub async fn get_unread_first_views(core: State<'_, AppCore>) -> Result<Vec<String>, IpcError> {
    core.get_unread_first_views()
        .await
        .map_err(to_ipc("get_unread_first_views"))
}

/// Groups unread messages on top in `view` or not; returns all such views.
#[tauri::command]
#[specta::specta]
pub async fn set_unread_first_view(
    core: State<'_, AppCore>,
    view: String,
    enabled: bool,
) -> Result<Vec<String>, IpcError> {
    core.set_unread_first_view(view, enabled)
        .await
        .map_err(to_ipc("set_unread_first_view"))
}

/// Makes a folder a favourite (at the end) or not; emits `mailboxesChanged`.
#[tauri::command]
#[specta::specta]
pub async fn set_mailbox_favorite(
    core: State<'_, AppCore>,
    mailbox_id: MailboxId,
    favorite: bool,
) -> Result<(), IpcError> {
    core.set_mailbox_favorite(mailbox_id, favorite)
        .await
        .map_err(to_ipc("set_mailbox_favorite"))
}

/// Gives a folder a colour of the account palette (0–7) or none (null); emits
/// `mailboxesChanged`.
#[tauri::command]
#[specta::specta]
pub async fn set_mailbox_color(
    core: State<'_, AppCore>,
    mailbox_id: MailboxId,
    color: Option<AccountColor>,
) -> Result<(), IpcError> {
    core.set_mailbox_color(mailbox_id, color)
        .await
        .map_err(to_ipc("set_mailbox_color"))
}

/// Puts the favourites in this order (the others after them); returns them in order.
#[tauri::command]
#[specta::specta]
pub async fn reorder_favorite_mailboxes(
    core: State<'_, AppCore>,
    mailbox_ids: Vec<MailboxId>,
) -> Result<Vec<Mailbox>, IpcError> {
    core.reorder_favorite_mailboxes(mailbox_ids)
        .await
        .map_err(to_ipc("reorder_favorite_mailboxes"))
}

/// The favourite folders of every account, in their order.
#[tauri::command]
#[specta::specta]
pub async fn list_favorite_mailboxes(core: State<'_, AppCore>) -> Result<Vec<Mailbox>, IpcError> {
    core.list_favorite_mailboxes()
        .await
        .map_err(to_ipc("list_favorite_mailboxes"))
}

/// Blocks a sender (new mail goes to junk, or is hidden on a read-only account); returns all
/// blocked senders.
#[tauri::command]
#[specta::specta]
pub async fn block_sender(
    core: State<'_, AppCore>,
    address: String,
) -> Result<Vec<BlockedSender>, IpcError> {
    core.block_sender(address)
        .await
        .map_err(to_ipc("block_sender"))
}

/// Unblocks a sender; returns all blocked senders.
#[tauri::command]
#[specta::specta]
pub async fn unblock_sender(
    core: State<'_, AppCore>,
    address: String,
) -> Result<Vec<BlockedSender>, IpcError> {
    core.unblock_sender(address)
        .await
        .map_err(to_ipc("unblock_sender"))
}

#[tauri::command]
#[specta::specta]
pub async fn list_blocked_senders(
    core: State<'_, AppCore>,
) -> Result<Vec<BlockedSender>, IpcError> {
    core.list_blocked_senders()
        .await
        .map_err(to_ipc("list_blocked_senders"))
}

/// Copies or moves messages to a folder of another account through the outbox, or (stage 21)
/// copies them to another folder of their own account. A move removes the source only after the
/// copy was confirmed in the target. `requestId` is a UUID per user action.
#[tauri::command]
#[specta::specta]
pub async fn transfer_messages(
    core: State<'_, AppCore>,
    messages: Vec<MessageRef>,
    target_mailbox_id: MailboxId,
    mode: TransferMode,
    request_id: String,
) -> Result<ActionResult, IpcError> {
    core.transfer_messages(messages, target_mailbox_id, mode, request_id)
        .await
        .map_err(to_ipc("transfer_messages"))
}

/// Stage 22 (decision 0081): messages dragged from any list and dropped on a folder: those of
/// the folder's account move there, those of other accounts are transferred, those already
/// there stay; `copy` copies all of them. `requestId` is a UUID per drop.
#[tauri::command]
#[specta::specta]
pub async fn drop_messages(
    core: State<'_, AppCore>,
    messages: Vec<MessageRef>,
    target_mailbox_id: MailboxId,
    mode: TransferMode,
    request_id: String,
) -> Result<DropResult, IpcError> {
    core.drop_messages(messages, target_mailbox_id, mode, request_id)
        .await
        .map_err(to_ipc("drop_messages"))
}
