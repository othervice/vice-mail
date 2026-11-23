//! Stage 15 (decision 0041): the guided first start. The core answers whether the main window
//! shows it and records how it ended; the steps use the commands they always use.

use mail_engine::AppCore;
use mail_model::{FirstStart, FirstStartEnd, IpcError};
use tauri::State;

use super::to_ipc;

/// Whether to show the guided first start: never ended and no account yet. An installation that
/// already has an account counts as done.
#[tauri::command]
#[specta::specta]
pub async fn get_first_start(core: State<'_, AppCore>) -> Result<FirstStart, IpcError> {
    core.get_first_start()
        .await
        .map_err(to_ipc("get_first_start"))
}

/// "Skip setup" or the last step: the first start does not show again on this device.
#[tauri::command]
#[specta::specta]
pub async fn finish_first_start(
    core: State<'_, AppCore>,
    end: FirstStartEnd,
) -> Result<FirstStart, IpcError> {
    core.finish_first_start(end)
        .await
        .map_err(to_ipc("finish_first_start"))
}
