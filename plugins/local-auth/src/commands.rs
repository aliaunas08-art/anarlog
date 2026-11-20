use tauri::Runtime;

use crate::LocalAuthPluginExt;

#[tauri::command]
#[specta::specta]
pub async fn available<R: Runtime>(app: tauri::AppHandle<R>) -> Result<bool, String> {
    let handle = app.clone();
    tauri::async_runtime::spawn_blocking(move || handle.local_auth().available())
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())
}

/// A one-off device authentication (note locks, settings changes in the
/// UI). It never changes the app lock: only `unlock_app` does.
#[tauri::command]
#[specta::specta]
pub async fn authenticate<R: Runtime>(
    app: tauri::AppHandle<R>,
    reason: String,
) -> Result<bool, String> {
    let handle = app.clone();
    let result =
        tauri::async_runtime::spawn_blocking(move || handle.local_auth().authenticate(&reason))
            .await
            .map_err(|error| error.to_string())?;
    match result {
        Ok(value) => Ok(value),
        Err(crate::Error::Unavailable) => Ok(false),
        Err(error) => Err(error.to_string()),
    }
}

#[derive(Debug, Clone, serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AccessContextInfo {
    pub actor_id: String,
    pub role: Option<String>,
    pub workspace_id: String,
    pub device_id: String,
    pub window: String,
    pub unlocked: bool,
}

/// The non-secret fields of the caller's access context (A1.3).
#[tauri::command]
#[specta::specta]
pub async fn access_context<R: Runtime>(
    app: tauri::AppHandle<R>,
    window: tauri::Window<R>,
) -> Result<AccessContextInfo, String> {
    let state = crate::app_lock::aspc_state(&app)?;
    let ctx = state.context(Some(window.label()));
    Ok(AccessContextInfo {
        actor_id: ctx.actor_id,
        role: ctx.role.map(|role| role.as_str().to_string()),
        workspace_id: ctx.workspace_id,
        device_id: ctx.device_id,
        window: window.label().to_string(),
        unlocked: ctx.unlocked,
    })
}

#[derive(Debug, Clone, serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AppLockState {
    pub enabled: bool,
    pub unlocked: bool,
}

#[derive(Debug, Clone, serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AppLockSettings {
    /// Minutes without input before the app relocks; 0 is off.
    pub idle_minutes: u32,
    pub idle_minute_choices: Vec<u32>,
}

impl From<anlg_aspc::LockSnapshot> for AppLockState {
    fn from(value: anlg_aspc::LockSnapshot) -> Self {
        Self {
            enabled: value.enabled,
            unlocked: value.unlocked,
        }
    }
}

#[tauri::command]
#[specta::specta]
pub async fn app_lock_state<R: Runtime>(app: tauri::AppHandle<R>) -> Result<AppLockState, String> {
    Ok(crate::app_lock::aspc_state(&app)?.lock_snapshot().into())
}

/// Runs device authentication in Rust and unlocks only on success (A4.2).
/// Errors with `aspc.auth_unavailable` if authentication is unavailable;
/// the app then stays locked (A4.3).
#[tauri::command]
#[specta::specta]
pub async fn unlock_app<R: Runtime>(app: tauri::AppHandle<R>) -> Result<AppLockState, String> {
    crate::app_lock::unlock(&app).await.map(Into::into)
}

#[tauri::command]
#[specta::specta]
pub async fn lock_app<R: Runtime>(app: tauri::AppHandle<R>) -> Result<AppLockState, String> {
    let state = crate::app_lock::aspc_state(&app)?;
    state.lock();
    Ok(state.lock_snapshot().into())
}

/// Turns the app lock on or off after a device authentication run here.
#[tauri::command]
#[specta::specta]
pub async fn set_app_lock_enabled<R: Runtime>(
    app: tauri::AppHandle<R>,
    window: tauri::Window<R>,
    enabled: bool,
) -> Result<AppLockState, String> {
    anlg_aspc::require(
        &app,
        &window,
        anlg_aspc::DataType::Settings,
        anlg_aspc::Action::Administer,
    )?;
    crate::app_lock::set_enabled(&app, enabled)
        .await
        .map(Into::into)
}

/// The idle relock setting, for the settings screen.
#[tauri::command]
#[specta::specta]
pub async fn app_lock_settings<R: Runtime>(
    app: tauri::AppHandle<R>,
    window: tauri::Window<R>,
) -> Result<AppLockSettings, String> {
    anlg_aspc::require(
        &app,
        &window,
        anlg_aspc::DataType::Settings,
        anlg_aspc::Action::Read,
    )?;
    use tauri::Manager;
    Ok(AppLockSettings {
        idle_minutes: app.state::<crate::app_lock::LockRuntime>().idle_minutes(),
        idle_minute_choices: crate::access::IDLE_MINUTE_CHOICES.to_vec(),
    })
}

/// Sets the idle relock time after a device authentication run here.
/// Returns the value now in force (unchanged if the prompt was cancelled).
#[tauri::command]
#[specta::specta]
pub async fn set_app_lock_idle_minutes<R: Runtime>(
    app: tauri::AppHandle<R>,
    window: tauri::Window<R>,
    minutes: u32,
) -> Result<u32, String> {
    anlg_aspc::require(
        &app,
        &window,
        anlg_aspc::DataType::Settings,
        anlg_aspc::Action::Administer,
    )?;
    crate::app_lock::set_idle_minutes(&app, minutes).await
}
