use mail_engine::{AppCore, DiagnosticsEnv};
use mail_model::{AppInfo, DiagnosticsExportResult, IpcError};
use tauri::{AppHandle, Runtime, State, WebviewWindow};

use super::to_ipc;

/// Version, commit, build date and platform of the running app.
#[tauri::command]
#[specta::specta]
pub fn get_app_info(core: State<'_, AppCore>) -> Result<AppInfo, IpcError> {
    Ok(core.app_info())
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
