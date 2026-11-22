use mail_engine::AppCore;
use mail_model::{AppInfo, IpcError};
use tauri::State;

/// Version, commit, build date and platform of the running app.
#[tauri::command]
#[specta::specta]
pub fn get_app_info(core: State<'_, AppCore>) -> Result<AppInfo, IpcError> {
    Ok(core.app_info())
}
