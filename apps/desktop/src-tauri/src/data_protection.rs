//! ASPC A4.1 / C2.1 at startup: tightens the app data directory's
//! permissions and checks FileVault, logging only paths and modes.

use std::sync::OnceLock;

use anlg_aspc::protection::{self, FileVault};

static FILEVAULT: OnceLock<FileVault> = OnceLock::new();

fn filevault() -> FileVault {
    *FILEVAULT.get_or_init(protection::filevault_status)
}

/// Runs off the main thread: the `fdesetup` call and the directory walk.
pub fn spawn(identifier: String) {
    std::thread::spawn(move || {
        if let Some(dir) = crate::db::desktop_db_dir(&identifier) {
            match protection::harden_app_data(&dir) {
                Ok(changes) => {
                    for change in changes {
                        tracing::warn!(
                            path = %change.path.display(),
                            from = format!("{:o}", change.from),
                            to = format!("{:o}", change.to),
                            "app_data_permissions_tightened"
                        );
                    }
                }
                Err(error) => tracing::error!(%error, "app_data_permissions_check_failed"),
            }
        }
        match filevault() {
            FileVault::On => tracing::info!("filevault_on"),
            FileVault::Off => tracing::warn!("filevault_off"),
            FileVault::Unknown => tracing::warn!("filevault_status_unknown"),
        }
    });
}

#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct DataProtectionStatus {
    /// `on`, `off` or `unknown`.
    pub filevault: String,
}

/// Whether FileVault is on, for the in-app warning. A device fact; no user
/// data (registry: `Public`).
#[tauri::command]
#[specta::specta]
pub async fn get_data_protection_status() -> DataProtectionStatus {
    let status = tauri::async_runtime::spawn_blocking(filevault)
        .await
        .unwrap_or(FileVault::Unknown);
    DataProtectionStatus {
        filevault: match status {
            FileVault::On => "on",
            FileVault::Off => "off",
            FileVault::Unknown => "unknown",
        }
        .to_string(),
    }
}

/// Where the library lives on this Mac, for Settings → Where your data
/// lives (ASPC D4.2). Read from the same places the app uses, so the screen
/// can't drift from them. Paths only; nothing is read from the files.
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct DataLocations {
    pub data_dir: Option<String>,
    pub database: Option<String>,
    pub audit_log: Option<String>,
    pub vault: Option<String>,
    pub sessions: Option<String>,
    pub search_index: Option<String>,
    pub logs: Option<String>,
}

#[tauri::command]
#[specta::specta]
pub async fn get_data_locations<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    window: tauri::Window<R>,
) -> Result<DataLocations, String> {
    use tauri::Manager;
    use tauri_plugin_settings::SettingsPluginExt;

    anlg_aspc::require(
        &app,
        &window,
        anlg_aspc::DataType::Settings,
        anlg_aspc::Action::Read,
    )?;
    let show = |path: std::path::PathBuf| path.display().to_string();
    let data_dir = crate::db::desktop_db_dir(&app.config().identifier);
    let vault = app
        .settings()
        .vault_base()
        .ok()
        .map(|base| base.into_std_path_buf());
    Ok(DataLocations {
        database: data_dir.as_ref().map(|dir| show(dir.join("app.db"))),
        audit_log: data_dir
            .as_ref()
            .map(|dir| show(dir.join(anlg_audit_log::AUDIT_DB_FILENAME))),
        data_dir: data_dir.map(show),
        sessions: vault.as_ref().map(|dir| show(dir.join("sessions"))),
        search_index: vault.as_ref().map(|dir| show(dir.join("search_index"))),
        vault: vault.map(show),
        logs: app.path().app_log_dir().ok().map(show),
    })
}
