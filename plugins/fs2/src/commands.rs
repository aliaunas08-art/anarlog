use std::path::{Path, PathBuf};

use crate::Fs2PluginExt;

#[tauri::command]
#[specta::specta]
pub(crate) async fn read_text_file<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    path: String,
) -> Result<String, String> {
    let path = PathBuf::from(path);
    app.fs2().read_text_file(&path).map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn remove<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    path: String,
) -> Result<(), String> {
    let path = PathBuf::from(path);
    app.fs2().remove(&path).map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn write_text_file<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    path: PathBuf,
    content: String,
) -> Result<(), String> {
    app.fs2()
        .write_text_file(&path, &content)
        .map_err(|e| e.to_string())
}

#[derive(Debug, Clone, serde::Deserialize, specta::Type)]
pub(crate) struct DialogFilter {
    name: String,
    extensions: Vec<String>,
}

fn apply_filters<R: tauri::Runtime>(
    mut builder: tauri_plugin_dialog::FileDialogBuilder<R>,
    filters: &[DialogFilter],
) -> tauri_plugin_dialog::FileDialogBuilder<R> {
    for filter in filters {
        let extensions: Vec<&str> = filter.extensions.iter().map(String::as_str).collect();
        builder = builder.add_filter(&filter.name, &extensions);
    }
    builder
}

/// Opens a native save dialog and returns the picked path, which
/// `write_text_file` and `export:export` then accept once (ASPC A3.6).
#[tauri::command]
#[specta::specta]
pub(crate) async fn pick_save_path<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    window: tauri::WebviewWindow<R>,
    default_path: Option<String>,
    filters: Vec<DialogFilter>,
) -> Result<Option<String>, String> {
    use tauri::Manager;
    use tauri_plugin_dialog::DialogExt;

    let mut builder = apply_filters(app.dialog().file().set_parent(&window), &filters);
    if let Some(default_path) = default_path.map(PathBuf::from) {
        if let Some(directory) = default_path.parent().filter(|dir| dir.is_dir()) {
            builder = builder.set_directory(directory);
        }
        if let Some(name) = default_path.file_name().and_then(|name| name.to_str()) {
            builder = builder.set_file_name(name);
        }
    }

    let (tx, rx) = tokio::sync::oneshot::channel();
    builder.save_file(move |picked| {
        let _ = tx.send(picked.and_then(|path| path.as_path().map(Path::to_path_buf)));
    });
    let Some(picked) = rx.await.map_err(|e| e.to_string())? else {
        return Ok(None);
    };
    let grants = app.state::<crate::grants::PathGrants>();
    grants
        .record(crate::grants::GrantKind::Write, &picked)
        .map_err(|e| e.to_string())?;
    Ok(Some(picked.to_string_lossy().into_owned()))
}

/// Opens a native file picker and returns the picked files, which
/// `importer:read_text_files` and `fs-sync:audio_import` then accept once.
#[tauri::command]
#[specta::specta]
pub(crate) async fn pick_open_paths<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    window: tauri::WebviewWindow<R>,
    title: Option<String>,
    multiple: bool,
    filters: Vec<DialogFilter>,
) -> Result<Vec<String>, String> {
    use tauri::Manager;
    use tauri_plugin_dialog::DialogExt;

    let mut builder = apply_filters(app.dialog().file().set_parent(&window), &filters);
    if let Some(title) = title {
        builder = builder.set_title(title);
    }
    let (tx, rx) = tokio::sync::oneshot::channel();
    let to_paths = |paths: Vec<tauri_plugin_dialog::FilePath>| -> Vec<PathBuf> {
        paths
            .into_iter()
            .filter_map(|path| path.as_path().map(Path::to_path_buf))
            .collect()
    };
    if multiple {
        builder.pick_files(move |picked| {
            let _ = tx.send(picked.map(to_paths).unwrap_or_default());
        });
    } else {
        builder.pick_file(move |picked| {
            let _ = tx.send(picked.map(|path| to_paths(vec![path])).unwrap_or_default());
        });
    }
    let picked = rx.await.map_err(|e| e.to_string())?;
    let grants = app.state::<crate::grants::PathGrants>();
    let mut paths = Vec::with_capacity(picked.len());
    for path in picked {
        grants
            .record(crate::grants::GrantKind::Read, &path)
            .map_err(|e| e.to_string())?;
        paths.push(path.to_string_lossy().into_owned());
    }
    Ok(paths)
}
