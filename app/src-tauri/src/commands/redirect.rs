//! Stage 19 (decision 0055): redirecting a message with `Resent-*` fields through the outbox.

use mail_engine::AppCore;
use mail_model::{IpcError, OperationId, OutgoingMail, RedirectRequest};
use tauri::State;

use super::to_ipc;

/// Redirects the message unchanged to new recipients; queued after the undo delay like a send
/// (`requestId` names the press: the same id twice queues once). `read_only`,
/// `invalid_address`, `invalid_argument`, `not_found` before anything is queued.
#[tauri::command]
#[specta::specta]
pub async fn redirect_message(
    core: State<'_, AppCore>,
    request: RedirectRequest,
    request_id: String,
) -> Result<OutgoingMail, IpcError> {
    core.redirect_message(request, request_id)
        .await
        .map_err(to_ipc("redirect_message"))
}

/// "Undo" of a redirect within the delay, or dropping a failed one: nothing is sent.
/// `send_in_progress` once the submission started.
#[tauri::command]
#[specta::specta]
pub async fn cancel_redirect(
    core: State<'_, AppCore>,
    operation_id: OperationId,
) -> Result<(), IpcError> {
    core.cancel_redirect(operation_id)
        .await
        .map_err(to_ipc("cancel_redirect"))
}
