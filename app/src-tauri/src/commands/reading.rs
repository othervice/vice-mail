//! Reading pane commands (stage 3): messages, external content, links, attachments, prefetch.
//! Each forwards to [`AppCore`]; the save dialog is the only thing done here, because it needs
//! the window.

use std::path::PathBuf;

use mail_engine::AppCore;
use mail_model::{
    AccountId, AttachmentPreview, BodyFormat, CoreError, IpcError, LinkInfo, MessageId,
    MessageView, OpenAttachmentResult, PrefetchSettings, PrefetchStatus, RemoteContentScope,
    RenderToken, SaveAllAttachmentsResult, SaveAttachmentResult,
};
use tauri::{Manager, Runtime, State, WebviewWindow};

use super::to_ipc;

/// The reading pane's data. Served locally when the body is stored, else `loading` and a fetch starts.
#[tauri::command]
#[specta::specta]
pub async fn get_message(
    core: State<'_, AppCore>,
    message_id: MessageId,
    format: BodyFormat,
) -> Result<MessageView, IpcError> {
    core.get_message(message_id, format)
        .await
        .map_err(to_ipc("get_message"))
}

/// Renders again with the external images loaded by the core; `sender` also allows them for the sender.
#[tauri::command]
#[specta::specta]
pub async fn load_remote_content(
    core: State<'_, AppCore>,
    message_id: MessageId,
    scope: RemoteContentScope,
) -> Result<MessageView, IpcError> {
    core.load_remote_content(message_id, scope)
        .await
        .map_err(to_ipc("load_remote_content"))
}

/// Senders whose external content is always loaded, for the settings.
#[tauri::command]
#[specta::specta]
pub async fn list_remote_content_senders(
    core: State<'_, AppCore>,
    account_id: AccountId,
) -> Result<Vec<String>, IpcError> {
    core.list_remote_content_senders(account_id)
        .await
        .map_err(to_ipc("list_remote_content_senders"))
}

/// Removes a sender from the remote-content allowlist.
#[tauri::command]
#[specta::specta]
pub async fn revoke_remote_content(
    core: State<'_, AppCore>,
    account_id: AccountId,
    sender: String,
) -> Result<(), IpcError> {
    core.revoke_remote_content(account_id, &sender)
        .await
        .map_err(to_ipc("revoke_remote_content"))
}

/// The real target of a link of one rendering, with warnings. `not_found` for an unknown token.
#[tauri::command]
#[specta::specta]
pub fn describe_link(
    core: State<'_, AppCore>,
    render_token: RenderToken,
    index: u32,
) -> Result<LinkInfo, IpcError> {
    core.describe_link(&render_token, index)
        .map_err(to_ipc("describe_link"))
}

/// Opens a link in the system browser (http, https); a `mailto:` link returns
/// `{kind: "compose", request}` for the app's own composer instead (stage 10).
#[tauri::command]
#[specta::specta]
pub async fn open_link(
    core: State<'_, AppCore>,
    render_token: RenderToken,
    index: u32,
) -> Result<mail_model::LinkOpened, IpcError> {
    core.open_link(&render_token, index)
        .await
        .map_err(to_ipc("open_link"))
}

/// Saves an attachment where the user picks in the system dialog.
#[tauri::command]
#[specta::specta]
pub async fn save_attachment<R: Runtime>(
    window: WebviewWindow<R>,
    core: State<'_, AppCore>,
    message_id: MessageId,
    index: u32,
) -> Result<SaveAttachmentResult, IpcError> {
    let name = core
        .attachment_name(message_id, index)
        .await
        .map_err(to_ipc("save_attachment"))?;
    let Some(path) = ask_save_path(&window, &name)
        .await
        .map_err(to_ipc("save_attachment"))?
    else {
        return Ok(SaveAttachmentResult::Cancelled);
    };
    let saved = core
        .save_attachment_to(message_id, index, path)
        .await
        .map_err(to_ipc("save_attachment"))?;
    Ok(SaveAttachmentResult::Saved {
        path: saved.display().to_string(),
    })
}

/// Opens a copy of an attachment with the system's default app; executables need `confirmedExecutable`.
#[tauri::command]
#[specta::specta]
pub async fn open_attachment(
    core: State<'_, AppCore>,
    message_id: MessageId,
    index: u32,
    confirmed_executable: bool,
) -> Result<OpenAttachmentResult, IpcError> {
    core.open_attachment(message_id, index, confirmed_executable)
        .await
        .map_err(to_ipc("open_attachment"))
}

/// Stage 16 (decision 0036): the preview of an attachment, page `page` of a PDF (0-based).
/// Images, PDF pages and text come decoded by the core; nothing of the attachment runs.
#[tauri::command]
#[specta::specta]
pub async fn preview_attachment(
    core: State<'_, AppCore>,
    message_id: MessageId,
    index: u32,
    page: u32,
) -> Result<AttachmentPreview, IpcError> {
    core.preview_attachment(message_id, index, page)
        .await
        .map_err(to_ipc("preview_attachment"))
}

/// Stage 16 (decision 0036): every attachment into the folder the user picks in the system
/// dialog; taken names get a number, nothing is replaced.
#[tauri::command]
#[specta::specta]
pub async fn save_all_attachments<R: Runtime>(
    window: WebviewWindow<R>,
    core: State<'_, AppCore>,
    message_id: MessageId,
) -> Result<SaveAllAttachmentsResult, IpcError> {
    let Some(folder) = ask_folder(&window)
        .await
        .map_err(to_ipc("save_all_attachments"))?
    else {
        return Ok(SaveAllAttachmentsResult::Cancelled);
    };
    let (saved, renamed) = core
        .save_all_attachments_to(message_id, folder.clone())
        .await
        .map_err(to_ipc("save_all_attachments"))?;
    Ok(SaveAllAttachmentsResult::Saved {
        folder: folder.display().to_string(),
        saved,
        renamed,
    })
}

/// Prefetch period and size limit.
#[tauri::command]
#[specta::specta]
pub async fn get_prefetch_settings(core: State<'_, AppCore>) -> Result<PrefetchSettings, IpcError> {
    core.get_prefetch_settings()
        .await
        .map_err(to_ipc("get_prefetch_settings"))
}

/// Stores the prefetch settings and starts a new prefetch run.
#[tauri::command]
#[specta::specta]
pub async fn set_prefetch_settings(
    core: State<'_, AppCore>,
    settings: PrefetchSettings,
) -> Result<PrefetchSettings, IpcError> {
    core.set_prefetch_settings(settings)
        .await
        .map_err(to_ipc("set_prefetch_settings"))
}

/// Prefetch state of an account; `prefetchProgress` follows.
#[tauri::command]
#[specta::specta]
pub async fn get_prefetch_status(
    core: State<'_, AppCore>,
    account_id: AccountId,
) -> Result<PrefetchStatus, IpcError> {
    core.get_prefetch_status(account_id)
        .await
        .map_err(to_ipc("get_prefetch_status"))
}

/// The system's save dialog, as a sheet of the window, starting in the Downloads folder with the
/// sanitised name. `None` when the user cancels. Only the path comes back; the core writes.
#[cfg(any(target_os = "macos", windows))]
pub(crate) async fn ask_save_path<R: Runtime>(
    window: &WebviewWindow<R>,
    name: &str,
) -> Result<Option<PathBuf>, CoreError> {
    // Stage 19: the desktop test answers the dialog itself (never in a shipped build).
    #[cfg(feature = "e2e")]
    if let Some(path) = crate::e2e::take_dialog_answer() {
        return Ok(Some(path));
    }
    let mut dialog = rfd::AsyncFileDialog::new()
        .set_file_name(name)
        .set_parent(window);
    if let Ok(downloads) = window.path().download_dir() {
        dialog = dialog.set_directory(downloads);
    }
    Ok(dialog
        .save_file()
        .await
        .map(|file| file.path().to_path_buf()))
}

/// The system's folder dialog for "save all", as a sheet of the window, starting in the
/// Downloads folder. `None` when the user cancels. Only the path comes back; the core writes.
#[cfg(any(target_os = "macos", windows))]
pub(crate) async fn ask_folder<R: Runtime>(
    window: &WebviewWindow<R>,
) -> Result<Option<PathBuf>, CoreError> {
    let mut dialog = rfd::AsyncFileDialog::new().set_parent(window);
    if let Ok(downloads) = window.path().download_dir() {
        dialog = dialog.set_directory(downloads);
    }
    Ok(dialog
        .pick_folder()
        .await
        .map(|folder| folder.path().to_path_buf()))
}

#[cfg(not(any(target_os = "macos", windows)))]
pub(crate) async fn ask_folder<R: Runtime>(
    _window: &WebviewWindow<R>,
) -> Result<Option<PathBuf>, CoreError> {
    Err(CoreError::Internal(
        "saving attachments needs the system dialog of macOS or Windows".into(),
    ))
}

#[cfg(not(any(target_os = "macos", windows)))]
pub(crate) async fn ask_save_path<R: Runtime>(
    _window: &WebviewWindow<R>,
    _name: &str,
) -> Result<Option<PathBuf>, CoreError> {
    Err(CoreError::Internal(
        "saving attachments needs the system dialog of macOS or Windows".into(),
    ))
}
