//! In-app updates (stage 15, decision 0037): the switch (also the answer to the one-time
//! question), "Check now", download and "Restart to install". Each one forwards to
//! [`AppCore`]. Failures of a step show up in the returned `status`; an error comes back only
//! when the step cannot start (`update_not_ready`) or the setting cannot be stored. Event:
//! `updateStatusChanged`.

use mail_engine::AppCore;
use mail_model::{IpcError, UpdateInfo};
use tauri::State;

use super::to_ipc;

/// Version, the switch, whether to ask the one-time question, and the current state.
#[tauri::command]
#[specta::specta]
pub async fn get_update_info(core: State<'_, AppCore>) -> Result<UpdateInfo, IpcError> {
    core.get_update_info()
        .await
        .map_err(to_ipc("get_update_info"))
}

/// "Check for updates automatically" on or off; either value answers the one-time question.
#[tauri::command]
#[specta::specta]
pub async fn set_update_auto_check(
    core: State<'_, AppCore>,
    enabled: bool,
) -> Result<UpdateInfo, IpcError> {
    core.set_update_auto_check(enabled)
        .await
        .map_err(to_ipc("set_update_auto_check"))
}

/// "Check now", whatever the switch says.
#[tauri::command]
#[specta::specta]
pub async fn check_for_updates(core: State<'_, AppCore>) -> Result<UpdateInfo, IpcError> {
    core.check_for_updates()
        .await
        .map_err(to_ipc("check_for_updates"))
}

/// Starts the download of the update found; progress and the outcome follow as events.
#[tauri::command]
#[specta::specta]
pub async fn download_update(core: State<'_, AppCore>) -> Result<UpdateInfo, IpcError> {
    core.download_update()
        .await
        .map_err(to_ipc("download_update"))
}

/// "Restart to install": installs the downloaded, verified update and restarts the app.
#[tauri::command]
#[specta::specta]
pub async fn install_update(core: State<'_, AppCore>) -> Result<UpdateInfo, IpcError> {
    core.install_update()
        .await
        .map_err(to_ipc("install_update"))
}
