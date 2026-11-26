//! The contact directory (stage 10, decision 0023): ranked pages, names, duplicate
//! suggestions, merges the user confirmed. Each one forwards to [`AppCore`].
//!
//! Stage 17 (decision 0050): the contacts view: filters, notes, a contact's mail and "mark as
//! spam". Contacts have no pictures since stage 22 (decision 0072).

use mail_engine::AppCore;
use mail_model::{
    ContactDuplicate, ContactEntry, ContactFilter, ContactMergeRequest, ContactMessages,
    ContactPage, IpcError, SpamMark, SpamMove, SpamUndo, SpamUndone,
};
use tauri::State;

use super::to_ipc;

/// One page of the directory, highest ranked first (frequency and recency). `query` filters by
/// word prefixes of names, addresses and domains; `cursor` is the previous page's `next`;
/// `limit` 1..200; `filter` (stage 17) leaves out list and system addresses (`people`).
#[tauri::command]
#[specta::specta]
pub async fn list_contacts(
    core: State<'_, AppCore>,
    query: Option<String>,
    cursor: Option<String>,
    limit: u32,
    filter: ContactFilter,
) -> Result<ContactPage, IpcError> {
    core.list_contacts(query, cursor, limit, filter)
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

/// Stage 17: sets the note of a contact, or with `null` (or empty text) removes it.
#[tauri::command]
#[specta::specta]
pub async fn set_contact_note(
    core: State<'_, AppCore>,
    email: String,
    note: Option<String>,
) -> Result<ContactEntry, IpcError> {
    core.set_contact_note(email, note)
        .await
        .map_err(to_ipc("set_contact_note"))
}

/// Stage 17: the newest messages from and to the contact, newest first (`limit` 1..50).
#[tauri::command]
#[specta::specta]
pub async fn list_contact_messages(
    core: State<'_, AppCore>,
    email: String,
    limit: u32,
) -> Result<ContactMessages, IpcError> {
    core.contact_messages(email, limit)
        .await
        .map_err(to_ipc("list_contact_messages"))
}

/// Stage 17: marks the contact as spam: its addresses become blocked senders.
#[tauri::command]
#[specta::specta]
pub async fn mark_contact_spam(
    core: State<'_, AppCore>,
    email: String,
) -> Result<SpamMark, IpcError> {
    core.mark_contact_spam(email)
        .await
        .map_err(to_ipc("mark_contact_spam"))
}

/// Stage 17: moves existing mail from the contact to junk, through the outbox, where the
/// account may write.
#[tauri::command]
#[specta::specta]
pub async fn move_contact_mail_to_junk(
    core: State<'_, AppCore>,
    email: String,
) -> Result<SpamMove, IpcError> {
    core.move_contact_mail_to_junk(email)
        .await
        .map_err(to_ipc("move_contact_mail_to_junk"))
}

/// Stage 17: undoes "mark as spam" (unblocks, moves the mail back).
#[tauri::command]
#[specta::specta]
pub async fn undo_contact_spam(
    core: State<'_, AppCore>,
    email: String,
    undo: SpamUndo,
) -> Result<SpamUndone, IpcError> {
    core.undo_contact_spam(email, undo)
        .await
        .map_err(to_ipc("undo_contact_spam"))
}
