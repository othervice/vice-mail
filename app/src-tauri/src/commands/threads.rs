//! Threads and the unified inbox (stage 7). Each one forwards to [`AppCore`].

use mail_engine::AppCore;
use mail_model::{
    IpcError, ListCursor, MailboxId, MessageId, MessagePage, ThreadPage, ThreadView, UnifiedView,
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
