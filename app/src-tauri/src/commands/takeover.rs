//! Stage 19 (decision 0057): taking over accounts, identities, signatures and local folders
//! from a Thunderbird profile. Passwords are never read (the core never opens `logins.json` or
//! `key4.db`); an account goes through the normal setup.

use std::path::PathBuf;

use mail_engine::AppCore;
use mail_model::{
    AccountId, CoreError, IpcError, MailboxId, MboxJob, TakeoverApplied, TakeoverOpen,
    TakeoverPreview, TakeoverToken, ThunderbirdProfile,
};
use tauri::{AppHandle, Manager, Runtime, State, WebviewWindow};

use super::to_ipc;

/// Desktop-test builds only: the folder that stands for Thunderbird's own (the fixture).
#[cfg(feature = "e2e")]
pub const E2E_THUNDERBIRD_DIR_ENV: &str = "MAIL_APP_E2E_THUNDERBIRD_DIR";

/// Where Thunderbird keeps `profiles.ini`: `~/Library/Thunderbird` on macOS,
/// `%APPDATA%\Thunderbird` on Windows (`~/.thunderbird` elsewhere).
fn thunderbird_base<R: Runtime>(app: &AppHandle<R>) -> Result<PathBuf, CoreError> {
    #[cfg(feature = "e2e")]
    if let Some(dir) = std::env::var_os(E2E_THUNDERBIRD_DIR_ENV) {
        return Ok(PathBuf::from(dir));
    }
    let path = app.path();
    let missing = |err: tauri::Error| CoreError::Internal(format!("no home folder: {err}"));
    if cfg!(target_os = "macos") {
        Ok(path
            .home_dir()
            .map_err(missing)?
            .join("Library/Thunderbird"))
    } else if cfg!(windows) {
        // Roaming AppData, which is %APPDATA%.
        Ok(path.config_dir().map_err(missing)?.join("Thunderbird"))
    } else {
        Ok(path.home_dir().map_err(missing)?.join(".thunderbird"))
    }
}

/// The profiles of Thunderbird at its usual place; empty when it is not installed there.
#[tauri::command]
#[specta::specta]
pub async fn list_thunderbird_profiles<R: Runtime>(
    app: AppHandle<R>,
    core: State<'_, AppCore>,
) -> Result<Vec<ThunderbirdProfile>, IpcError> {
    let base = thunderbird_base(&app).map_err(to_ipc("list_thunderbird_profiles"))?;
    core.thunderbird_profiles(base)
        .await
        .map_err(to_ipc("list_thunderbird_profiles"))
}

/// Reads profile `index` of the list and shows what it holds.
#[tauri::command]
#[specta::specta]
pub async fn open_thunderbird_profile<R: Runtime>(
    app: AppHandle<R>,
    core: State<'_, AppCore>,
    index: u32,
) -> Result<TakeoverPreview, IpcError> {
    let base = thunderbird_base(&app).map_err(to_ipc("open_thunderbird_profile"))?;
    core.open_thunderbird_profile(base, index)
        .await
        .map_err(to_ipc("open_thunderbird_profile"))
}

/// "Choose a profile folder…": the system's folder dialog, then the core reads the folder (a
/// profile, or Thunderbird's own folder with its default profile).
#[tauri::command]
#[specta::specta]
pub async fn choose_thunderbird_profile<R: Runtime>(
    app: AppHandle<R>,
    window: WebviewWindow<R>,
    core: State<'_, AppCore>,
) -> Result<TakeoverOpen, IpcError> {
    let start = thunderbird_base(&app).ok();
    let Some(dir) = ask_profile_dir(&window, start)
        .await
        .map_err(to_ipc("choose_thunderbird_profile"))?
    else {
        return Ok(TakeoverOpen::Cancelled);
    };
    let preview = core
        .open_thunderbird_dir(dir)
        .await
        .map_err(to_ipc("choose_thunderbird_profile"))?;
    Ok(TakeoverOpen::Opened { preview })
}

/// After the account was set up: Thunderbird's other identities and the signatures.
#[tauri::command]
#[specta::specta]
pub async fn apply_thunderbird_account(
    core: State<'_, AppCore>,
    token: TakeoverToken,
    index: u32,
    account_id: AccountId,
) -> Result<TakeoverApplied, IpcError> {
    let token = checked(&token).map_err(to_ipc("apply_thunderbird_account"))?;
    core.apply_thunderbird_account(token, index, account_id)
        .await
        .map_err(to_ipc("apply_thunderbird_account"))
}

/// Imports a local folder of the profile into a folder through the outbox (an mbox job).
#[tauri::command]
#[specta::specta]
pub async fn import_thunderbird_folder(
    core: State<'_, AppCore>,
    token: TakeoverToken,
    index: u32,
    mailbox_id: MailboxId,
) -> Result<MboxJob, IpcError> {
    let token = checked(&token).map_err(to_ipc("import_thunderbird_folder"))?;
    core.import_thunderbird_folder(token, index, mailbox_id)
        .await
        .map_err(to_ipc("import_thunderbird_folder"))
}

fn checked(token: &TakeoverToken) -> Result<TakeoverToken, CoreError> {
    TakeoverToken::parse(token.as_str())
        .ok_or_else(|| CoreError::InvalidArgument("not a token the core made".into()))
}

#[cfg(any(target_os = "macos", windows))]
async fn ask_profile_dir<R: Runtime>(
    window: &WebviewWindow<R>,
    start: Option<PathBuf>,
) -> Result<Option<PathBuf>, CoreError> {
    #[cfg(feature = "e2e")]
    if let Some(path) = crate::e2e::take_dialog_answer() {
        return Ok(Some(path));
    }
    let mut dialog = rfd::AsyncFileDialog::new().set_parent(window);
    if let Some(start) = start.filter(|s| s.is_dir()) {
        dialog = dialog.set_directory(start);
    }
    Ok(dialog
        .pick_folder()
        .await
        .map(|folder| folder.path().to_path_buf()))
}

#[cfg(not(any(target_os = "macos", windows)))]
async fn ask_profile_dir<R: Runtime>(
    _window: &WebviewWindow<R>,
    _start: Option<PathBuf>,
) -> Result<Option<PathBuf>, CoreError> {
    Err(CoreError::Internal(
        "choosing a folder needs the system dialog of macOS or Windows".into(),
    ))
}
