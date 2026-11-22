use mail_engine::AppCore;
use mail_model::{IpcError, Preferences};
use tauri::State;

use super::to_ipc;

/// Stored theme and language preferences, defaults when nothing is stored.
#[tauri::command]
#[specta::specta]
pub async fn get_preferences(core: State<'_, AppCore>) -> Result<Preferences, IpcError> {
    core.get_preferences()
        .await
        .map_err(to_ipc("get_preferences"))
}

/// Stores the preferences and emits `preferencesChanged` to all windows.
#[tauri::command]
#[specta::specta]
pub async fn set_preferences(
    core: State<'_, AppCore>,
    preferences: Preferences,
) -> Result<Preferences, IpcError> {
    core.set_preferences(preferences)
        .await
        .map_err(to_ipc("set_preferences"))
}
