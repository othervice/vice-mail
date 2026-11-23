//! Stage 9 commands (decision 0020): the inbox with categories and bundles, "Why here?" and the
//! sender rules, snoozes, pins, tasks, local rules, notification settings and texts, the extras
//! of the reading pane (unsubscribe, invitations), `.eml` export, printing and the default mail
//! app. Each forwards to [`AppCore`]; the save dialog, printing and the default-app switch are
//! done here because they need the window or the system.

use mail_engine::AppCore;
use mail_model::{
    DefaultMailApp, DefaultMailAppOutcome, InboxCursor, InboxFilter, InboxPage, InboxScope,
    InboxSettings, IpcError, ListCursor, MailRule, MarkScope, MessageCategory, MessageCategoryInfo,
    MessageExtras, MessageId, MessageListItem, MessagePage, NotificationSettings,
    NotificationTexts, RuleId, RuleInput, SenderKey, SenderRule, SnoozeChoice, SnoozeTimes,
    TaskFilter, TaskMark, UnsubscribeMethod, UnsubscribeOutcome,
};
use tauri::{AppHandle, Runtime, State, WebviewWindow};

use super::to_ipc;

// ---------------------------------------------------------------------------------------------
// Inbox

/// One page of the inbox: rows with date sections, bundles when categories are on, and on the
/// first page the pinned, returned and priority messages. `limit` counts messages.
#[tauri::command]
#[specta::specta]
pub async fn list_inbox(
    core: State<'_, AppCore>,
    scope: InboxScope,
    cursor: Option<InboxCursor>,
    limit: u32,
) -> Result<InboxPage, IpcError> {
    core.list_inbox(scope, cursor, limit)
        .await
        .map_err(to_ipc("list_inbox"))
}

/// The inbox by category or the important senders ("Alle anzeigen").
#[tauri::command]
#[specta::specta]
pub async fn list_inbox_filtered(
    core: State<'_, AppCore>,
    scope: InboxScope,
    filter: InboxFilter,
    cursor: Option<ListCursor>,
    limit: u32,
) -> Result<MessagePage, IpcError> {
    core.list_inbox_filtered(scope, filter, cursor, limit)
        .await
        .map_err(to_ipc("list_inbox_filtered"))
}

#[tauri::command]
#[specta::specta]
pub async fn get_inbox_settings(core: State<'_, AppCore>) -> Result<InboxSettings, IpcError> {
    core.get_inbox_settings()
        .await
        .map_err(to_ipc("get_inbox_settings"))
}

/// Categories on or off, bundles per category; emits `inboxChanged`.
#[tauri::command]
#[specta::specta]
pub async fn set_inbox_settings(
    core: State<'_, AppCore>,
    settings: InboxSettings,
) -> Result<InboxSettings, IpcError> {
    core.set_inbox_settings(settings)
        .await
        .map_err(to_ipc("set_inbox_settings"))
}

// ---------------------------------------------------------------------------------------------
// Categories and sender rules

/// "Why here?" for a message, with the keys an "always as …" can use.
#[tauri::command]
#[specta::specta]
pub async fn get_message_category(
    core: State<'_, AppCore>,
    message_id: MessageId,
) -> Result<MessageCategoryInfo, IpcError> {
    core.get_message_category(message_id)
        .await
        .map_err(to_ipc("get_message_category"))
}

/// "Always as …" for a sender, domain or list (null removes it); applied to every message in
/// the background, `inboxChanged` follows.
#[tauri::command]
#[specta::specta]
pub async fn set_sender_category(
    core: State<'_, AppCore>,
    key: SenderKey,
    category: Option<MessageCategory>,
) -> Result<Option<SenderRule>, IpcError> {
    core.set_sender_category(key, category)
        .await
        .map_err(to_ipc("set_sender_category"))
}

/// Marks a sender, domain or list important (the "Priority" block), or not.
#[tauri::command]
#[specta::specta]
pub async fn set_sender_important(
    core: State<'_, AppCore>,
    key: SenderKey,
    important: bool,
) -> Result<Option<SenderRule>, IpcError> {
    core.set_sender_important(key, important)
        .await
        .map_err(to_ipc("set_sender_important"))
}

#[tauri::command]
#[specta::specta]
pub async fn list_sender_rules(core: State<'_, AppCore>) -> Result<Vec<SenderRule>, IpcError> {
    core.list_sender_rules()
        .await
        .map_err(to_ipc("list_sender_rules"))
}

#[tauri::command]
#[specta::specta]
pub async fn delete_sender_rule(core: State<'_, AppCore>, id: String) -> Result<(), IpcError> {
    core.delete_sender_rule(id)
        .await
        .map_err(to_ipc("delete_sender_rule"))
}

// ---------------------------------------------------------------------------------------------
// Snoozes, pins, tasks

/// The times the snooze menu offers now.
#[tauri::command]
#[specta::specta]
pub fn get_snooze_times(core: State<'_, AppCore>) -> SnoozeTimes {
    core.snooze_times()
}

/// Snoozes messages; answers when they come back.
#[tauri::command]
#[specta::specta]
pub async fn snooze_messages(
    core: State<'_, AppCore>,
    message_ids: Vec<MessageId>,
    choice: SnoozeChoice,
) -> Result<mail_model::SnoozeResult, IpcError> {
    core.snooze_messages(message_ids, choice)
        .await
        .map(|until| mail_model::SnoozeResult { until })
        .map_err(to_ipc("snooze_messages"))
}

/// Ends snoozes (waiting ones come back now) or clears the mark of returned ones.
#[tauri::command]
#[specta::specta]
pub async fn unsnooze_messages(
    core: State<'_, AppCore>,
    message_ids: Vec<MessageId>,
) -> Result<(), IpcError> {
    core.unsnooze_messages(message_ids)
        .await
        .map_err(to_ipc("unsnooze_messages"))
}

#[tauri::command]
#[specta::specta]
pub async fn set_pinned(
    core: State<'_, AppCore>,
    message_ids: Vec<MessageId>,
    pinned: bool,
) -> Result<(), IpcError> {
    core.set_pinned(message_ids, pinned)
        .await
        .map_err(to_ipc("set_pinned"))
}

/// A task (`{ done: false }`), done (`{ done: true }`) or no task (null).
#[tauri::command]
#[specta::specta]
pub async fn set_task(
    core: State<'_, AppCore>,
    message_ids: Vec<MessageId>,
    mark: Option<TaskMark>,
) -> Result<(), IpcError> {
    core.set_task(message_ids, mark)
        .await
        .map_err(to_ipc("set_task"))
}

/// "Später": waiting snoozes, the next due first.
#[tauri::command]
#[specta::specta]
pub async fn list_snoozed(
    core: State<'_, AppCore>,
    scope: MarkScope,
) -> Result<Vec<MessageListItem>, IpcError> {
    core.list_snoozed(scope)
        .await
        .map_err(to_ipc("list_snoozed"))
}

/// "Angepinnt": the most recently pinned first.
#[tauri::command]
#[specta::specta]
pub async fn list_pinned(
    core: State<'_, AppCore>,
    scope: MarkScope,
) -> Result<Vec<MessageListItem>, IpcError> {
    core.list_pinned(scope).await.map_err(to_ipc("list_pinned"))
}

/// "Aufgaben": open, done or all tasks, the newest first.
#[tauri::command]
#[specta::specta]
pub async fn list_tasks(
    core: State<'_, AppCore>,
    scope: MarkScope,
    filter: TaskFilter,
) -> Result<Vec<MessageListItem>, IpcError> {
    core.list_tasks(scope, filter)
        .await
        .map_err(to_ipc("list_tasks"))
}

// ---------------------------------------------------------------------------------------------
// Local rules

#[tauri::command]
#[specta::specta]
pub async fn list_rules(core: State<'_, AppCore>) -> Result<Vec<MailRule>, IpcError> {
    core.list_rules().await.map_err(to_ipc("list_rules"))
}

#[tauri::command]
#[specta::specta]
pub async fn create_rule(core: State<'_, AppCore>, input: RuleInput) -> Result<MailRule, IpcError> {
    core.create_rule(input).await.map_err(to_ipc("create_rule"))
}

#[tauri::command]
#[specta::specta]
pub async fn update_rule(
    core: State<'_, AppCore>,
    id: RuleId,
    input: RuleInput,
) -> Result<MailRule, IpcError> {
    core.update_rule(id, input)
        .await
        .map_err(to_ipc("update_rule"))
}

#[tauri::command]
#[specta::specta]
pub async fn delete_rule(core: State<'_, AppCore>, id: RuleId) -> Result<(), IpcError> {
    core.delete_rule(id).await.map_err(to_ipc("delete_rule"))
}

/// Puts the rules in this order; returns all of them.
#[tauri::command]
#[specta::specta]
pub async fn reorder_rules(
    core: State<'_, AppCore>,
    ids: Vec<RuleId>,
) -> Result<Vec<MailRule>, IpcError> {
    core.reorder_rules(ids)
        .await
        .map_err(to_ipc("reorder_rules"))
}

// ---------------------------------------------------------------------------------------------
// Notifications

#[tauri::command]
#[specta::specta]
pub async fn get_notification_settings(
    core: State<'_, AppCore>,
) -> Result<NotificationSettings, IpcError> {
    core.get_notification_settings()
        .await
        .map_err(to_ipc("get_notification_settings"))
}

/// Stores the settings and updates the Dock or taskbar counter.
#[tauri::command]
#[specta::specta]
pub async fn set_notification_settings<R: Runtime>(
    app: AppHandle<R>,
    core: State<'_, AppCore>,
    settings: NotificationSettings,
) -> Result<NotificationSettings, IpcError> {
    let stored = core
        .set_notification_settings(settings)
        .await
        .map_err(to_ipc("set_notification_settings"))?;
    crate::notifications::refresh_badge(&app, &core).await;
    Ok(stored)
}

/// The texts of system notifications in the UI's language (send at start and on a language
/// change).
#[tauri::command]
#[specta::specta]
pub fn set_notification_texts(core: State<'_, AppCore>, texts: NotificationTexts) {
    core.set_notification_texts(texts);
}

/// Stage 16: the notification sounds this platform offers (`Default` first), for
/// `NotificationSettings.sound`.
#[tauri::command]
#[specta::specta]
pub fn get_notification_sound_choices(
    core: State<'_, AppCore>,
) -> mail_model::NotificationSoundChoices {
    core.notification_sound_choices()
}

// ---------------------------------------------------------------------------------------------
// Reading pane extras, unsubscribing, invitations, export, printing

/// "Why here?", unsubscribe options and calendar invitations of a message.
#[tauri::command]
#[specta::specta]
pub async fn get_message_extras(
    core: State<'_, AppCore>,
    message_id: MessageId,
) -> Result<MessageExtras, IpcError> {
    core.get_message_extras(message_id)
        .await
        .map_err(to_ipc("get_message_extras"))
}

/// Unsubscribes after the user's click (`oneClick` POSTs; `mailto` returns the composer's
/// fields; `web` opens the page in the browser, after the UI asked).
#[tauri::command]
#[specta::specta]
pub async fn unsubscribe(
    core: State<'_, AppCore>,
    message_id: MessageId,
    method: UnsubscribeMethod,
) -> Result<UnsubscribeOutcome, IpcError> {
    core.unsubscribe(message_id, method)
        .await
        .map_err(to_ipc("unsubscribe"))
}

/// "Im Kalender öffnen": hands the invitation to the system's calendar app.
#[tauri::command]
#[specta::specta]
pub async fn open_invite(
    core: State<'_, AppCore>,
    message_id: MessageId,
    part_id: u32,
) -> Result<(), IpcError> {
    core.open_invite(message_id, part_id)
        .await
        .map_err(to_ipc("open_invite"))
}

/// Saves the message as `.eml` where the user chooses (system dialog); null when cancelled,
/// else the path.
#[tauri::command]
#[specta::specta]
pub async fn export_message<R: Runtime>(
    window: WebviewWindow<R>,
    core: State<'_, AppCore>,
    message_id: MessageId,
) -> Result<Option<String>, IpcError> {
    let name = core
        .export_file_name(message_id)
        .await
        .map_err(to_ipc("export_message"))?;
    let Some(path) = super::reading::ask_save_path(&window, &name)
        .await
        .map_err(to_ipc("export_message"))?
    else {
        return Ok(None);
    };
    let saved = core
        .export_message(message_id, path)
        .await
        .map_err(to_ipc("export_message"))?;
    Ok(Some(saved.display().to_string()))
}

/// Opens the system's print dialog for the window (which offers "Save as PDF"). The UI shows
/// only the cleaned message while printing (print styles).
#[tauri::command]
#[specta::specta]
pub fn print_window<R: Runtime>(window: WebviewWindow<R>) -> Result<(), IpcError> {
    window.print().map_err(|err| {
        to_ipc("print_window")(mail_model::CoreError::Internal(format!("print: {err}")))
    })
}

// ---------------------------------------------------------------------------------------------
// The default mail app

/// Whether the app handles `mailto:` links, and whether it can make itself the default.
#[tauri::command]
#[specta::specta]
pub async fn get_default_mail_app<R: Runtime>(app: AppHandle<R>) -> DefaultMailApp {
    crate::default_app::status(&app).await
}

/// Makes the app the default mail app where the system allows it (macOS), else opens the
/// system's settings for it (Windows).
#[tauri::command]
#[specta::specta]
pub async fn set_default_mail_app<R: Runtime>(app: AppHandle<R>) -> DefaultMailAppOutcome {
    crate::default_app::make_default(&app).await
}

/// The `mailto:` links the app was started with (or got before the UI was ready), as composer
/// fields. Call once at start; later links arrive as `composeRequested`.
#[tauri::command]
#[specta::specta]
pub fn take_launch_links(
    core: State<'_, AppCore>,
    queue: State<'_, crate::links::LinkQueue>,
) -> Vec<mail_model::ComposeRequest> {
    queue
        .take()
        .into_iter()
        .filter_map(|url| match core.parse_mailto_url(&url) {
            Ok(request) => Some(request),
            Err(err) => {
                tracing::warn!(code = ?err.code(), "mailto link refused");
                None
            }
        })
        .collect()
}
