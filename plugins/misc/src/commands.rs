use crate::MiscPluginExt;

#[tauri::command]
#[specta::specta]
pub async fn get_git_hash<R: tauri::Runtime>(app: tauri::AppHandle<R>) -> Result<String, String> {
    Ok(app.misc().get_git_hash())
}

#[tauri::command]
#[specta::specta]
pub async fn get_fingerprint<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
) -> Result<String, String> {
    Ok(app.misc().get_fingerprint())
}

#[tauri::command]
#[specta::specta]
pub async fn get_device_info<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    locale: Option<String>,
) -> Result<crate::DeviceInfo, String> {
    Ok(app.misc().get_device_info(locale))
}

#[tauri::command]
#[specta::specta]
pub async fn get_process_memory_bytes<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
) -> Result<u64, String> {
    app.misc().get_process_memory_bytes()
}

#[tauri::command]
#[specta::specta]
pub async fn opinionated_md_to_html<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    text: String,
) -> Result<String, String> {
    app.misc().opinionated_md_to_html(&text)
}

/// Asks the user, in a native dialog, to allow an extra host for the `http`
/// plugin (a custom provider base URL). Returns whether it is allowed.
#[tauri::command]
#[specta::specta]
pub async fn allow_http_origin<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    aspc_window: tauri::Window<R>,
    window: tauri::WebviewWindow<R>,
    url: String,
) -> Result<bool, String> {
    anlg_aspc::require_ignoring_lock(
        &aspc_window,
        &aspc_window,
        anlg_aspc::DataType::Settings,
        anlg_aspc::Action::Administer,
    )
    .map_err(|denied| denied.to_string())?;
    crate::http_origins::request(&app, &window, &url).await
}
