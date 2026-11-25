//! Stage 9 (decision 0027): `.eml` files, shown read-only in a window of their own. The open
//! and save dialogs are the only things done here, because they need the window; reading,
//! checking and rendering are the core's.

use std::path::PathBuf;

use mail_engine::AppCore;
use mail_model::{
    BodyFormat, CoreError, EmlFileId, EmlMessageView, IpcError, MailScheme, OpenAttachmentResult,
    OpenEmlFileResult, SaveAttachmentResult,
};
use tauri::{AppHandle, Runtime, State, WebviewWindow};

use super::reading::ask_save_path;
use super::to_ipc;
use crate::windows;

/// "Open .eml file…": the system's open dialog (only `.eml`), then the core reads and checks the
/// file and it opens in a window of its own. `cancelled` when the user closes the dialog;
/// `eml_too_large`, `eml_invalid`, `eml_unreadable` when the file cannot be shown (no window
/// opens then).
#[tauri::command]
#[specta::specta]
pub async fn open_eml_file<R: Runtime>(
    app: AppHandle<R>,
    window: WebviewWindow<R>,
    core: State<'_, AppCore>,
) -> Result<OpenEmlFileResult, IpcError> {
    let Some(path) = ask_eml_path(&window)
        .await
        .map_err(to_ipc("open_eml_file"))?
    else {
        return Ok(OpenEmlFileResult::Cancelled);
    };
    let file = core
        .open_eml_file(path)
        .await
        .map_err(to_ipc("open_eml_file"))?;
    let theme = super::ui::theme(&core).await;
    windows::open_eml_window(&app, &file, theme).map_err(|err| {
        core.close_eml_file(&file.id);
        to_ipc("open_eml_file")(CoreError::Internal(format!("open window: {err}")))
    })?;
    Ok(OpenEmlFileResult::Opened { file })
}

/// The opened file as the reading pane shows it (sanitised HTML in the sandboxed frame, or
/// text). The typed error the file was refused with, if the system handed over a file that is
/// no mail; `not_found` once the file was closed.
#[tauri::command]
#[specta::specta]
pub async fn get_eml_message(
    core: State<'_, AppCore>,
    file_id: EmlFileId,
    format: BodyFormat,
    scheme: Option<MailScheme>,
) -> Result<EmlMessageView, IpcError> {
    core.get_eml_message_for(checked(file_id)?, format, scheme.unwrap_or_default())
        .await
        .map_err(to_ipc("get_eml_message"))
}

/// Renders the file again with its external images, loaded by the core, for this rendering
/// only (there is no sender list without an account).
#[tauri::command]
#[specta::specta]
pub async fn load_eml_remote_content(
    core: State<'_, AppCore>,
    file_id: EmlFileId,
    scheme: Option<MailScheme>,
) -> Result<EmlMessageView, IpcError> {
    core.load_eml_remote_content_for(checked(file_id)?, scheme.unwrap_or_default())
        .await
        .map_err(to_ipc("load_eml_remote_content"))
}

/// Saves an attachment of the file where the user picks in the system dialog.
#[tauri::command]
#[specta::specta]
pub async fn save_eml_attachment<R: Runtime>(
    window: WebviewWindow<R>,
    core: State<'_, AppCore>,
    file_id: EmlFileId,
    index: u32,
) -> Result<SaveAttachmentResult, IpcError> {
    let id = checked(file_id)?;
    let name = core
        .eml_attachment_name(id.clone(), index)
        .await
        .map_err(to_ipc("save_eml_attachment"))?;
    let Some(path) = ask_save_path(&window, &name)
        .await
        .map_err(to_ipc("save_eml_attachment"))?
    else {
        return Ok(SaveAttachmentResult::Cancelled);
    };
    let saved = core
        .save_eml_attachment_to(id, index, path)
        .await
        .map_err(to_ipc("save_eml_attachment"))?;
    Ok(SaveAttachmentResult::Saved {
        path: saved.display().to_string(),
    })
}

/// Stage 16 (decision 0036): the preview of an attachment of the file (page `page` of a PDF).
#[tauri::command]
#[specta::specta]
pub async fn preview_eml_attachment(
    core: State<'_, AppCore>,
    file_id: EmlFileId,
    index: u32,
    page: u32,
) -> Result<mail_model::AttachmentPreview, IpcError> {
    core.preview_eml_attachment(checked(file_id)?, index, page)
        .await
        .map_err(to_ipc("preview_eml_attachment"))
}

/// Stage 16 (decision 0036): every attachment of the file into a folder the user picks.
#[tauri::command]
#[specta::specta]
pub async fn save_all_eml_attachments<R: Runtime>(
    window: WebviewWindow<R>,
    core: State<'_, AppCore>,
    file_id: EmlFileId,
) -> Result<mail_model::SaveAllAttachmentsResult, IpcError> {
    let id = checked(file_id)?;
    let Some(folder) = super::reading::ask_folder(&window)
        .await
        .map_err(to_ipc("save_all_eml_attachments"))?
    else {
        return Ok(mail_model::SaveAllAttachmentsResult::Cancelled);
    };
    let (saved, renamed) = core
        .save_all_eml_attachments_to(id, folder.clone())
        .await
        .map_err(to_ipc("save_all_eml_attachments"))?;
    Ok(mail_model::SaveAllAttachmentsResult::Saved {
        folder: folder.display().to_string(),
        saved,
        renamed,
    })
}

/// Opens a read-only copy of an attachment of the file; executables need `confirmedExecutable`.
#[tauri::command]
#[specta::specta]
pub async fn open_eml_attachment(
    core: State<'_, AppCore>,
    file_id: EmlFileId,
    index: u32,
    confirmed_executable: bool,
) -> Result<OpenAttachmentResult, IpcError> {
    core.open_eml_attachment(checked(file_id)?, index, confirmed_executable)
        .await
        .map_err(to_ipc("open_eml_attachment"))
}

/// The id as the core made it (32 lowercase hex digits); anything else is `invalid_argument`.
fn checked(id: EmlFileId) -> Result<EmlFileId, IpcError> {
    EmlFileId::parse(id.as_str()).ok_or_else(|| {
        to_ipc("eml")(CoreError::InvalidArgument(
            "the file id is not one the core made".into(),
        ))
    })
}

/// The system's open dialog for one `.eml` file, as a sheet of the window. `None` when the user
/// cancels. Only the path comes back; the core reads.
#[cfg(any(target_os = "macos", windows))]
async fn ask_eml_path<R: Runtime>(window: &WebviewWindow<R>) -> Result<Option<PathBuf>, CoreError> {
    Ok(rfd::AsyncFileDialog::new()
        .add_filter("Email message", &["eml"])
        .set_parent(window)
        .pick_file()
        .await
        .map(|file| file.path().to_path_buf()))
}

#[cfg(not(any(target_os = "macos", windows)))]
async fn ask_eml_path<R: Runtime>(
    _window: &WebviewWindow<R>,
) -> Result<Option<PathBuf>, CoreError> {
    Err(CoreError::Internal(
        "opening files needs the system dialog of macOS or Windows".into(),
    ))
}
