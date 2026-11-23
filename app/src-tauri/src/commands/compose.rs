//! Composing and sending (stage 5, decision 0018): drafts, attachments, sending, the outbox
//! view, compose settings, identities with signatures, contact suggestions. Each one forwards
//! to [`AppCore`]; the file dialog and the check of dropped paths are the only things done
//! here, because they need the window.

use std::path::PathBuf;

use mail_engine::AppCore;
use mail_model::{
    AccountId, AccountSendSettings, ComposeIdentity, ComposeSettings, ContactSuggestion, CoreError,
    Draft, DraftId, DraftSummary, DraftUpdate, IdentityId, IpcError, MessageId, NewDraft,
    OperationId, OutgoingMail, SentCopyMode, UnclearDecision,
};
use tauri::{Runtime, State, WebviewWindow};

use super::to_ipc;
use crate::drops::DropRegistry;

/// A new draft: empty, a reply, a reply to all, or a forward (with the original's attachments).
#[tauri::command]
#[specta::specta]
pub async fn create_draft(core: State<'_, AppCore>, request: NewDraft) -> Result<Draft, IpcError> {
    core.create_draft(request)
        .await
        .map_err(to_ipc("create_draft"))
}

#[tauri::command]
#[specta::specta]
pub async fn get_draft(core: State<'_, AppCore>, draft_id: DraftId) -> Result<Draft, IpcError> {
    core.get_draft(draft_id).await.map_err(to_ipc("get_draft"))
}

/// Saves the composer's fields locally; call it every few seconds while the user types.
#[tauri::command]
#[specta::specta]
pub async fn save_draft(
    core: State<'_, AppCore>,
    draft_id: DraftId,
    update: DraftUpdate,
) -> Result<Draft, IpcError> {
    core.save_draft(draft_id, update)
        .await
        .map_err(to_ipc("save_draft"))
}

/// The composer closed: the newest version goes to the server's Drafts folder now.
#[tauri::command]
#[specta::specta]
pub async fn close_draft(core: State<'_, AppCore>, draft_id: DraftId) -> Result<Draft, IpcError> {
    core.close_draft(draft_id)
        .await
        .map_err(to_ipc("close_draft"))
}

/// Discards a draft, its attachments and its copy in the Drafts folder.
#[tauri::command]
#[specta::specta]
pub async fn delete_draft(core: State<'_, AppCore>, draft_id: DraftId) -> Result<(), IpcError> {
    core.delete_draft(draft_id)
        .await
        .map_err(to_ipc("delete_draft"))
}

/// Local drafts, newest change first; all accounts when `accountId` is null.
#[tauri::command]
#[specta::specta]
pub async fn list_drafts(
    core: State<'_, AppCore>,
    account_id: Option<AccountId>,
) -> Result<Vec<DraftSummary>, IpcError> {
    core.list_drafts(account_id)
        .await
        .map_err(to_ipc("list_drafts"))
}

/// Opens a message of the Drafts folder in the composer.
#[tauri::command]
#[specta::specta]
pub async fn open_draft_from_message(
    core: State<'_, AppCore>,
    message_id: MessageId,
) -> Result<Draft, IpcError> {
    core.open_draft_from_message(message_id)
        .await
        .map_err(to_ipc("open_draft_from_message"))
}

/// Lets the user pick files in the system dialog and attaches them; cancelling changes nothing.
#[tauri::command]
#[specta::specta]
pub async fn pick_attachments<R: Runtime>(
    window: WebviewWindow<R>,
    core: State<'_, AppCore>,
    draft_id: DraftId,
) -> Result<Draft, IpcError> {
    let paths = ask_open_paths(&window)
        .await
        .map_err(to_ipc("pick_attachments"))?;
    if paths.is_empty() {
        return core
            .get_draft(draft_id)
            .await
            .map_err(to_ipc("pick_attachments"));
    }
    core.add_draft_attachments(draft_id, paths)
        .await
        .map_err(to_ipc("pick_attachments"))
}

/// Attaches files dropped on the window: only paths of a drop the shell saw itself, shortly
/// before (`invalid_argument` otherwise; see `drops.rs`).
#[tauri::command]
#[specta::specta]
pub async fn add_dropped_attachments(
    core: State<'_, AppCore>,
    drops: State<'_, DropRegistry>,
    draft_id: DraftId,
    paths: Vec<String>,
) -> Result<Draft, IpcError> {
    let paths = drops.take(&paths).map_err(|path| {
        to_ipc("add_dropped_attachments")(CoreError::InvalidArgument(format!(
            "{path} was not dropped on the window"
        )))
    })?;
    core.add_draft_attachments(draft_id, paths)
        .await
        .map_err(to_ipc("add_dropped_attachments"))
}

#[tauri::command]
#[specta::specta]
pub async fn remove_draft_attachment(
    core: State<'_, AppCore>,
    draft_id: DraftId,
    attachment_id: String,
) -> Result<Draft, IpcError> {
    core.remove_draft_attachment(draft_id, attachment_id)
        .await
        .map_err(to_ipc("remove_draft_attachment"))
}

/// Queues the draft for sending after the undo delay. `requestId` is a UUID per press of "send".
#[tauri::command]
#[specta::specta]
pub async fn send_draft(
    core: State<'_, AppCore>,
    draft_id: DraftId,
    request_id: String,
) -> Result<OutgoingMail, IpcError> {
    core.send_draft(draft_id, request_id)
        .await
        .map_err(to_ipc("send_draft"))
}

/// Undo within the delay, or edit a failed or unclear send: back to the draft, nothing sent.
#[tauri::command]
#[specta::specta]
pub async fn cancel_send(core: State<'_, AppCore>, draft_id: DraftId) -> Result<Draft, IpcError> {
    core.cancel_send(draft_id)
        .await
        .map_err(to_ipc("cancel_send"))
}

/// The outbox view of an account, newest first, below `before`. `limit` 1..500.
#[tauri::command]
#[specta::specta]
pub async fn list_outgoing(
    core: State<'_, AppCore>,
    account_id: AccountId,
    before: Option<OperationId>,
    limit: u32,
) -> Result<Vec<OutgoingMail>, IpcError> {
    core.list_outgoing(account_id, before, limit)
        .await
        .map_err(to_ipc("list_outgoing"))
}

/// The user's answer to `sendNeedsDecision`.
#[tauri::command]
#[specta::specta]
pub async fn resolve_unclear_send(
    core: State<'_, AppCore>,
    operation_id: OperationId,
    decision: UnclearDecision,
) -> Result<OutgoingMail, IpcError> {
    core.resolve_unclear_send(operation_id, decision)
        .await
        .map_err(to_ipc("resolve_unclear_send"))
}

#[tauri::command]
#[specta::specta]
pub async fn get_compose_settings(core: State<'_, AppCore>) -> Result<ComposeSettings, IpcError> {
    core.get_compose_settings()
        .await
        .map_err(to_ipc("get_compose_settings"))
}

/// Stores the undo delay (0 to 30 seconds).
#[tauri::command]
#[specta::specta]
pub async fn set_compose_settings(
    core: State<'_, AppCore>,
    settings: ComposeSettings,
) -> Result<ComposeSettings, IpcError> {
    core.set_compose_settings(settings)
        .await
        .map_err(to_ipc("set_compose_settings"))
}

#[tauri::command]
#[specta::specta]
pub async fn get_send_settings(
    core: State<'_, AppCore>,
    account_id: AccountId,
) -> Result<AccountSendSettings, IpcError> {
    core.get_send_settings(account_id)
        .await
        .map_err(to_ipc("get_send_settings"))
}

/// Who files sent mail of the account: `auto`, `app` or `server`.
#[tauri::command]
#[specta::specta]
pub async fn set_sent_copy_mode(
    core: State<'_, AppCore>,
    account_id: AccountId,
    mode: SentCopyMode,
) -> Result<AccountSendSettings, IpcError> {
    core.set_sent_copy_mode(account_id, mode)
        .await
        .map_err(to_ipc("set_sent_copy_mode"))
}

/// Identities of an account with their text signatures, the default first.
#[tauri::command]
#[specta::specta]
pub async fn list_compose_identities(
    core: State<'_, AppCore>,
    account_id: AccountId,
) -> Result<Vec<ComposeIdentity>, IpcError> {
    core.list_compose_identities(account_id)
        .await
        .map_err(to_ipc("list_compose_identities"))
}

/// Sets or removes (null) an identity's text signature.
#[tauri::command]
#[specta::specta]
pub async fn set_identity_signature(
    core: State<'_, AppCore>,
    identity_id: IdentityId,
    signature: Option<String>,
) -> Result<ComposeIdentity, IpcError> {
    core.set_identity_signature(identity_id, signature)
        .await
        .map_err(to_ipc("set_identity_signature"))
}

/// Autocomplete from addresses of earlier mail. `limit` 1..50.
#[tauri::command]
#[specta::specta]
pub async fn search_contacts(
    core: State<'_, AppCore>,
    query: String,
    limit: u32,
) -> Result<Vec<ContactSuggestion>, IpcError> {
    core.search_contacts(query, limit)
        .await
        .map_err(to_ipc("search_contacts"))
}

/// The system's open dialog for several files, as a sheet of the window. Empty when the user
/// cancels. Only the paths come back; the core copies the files.
#[cfg(any(target_os = "macos", windows))]
async fn ask_open_paths<R: Runtime>(window: &WebviewWindow<R>) -> Result<Vec<PathBuf>, CoreError> {
    Ok(rfd::AsyncFileDialog::new()
        .set_parent(window)
        .pick_files()
        .await
        .map(|files| files.iter().map(|f| f.path().to_path_buf()).collect())
        .unwrap_or_default())
}

#[cfg(not(any(target_os = "macos", windows)))]
async fn ask_open_paths<R: Runtime>(_window: &WebviewWindow<R>) -> Result<Vec<PathBuf>, CoreError> {
    Err(CoreError::Internal(
        "attaching files needs the system dialog of macOS or Windows".into(),
    ))
}
