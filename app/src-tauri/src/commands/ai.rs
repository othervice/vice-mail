//! AI in the app (stage 13, decisions 0019 and 0024): settings with provider profiles (keys go
//! to the keychain and never come back), the choice per function, the cloud permission per
//! account, consent, the request log, the functions (summary, reply suggestion and its draft,
//! writing help, questions), model list, connection test, cancelling, and the open-reply
//! hints (no AI), and the dry run that shows the exact size before consent (decision 0028).
//! Each one forwards to [`AppCore`]. Event: `aiRequest`.

use mail_engine::AppCore;
use mail_model::{
    AccountId, AiAnswer, AiAvailability, AiConnectionTest, AiFunction, AiModel, AiProfileInput,
    AiReplySuggestion, AiRequestId, AiRequestPage, AiSettingsInfo, AiSummary, AiTransfer,
    AiTransferInput, AiWritingResult, AiWritingTask, Draft, IpcError, MessageId, OpenReplyHint,
    OpenReplySettings,
};
use tauri::State;

use super::to_ipc;

/// Switch, profiles, functions, accounts, presets, open-reply settings.
#[tauri::command]
#[specta::specta]
pub async fn get_ai_settings(core: State<'_, AppCore>) -> Result<AiSettingsInfo, IpcError> {
    core.get_ai_settings()
        .await
        .map_err(to_ipc("get_ai_settings"))
}

/// Which functions are usable now (the AI panel is hidden without any).
#[tauri::command]
#[specta::specta]
pub async fn get_ai_availability(core: State<'_, AppCore>) -> Result<AiAvailability, IpcError> {
    core.get_ai_availability()
        .await
        .map_err(to_ipc("get_ai_availability"))
}

/// The master switch. Off: no request leaves the device; running requests are cancelled.
#[tauri::command]
#[specta::specta]
pub async fn set_ai_enabled(
    core: State<'_, AppCore>,
    enabled: bool,
) -> Result<AiSettingsInfo, IpcError> {
    core.set_ai_enabled(enabled)
        .await
        .map_err(to_ipc("set_ai_enabled"))
}

/// Creates (`id` null) or changes a profile; the key goes to the keychain.
#[tauri::command]
#[specta::specta]
pub async fn save_ai_profile(
    core: State<'_, AppCore>,
    profile: AiProfileInput,
) -> Result<AiSettingsInfo, IpcError> {
    core.save_ai_profile(profile)
        .await
        .map_err(to_ipc("save_ai_profile"))
}

/// Deletes a profile and its key; functions that used it are off.
#[tauri::command]
#[specta::specta]
pub async fn delete_ai_profile(
    core: State<'_, AppCore>,
    profile_id: String,
) -> Result<AiSettingsInfo, IpcError> {
    core.delete_ai_profile(profile_id)
        .await
        .map_err(to_ipc("delete_ai_profile"))
}

/// The profile of a function, or off (`null`).
#[tauri::command]
#[specta::specta]
pub async fn set_ai_function(
    core: State<'_, AppCore>,
    ai_function: AiFunction,
    profile_id: Option<String>,
) -> Result<AiSettingsInfo, IpcError> {
    core.set_ai_function(ai_function, profile_id)
        .await
        .map_err(to_ipc("set_ai_function"))
}

/// Whether cloud providers may see an account's mail.
#[tauri::command]
#[specta::specta]
pub async fn set_ai_account_cloud(
    core: State<'_, AppCore>,
    account_id: AccountId,
    allowed: bool,
) -> Result<AiSettingsInfo, IpcError> {
    core.set_ai_account_cloud(account_id, allowed)
        .await
        .map_err(to_ipc("set_ai_account_cloud"))
}

/// Records (`granted`) or withdraws the consent for a cloud profile.
#[tauri::command]
#[specta::specta]
pub async fn set_ai_consent(
    core: State<'_, AppCore>,
    profile_id: String,
    granted: bool,
) -> Result<AiSettingsInfo, IpcError> {
    core.set_ai_consent(profile_id, granted)
        .await
        .map_err(to_ipc("set_ai_consent"))
}

/// Open-reply hints on or off and after how many days.
#[tauri::command]
#[specta::specta]
pub async fn set_open_reply_settings(
    core: State<'_, AppCore>,
    settings: OpenReplySettings,
) -> Result<AiSettingsInfo, IpcError> {
    core.set_open_reply_settings(settings)
        .await
        .map_err(to_ipc("set_open_reply_settings"))
}

/// The models of a profile (needs AI on; sends no mail).
#[tauri::command]
#[specta::specta]
pub async fn list_ai_models(
    core: State<'_, AppCore>,
    profile_id: String,
) -> Result<Vec<AiModel>, IpcError> {
    core.list_ai_models(profile_id)
        .await
        .map_err(to_ipc("list_ai_models"))
}

/// Model list and a short request without mail content.
#[tauri::command]
#[specta::specta]
pub async fn test_ai_connection(
    core: State<'_, AppCore>,
    profile_id: String,
) -> Result<AiConnectionTest, IpcError> {
    core.test_ai_connection(profile_id)
        .await
        .map_err(to_ipc("test_ai_connection"))
}

/// The request log, newest first (metadata only).
#[tauri::command]
#[specta::specta]
pub async fn list_ai_requests(
    core: State<'_, AppCore>,
    before: Option<AiRequestId>,
    limit: u32,
) -> Result<AiRequestPage, IpcError> {
    core.list_ai_requests(before, limit)
        .await
        .map_err(to_ipc("list_ai_requests"))
}

/// Cancels a running request; `false` if none runs under this id.
#[tauri::command]
#[specta::specta]
pub fn cancel_ai_request(core: State<'_, AppCore>, request_id: String) -> bool {
    core.cancel_ai_request(&request_id)
}

/// Summarises the thread of a message; progress as `aiRequest` events under `requestId`.
#[tauri::command]
#[specta::specta]
pub async fn ai_summarize_thread(
    core: State<'_, AppCore>,
    request_id: String,
    message_id: MessageId,
) -> Result<AiSummary, IpcError> {
    core.ai_summarize_thread(request_id, message_id)
        .await
        .map_err(to_ipc("ai_summarize_thread"))
}

/// A reply suggestion to a message, as text for a card.
#[tauri::command]
#[specta::specta]
pub async fn ai_suggest_reply(
    core: State<'_, AppCore>,
    request_id: String,
    message_id: MessageId,
) -> Result<AiReplySuggestion, IpcError> {
    core.ai_suggest_reply(request_id, message_id)
        .await
        .map_err(to_ipc("ai_suggest_reply"))
}

/// "Open draft": a reply draft to the message with the (edited) suggestion as its body.
#[tauri::command]
#[specta::specta]
pub async fn ai_open_reply_draft(
    core: State<'_, AppCore>,
    message_id: MessageId,
    text: String,
) -> Result<Draft, IpcError> {
    core.ai_open_reply_draft(message_id, text)
        .await
        .map_err(to_ipc("ai_open_reply_draft"))
}

/// The writing help on a passage; returns a preview.
#[tauri::command]
#[specta::specta]
pub async fn ai_writing_help(
    core: State<'_, AppCore>,
    request_id: String,
    passage: String,
    task: AiWritingTask,
) -> Result<AiWritingResult, IpcError> {
    core.ai_writing_help(request_id, passage, task)
        .await
        .map_err(to_ipc("ai_writing_help"))
}

/// A question in natural language over the given accounts (`null`: all).
#[tauri::command]
#[specta::specta]
pub async fn ai_ask(
    core: State<'_, AppCore>,
    request_id: String,
    question: String,
    account_ids: Option<Vec<AccountId>>,
) -> Result<AiAnswer, IpcError> {
    core.ai_ask(request_id, question, account_ids)
        .await
        .map_err(to_ipc("ai_ask"))
}

/// Decision 0028: what a function would send for this input (mails, bytes, accounts, host,
/// model, trimming), prepared like the request but never sent; no consent needed.
#[tauri::command]
#[specta::specta]
pub async fn ai_prepare_transfer(
    core: State<'_, AppCore>,
    input: AiTransferInput,
) -> Result<AiTransfer, IpcError> {
    core.ai_prepare_transfer(input)
        .await
        .map_err(to_ipc("ai_prepare_transfer"))
}

/// Open-reply hints over every account (no AI).
#[tauri::command]
#[specta::specta]
pub async fn list_open_reply_hints(
    core: State<'_, AppCore>,
) -> Result<Vec<OpenReplyHint>, IpcError> {
    core.list_open_reply_hints()
        .await
        .map_err(to_ipc("list_open_reply_hints"))
}

/// Dismisses the hint of a message for good.
#[tauri::command]
#[specta::specta]
pub async fn dismiss_open_reply_hint(
    core: State<'_, AppCore>,
    message_id: MessageId,
) -> Result<(), IpcError> {
    core.dismiss_open_reply_hint(message_id)
        .await
        .map_err(to_ipc("dismiss_open_reply_hint"))
}
