//! Stage 19 (decision 0056): contact groups on the contacts page and in the composer.

use std::path::PathBuf;

use mail_engine::AppCore;
use mail_model::{
    Address, ContactExport, ContactList, ContactListId, ContactListSummary, CoreError, IpcError,
    VcardExportOutcome, VcardImportOutcome, VcardVersionChoice,
};
use tauri::{Runtime, State, WebviewWindow};

use super::to_ipc;

/// Every contact group, by name, with the number of members.
#[tauri::command]
#[specta::specta]
pub async fn list_contact_lists(
    core: State<'_, AppCore>,
) -> Result<Vec<ContactListSummary>, IpcError> {
    core.list_contact_lists()
        .await
        .map_err(to_ipc("list_contact_lists"))
}

/// One group with its members in the order added (the composer expands a group into these).
#[tauri::command]
#[specta::specta]
pub async fn get_contact_list(
    core: State<'_, AppCore>,
    id: ContactListId,
) -> Result<ContactList, IpcError> {
    core.get_contact_list(id)
        .await
        .map_err(to_ipc("get_contact_list"))
}

/// A new group, optionally with members. `invalid_argument` for an unusable name or beyond the
/// limits, `invalid_address` for an address that cannot be one.
#[tauri::command]
#[specta::specta]
pub async fn create_contact_list(
    core: State<'_, AppCore>,
    name: String,
    members: Vec<Address>,
) -> Result<ContactList, IpcError> {
    core.create_contact_list(name, members)
        .await
        .map_err(to_ipc("create_contact_list"))
}

#[tauri::command]
#[specta::specta]
pub async fn rename_contact_list(
    core: State<'_, AppCore>,
    id: ContactListId,
    name: String,
) -> Result<ContactList, IpcError> {
    core.rename_contact_list(id, name)
        .await
        .map_err(to_ipc("rename_contact_list"))
}

/// Removes the group; its members' contacts stay.
#[tauri::command]
#[specta::specta]
pub async fn delete_contact_list(
    core: State<'_, AppCore>,
    id: ContactListId,
) -> Result<(), IpcError> {
    core.delete_contact_list(id)
        .await
        .map_err(to_ipc("delete_contact_list"))
}

/// Adds members at the end; addresses in the group already stay as they are.
#[tauri::command]
#[specta::specta]
pub async fn add_contact_list_members(
    core: State<'_, AppCore>,
    id: ContactListId,
    members: Vec<Address>,
) -> Result<ContactList, IpcError> {
    core.add_contact_list_members(id, members)
        .await
        .map_err(to_ipc("add_contact_list_members"))
}

/// Removes the member with this address.
#[tauri::command]
#[specta::specta]
pub async fn remove_contact_list_member(
    core: State<'_, AppCore>,
    id: ContactListId,
    email: String,
) -> Result<ContactList, IpcError> {
    core.remove_contact_list_member(id, email)
        .await
        .map_err(to_ipc("remove_contact_list_member"))
}

/// Groups for the composer's autocomplete (every word of `query` starts a word of the name).
#[tauri::command]
#[specta::specta]
pub async fn suggest_contact_lists(
    core: State<'_, AppCore>,
    query: String,
    limit: u32,
) -> Result<Vec<ContactListSummary>, IpcError> {
    core.suggest_contact_lists(query, limit)
        .await
        .map_err(to_ipc("suggest_contact_lists"))
}

/// Stage 19: "Import vCard…": the system's open dialog for a `.vcf` file, then the core reads
/// it. `cancelled` when the user closes the dialog.
#[tauri::command]
#[specta::specta]
pub async fn import_vcards<R: Runtime>(
    window: WebviewWindow<R>,
    core: State<'_, AppCore>,
) -> Result<VcardImportOutcome, IpcError> {
    let Some(path) = ask_vcard_path(&window)
        .await
        .map_err(to_ipc("import_vcards"))?
    else {
        return Ok(VcardImportOutcome::Cancelled);
    };
    let report = core
        .import_vcard_file(path)
        .await
        .map_err(to_ipc("import_vcards"))?;
    Ok(VcardImportOutcome::Imported { report })
}

/// Stage 19: "Export as vCard…": the system's save dialog, then the core writes the file.
#[tauri::command]
#[specta::specta]
pub async fn export_vcards<R: Runtime>(
    window: WebviewWindow<R>,
    core: State<'_, AppCore>,
    selection: ContactExport,
    version: VcardVersionChoice,
) -> Result<VcardExportOutcome, IpcError> {
    let name = core
        .vcard_export_name(&selection)
        .await
        .map_err(to_ipc("export_vcards"))?;
    let Some(path) = super::reading::ask_save_path(&window, &name)
        .await
        .map_err(to_ipc("export_vcards"))?
    else {
        return Ok(VcardExportOutcome::Cancelled);
    };
    let cards = core
        .export_vcards(selection, version, path.clone())
        .await
        .map_err(to_ipc("export_vcards"))?;
    Ok(VcardExportOutcome::Saved {
        path: path.display().to_string(),
        cards,
    })
}

/// The system's open dialog for one vCard file, as a sheet of the window. Only the path comes
/// back; the core reads.
#[cfg(any(target_os = "macos", windows))]
async fn ask_vcard_path<R: Runtime>(
    window: &WebviewWindow<R>,
) -> Result<Option<PathBuf>, CoreError> {
    #[cfg(feature = "e2e")]
    if let Some(path) = crate::e2e::take_dialog_answer() {
        return Ok(Some(path));
    }
    Ok(rfd::AsyncFileDialog::new()
        .add_filter("vCard", &["vcf", "vcard"])
        .set_parent(window)
        .pick_file()
        .await
        .map(|file| file.path().to_path_buf()))
}

#[cfg(not(any(target_os = "macos", windows)))]
async fn ask_vcard_path<R: Runtime>(
    _window: &WebviewWindow<R>,
) -> Result<Option<PathBuf>, CoreError> {
    Err(CoreError::Internal(
        "opening files needs the system dialog of macOS or Windows".into(),
    ))
}
