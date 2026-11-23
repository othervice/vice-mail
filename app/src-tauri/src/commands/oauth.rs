//! Stage 14 (decision 0042): accounts that sign in with Microsoft or Google. The browser step
//! runs in the core (system browser, loopback redirect); these commands only forward.

use mail_engine::AppCore;
use mail_model::{
    Account, AccountId, AccountSettings, IpcError, OAuthAvailability, OAuthPageTexts, OAuthProvider,
};
use tauri::State;

use super::to_ipc;

/// Which providers this build can sign in with (a client ID was configured when it was built).
#[tauri::command]
#[specta::specta]
pub fn get_oauth_availability(core: State<'_, AppCore>) -> OAuthAvailability {
    core.oauth_availability()
}

/// Signs in with `provider` in the system browser, checks the IMAP login with the token and
/// adds the account read-only. Waits for the browser (up to five minutes);
/// `cancelOauthSignIn` ends the wait.
#[tauri::command]
#[specta::specta]
pub async fn add_oauth_account(
    core: State<'_, AppCore>,
    settings: AccountSettings,
    provider: OAuthProvider,
    texts: OAuthPageTexts,
) -> Result<Account, IpcError> {
    core.add_oauth_account(settings, provider, texts)
        .await
        .map_err(to_ipc("add_oauth_account"))
}

/// Signs in again for an account whose permission ended (or that came from another device).
#[tauri::command]
#[specta::specta]
pub async fn sign_in_again(
    core: State<'_, AppCore>,
    account_id: AccountId,
    texts: OAuthPageTexts,
) -> Result<(), IpcError> {
    core.sign_in_again(account_id, texts)
        .await
        .map_err(to_ipc("sign_in_again"))
}

/// Ends a running sign-in; its command fails with `oauth_cancelled`. `false` if none ran.
#[tauri::command]
#[specta::specta]
pub fn cancel_oauth_sign_in(core: State<'_, AppCore>) -> bool {
    core.cancel_oauth_sign_in()
}
