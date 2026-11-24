use crate::AgentPluginExt;

#[tauri::command]
#[specta::specta]
pub fn health_check<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
) -> Result<anlg_agent_core::HealthCheckResponse, String> {
    Ok(app.agent().health_check())
}

#[tauri::command]
#[specta::specta]
pub fn install_cli<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    aspc_window: tauri::Window<R>,
    payload: anlg_agent_core::InstallCliRequest,
) -> Result<anlg_agent_core::InstallCliResponse, String> {
    anlg_aspc::require(
        &aspc_window,
        &aspc_window,
        anlg_aspc::DataType::Settings,
        anlg_aspc::Action::Administer,
    )
    .map_err(|denied| denied.to_string())?;
    app.agent().install_cli(payload)
}

#[tauri::command]
#[specta::specta]
pub fn uninstall_cli<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    aspc_window: tauri::Window<R>,
    payload: anlg_agent_core::UninstallCliRequest,
) -> Result<anlg_agent_core::UninstallCliResponse, String> {
    anlg_aspc::require(
        &aspc_window,
        &aspc_window,
        anlg_aspc::DataType::Settings,
        anlg_aspc::Action::Administer,
    )
    .map_err(|denied| denied.to_string())?;
    app.agent().uninstall_cli(payload)
}
