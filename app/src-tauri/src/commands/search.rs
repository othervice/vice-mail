//! Search (stage 7): local search with filters, search on the server, the index state and a
//! rebuild; saved searches and this device's recent searches (decision 0026). Each one forwards
//! to [`AppCore`].

use mail_engine::AppCore;
use mail_model::{
    IpcError, RecentSearch, SavedSearch, SavedSearchId, SearchIndexStatus, SearchPage,
    SearchRequest, ServerSearchResult,
};
use tauri::State;

use super::to_ipc;

/// Searches the local messages: hits newest first with highlights, problems and chips.
#[tauri::command]
#[specta::specta]
pub async fn search(
    core: State<'_, AppCore>,
    request: SearchRequest,
) -> Result<SearchPage, IpcError> {
    core.search(request).await.map_err(to_ipc("search"))
}

/// Searches on the server (IMAP SEARCH, read-only) for messages whose body is not local.
#[tauri::command]
#[specta::specta]
pub async fn search_on_server(
    core: State<'_, AppCore>,
    request: SearchRequest,
) -> Result<ServerSearchResult, IpcError> {
    core.search_on_server(request)
        .await
        .map_err(to_ipc("search_on_server"))
}

/// Whether the full-text index is complete or being built.
#[tauri::command]
#[specta::specta]
pub async fn get_search_index_status(
    core: State<'_, AppCore>,
) -> Result<SearchIndexStatus, IpcError> {
    core.get_search_index_status()
        .await
        .map_err(to_ipc("get_search_index_status"))
}

/// Rebuilds the full-text index in the background; search keeps working meanwhile.
#[tauri::command]
#[specta::specta]
pub async fn rebuild_search_index(core: State<'_, AppCore>) -> Result<SearchIndexStatus, IpcError> {
    core.rebuild_search_index()
        .await
        .map_err(to_ipc("rebuild_search_index"))
}

// Saved and recent searches (stage 7, decision 0026).

/// Every saved search, in the sidebar's order.
#[tauri::command]
#[specta::specta]
pub async fn list_saved_searches(core: State<'_, AppCore>) -> Result<Vec<SavedSearch>, IpcError> {
    core.list_saved_searches()
        .await
        .map_err(to_ipc("list_saved_searches"))
}

/// Saves a search (it searches all accounts) after the others; `saved_search_exists`,
/// `saved_search_limit`, `invalid_argument`.
#[tauri::command]
#[specta::specta]
pub async fn save_search(
    core: State<'_, AppCore>,
    name: String,
    query: String,
) -> Result<SavedSearch, IpcError> {
    core.save_search(name, query)
        .await
        .map_err(to_ipc("save_search"))
}

/// Renames a saved search; its query and place stay.
#[tauri::command]
#[specta::specta]
pub async fn rename_saved_search(
    core: State<'_, AppCore>,
    saved_search_id: SavedSearchId,
    name: String,
) -> Result<SavedSearch, IpcError> {
    core.rename_saved_search(saved_search_id, name)
        .await
        .map_err(to_ipc("rename_saved_search"))
}

#[tauri::command]
#[specta::specta]
pub async fn delete_saved_search(
    core: State<'_, AppCore>,
    saved_search_id: SavedSearchId,
) -> Result<(), IpcError> {
    core.delete_saved_search(saved_search_id)
        .await
        .map_err(to_ipc("delete_saved_search"))
}

/// This device's recent searches, newest first.
#[tauri::command]
#[specta::specta]
pub async fn list_recent_searches(core: State<'_, AppCore>) -> Result<Vec<RecentSearch>, IpcError> {
    core.list_recent_searches()
        .await
        .map_err(to_ipc("list_recent_searches"))
}

/// Puts a search on top of this device's history; returns the history.
#[tauri::command]
#[specta::specta]
pub async fn add_recent_search(
    core: State<'_, AppCore>,
    query: String,
) -> Result<Vec<RecentSearch>, IpcError> {
    core.add_recent_search(query)
        .await
        .map_err(to_ipc("add_recent_search"))
}

#[tauri::command]
#[specta::specta]
pub async fn clear_recent_searches(core: State<'_, AppCore>) -> Result<(), IpcError> {
    core.clear_recent_searches()
        .await
        .map_err(to_ipc("clear_recent_searches"))
}
