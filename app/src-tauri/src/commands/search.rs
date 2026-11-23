//! Search (stage 7): local search with filters, search on the server, the index state and a
//! rebuild. Each one forwards to [`AppCore`].

use mail_engine::AppCore;
use mail_model::{IpcError, SearchIndexStatus, SearchPage, SearchRequest, ServerSearchResult};
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
