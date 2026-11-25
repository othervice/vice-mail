//! Stage 19, task A commands (decisions 0061 and 0062): sorted and filtered list views, the
//! order per view and the table presentation of the list. Each forwards to [`AppCore`].

use mail_engine::AppCore;
use mail_model::{
    InboxScope, IpcError, ListOrder, ListTableSettings, ListViewRequest, ViewCursor,
    ViewMessagePage, ViewOrder, ViewThreadPage,
};
use tauri::State;

use super::to_ipc;

/// One page of a view in another order than newest first, or with the quick filter: one row per
/// message. `limit` 1..=500; pass `nextCursor` back unchanged.
#[tauri::command]
#[specta::specta]
pub async fn list_view_messages(
    core: State<'_, AppCore>,
    request: ListViewRequest,
    cursor: Option<ViewCursor>,
    limit: u32,
) -> Result<ViewMessagePage, IpcError> {
    core.list_view_messages(request, cursor, limit)
        .await
        .map_err(to_ipc("list_view_messages"))
}

/// The same as conversations (decision 0039): one row per conversation, at the place of its
/// newest message in the view that matches the filter.
#[tauri::command]
#[specta::specta]
pub async fn list_view_conversations(
    core: State<'_, AppCore>,
    request: ListViewRequest,
    cursor: Option<ViewCursor>,
    limit: u32,
) -> Result<ViewThreadPage, IpcError> {
    core.list_view_conversations(request, cursor, limit)
        .await
        .map_err(to_ipc("list_view_conversations"))
}

/// How many messages of the view match the quick filter.
#[tauri::command]
#[specta::specta]
pub async fn count_list_view(
    core: State<'_, AppCore>,
    request: ListViewRequest,
) -> Result<u32, IpcError> {
    core.count_list_view(request)
        .await
        .map_err(to_ipc("count_list_view"))
}

/// The tags in the view's mailboxes, for the quick filter (keywords not starting with `$`).
#[tauri::command]
#[specta::specta]
pub async fn list_view_tags(
    core: State<'_, AppCore>,
    scope: InboxScope,
) -> Result<Vec<String>, IpcError> {
    core.list_view_tags(scope)
        .await
        .map_err(to_ipc("list_view_tags"))
}

/// The views with an order of their own (device setting).
#[tauri::command]
#[specta::specta]
pub async fn get_list_orders(core: State<'_, AppCore>) -> Result<Vec<ViewOrder>, IpcError> {
    core.get_list_orders()
        .await
        .map_err(to_ipc("get_list_orders"))
}

/// Sets the order of a view (null or newest first removes it); answers every view's order.
#[tauri::command]
#[specta::specta]
pub async fn set_list_order(
    core: State<'_, AppCore>,
    view: String,
    order: Option<ListOrder>,
) -> Result<Vec<ViewOrder>, IpcError> {
    core.set_list_order(view, order)
        .await
        .map_err(to_ipc("set_list_order"))
}

/// The list's presentation (rows or table) and the table's columns on this device.
#[tauri::command]
#[specta::specta]
pub async fn get_list_table(core: State<'_, AppCore>) -> Result<ListTableSettings, IpcError> {
    core.get_list_table()
        .await
        .map_err(to_ipc("get_list_table"))
}

/// Stores the list's presentation and the table's columns; answers what is stored.
#[tauri::command]
#[specta::specta]
pub async fn set_list_table(
    core: State<'_, AppCore>,
    settings: ListTableSettings,
) -> Result<ListTableSettings, IpcError> {
    core.set_list_table(settings)
        .await
        .map_err(to_ipc("set_list_table"))
}
