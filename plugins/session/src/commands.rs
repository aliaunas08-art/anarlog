use tauri::Manager;

use crate::{
    attachments, conflicts, creation, deletion, folder_catalog, folder_materials, move_contents,
    participants, proposals,
};

macro_rules! session_write_command {
    ($name:ident, $module:ident, $function:ident, $request:ty, $result:ty) => {
        #[tauri::command]
        #[specta::specta]
        pub(crate) async fn $name<R: tauri::Runtime>(
            app: tauri::AppHandle<R>,
            request: $request,
        ) -> Result<$result, String> {
            let runtime = app
                .try_state::<tauri_plugin_db::ManagedState>()
                .map(|state| state.inner().clone())
                .ok_or_else(|| "database is not ready yet".to_string())?;
            let _guard = runtime.synced_write_guard().await;
            $module::$function(runtime.pool(), request).await
        }
    };
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn create_session<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    aspc_window: tauri::Window<R>,
    request: creation::CreateSessionRequest,
) -> Result<String, String> {
    anlg_aspc::require_ignoring_lock(
        &app,
        &aspc_window,
        anlg_aspc::DataType::Notes,
        anlg_aspc::Action::Create,
    )?;
    let runtime = db_runtime(&app)?;
    let _guard = runtime.synced_write_guard().await;
    creation::create_session(runtime.pool(), request).await
}
#[tauri::command]
#[specta::specta]
pub(crate) async fn create_session_for_event<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    aspc_window: tauri::Window<R>,
    request: creation::CreateEventSessionRequest,
) -> Result<Option<creation::EventSessionResult>, String> {
    anlg_aspc::require_ignoring_lock(
        &app,
        &aspc_window,
        anlg_aspc::DataType::Notes,
        anlg_aspc::Action::Create,
    )?;
    let runtime = db_runtime(&app)?;
    let _guard = runtime.synced_write_guard().await;
    creation::create_session_for_event(runtime.pool(), request).await
}
#[tauri::command]
#[specta::specta]
pub(crate) async fn soft_delete_session<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    window: tauri::Window<R>,
    request: deletion::TombstoneSessionRequest,
) -> Result<Option<deletion::DeletedSessionRow>, String> {
    anlg_aspc::require(
        &app,
        &window,
        anlg_aspc::DataType::Notes,
        anlg_aspc::Action::Delete,
    )?;
    let runtime = db_runtime(&app)?;
    let _guard = runtime.synced_write_guard().await;
    deletion::soft_delete_session(runtime.pool(), request).await
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn restore_deleted_session<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    window: tauri::Window<R>,
    request: deletion::TombstoneSessionRequest,
) -> Result<deletion::RestoreDeletedSessionOutcome, String> {
    anlg_aspc::require(
        &app,
        &window,
        anlg_aspc::DataType::Notes,
        anlg_aspc::Action::Edit,
    )?;
    let runtime = db_runtime(&app)?;
    let _guard = runtime.synced_write_guard().await;
    deletion::restore_deleted_session(runtime.pool(), request).await
}

/// ASPC D2.2: permanently deletes a soft-deleted meeting once its undo
/// window is over (the webview calls this when the undo toast closes).
/// A meeting that is not soft-deleted is left alone.
#[tauri::command]
#[specta::specta]
pub(crate) async fn purge_deleted_session<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    window: tauri::Window<R>,
    session_id: String,
) -> Result<crate::purge::PurgeReport, String> {
    anlg_aspc::require(
        &app,
        &window,
        anlg_aspc::DataType::Notes,
        anlg_aspc::Action::Delete,
    )?;
    crate::purge::purge_sessions(&app, &[session_id], anlg_audit_log::Source::App).await
}

fn db_runtime<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> Result<tauri_plugin_db::ManagedState, String> {
    app.try_state::<tauri_plugin_db::ManagedState>()
        .map(|state| state.inner().clone())
        .ok_or_else(|| "database is not ready yet".to_string())
}
#[tauri::command]
#[specta::specta]
pub(crate) async fn add_session_participant<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    aspc_window: tauri::Window<R>,
    request: participants::AddSessionParticipantRequest,
) -> Result<(), String> {
    anlg_aspc::require(
        &app,
        &aspc_window,
        anlg_aspc::DataType::Notes,
        anlg_aspc::Action::Edit,
    )?;
    let runtime = db_runtime(&app)?;
    let _guard = runtime.synced_write_guard().await;
    participants::add_session_participant(runtime.pool(), request).await
}
#[tauri::command]
#[specta::specta]
pub(crate) async fn remove_session_participant<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    aspc_window: tauri::Window<R>,
    request: participants::RemoveSessionParticipantRequest,
) -> Result<(), String> {
    anlg_aspc::require(
        &app,
        &aspc_window,
        anlg_aspc::DataType::Notes,
        anlg_aspc::Action::Edit,
    )?;
    let runtime = db_runtime(&app)?;
    let _guard = runtime.synced_write_guard().await;
    participants::remove_session_participant(runtime.pool(), request).await
}
#[tauri::command]
#[specta::specta]
pub(crate) async fn persist_chat_session_proposal<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    aspc_window: tauri::Window<R>,
    request: proposals::PersistChatSessionProposalRequest,
) -> Result<(), String> {
    anlg_aspc::require(
        &app,
        &aspc_window,
        anlg_aspc::DataType::Notes,
        anlg_aspc::Action::Create,
    )?;
    let runtime = db_runtime(&app)?;
    let _guard = runtime.synced_write_guard().await;
    proposals::persist_chat_session_proposal(runtime.pool(), request).await
}
#[tauri::command]
#[specta::specta]
pub(crate) async fn set_session_proposal_status<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    aspc_window: tauri::Window<R>,
    request: proposals::SetSessionProposalStatusRequest,
) -> Result<(), String> {
    anlg_aspc::require(
        &app,
        &aspc_window,
        anlg_aspc::DataType::Notes,
        anlg_aspc::Action::Edit,
    )?;
    let runtime = db_runtime(&app)?;
    let _guard = runtime.synced_write_guard().await;
    proposals::set_session_proposal_status(runtime.pool(), request).await
}
#[tauri::command]
#[specta::specta]
pub(crate) async fn resolve_session_conflicts<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    aspc_window: tauri::Window<R>,
    request: conflicts::ResolveSessionConflictsRequest,
) -> Result<(), String> {
    anlg_aspc::require(
        &app,
        &aspc_window,
        anlg_aspc::DataType::Notes,
        anlg_aspc::Action::Edit,
    )?;
    let runtime = db_runtime(&app)?;
    let _guard = runtime.synced_write_guard().await;
    conflicts::resolve_session_conflicts(runtime.pool(), request).await
}
#[tauri::command]
#[specta::specta]
pub(crate) async fn resolve_session_conflict<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    aspc_window: tauri::Window<R>,
    request: conflicts::ResolveSessionConflictRequest,
) -> Result<(), String> {
    anlg_aspc::require(
        &app,
        &aspc_window,
        anlg_aspc::DataType::Notes,
        anlg_aspc::Action::Edit,
    )?;
    let runtime = db_runtime(&app)?;
    let _guard = runtime.synced_write_guard().await;
    conflicts::resolve_session_conflict(runtime.pool(), request).await
}
#[tauri::command]
#[specta::specta]
pub(crate) async fn move_session_contents<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    aspc_window: tauri::Window<R>,
    request: move_contents::MoveSessionContentsRequest,
) -> Result<(), String> {
    anlg_aspc::require(
        &app,
        &aspc_window,
        anlg_aspc::DataType::Notes,
        anlg_aspc::Action::Edit,
    )?;
    let runtime = db_runtime(&app)?;
    let _guard = runtime.synced_write_guard().await;
    move_contents::move_session_contents(runtime.pool(), request).await
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn catalog_note_attachment<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    aspc_window: tauri::Window<R>,
    request: attachments::CatalogNoteAttachmentRequest,
) -> Result<(), String> {
    anlg_aspc::require_ignoring_lock(
        &app,
        &aspc_window,
        anlg_aspc::DataType::Notes,
        anlg_aspc::Action::Create,
    )?;
    let runtime = db_runtime(&app)?;
    let _guard = runtime.synced_write_guard().await;
    attachments::catalog_note_attachment(runtime.pool(), request).await
}
#[tauri::command]
#[specta::specta]
pub(crate) async fn catalog_session_audio<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    aspc_window: tauri::Window<R>,
    request: attachments::CatalogSessionAudioRequest,
) -> Result<(), String> {
    anlg_aspc::require_ignoring_lock(
        &app,
        &aspc_window,
        anlg_aspc::DataType::Recording,
        anlg_aspc::Action::Create,
    )?;
    let runtime = db_runtime(&app)?;
    let _guard = runtime.synced_write_guard().await;
    attachments::catalog_session_audio(runtime.pool(), request).await
}
session_write_command!(
    mark_session_audio_transcription_complete,
    attachments,
    mark_session_audio_transcription_complete,
    attachments::SessionAudioRequest,
    ()
);
session_write_command!(
    set_attachment_cloud_sync_enabled,
    attachments,
    set_attachment_cloud_sync_enabled,
    attachments::SetAttachmentCloudSyncEnabledRequest,
    ()
);
#[tauri::command]
#[specta::specta]
pub(crate) async fn tombstone_session_audio<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    window: tauri::Window<R>,
    request: attachments::SessionAudioRequest,
) -> Result<(), String> {
    anlg_aspc::require(
        &app,
        &window,
        anlg_aspc::DataType::Recording,
        anlg_aspc::Action::Delete,
    )?;
    let runtime = db_runtime(&app)?;
    let _guard = runtime.synced_write_guard().await;
    attachments::tombstone_session_audio(runtime.pool(), request).await
}
#[tauri::command]
#[specta::specta]
pub(crate) async fn mark_session_audio_absent<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    aspc_window: tauri::Window<R>,
    request: attachments::SessionAudioRequest,
) -> Result<(), String> {
    anlg_aspc::require_ignoring_lock(
        &app,
        &aspc_window,
        anlg_aspc::DataType::Recording,
        anlg_aspc::Action::Edit,
    )?;
    let runtime = db_runtime(&app)?;
    let _guard = runtime.synced_write_guard().await;
    attachments::mark_session_audio_absent(runtime.pool(), request).await
}
#[tauri::command]
#[specta::specta]
pub(crate) async fn catalog_folder_material<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    aspc_window: tauri::Window<R>,
    request: folder_materials::CatalogFolderMaterialRequest,
) -> Result<(), String> {
    anlg_aspc::require(
        &app,
        &aspc_window,
        anlg_aspc::DataType::Notes,
        anlg_aspc::Action::Create,
    )?;
    let runtime = db_runtime(&app)?;
    let _guard = runtime.synced_write_guard().await;
    folder_materials::catalog_folder_material(runtime.pool(), request).await
}
#[tauri::command]
#[specta::specta]
pub(crate) async fn tombstone_folder_material<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    window: tauri::Window<R>,
    request: folder_materials::TombstoneFolderMaterialRequest,
) -> Result<(), String> {
    anlg_aspc::require(
        &app,
        &window,
        anlg_aspc::DataType::Notes,
        anlg_aspc::Action::Delete,
    )?;
    let runtime = db_runtime(&app)?;
    let _guard = runtime.synced_write_guard().await;
    folder_materials::tombstone_folder_material(runtime.pool(), request).await
}
#[tauri::command]
#[specta::specta]
pub(crate) async fn ensure_folder_catalog<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    aspc_window: tauri::Window<R>,
    request: folder_catalog::EnsureFolderCatalogRequest,
) -> Result<(), String> {
    anlg_aspc::require_ignoring_lock(
        &app,
        &aspc_window,
        anlg_aspc::DataType::Notes,
        anlg_aspc::Action::Create,
    )?;
    let runtime = db_runtime(&app)?;
    let _guard = runtime.synced_write_guard().await;
    folder_catalog::ensure_folder_catalog(runtime.pool(), request).await
}
#[tauri::command]
#[specta::specta]
pub(crate) async fn rename_folder_catalog<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    aspc_window: tauri::Window<R>,
    request: folder_catalog::RenameFolderCatalogRequest,
) -> Result<(), String> {
    anlg_aspc::require(
        &app,
        &aspc_window,
        anlg_aspc::DataType::Notes,
        anlg_aspc::Action::Edit,
    )?;
    let runtime = db_runtime(&app)?;
    let _guard = runtime.synced_write_guard().await;
    folder_catalog::rename_folder_catalog(runtime.pool(), request).await
}
#[tauri::command]
#[specta::specta]
pub(crate) async fn delete_folder_catalog<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    window: tauri::Window<R>,
    request: folder_catalog::DeleteFolderCatalogRequest,
) -> Result<(), String> {
    anlg_aspc::require(
        &app,
        &window,
        anlg_aspc::DataType::Notes,
        anlg_aspc::Action::Delete,
    )?;
    let runtime = db_runtime(&app)?;
    let _guard = runtime.synced_write_guard().await;
    folder_catalog::delete_folder_catalog(runtime.pool(), request).await
}
#[tauri::command]
#[specta::specta]
pub(crate) async fn update_folder_instructions<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    aspc_window: tauri::Window<R>,
    request: folder_catalog::UpdateFolderInstructionsRequest,
) -> Result<(), String> {
    anlg_aspc::require(
        &app,
        &aspc_window,
        anlg_aspc::DataType::Notes,
        anlg_aspc::Action::Edit,
    )?;
    let runtime = db_runtime(&app)?;
    let _guard = runtime.synced_write_guard().await;
    folder_catalog::update_folder_instructions(runtime.pool(), request).await
}
#[tauri::command]
#[specta::specta]
pub(crate) async fn update_folder_workspace<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    aspc_window: tauri::Window<R>,
    request: folder_catalog::UpdateFolderWorkspaceRequest,
) -> Result<(), String> {
    anlg_aspc::require(
        &app,
        &aspc_window,
        anlg_aspc::DataType::Notes,
        anlg_aspc::Action::Edit,
    )?;
    let runtime = db_runtime(&app)?;
    let _guard = runtime.synced_write_guard().await;
    folder_catalog::update_folder_workspace(runtime.pool(), request).await
}
#[tauri::command]
#[specta::specta]
pub(crate) async fn update_folder_icon<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    aspc_window: tauri::Window<R>,
    request: folder_catalog::UpdateFolderIconRequest,
) -> Result<(), String> {
    anlg_aspc::require(
        &app,
        &aspc_window,
        anlg_aspc::DataType::Notes,
        anlg_aspc::Action::Edit,
    )?;
    let runtime = db_runtime(&app)?;
    let _guard = runtime.synced_write_guard().await;
    folder_catalog::update_folder_icon(runtime.pool(), request).await
}

/// The folder that holds `app.db` (where a restore is staged and applied).
async fn database_dir(pool: &sqlx::SqlitePool) -> Result<std::path::PathBuf, String> {
    let file: String =
        sqlx::query_scalar("SELECT file FROM pragma_database_list WHERE name = 'main'")
            .fetch_one(pool)
            .await
            .map_err(|e| e.to_string())?;
    std::path::Path::new(&file)
        .parent()
        .filter(|_| !file.is_empty())
        .map(std::path::Path::to_path_buf)
        .ok_or_else(|| "backup.no_database_file".to_string())
}

fn vault_sessions_dir<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Option<std::path::PathBuf> {
    use tauri_plugin_settings::SettingsPluginExt;
    app.settings()
        .vault_base()
        .ok()
        .map(|base| base.join("sessions").into_std_path_buf())
}

/// A native OK/Cancel dialog shown by Rust, so a webview can't confirm for
/// the user (ASPC C5).
async fn confirm<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    title: &str,
    message: &str,
    ok: &str,
) -> bool {
    use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .message(message)
        .title(title)
        .kind(MessageDialogKind::Warning)
        .buttons(MessageDialogButtons::OkCancelCustom(
            ok.to_string(),
            "Cancel".to_string(),
        ))
        .show(move |confirmed| {
            let _ = tx.send(confirmed);
        });
    rx.await.unwrap_or(false)
}

/// ASPC C5: writes a backup of the library to `path`, which the user just
/// picked in `fs2:pick_save_path` (A3.6, single use). Rust asks the user to
/// confirm that the backup is not encrypted.
#[tauri::command]
#[specta::specta]
pub(crate) async fn backup_library<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    window: tauri::Window<R>,
    path: String,
) -> Result<crate::backup::BackupSummary, String> {
    use tauri_plugin_fs2::Fs2PluginExt;
    anlg_aspc::require(
        &app,
        &window,
        anlg_aspc::DataType::Notes,
        anlg_aspc::Action::Export,
    )?;
    let path = app
        .fs2()
        .authorize_write(std::path::Path::new(&path))
        .map_err(|e| e.to_string())?;
    let runtime = db_runtime(&app)?;
    if !confirm(
        &app,
        "Back up library",
        "This backup is not encrypted. Anyone who can open the file can read your meetings, transcripts and recordings. Keep it somewhere only you can reach.",
        "Back Up",
    )
    .await
    {
        return Err("backup.cancelled".into());
    }
    let summary = crate::backup::write_backup(
        runtime.pool(),
        vault_sessions_dir(&app).as_deref(),
        &path,
        &app.package_info().version.to_string(),
    )
    .await?;
    anlg_audit_log::record(
        &app,
        anlg_audit_log::NewEntry::new(anlg_audit_log::actions::BACKUP_CREATED, "library", "")
            .flag("encrypted", false)
            .count("meetings", summary.meetings)
            .count("files", summary.files as i64),
    );
    Ok(summary)
}

/// ASPC C5: checks the backup at `path` (picked by the user in
/// `fs2:pick_open_paths`, single use) and, after a confirmation shown by
/// Rust, stages it. It replaces the library on the next launch; the current
/// library is kept beside it.
#[tauri::command]
#[specta::specta]
pub(crate) async fn restore_library<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    window: tauri::Window<R>,
    path: String,
) -> Result<crate::backup::RestoreSummary, String> {
    use tauri_plugin_fs2::Fs2PluginExt;
    anlg_aspc::require(
        &app,
        &window,
        anlg_aspc::DataType::Settings,
        anlg_aspc::Action::Administer,
    )?;
    let path = app
        .fs2()
        .authorize_read(std::path::Path::new(&path))
        .map_err(|e| e.to_string())?;
    let runtime = db_runtime(&app)?;
    let data_dir = database_dir(runtime.pool()).await?;
    if !confirm(
        &app,
        "Restore library",
        "Replace your library with this backup? Your current meetings are not deleted: they are kept in the app's data folder, renamed with \".pre-restore\". The restore finishes when the app restarts.",
        "Restore",
    )
    .await
    {
        return Err("backup.cancelled".into());
    }
    let summary =
        crate::backup::stage_restore(&path, &data_dir, vault_sessions_dir(&app).as_deref()).await?;
    anlg_audit_log::record(
        &app,
        anlg_audit_log::NewEntry::new(anlg_audit_log::actions::BACKUP_RESTORED, "library", "")
            .flag("staged", true)
            .count("meetings", summary.meetings)
            .count("files", summary.files as i64),
    );
    Ok(summary)
}
