//! Stage 20 (decision 0058): the attachment reminder before sending, and reminders when no
//! reply comes. Each one forwards to [`AppCore`].

use mail_engine::AppCore;
use mail_model::{
    DraftId, FollowUpChoice, FollowUpResult, FollowUpTimes, IpcError, MarkScope, MessageId,
    MessageListItem,
};
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

/// Decision 0059: the times the "Remind me if no reply" menu offers now.
#[tauri::command]
#[specta::specta]
pub fn get_follow_up_times(core: State<'_, AppCore>) -> FollowUpTimes {
    core.follow_up_times()
}

/// Sets or moves "remind me if no reply" on messages; answers the time and how many already
/// have a reply.
#[tauri::command]
#[specta::specta]
pub async fn set_follow_up(
    core: State<'_, AppCore>,
    message_ids: Vec<MessageId>,
    choice: FollowUpChoice,
) -> Result<FollowUpResult, IpcError> {
    core.set_follow_up(message_ids, choice)
        .await
        .map_err(to_ipc("set_follow_up"))
}

/// Removes the reminders of messages.
#[tauri::command]
#[specta::specta]
pub async fn clear_follow_up(
    core: State<'_, AppCore>,
    message_ids: Vec<MessageId>,
) -> Result<(), IpcError> {
    core.clear_follow_up(message_ids)
        .await
        .map_err(to_ipc("clear_follow_up"))
}

/// "Waiting for reply": the reminders of the scope, the next due first.
#[tauri::command]
#[specta::specta]
pub async fn list_follow_ups(
    core: State<'_, AppCore>,
    scope: MarkScope,
) -> Result<Vec<MessageListItem>, IpcError> {
    core.list_follow_ups(scope)
        .await
        .map_err(to_ipc("list_follow_ups"))
}
