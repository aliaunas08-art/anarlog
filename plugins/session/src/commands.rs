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

session_write_command!(
    create_session,
    creation,
    create_session,
    creation::CreateSessionRequest,
    String
);
session_write_command!(
    create_session_for_event,
    creation,
    create_session_for_event,
    creation::CreateEventSessionRequest,
    Option<creation::EventSessionResult>
);
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
session_write_command!(
    add_session_participant,
    participants,
    add_session_participant,
    participants::AddSessionParticipantRequest,
    ()
);
session_write_command!(
    remove_session_participant,
    participants,
    remove_session_participant,
    participants::RemoveSessionParticipantRequest,
    ()
);
session_write_command!(
    persist_chat_session_proposal,
    proposals,
    persist_chat_session_proposal,
    proposals::PersistChatSessionProposalRequest,
    ()
);
session_write_command!(
    set_session_proposal_status,
    proposals,
    set_session_proposal_status,
    proposals::SetSessionProposalStatusRequest,
    ()
);
session_write_command!(
    resolve_session_conflicts,
    conflicts,
    resolve_session_conflicts,
    conflicts::ResolveSessionConflictsRequest,
    ()
);
session_write_command!(
    resolve_session_conflict,
    conflicts,
    resolve_session_conflict,
    conflicts::ResolveSessionConflictRequest,
    ()
);
session_write_command!(
    move_session_contents,
    move_contents,
    move_session_contents,
    move_contents::MoveSessionContentsRequest,
    ()
);

session_write_command!(
    catalog_note_attachment,
    attachments,
    catalog_note_attachment,
    attachments::CatalogNoteAttachmentRequest,
    ()
);
session_write_command!(
    catalog_session_audio,
    attachments,
    catalog_session_audio,
    attachments::CatalogSessionAudioRequest,
    ()
);
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
session_write_command!(
    tombstone_session_audio,
    attachments,
    tombstone_session_audio,
    attachments::SessionAudioRequest,
    ()
);
session_write_command!(
    mark_session_audio_absent,
    attachments,
    mark_session_audio_absent,
    attachments::SessionAudioRequest,
    ()
);
session_write_command!(
    catalog_folder_material,
    folder_materials,
    catalog_folder_material,
    folder_materials::CatalogFolderMaterialRequest,
    ()
);
session_write_command!(
    tombstone_folder_material,
    folder_materials,
    tombstone_folder_material,
    folder_materials::TombstoneFolderMaterialRequest,
    ()
);
session_write_command!(
    ensure_folder_catalog,
    folder_catalog,
    ensure_folder_catalog,
    folder_catalog::EnsureFolderCatalogRequest,
    ()
);
session_write_command!(
    rename_folder_catalog,
    folder_catalog,
    rename_folder_catalog,
    folder_catalog::RenameFolderCatalogRequest,
    ()
);
session_write_command!(
    delete_folder_catalog,
    folder_catalog,
    delete_folder_catalog,
    folder_catalog::DeleteFolderCatalogRequest,
    ()
);
session_write_command!(
    update_folder_instructions,
    folder_catalog,
    update_folder_instructions,
    folder_catalog::UpdateFolderInstructionsRequest,
    ()
);
session_write_command!(
    update_folder_workspace,
    folder_catalog,
    update_folder_workspace,
    folder_catalog::UpdateFolderWorkspaceRequest,
    ()
);
session_write_command!(
    update_folder_icon,
    folder_catalog,
    update_folder_icon,
    folder_catalog::UpdateFolderIconRequest,
    ()
);
