//! Stage 20, task C (decision 0060): the "Attachments" view across accounts. Opening, saving and
//! previewing use the reading pane's commands with the listed index.

use mail_engine::AppCore;
use mail_model::{AttachmentCursor, AttachmentFilter, AttachmentPage, IpcError};
use tauri::State;

use super::to_ipc;

/// One page of the attachments of the scope's accounts, newest message first, filtered by kind,
/// sender and file name. `limit` 1..=500.
#[tauri::command]
#[specta::specta]
pub async fn list_attachments(
    core: State<'_, AppCore>,
    filter: AttachmentFilter,
    cursor: Option<AttachmentCursor>,
    limit: u32,
) -> Result<AttachmentPage, IpcError> {
    core.list_attachments(filter, cursor, limit)
        .await
        .map_err(to_ipc("list_attachments"))
}
