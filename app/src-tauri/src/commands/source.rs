//! Stage 19 (decision 0055): a message's source in a window of its own, and its header fields
//! for "all headers" in the reading pane.

use mail_engine::AppCore;
use mail_model::{AccountId, CoreError, HeaderField, IpcError, MessageId, MessageSource};
use tauri::{AppHandle, Runtime, State};

use super::to_ipc;
use super::ui::theme;
use crate::windows;

/// Shows the source of one message in a window of its own (`?window=source&accountId=…&
/// messageId=…`, capability `source-window`: it may close itself, nothing else). A second call
/// for the same message focuses its window. `not_found` if the message does not exist or
/// belongs to another account.
#[tauri::command]
#[specta::specta]
pub async fn open_source_window<R: Runtime>(
    app: AppHandle<R>,
    core: State<'_, AppCore>,
    account_id: AccountId,
    message_id: MessageId,
) -> Result<(), IpcError> {
    if windows::focus_window(&app, &windows::source_window_label(account_id, message_id)) {
        return Ok(());
    }
    let subject = core
        .source_window_subject(account_id, message_id)
        .await
        .map_err(to_ipc("open_source_window"))?;
    windows::open_source_window(&app, account_id, message_id, &subject, theme(&core).await)
        .map(drop)
        .map_err(|err| {
            to_ipc("open_source_window")(CoreError::Internal(format!("open window: {err}")))
        })
}

/// The message as plain text (at most `MAX_SOURCE_BYTES`, cut at a line break; `truncated`
/// says so and the window offers "Save as .eml" for the whole).
#[tauri::command]
#[specta::specta]
pub async fn get_message_source(
    core: State<'_, AppCore>,
    message_id: MessageId,
) -> Result<MessageSource, IpcError> {
    core.get_message_source(message_id)
        .await
        .map_err(to_ipc("get_message_source"))
}

/// Every header field of the message, in order, unfolded and decoded ("all headers").
#[tauri::command]
#[specta::specta]
pub async fn get_message_headers(
    core: State<'_, AppCore>,
    message_id: MessageId,
) -> Result<Vec<HeaderField>, IpcError> {
    core.get_message_headers(message_id)
        .await
        .map_err(to_ipc("get_message_headers"))
}
