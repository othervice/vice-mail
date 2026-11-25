//! Stage 19 (decision 0057): a folder exported as an mbox file, an mbox file imported into a
//! folder through the outbox; both run in the background (`mboxJobChanged`).

use std::path::PathBuf;

use mail_engine::AppCore;
use mail_model::{CoreError, IpcError, MailboxId, MboxJob, MboxJobId, MboxStart};
use tauri::{Runtime, State, WebviewWindow};

use super::to_ipc;

/// "Export as mbox…": the system's save dialog, then the export runs in the background.
#[tauri::command]
#[specta::specta]
pub async fn export_mailbox_mbox<R: Runtime>(
    window: WebviewWindow<R>,
    core: State<'_, AppCore>,
    mailbox_id: MailboxId,
) -> Result<MboxStart, IpcError> {
    let name = core
        .mbox_export_name(mailbox_id)
        .await
        .map_err(to_ipc("export_mailbox_mbox"))?;
    let Some(path) = super::reading::ask_save_path(&window, &name)
        .await
        .map_err(to_ipc("export_mailbox_mbox"))?
    else {
        return Ok(MboxStart::Cancelled);
    };
    let job = core
        .start_mbox_export(mailbox_id, path)
        .await
        .map_err(to_ipc("export_mailbox_mbox"))?;
    Ok(MboxStart::Started { job })
}

/// "Import mbox…": the system's open dialog, then the import runs in the background through
/// the outbox. `read_only` without writes on the folder's account (before the dialog).
#[tauri::command]
#[specta::specta]
pub async fn import_mailbox_mbox<R: Runtime>(
    window: WebviewWindow<R>,
    core: State<'_, AppCore>,
    mailbox_id: MailboxId,
) -> Result<MboxStart, IpcError> {
    core.check_mbox_import(mailbox_id)
        .await
        .map_err(to_ipc("import_mailbox_mbox"))?;
    let Some(path) = ask_mbox_path(&window)
        .await
        .map_err(to_ipc("import_mailbox_mbox"))?
    else {
        return Ok(MboxStart::Cancelled);
    };
    let job = core
        .start_mbox_import(mailbox_id, path)
        .await
        .map_err(to_ipc("import_mailbox_mbox"))?;
    Ok(MboxStart::Started { job })
}

/// Stops a running export or import; the job reports `cancelled` once it stopped.
#[tauri::command]
#[specta::specta]
pub fn cancel_mbox_job(core: State<'_, AppCore>, id: MboxJobId) -> Result<MboxJob, IpcError> {
    let id = MboxJobId::parse(id.as_str()).ok_or_else(|| {
        to_ipc("cancel_mbox_job")(CoreError::InvalidArgument("not a job id".into()))
    })?;
    core.cancel_mbox_job(id).map_err(to_ipc("cancel_mbox_job"))
}

/// The jobs of this session, running and finished.
#[tauri::command]
#[specta::specta]
pub fn list_mbox_jobs(core: State<'_, AppCore>) -> Vec<MboxJob> {
    core.list_mbox_jobs()
}

/// The system's open dialog for an mbox file (any name: Thunderbird's folders have none).
#[cfg(any(target_os = "macos", windows))]
async fn ask_mbox_path<R: Runtime>(
    window: &WebviewWindow<R>,
) -> Result<Option<PathBuf>, CoreError> {
    #[cfg(feature = "e2e")]
    if let Some(path) = crate::e2e::take_dialog_answer() {
        return Ok(Some(path));
    }
    Ok(rfd::AsyncFileDialog::new()
        .set_parent(window)
        .pick_file()
        .await
        .map(|file| file.path().to_path_buf()))
}

#[cfg(not(any(target_os = "macos", windows)))]
async fn ask_mbox_path<R: Runtime>(
    _window: &WebviewWindow<R>,
) -> Result<Option<PathBuf>, CoreError> {
    Err(CoreError::Internal(
        "opening files needs the system dialog of macOS or Windows".into(),
    ))
}
