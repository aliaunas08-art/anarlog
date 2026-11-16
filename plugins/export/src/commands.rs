use std::path::PathBuf;

use crate::ExportPluginExt;

#[tauri::command]
#[specta::specta]
pub(crate) async fn export<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    path: PathBuf,
    input: crate::ExportInput,
) -> Result<(), String> {
    use tauri_plugin_fs2::Fs2PluginExt;

    // ASPC A3.6: only the app's roots or a path just picked in a save dialog.
    let path = app
        .fs2()
        .authorize_write(&path)
        .map_err(|e| e.to_string())?;
    app.export()
        .export_pdf(&path, input)
        .map_err(|e| e.to_string())
}
