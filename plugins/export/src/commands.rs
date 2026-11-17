use std::path::PathBuf;

use crate::ExportPluginExt;

#[tauri::command]
#[specta::specta]
pub(crate) async fn export<R: tauri::Runtime>(
    aspc_window: tauri::Window<R>,
    app: tauri::AppHandle<R>,
    path: PathBuf,
    input: crate::ExportInput,
) -> Result<(), String> {
    anlg_aspc::require(
        &app,
        &aspc_window,
        anlg_aspc::DataType::Notes,
        anlg_aspc::Action::Export,
    )
    .map_err(|denied| denied.to_string())?;
    use tauri_plugin_fs2::Fs2PluginExt;

    // ASPC A3.6: only the app's roots or a path just picked in a save dialog.
    let path = app
        .fs2()
        .authorize_write(&path)
        .map_err(|e| e.to_string())?;
    app.export()
        .export_pdf(&path, input)
        .map_err(|e| e.to_string())?;
    anlg_audit_log::record(
        &app,
        anlg_audit_log::NewEntry::new(anlg_audit_log::actions::MEETING_EXPORTED, "meeting", "")
            .id("format", "pdf"),
    );
    Ok(())
}
