//! Stage 10 (decision 0023): sending later, missed scheduled sends, the server's size limit,
//! inline images, pasted HTML, forwarding as an attachment (through `createDraft`). Each
//! command forwards to [`AppCore`]; the image dialog is done here, because it needs the
//! window. Dropped images come as uploads (stage 23, decision 0082).

#[cfg(any(target_os = "macos", windows))]
use std::path::PathBuf;

use mail_engine::AppCore;
use mail_model::{
    CleanedPaste, CoreError, DraftId, DraftSizeCheck, InlineImagesAdded, IpcError,
    MissedSendDecision, OperationId, OutgoingMail, SendLater, UploadId,
};
use tauri::{Runtime, State, WebviewWindow};

use super::to_ipc;

/// "Send later": queues the draft for `when.sendAt` (Unix ms). The message goes out only if the app
/// runs and is online at that time. `requestId` is a UUID per press of the button.
#[tauri::command]
#[specta::specta]
pub async fn schedule_send(
    core: State<'_, AppCore>,
    draft_id: DraftId,
    request_id: String,
    when: SendLater,
) -> Result<OutgoingMail, IpcError> {
    core.schedule_send(draft_id, request_id, when.send_at)
        .await
        .map_err(to_ipc("schedule_send"))
}

/// Scheduled sends of every account (waiting or missed), the earliest first.
#[tauri::command]
#[specta::specta]
pub async fn list_scheduled_sends(core: State<'_, AppCore>) -> Result<Vec<OutgoingMail>, IpcError> {
    core.list_scheduled_sends()
        .await
        .map_err(to_ipc("list_scheduled_sends"))
}

/// Moves a scheduled send to `when.sendAt`, or (null) sends it now after the undo delay.
#[tauri::command]
#[specta::specta]
pub async fn reschedule_send(
    core: State<'_, AppCore>,
    operation_id: OperationId,
    when: Option<SendLater>,
) -> Result<OutgoingMail, IpcError> {
    core.reschedule_send(operation_id, when.map(|w| w.send_at))
        .await
        .map_err(to_ipc("reschedule_send"))
}

/// The answer to a scheduled send missed while the app was off; null after `keep_as_draft`.
#[tauri::command]
#[specta::specta]
pub async fn resolve_missed_send(
    core: State<'_, AppCore>,
    operation_id: OperationId,
    decision: MissedSendDecision,
) -> Result<Option<OutgoingMail>, IpcError> {
    core.resolve_missed_send(operation_id, decision)
        .await
        .map_err(to_ipc("resolve_missed_send"))
}

/// The draft's size against the SMTP server's limit (SIZE); the first call per account and
/// session asks the server (up to 15 seconds).
#[tauri::command]
#[specta::specta]
pub async fn check_draft_size(
    core: State<'_, AppCore>,
    draft_id: DraftId,
) -> Result<DraftSizeCheck, IpcError> {
    core.check_draft_size(draft_id)
        .await
        .map_err(to_ipc("check_draft_size"))
}

/// Lets the user pick images in the system dialog and adds them as inline images; cancelling
/// adds nothing.
#[tauri::command]
#[specta::specta]
pub async fn pick_inline_images<R: Runtime>(
    window: WebviewWindow<R>,
    core: State<'_, AppCore>,
    draft_id: DraftId,
) -> Result<InlineImagesAdded, IpcError> {
    let paths = ask_image_paths(&window)
        .await
        .map_err(to_ipc("pick_inline_images"))?;
    core.add_inline_images(draft_id, paths)
        .await
        .map_err(to_ipc("pick_inline_images"))
}

/// Stage 23 (decision 0082): images dropped on a window, handed over as uploads, added as
/// inline images with the checks of picked ones; the uploads are used up either way.
#[tauri::command]
#[specta::specta]
pub async fn add_uploaded_inline_images(
    core: State<'_, AppCore>,
    draft_id: DraftId,
    uploads: Vec<UploadId>,
) -> Result<InlineImagesAdded, IpcError> {
    core.add_uploaded_inline_images(draft_id, uploads)
        .await
        .map_err(to_ipc("add_uploaded_inline_images"))
}

/// An inline image of a draft as a `data:` URL, for showing it in the editor.
#[tauri::command]
#[specta::specta]
pub async fn inline_image_data(
    core: State<'_, AppCore>,
    draft_id: DraftId,
    attachment_id: String,
) -> Result<String, IpcError> {
    core.inline_image_data(draft_id, attachment_id)
        .await
        .map_err(to_ipc("inline_image_data"))
}

/// Cleans HTML pasted into the editor (Word, web pages): no script, no external reference.
#[tauri::command]
#[specta::specta]
pub async fn clean_pasted_html(
    core: State<'_, AppCore>,
    html: String,
) -> Result<CleanedPaste, IpcError> {
    core.clean_pasted_html(html)
        .await
        .map_err(to_ipc("clean_pasted_html"))
}

/// The system's open dialog for images (PNG, JPEG, GIF, WebP), as a sheet of the window.
#[cfg(any(target_os = "macos", windows))]
async fn ask_image_paths<R: Runtime>(window: &WebviewWindow<R>) -> Result<Vec<PathBuf>, CoreError> {
    Ok(rfd::AsyncFileDialog::new()
        .set_parent(window)
        .add_filter("Images", &["png", "jpg", "jpeg", "gif", "webp"])
        .pick_files()
        .await
        .map(|files| files.iter().map(|f| f.path().to_path_buf()).collect())
        .unwrap_or_default())
}

#[cfg(not(any(target_os = "macos", windows)))]
async fn ask_image_paths<R: Runtime>(
    _window: &WebviewWindow<R>,
) -> Result<Vec<std::path::PathBuf>, CoreError> {
    Err(CoreError::Internal(
        "adding images needs the system dialog of macOS or Windows".into(),
    ))
}
