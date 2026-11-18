use tauri::Manager;

use crate::{SearchDocument, SearchRequest, SearchResult, TantivyPluginExt};

#[tauri::command]
#[specta::specta]
pub(crate) async fn search<R: tauri::Runtime>(
    aspc_window: tauri::Window<R>,
    app: tauri::AppHandle<R>,
    request: SearchRequest,
) -> Result<SearchResult, String> {
    anlg_aspc::require(
        &app,
        &aspc_window,
        anlg_aspc::DataType::Notes,
        anlg_aspc::Action::Read,
    )
    .map_err(|denied| denied.to_string())?;
    // C1.2: only the local workspace's documents. No workspace yet (schema
    // not ready) means no results, never all of them.
    let scope = app
        .try_state::<std::sync::Arc<anlg_aspc::AspcState>>()
        .and_then(|state| state.scope())
        .ok_or_else(|| "aspc.no_context: no workspace".to_string())?;
    let mut request = request;
    request.filters.exclude_session_ids.clear();
    if request.filters.session_id.is_none() && !request.filters.include_private {
        let source = app
            .try_state::<crate::PrivateSessionSource>()
            .map(|state| state.inner().clone())
            .ok_or_else(|| "aspc.private_filter_unavailable".to_string())?;
        request.filters.exclude_session_ids = (source.0)().await?;
    }
    app.tantivy()
        .search(&scope, request)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn reindex<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    aspc_window: tauri::Window<R>,
    collection: Option<String>,
) -> Result<(), String> {
    anlg_aspc::require(
        &aspc_window,
        &aspc_window,
        anlg_aspc::DataType::Notes,
        anlg_aspc::Action::Edit,
    )
    .map_err(|denied| denied.to_string())?;
    app.tantivy()
        .reindex(collection)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn add_document<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    aspc_window: tauri::Window<R>,
    document: SearchDocument,
    collection: Option<String>,
) -> Result<(), String> {
    anlg_aspc::require(
        &aspc_window,
        &aspc_window,
        anlg_aspc::DataType::Notes,
        anlg_aspc::Action::Edit,
    )
    .map_err(|denied| denied.to_string())?;
    app.tantivy()
        .add_document(collection, document)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn update_document<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    aspc_window: tauri::Window<R>,
    document: SearchDocument,
    collection: Option<String>,
) -> Result<(), String> {
    anlg_aspc::require(
        &aspc_window,
        &aspc_window,
        anlg_aspc::DataType::Notes,
        anlg_aspc::Action::Edit,
    )
    .map_err(|denied| denied.to_string())?;
    app.tantivy()
        .update_document(collection, document)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn update_documents<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    aspc_window: tauri::Window<R>,
    documents: Vec<SearchDocument>,
    collection: Option<String>,
) -> Result<(), String> {
    anlg_aspc::require(
        &aspc_window,
        &aspc_window,
        anlg_aspc::DataType::Notes,
        anlg_aspc::Action::Edit,
    )
    .map_err(|denied| denied.to_string())?;
    app.tantivy()
        .update_documents(collection, documents)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn remove_document<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    aspc_window: tauri::Window<R>,
    id: String,
    collection: Option<String>,
) -> Result<(), String> {
    anlg_aspc::require(
        &aspc_window,
        &aspc_window,
        anlg_aspc::DataType::Notes,
        anlg_aspc::Action::Edit,
    )
    .map_err(|denied| denied.to_string())?;
    app.tantivy()
        .remove_document(collection, id)
        .await
        .map_err(|e| e.to_string())
}
