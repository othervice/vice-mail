//! Composing and sending (stage 5, decision 0018): drafts, attachments, sending, the outbox
//! view, compose settings, identities with signatures, contact suggestions. Each one forwards
//! to [`AppCore`]; the file dialog is the only thing done here, because it needs the window.
//! Files dropped on a window come as bytes (stage 23, decision 0082: `begin_file_upload`,
//! `append_file_upload`, then `add_uploaded_attachments`); no command takes a path from the page.

use std::path::PathBuf;

use mail_engine::AppCore;
use mail_model::{
    AccountId, AccountSendSettings, ComposeIdentity, ComposeSettings, ContactSuggestion, CoreError,
    Draft, DraftId, DraftSendOptions, DraftSummary, DraftUpdate, IdentityId, IpcError, MessageId,
    NewDraft, OperationId, OutgoingMail, QuoteLanguage, ReadReceiptInfo, SentCopyMode,
    UnclearDecision, UploadId,
};
use tauri::{Runtime, State, WebviewWindow};

use super::to_ipc;

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

/// Stage 23 (decision 0082): announces a file dropped on a window, which the page hands over as
/// bytes: its name (only the name is kept) and its size. Answers the id its pieces go to.
#[tauri::command]
#[specta::specta]
pub async fn begin_file_upload(
    core: State<'_, AppCore>,
    name: String,
    size: u32,
) -> Result<UploadId, IpcError> {
    core.begin_file_upload(name, u64::from(size))
        .await
        .map_err(to_ipc("begin_file_upload"))
}

/// Stage 23: the next piece of an upload as base64 (at most `MAX_UPLOAD_CHUNK_BYTES` decoded),
/// at `offset` = the bytes written so far. Answers the bytes written now.
#[tauri::command]
#[specta::specta]
pub async fn append_file_upload(
    core: State<'_, AppCore>,
    upload_id: UploadId,
    offset: u32,
    piece: String,
) -> Result<u32, IpcError> {
    let written = core
        .append_file_upload_base64(upload_id, u64::from(offset), &piece)
        .await
        .map_err(to_ipc("append_file_upload"))?;
    Ok(u32::try_from(written).unwrap_or(u32::MAX))
}

/// Stage 23: drops an upload the page gave up (an unknown one is no error).
#[tauri::command]
#[specta::specta]
pub async fn discard_file_upload(
    core: State<'_, AppCore>,
    upload_id: UploadId,
) -> Result<(), IpcError> {
    core.discard_file_upload(upload_id)
        .await
        .map_err(to_ipc("discard_file_upload"))
}

/// Stage 23: complete uploads attached like files from the system dialog (same limits); they
/// are used up either way.
#[tauri::command]
#[specta::specta]
pub async fn add_uploaded_attachments(
    core: State<'_, AppCore>,
    draft_id: DraftId,
    uploads: Vec<UploadId>,
) -> Result<Draft, IpcError> {
    core.add_uploaded_attachments(draft_id, uploads)
        .await
        .map_err(to_ipc("add_uploaded_attachments"))
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

// ---------------------------------------------------------------------------------------------
// Stage 16 (decision 0033): send options of a draft, read receipts (RFC 8098)

/// "Senden und archivieren" and the read-receipt request of a draft. `send_in_progress` while
/// the draft is frozen.
#[tauri::command]
#[specta::specta]
pub async fn set_draft_send_options(
    core: State<'_, AppCore>,
    draft_id: DraftId,
    options: DraftSendOptions,
) -> Result<Draft, IpcError> {
    core.set_draft_send_options(draft_id, options)
        .await
        .map_err(to_ipc("set_draft_send_options"))
}

/// Read receipts of a message: the request of an incoming one and the user's answer, whether
/// a sent one asked for a receipt, and the receipts that came back for it.
#[tauri::command]
#[specta::specta]
pub async fn get_read_receipt(
    core: State<'_, AppCore>,
    message_id: MessageId,
) -> Result<ReadReceiptInfo, IpcError> {
    core.get_read_receipt(message_id)
        .await
        .map_err(to_ipc("get_read_receipt"))
}

/// The user's explicit "send" to a read-receipt request; the only way one is sent.
/// `requestId` is a UUID per press.
#[tauri::command]
#[specta::specta]
pub async fn send_read_receipt(
    core: State<'_, AppCore>,
    message_id: MessageId,
    language: QuoteLanguage,
    request_id: String,
) -> Result<OutgoingMail, IpcError> {
    core.send_read_receipt(message_id, language, request_id)
        .await
        .map_err(to_ipc("send_read_receipt"))
}

/// The user's "do not send" to a read-receipt request.
#[tauri::command]
#[specta::specta]
pub async fn decline_read_receipt(
    core: State<'_, AppCore>,
    message_id: MessageId,
    request_id: String,
) -> Result<ReadReceiptInfo, IpcError> {
    core.decline_read_receipt(message_id, request_id)
        .await
        .map_err(to_ipc("decline_read_receipt"))
}
