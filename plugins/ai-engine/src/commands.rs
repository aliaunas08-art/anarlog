use std::sync::Arc;

use anlg_ai_engine::{
    Engine, EngineError, EngineEvent, EngineRequest, EngineStatus, KeyStatus, Quotas,
};
use anlg_aspc::{Action, DataType, Denied};
use tauri::Manager;
use tauri::ipc::Channel;

fn denied(denied: Denied) -> EngineError {
    EngineError::new(denied.code.as_str(), denied.to_string())
}

fn engine<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Result<Arc<Engine>, EngineError> {
    app.try_state::<Arc<Engine>>()
        .map(|state| Arc::clone(state.inner()))
        .ok_or_else(|| {
            EngineError::new(
                anlg_ai_engine::codes::UNAVAILABLE,
                "the AI engine isn't running",
            )
        })
}

/// Sends one Messages API request through the Engine and streams the
/// response back over `on_event`. `(Notes, Read)`: the composer may use it.
#[tauri::command]
#[specta::specta]
pub async fn request<R: tauri::Runtime>(
    aspc_window: tauri::Window<R>,
    app: tauri::AppHandle<R>,
    request: EngineRequest,
    on_event: Channel<EngineEvent>,
) -> Result<(), EngineError> {
    anlg_aspc::require(&app, &aspc_window, DataType::Notes, Action::Read).map_err(denied)?;
    let engine = engine(&app)?;
    engine
        .request(request, |event| {
            let _ = on_event.send(event);
        })
        .await
}

#[tauri::command]
#[specta::specta]
pub async fn cancel_request<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    request_id: String,
) -> Result<(), EngineError> {
    engine(&app)?.cancel(&request_id);
    Ok(())
}

/// Mode, model, usage, quotas and the pause reason. No key material.
#[tauri::command]
#[specta::specta]
pub async fn status<R: tauri::Runtime>(
    aspc_window: tauri::Window<R>,
    app: tauri::AppHandle<R>,
) -> Result<EngineStatus, EngineError> {
    anlg_aspc::require(&app, &aspc_window, DataType::Settings, Action::Read).map_err(denied)?;
    Ok(engine(&app)?.status())
}

/// Whether a key is stored. Presence only: an Admin's presence-only grant
/// is enough, and the key is never returned.
#[tauri::command]
#[specta::specta]
pub async fn key_status<R: tauri::Runtime>(
    aspc_window: tauri::Window<R>,
    app: tauri::AppHandle<R>,
) -> Result<KeyStatus, EngineError> {
    anlg_aspc::require(&app, &aspc_window, DataType::Keys, Action::Read).map_err(denied)?;
    engine(&app)?.key_status().await
}

/// Writes the key the user typed into the Keychain. Never echoed back.
#[tauri::command]
#[specta::specta]
pub async fn set_api_key<R: tauri::Runtime>(
    aspc_window: tauri::Window<R>,
    app: tauri::AppHandle<R>,
    key: String,
) -> Result<KeyStatus, EngineError> {
    anlg_aspc::require(&app, &aspc_window, DataType::Keys, Action::Edit)
        .and_then(anlg_aspc::Grant::require_full)
        .map_err(denied)?;
    engine(&app)?.set_api_key(key).await
}

#[tauri::command]
#[specta::specta]
pub async fn clear_api_key<R: tauri::Runtime>(
    aspc_window: tauri::Window<R>,
    app: tauri::AppHandle<R>,
) -> Result<KeyStatus, EngineError> {
    anlg_aspc::require(&app, &aspc_window, DataType::Keys, Action::Edit)
        .and_then(anlg_aspc::Grant::require_full)
        .map_err(denied)?;
    engine(&app)?.clear_api_key().await
}

/// Quotas are validated against fixed ranges in Rust.
#[tauri::command]
#[specta::specta]
pub async fn set_quotas<R: tauri::Runtime>(
    aspc_window: tauri::Window<R>,
    app: tauri::AppHandle<R>,
    quotas: Quotas,
) -> Result<EngineStatus, EngineError> {
    anlg_aspc::require(&app, &aspc_window, DataType::Settings, Action::Administer)
        .map_err(denied)?;
    engine(&app)?.set_quotas(quotas)
}
