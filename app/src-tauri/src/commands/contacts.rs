//! The contact directory (stage 10, decision 0023): ranked pages, names, duplicate
//! suggestions, merges the user confirmed. Each one forwards to [`AppCore`].
//!
//! Stage 17 (decision 0050): the contacts view: filters, notes, pictures (the system dialog and
//! the check of dropped paths are here, because they need the window; the core reads the
//! file), a contact's mail and "mark as spam".

use mail_engine::AppCore;
use mail_model::{
    ContactDuplicate, ContactEntry, ContactFilter, ContactMergeRequest, ContactMessages,
    ContactPage, ContactPicture, ContactPictureRef, ContactPictureSource, CoreError, IpcError,
    PictureCrop, SpamMark, SpamMove, SpamUndo, SpamUndone,
};
use tauri::{Runtime, State, WebviewWindow};

use super::to_ipc;
use crate::drops::DropRegistry;

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

/// Stage 17: asks for an image file in the system dialog; the core reads it and has the
/// helper encode it for the cropper. `null` when the user cancelled.
#[tauri::command]
#[specta::specta]
pub async fn pick_contact_picture<R: Runtime>(
    window: WebviewWindow<R>,
    core: State<'_, AppCore>,
) -> Result<Option<ContactPictureSource>, IpcError> {
    let Some(path) = ask_picture_path(&window)
        .await
        .map_err(to_ipc("pick_contact_picture"))?
    else {
        return Ok(None);
    };
    core.load_contact_picture(path)
        .await
        .map(Some)
        .map_err(to_ipc("pick_contact_picture"))
}

/// Stage 17: an image dropped on the window: only a path of a drop the shell saw itself,
/// shortly before (`invalid_argument` otherwise; see `drops.rs`).
#[tauri::command]
#[specta::specta]
pub async fn load_dropped_contact_picture(
    core: State<'_, AppCore>,
    drops: State<'_, DropRegistry>,
    path: String,
) -> Result<ContactPictureSource, IpcError> {
    let mut paths = drops.take(&[path]).map_err(|path| {
        to_ipc("load_dropped_contact_picture")(CoreError::InvalidArgument(format!(
            "{path} was not dropped on the window"
        )))
    })?;
    let path = paths.pop().ok_or_else(|| {
        to_ipc("load_dropped_contact_picture")(CoreError::InvalidArgument("no file".into()))
    })?;
    core.load_contact_picture(path)
        .await
        .map_err(to_ipc("load_dropped_contact_picture"))
}

/// Stage 17: cuts the square `crop` out of the chosen image `token` and stores it as the
/// contact's picture.
#[tauri::command]
#[specta::specta]
pub async fn set_contact_picture(
    core: State<'_, AppCore>,
    email: String,
    token: String,
    crop: PictureCrop,
) -> Result<ContactEntry, IpcError> {
    core.set_contact_picture(email, token, crop)
        .await
        .map_err(to_ipc("set_contact_picture"))
}

/// Stage 17: removes the contact's picture (the initials show again).
#[tauri::command]
#[specta::specta]
pub async fn remove_contact_picture(
    core: State<'_, AppCore>,
    email: String,
) -> Result<ContactEntry, IpcError> {
    core.remove_contact_picture(email)
        .await
        .map_err(to_ipc("remove_contact_picture"))
}

/// Stage 17: the picture of the contact with this address, `null` without one.
#[tauri::command]
#[specta::specta]
pub async fn get_contact_picture(
    core: State<'_, AppCore>,
    email: String,
) -> Result<Option<ContactPicture>, IpcError> {
    core.get_contact_picture(email)
        .await
        .map_err(to_ipc("get_contact_picture"))
}

/// Stage 17: which addresses show a picture.
#[tauri::command]
#[specta::specta]
pub async fn list_contact_pictures(
    core: State<'_, AppCore>,
) -> Result<Vec<ContactPictureRef>, IpcError> {
    core.list_contact_pictures()
        .await
        .map_err(to_ipc("list_contact_pictures"))
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

#[cfg(any(target_os = "macos", windows))]
async fn ask_picture_path<R: Runtime>(
    window: &WebviewWindow<R>,
) -> Result<Option<std::path::PathBuf>, CoreError> {
    Ok(rfd::AsyncFileDialog::new()
        .set_parent(window)
        .add_filter("Images", &["png", "jpg", "jpeg", "gif", "webp"])
        .pick_file()
        .await
        .map(|file| file.path().to_path_buf()))
}

#[cfg(not(any(target_os = "macos", windows)))]
async fn ask_picture_path<R: Runtime>(
    _window: &WebviewWindow<R>,
) -> Result<Option<std::path::PathBuf>, CoreError> {
    Err(CoreError::Internal(
        "choosing a picture needs the system dialog of macOS or Windows".into(),
    ))
}
