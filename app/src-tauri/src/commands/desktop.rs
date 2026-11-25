//! Stage 21 (decisions 0069, 0070): running in the background, pausing notifications, starting
//! with the system, the tray menu's texts and the main window's lifecycle. Settings and the pause
//! live in the core; the shell applies them to the tray icon, the window and the system's login
//! items.

use mail_engine::AppCore;
use mail_model::{
    Autostart, BackgroundSettings, CoreError, DesktopTexts, IpcError, NotificationPause,
    PauseNotifications,
};
use tauri::{AppHandle, Manager, Runtime, State, WebviewWindow};
use tauri_specta::Event;

use super::to_ipc;
use crate::lifecycle::Lifecycle;

/// Whether closing the main window keeps the app running, and the macOS menu-bar icon.
#[tauri::command]
#[specta::specta]
pub async fn get_background_settings(
    core: State<'_, AppCore>,
) -> Result<BackgroundSettings, IpcError> {
    core.get_background_settings()
        .await
        .map_err(to_ipc("get_background_settings"))
}

/// Stores them; the shell follows at once (the next close, the menu-bar icon).
#[tauri::command]
#[specta::specta]
pub async fn set_background_settings<R: Runtime>(
    app: AppHandle<R>,
    core: State<'_, AppCore>,
    settings: BackgroundSettings,
) -> Result<BackgroundSettings, IpcError> {
    let stored = core
        .set_background_settings(settings)
        .await
        .map_err(to_ipc("set_background_settings"))?;
    if let Some(lifecycle) = app.try_state::<Lifecycle>() {
        lifecycle.set_settings(stored);
    }
    crate::tray::refresh(&app);
    Ok(stored)
}

/// Whether the app starts with the system, as the system has it.
#[tauri::command]
#[specta::specta]
pub fn get_autostart<R: Runtime>(app: AppHandle<R>) -> Autostart {
    crate::autostart_status(&app)
}

/// Registers or removes the login item; `invalid_argument` where this build cannot register
/// itself (a development build), `internal` when the system refuses.
#[tauri::command]
#[specta::specta]
pub fn set_autostart<R: Runtime>(app: AppHandle<R>, enabled: bool) -> Result<Autostart, IpcError> {
    crate::set_autostart(&app, enabled).map_err(to_ipc("set_autostart"))
}

/// Whether notifications are paused, and until when.
#[tauri::command]
#[specta::specta]
pub async fn get_notification_pause(
    core: State<'_, AppCore>,
) -> Result<NotificationPause, IpcError> {
    core.notification_pause()
        .await
        .map_err(to_ipc("get_notification_pause"))
}

/// Pauses for an hour or until tomorrow 08:00, or resumes (`notificationPauseChanged`).
#[tauri::command]
#[specta::specta]
pub async fn pause_notifications(
    core: State<'_, AppCore>,
    choice: PauseNotifications,
) -> Result<NotificationPause, IpcError> {
    core.pause_notifications(choice)
        .await
        .map_err(to_ipc("pause_notifications"))
}

/// The tray menu's texts in the UI's language; kept for a start in the background.
#[tauri::command]
#[specta::specta]
pub async fn set_desktop_texts<R: Runtime>(
    app: AppHandle<R>,
    core: State<'_, AppCore>,
    texts: DesktopTexts,
) -> Result<(), IpcError> {
    core.set_desktop_texts(texts)
        .await
        .map_err(to_ipc("set_desktop_texts"))?;
    crate::tray::refresh(&app);
    Ok(())
}

/// The main window's page listens to core events now: the requests that waited for it
/// (`openMessageRequested`, `openInboxRequested`, `composeRequested`) follow. Other windows: no-op.
#[tauri::command]
#[specta::specta]
pub fn main_window_ready<R: Runtime>(app: AppHandle<R>, window: WebviewWindow<R>) {
    if window.label() != crate::MAIN_WINDOW {
        return;
    }
    let Some(lifecycle) = app.try_state::<Lifecycle>() else {
        return;
    };
    for event in lifecycle.page_listens() {
        if let Err(err) = crate::CoreEventMessage(event).emit_to(&app, crate::MAIN_WINDOW) {
            tracing::warn!(%err, "a waiting request did not reach the main window");
        }
    }
}

/// The answer to the one-time note (`closeToBackgroundAsked`): the main window goes into the
/// background (`quit` false) or the app quits. Only from the main window.
#[tauri::command]
#[specta::specta]
pub async fn close_main_window<R: Runtime>(
    app: AppHandle<R>,
    window: WebviewWindow<R>,
    quit: bool,
) -> Result<(), IpcError> {
    if window.label() != crate::MAIN_WINDOW {
        return Err(to_ipc("close_main_window")(CoreError::InvalidArgument(
            "only the main window closes itself this way".into(),
        )));
    }
    crate::lifecycle::close_main_window(&app, quit).await;
    Ok(())
}
