//! Stage 18 (decision 0051): the commands of the new-mail popup, the only ones its window may
//! call (`crate::popup::POPUP_COMMANDS`). Each forwards to [`AppCore`]; "Read" and "N more" also
//! bring the main window to the front, and the page's measured height sizes the window.

use mail_engine::AppCore;
use mail_model::{CoreError, IpcError, MessageId, NewMailPopup};
use tauri::{AppHandle, Runtime, State, WebviewWindow};

use super::to_ipc;

/// The popup as it is now: at most three cards (newest first), how many more wait, and the
/// app's theme, language and accent.
#[tauri::command]
#[specta::specta]
pub async fn get_new_mail_popup(core: State<'_, AppCore>) -> Result<NewMailPopup, IpcError> {
    Ok(core.new_mail_popup().await)
}

/// "Read": the card goes, the main window comes to the front and shows the message in its
/// inbox (`openMessageRequested`). `not_found` if the card is gone.
#[tauri::command]
#[specta::specta]
pub async fn read_from_popup<R: Runtime>(
    app: AppHandle<R>,
    core: State<'_, AppCore>,
    message_id: MessageId,
) -> Result<NewMailPopup, IpcError> {
    let popup = core
        .read_from_popup(message_id)
        .await
        .map_err(to_ipc("read_from_popup"))?;
    crate::notifications::show_main_window(&app);
    Ok(popup)
}

/// "Delete": the list's delete through the outbox into the trash; the card shows "Deleted"
/// with Undo. `read_only` for a read-only account, `missing_mailbox` without a trash folder.
#[tauri::command]
#[specta::specta]
pub async fn delete_from_popup(
    core: State<'_, AppCore>,
    message_id: MessageId,
) -> Result<NewMailPopup, IpcError> {
    core.delete_from_popup(message_id)
        .await
        .map_err(to_ipc("delete_from_popup"))
}

/// "Undo" on a deleted card.
#[tauri::command]
#[specta::specta]
pub async fn undo_popup_delete(
    core: State<'_, AppCore>,
    message_id: MessageId,
) -> Result<NewMailPopup, IpcError> {
    core.undo_popup_delete(message_id)
        .await
        .map_err(to_ipc("undo_popup_delete"))
}

/// Closes one card, or all of them (`null`: the popup hid itself). The window closes with the
/// last card.
#[tauri::command]
#[specta::specta]
pub async fn dismiss_popup(
    core: State<'_, AppCore>,
    message_id: Option<MessageId>,
) -> Result<NewMailPopup, IpcError> {
    Ok(core.dismiss_popup(message_id).await)
}

/// "N more": the popup closes, the main window comes to the front with the inbox.
#[tauri::command]
#[specta::specta]
pub async fn open_inbox_from_popup<R: Runtime>(
    app: AppHandle<R>,
    core: State<'_, AppCore>,
) -> Result<NewMailPopup, IpcError> {
    let popup = core.open_inbox_from_popup().await;
    crate::notifications::show_main_window(&app);
    Ok(popup)
}

/// The popup's page measured itself: its window takes `height` (logical pixels, within limits)
/// at the bottom right of the screen and appears if it has not yet. Only for the popup's own
/// window (`invalid_argument` from any other).
#[tauri::command]
#[specta::specta]
pub async fn fit_new_mail_popup<R: Runtime>(
    app: AppHandle<R>,
    window: WebviewWindow<R>,
    height: f64,
) -> Result<(), IpcError> {
    if window.label() != crate::popup::POPUP_WINDOW {
        return Err(to_ipc("fit_new_mail_popup")(CoreError::InvalidArgument(
            "only the new-mail popup sizes itself".into(),
        )));
    }
    crate::popup::fit(&app, &window, height)
        .map_err(|err| to_ipc("fit_new_mail_popup")(CoreError::Internal(err.to_string())))
}
