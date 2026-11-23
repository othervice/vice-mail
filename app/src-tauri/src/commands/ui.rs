//! Stage 8 (decision 0025): UI preferences, the layout of the calling window, message and
//! composer windows.

use mail_engine::AppCore;
use mail_model::{AccountId, CoreError, DraftId, IpcError, MessageId, UiPreferences, WindowLayout};
use tauri::{AppHandle, Runtime, State, WebviewWindow};

use super::to_ipc;
use crate::windows;

/// Shortcut changes of both platforms, text size and recent palette commands; defaults when
/// nothing is stored.
#[tauri::command]
#[specta::specta]
pub async fn get_ui_preferences(core: State<'_, AppCore>) -> Result<UiPreferences, IpcError> {
    core.get_ui_preferences()
        .await
        .map_err(to_ipc("get_ui_preferences"))
}

/// Validates and stores the UI preferences, emits `uiPreferencesChanged` to all windows and
/// returns what is stored (each platform's list ordered by command). `invalid_argument` with a
/// detail when a limit is broken; nothing is stored then.
#[tauri::command]
#[specta::specta]
pub async fn set_ui_preferences(
    core: State<'_, AppCore>,
    preferences: UiPreferences,
) -> Result<UiPreferences, IpcError> {
    core.set_ui_preferences(preferences)
        .await
        .map_err(to_ipc("set_ui_preferences"))
}

/// The layout of the calling window (the shell knows which window asks; all message windows
/// share one layout). Defaults when nothing is stored.
#[tauri::command]
#[specta::specta]
pub async fn get_window_layout<R: Runtime>(
    core: State<'_, AppCore>,
    window: WebviewWindow<R>,
) -> Result<WindowLayout, IpcError> {
    let slot = windows::layout_slot(window.label()).map_err(to_ipc("get_window_layout"))?;
    core.get_window_layout(slot)
        .await
        .map_err(to_ipc("get_window_layout"))
}

/// Stores the layout of the calling window on this device and returns it.
#[tauri::command]
#[specta::specta]
pub async fn set_window_layout<R: Runtime>(
    core: State<'_, AppCore>,
    window: WebviewWindow<R>,
    layout: WindowLayout,
) -> Result<WindowLayout, IpcError> {
    let slot = windows::layout_slot(window.label()).map_err(to_ipc("set_window_layout"))?;
    core.set_window_layout(slot, layout)
        .await
        .map_err(to_ipc("set_window_layout"))
}

/// Shows one message in a window of its own: the app's page with
/// `?window=message&accountId=<uuid>&messageId=<id>`, the same CSP, capability and navigation
/// guards as the main window. A second call for the same message focuses its window.
/// `not_found` if the message does not exist or belongs to another account.
#[tauri::command]
#[specta::specta]
pub async fn open_message_window<R: Runtime>(
    app: AppHandle<R>,
    core: State<'_, AppCore>,
    account_id: AccountId,
    message_id: MessageId,
) -> Result<(), IpcError> {
    if windows::focus_window(&app, &windows::message_window_label(account_id, message_id)) {
        return Ok(());
    }
    let subject = core
        .message_window_subject(account_id, message_id)
        .await
        .map_err(to_ipc("open_message_window"))?;
    windows::open_message_window(&app, account_id, message_id, &subject, theme(&core).await)
        .map(drop)
        .map_err(|err| {
            to_ipc("open_message_window")(CoreError::Internal(format!("open window: {err}")))
        })
}

/// Shows the composer of one draft in a window of its own (stage 10 composer, several drafts at
/// once): the app's page with `?window=composer&draftId=<uuid>`, the same CSP, capability and
/// guards as the main window. A second call for the same draft focuses its window.
/// `invalid_argument` for an id that is no UUID, `not_found` if the draft does not exist.
#[tauri::command]
#[specta::specta]
pub async fn open_composer_window<R: Runtime>(
    app: AppHandle<R>,
    core: State<'_, AppCore>,
    draft_id: DraftId,
) -> Result<(), IpcError> {
    let draft_id = DraftId::parse(draft_id.as_str()).ok_or_else(|| {
        to_ipc("open_composer_window")(CoreError::InvalidArgument(
            "the draft id is not a UUID".into(),
        ))
    })?;
    if windows::focus_window(&app, &windows::composer_window_label(&draft_id)) {
        return Ok(());
    }
    let draft = core
        .get_draft(draft_id.clone())
        .await
        .map_err(to_ipc("open_composer_window"))?;
    windows::open_composer_window(&app, &draft_id, &draft.subject, theme(&core).await)
        .map(drop)
        .map_err(|err| {
            to_ipc("open_composer_window")(CoreError::Internal(format!("open window: {err}")))
        })
}

/// The window frame's theme for a new window, from the stored preferences.
pub(crate) async fn theme(core: &AppCore) -> Option<tauri::Theme> {
    match core.get_preferences().await {
        Ok(preferences) => crate::window_theme::window_theme(preferences.theme),
        Err(_) => None,
    }
}
