//! Threads and the unified inbox (stage 7), and the sorted inbox as conversations with the
//! messages an action on conversation rows touches (decision 0039). Each one forwards to
//! [`AppCore`].

use mail_engine::AppCore;
use mail_model::{
    ConversationIntent, InboxCursor, InboxFilter, InboxPage, InboxScope, IpcError, ListCursor,
    MailboxId, MessageId, MessageListItem, MessagePage, MessageRef, ThreadPage, ThreadView,
    UnifiedView, UnreadFirstCursor, UnreadFirstThreadPage,
};
use tauri::State;

use super::to_ipc;

/// Threads of a mailbox, newest first. `limit` 1..500.
#[tauri::command]
#[specta::specta]
pub async fn list_threads(
    core: State<'_, AppCore>,
    mailbox_id: MailboxId,
    cursor: Option<ListCursor>,
    limit: u32,
) -> Result<ThreadPage, IpcError> {
    core.list_threads(mailbox_id, cursor, limit)
        .await
        .map_err(to_ipc("list_threads"))
}

/// Threads of the unified inbox or of a group's inboxes. `limit` 1..500.
#[tauri::command]
#[specta::specta]
pub async fn list_unified_threads(
    core: State<'_, AppCore>,
    view: UnifiedView,
    cursor: Option<ListCursor>,
    limit: u32,
) -> Result<ThreadPage, IpcError> {
    core.list_unified_threads(view, cursor, limit)
        .await
        .map_err(to_ipc("list_unified_threads"))
}

/// The thread of a message over all folders of its account, oldest first. `mailboxId` is the
/// folder it was opened from.
#[tauri::command]
#[specta::specta]
pub async fn get_thread(
    core: State<'_, AppCore>,
    message_id: MessageId,
    mailbox_id: Option<MailboxId>,
) -> Result<ThreadView, IpcError> {
    core.get_thread(message_id, mailbox_id)
        .await
        .map_err(to_ipc("get_thread"))
}

/// The unified inbox (or a group's), newest first. `limit` 1..500.
#[tauri::command]
#[specta::specta]
pub async fn list_unified_inbox(
    core: State<'_, AppCore>,
    view: UnifiedView,
    cursor: Option<ListCursor>,
    limit: u32,
) -> Result<MessagePage, IpcError> {
    core.list_unified_inbox(view, cursor, limit)
        .await
        .map_err(to_ipc("list_unified_inbox"))
}

/// Stage 23 (decision 0087): "Sent" over all visible accounts (or a group's), newest first.
/// `limit` 1..500.
#[tauri::command]
#[specta::specta]
pub async fn list_unified_sent(
    core: State<'_, AppCore>,
    view: UnifiedView,
    cursor: Option<ListCursor>,
    limit: u32,
) -> Result<MessagePage, IpcError> {
    core.list_unified_sent(view, cursor, limit)
        .await
        .map_err(to_ipc("list_unified_sent"))
}

/// Stage 23 (decision 0087): the threads of the Sent folders of all visible accounts (or a
/// group's). `limit` 1..500.
#[tauri::command]
#[specta::specta]
pub async fn list_unified_sent_threads(
    core: State<'_, AppCore>,
    view: UnifiedView,
    cursor: Option<ListCursor>,
    limit: u32,
) -> Result<ThreadPage, IpcError> {
    core.list_unified_sent_threads(view, cursor, limit)
        .await
        .map_err(to_ipc("list_unified_sent_threads"))
}

/// Decision 0039: the sorted inbox as conversations (one row per conversation at its newest
/// message, summaries in `conversations`). `limit` 1..500 counts conversations.
#[tauri::command]
#[specta::specta]
pub async fn list_inbox_conversations(
    core: State<'_, AppCore>,
    scope: InboxScope,
    cursor: Option<InboxCursor>,
    limit: u32,
) -> Result<InboxPage, IpcError> {
    core.list_inbox_conversations(scope, cursor, limit)
        .await
        .map_err(to_ipc("list_inbox_conversations"))
}

/// Decision 0039: the inbox by category, the important senders or unread only, as
/// conversations. `limit` 1..500.
#[tauri::command]
#[specta::specta]
pub async fn list_inbox_conversations_filtered(
    core: State<'_, AppCore>,
    scope: InboxScope,
    filter: InboxFilter,
    cursor: Option<ListCursor>,
    limit: u32,
) -> Result<ThreadPage, IpcError> {
    core.list_inbox_conversations_filtered(scope, filter, cursor, limit)
        .await
        .map_err(to_ipc("list_inbox_conversations_filtered"))
}

/// Decision 0039: "unread first" as conversations. `limit` 1..500.
#[tauri::command]
#[specta::specta]
pub async fn list_unread_first_conversations(
    core: State<'_, AppCore>,
    scope: InboxScope,
    cursor: Option<UnreadFirstCursor>,
    limit: u32,
) -> Result<UnreadFirstThreadPage, IpcError> {
    core.list_unread_first_conversations(scope, cursor, limit)
        .await
        .map_err(to_ipc("list_unread_first_conversations"))
}

/// Decision 0039: which messages an action on conversation rows touches (`rows`: each row's
/// message in the mailbox it is listed in), as list rows to run the action on.
#[tauri::command]
#[specta::specta]
pub async fn conversation_targets(
    core: State<'_, AppCore>,
    rows: Vec<MessageRef>,
    intent: ConversationIntent,
) -> Result<Vec<MessageListItem>, IpcError> {
    core.conversation_targets(rows, intent)
        .await
        .map_err(to_ipc("conversation_targets"))
}
