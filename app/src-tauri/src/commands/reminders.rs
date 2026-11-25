//! Stage 20 (decision 0058): the attachment reminder before sending. Forwards to
//! [`AppCore`].

use mail_engine::AppCore;
use mail_model::{DraftId, IpcError};
use tauri::State;

use super::to_ipc;

/// Asked by the composer before it sends or schedules: the word of the user's own text that
/// speaks of an attachment while the draft has none; null with the setting off or nothing to
/// remind of. Never asked for sends of the outbox.
#[tauri::command]
#[specta::specta]
pub async fn check_attachment_reminder(
    core: State<'_, AppCore>,
    draft_id: DraftId,
) -> Result<Option<String>, IpcError> {
    core.check_attachment_reminder(draft_id)
        .await
        .map_err(to_ipc("check_attachment_reminder"))
}
