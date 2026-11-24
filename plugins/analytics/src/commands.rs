use crate::AnalyticsPluginExt;

#[tauri::command]
#[specta::specta]
pub(crate) fn event_fire_and_forget<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    payload: anlg_analytics::AnalyticsPayload,
) {
    app.analytics().event_fire_and_forget(payload);
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn event<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    payload: anlg_analytics::AnalyticsPayload,
) -> Result<(), String> {
    app.analytics()
        .event(payload)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn set_properties<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    payload: anlg_analytics::PropertiesPayload,
) -> Result<(), String> {
    app.analytics()
        .set_properties(payload)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn set_disabled<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    aspc_window: tauri::Window<R>,
    disabled: bool,
) -> Result<(), String> {
    anlg_aspc::require(
        &aspc_window,
        &aspc_window,
        anlg_aspc::DataType::Settings,
        anlg_aspc::Action::Edit,
    )
    .map_err(|denied| denied.to_string())?;
    app.analytics()
        .set_disabled(disabled)
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn is_disabled<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
) -> Result<bool, String> {
    app.analytics().is_disabled().map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn identify<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    user_id: String,
    payload: anlg_analytics::PropertiesPayload,
) -> Result<(), String> {
    app.analytics()
        .identify(user_id, payload)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub(crate) fn clear_groups<R: tauri::Runtime>(app: tauri::AppHandle<R>) {
    app.analytics().clear_groups();
}
