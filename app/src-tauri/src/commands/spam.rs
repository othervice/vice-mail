//! Stage 22 (decision 0077): the learning spam filter. Each command forwards to
//! [`AppCore`]; "Spam" and "Not spam" on the server stay `applyAction` (they teach the filter
//! there).

use mail_engine::AppCore;
use mail_model::{IpcError, MessageId, SpamFilterSettings, SpamFilterStatus, SpamVerdict};
use tauri::State;

use super::to_ipc;

/// What the spam filter learned on this device, its setting, and learning from existing mail.
#[tauri::command]
#[specta::specta]
pub async fn get_spam_filter(core: State<'_, AppCore>) -> Result<SpamFilterStatus, IpcError> {
    core.get_spam_filter()
        .await
        .map_err(to_ipc("get_spam_filter"))
}

/// Stores the setting (on or off, excluded accounts; synchronised) and returns the status.
#[tauri::command]
#[specta::specta]
pub async fn set_spam_filter(
    core: State<'_, AppCore>,
    settings: SpamFilterSettings,
) -> Result<SpamFilterStatus, IpcError> {
    core.set_spam_filter(settings)
        .await
        .map_err(to_ipc("set_spam_filter"))
}

/// Starts learning from the junk folders and read inbox mail in the background; progress comes
/// as `spamFilterChanged`. `invalid_argument` while the filter is off.
#[tauri::command]
#[specta::specta]
pub async fn learn_spam_from_mail(core: State<'_, AppCore>) -> Result<SpamFilterStatus, IpcError> {
    core.learn_spam_from_mail()
        .await
        .map_err(to_ipc("learn_spam_from_mail"))
}

/// Stops learning from existing mail; what was learned stays.
#[tauri::command]
#[specta::specta]
pub fn cancel_spam_learning(core: State<'_, AppCore>) {
    core.cancel_spam_learning();
}

/// Forgets everything the filter learned and its marks.
#[tauri::command]
#[specta::specta]
pub async fn forget_spam_filter(core: State<'_, AppCore>) -> Result<SpamFilterStatus, IpcError> {
    core.forget_spam_filter()
        .await
        .map_err(to_ipc("forget_spam_filter"))
}

/// The filter's verdict on a message (moved here, or suspected), for the reading pane's note.
#[tauri::command]
#[specta::specta]
pub async fn get_spam_verdict(
    core: State<'_, AppCore>,
    message_id: MessageId,
) -> Result<Option<SpamVerdict>, IpcError> {
    core.get_spam_verdict(message_id)
        .await
        .map_err(to_ipc("get_spam_verdict"))
}

/// "Spam" (`spam`) or "Not spam" on this device only, for mail the filter marked where the
/// account may not write or has no junk folder; teaches the filter. Returns how many marks
/// changed.
#[tauri::command]
#[specta::specta]
pub async fn mark_spam_locally(
    core: State<'_, AppCore>,
    message_ids: Vec<MessageId>,
    spam: bool,
) -> Result<u32, IpcError> {
    core.mark_spam_locally(message_ids, spam)
        .await
        .map_err(to_ipc("mark_spam_locally"))
}
