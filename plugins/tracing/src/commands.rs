use std::path::PathBuf;

use crate::{Level, TracingPluginExt};

#[tauri::command]
#[specta::specta]
pub async fn logs_dir<R: tauri::Runtime>(app: tauri::AppHandle<R>) -> Result<PathBuf, String> {
    app.tracing().logs_dir().map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub async fn do_log<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    level: Level,
    data: Vec<serde_json::Value>,
) -> Result<(), String> {
    app.tracing().do_log(level, data).map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub async fn log_content<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    aspc_window: tauri::Window<R>,
    cloudsync_only: Option<bool>,
) -> Result<Option<String>, String> {
    anlg_aspc::require(
        &aspc_window,
        &aspc_window,
        anlg_aspc::DataType::Settings,
        anlg_aspc::Action::Read,
    )
    .map_err(|denied| denied.to_string())?;
    app.tracing()
        .filtered_log_content(cloudsync_only.unwrap_or(false))
        .map_err(|e| e.to_string())
}
