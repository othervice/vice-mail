use mail_engine::{AppCore, DiagnosticsEnv};
use mail_model::{AppInfo, DiagnosticsExportResult, IpcError, StartupProblem, WindowBackdrop};
use tauri::{AppHandle, Manager, Runtime, State, WebviewWindow};

use super::to_ipc;

/// Version, commit, build date and platform of the running app.
#[tauri::command]
#[specta::specta]
pub fn get_app_info(core: State<'_, AppCore>) -> Result<AppInfo, IpcError> {
    Ok(core.app_info())
}

/// Stage 22 (decision 0079): what the system draws where the calling window's page is
/// transparent: `mica` for the main window on Windows 11 (unless transparency effects are off or
/// a contrast theme is on), `none` everywhere else. The UI leaves its chrome transparent only
/// for `mica`.
#[tauri::command]
#[specta::specta]
pub fn get_window_backdrop<R: Runtime>(
    window: WebviewWindow<R>,
) -> Result<WindowBackdrop, IpcError> {
    Ok(window
        .try_state::<crate::backdrop::Backdrops>()
        .map(|backdrops| backdrops.get(window.label()))
        .unwrap_or_default())
}

/// What kept the local database from opening at this start, if anything (the app shell shows
/// it with what to do); `null` when it opened.
#[tauri::command]
#[specta::specta]
pub fn get_startup_problem(core: State<'_, AppCore>) -> Result<Option<StartupProblem>, IpcError> {
    Ok(core.startup_problem())
}

/// Stage 15, "Export diagnostics…": the system's save dialog (Downloads, a dated name), then the
/// core collects, redacts and writes the file. The shell only adds what the core cannot know:
/// the product name, the app identifier, the log directory and the web view's version.
/// `locale` is the web view's language. `cancelled` when the dialog is closed;
/// `diagnostics_write_failed` when the file cannot be written.
#[tauri::command]
#[specta::specta]
pub async fn export_diagnostics<R: Runtime>(
    app: AppHandle<R>,
    window: WebviewWindow<R>,
    core: State<'_, AppCore>,
    locale: String,
) -> Result<DiagnosticsExportResult, IpcError> {
    let product = app
        .config()
        .product_name
        .clone()
        .unwrap_or_else(|| app.package_info().name.clone());
    let name = core.diagnostics_file_name(&product);
    let Some(path) = super::reading::ask_save_path(&window, &name)
        .await
        .map_err(to_ipc("export_diagnostics"))?
    else {
        return Ok(DiagnosticsExportResult::Cancelled);
    };
    let env = DiagnosticsEnv {
        product_name: product,
        app_identifier: app.config().identifier.clone(),
        log_dir: crate::log_dir(&app).ok(),
        webview_version: tauri::webview_version().ok(),
        locale,
    };
    let saved = core
        .export_diagnostics(env, path)
        .await
        .map_err(to_ipc("export_diagnostics"))?;
    Ok(DiagnosticsExportResult::Saved {
        path: saved.display().to_string(),
    })
}
