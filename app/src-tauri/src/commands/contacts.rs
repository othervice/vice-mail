//! The contact directory (stage 10, decision 0023): ranked pages, names, duplicate
//! suggestions, merges the user confirmed. Each one forwards to [`AppCore`].

use mail_engine::AppCore;
use mail_model::{ContactDuplicate, ContactEntry, ContactMergeRequest, ContactPage, IpcError};
use tauri::State;

use super::to_ipc;

/// One page of the directory, highest ranked first (frequency and recency). `query` filters by
/// word prefixes of names and addresses; `cursor` is the previous page's `next`; `limit` 1..200.
#[tauri::command]
#[specta::specta]
pub async fn list_contacts(
    core: State<'_, AppCore>,
    query: Option<String>,
    cursor: Option<String>,
    limit: u32,
) -> Result<ContactPage, IpcError> {
    core.list_contacts(query, cursor, limit)
        .await
        .map_err(to_ipc("list_contacts"))
}

/// The contact with this address (any of its addresses).
#[tauri::command]
#[specta::specta]
pub async fn get_contact(
    core: State<'_, AppCore>,
    email: String,
) -> Result<ContactEntry, IpcError> {
    core.get_contact(email).await.map_err(to_ipc("get_contact"))
}

/// Sets the name the user gives a contact, or with `null` goes back to the names seen in mail.
#[tauri::command]
#[specta::specta]
pub async fn set_contact_name(
    core: State<'_, AppCore>,
    email: String,
    name: Option<String>,
) -> Result<ContactEntry, IpcError> {
    core.set_contact_name(email, name)
        .await
        .map_err(to_ipc("set_contact_name"))
}

/// Possible duplicates, the surest first. Nothing is merged by this.
#[tauri::command]
#[specta::specta]
pub async fn list_contact_duplicates(
    core: State<'_, AppCore>,
) -> Result<Vec<ContactDuplicate>, IpcError> {
    core.contact_duplicates()
        .await
        .map_err(to_ipc("list_contact_duplicates"))
}

/// Hides a duplicate suggestion for good: the `key`s of its contacts (one or two).
#[tauri::command]
#[specta::specta]
pub async fn dismiss_contact_duplicate(
    core: State<'_, AppCore>,
    contacts: Vec<String>,
) -> Result<(), IpcError> {
    core.dismiss_contact_duplicate(contacts)
        .await
        .map_err(to_ipc("dismiss_contact_duplicate"))
}

/// Merges contacts; only with `confirmed: true` (the user confirmed), else
/// `confirmation_required` and nothing changes.
#[tauri::command]
#[specta::specta]
pub async fn merge_contacts(
    core: State<'_, AppCore>,
    request: ContactMergeRequest,
) -> Result<ContactEntry, IpcError> {
    core.merge_contacts(request)
        .await
        .map_err(to_ipc("merge_contacts"))
}

/// Takes an address out of a merged contact (the preferred address takes it apart entirely).
#[tauri::command]
#[specta::specta]
pub async fn separate_contact(
    core: State<'_, AppCore>,
    email: String,
) -> Result<ContactEntry, IpcError> {
    core.separate_contact(email)
        .await
        .map_err(to_ipc("separate_contact"))
}
