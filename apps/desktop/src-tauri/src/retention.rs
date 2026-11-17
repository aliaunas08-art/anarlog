//! ASPC D2.2 sweep: at startup, permanently deletes every meeting that was
//! soft-deleted and whose undo window is over: one deleted just before a
//! quit (the delete happens on the next launch; it is never restored), and
//! deletions that arrived from other devices.

use tauri::AppHandle;

pub fn spawn(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        crate::search_index::wait_for_tantivy(&app).await;
        if !crate::search_index::wait_for_database_ready(&app).await {
            return;
        }
        match tauri_plugin_session::purge::purge_expired_soft_deletes(
            &app,
            chrono::Utc::now(),
            anlg_audit_log::Source::Job,
        )
        .await
        {
            Ok(report) if report.purged > 0 => {
                tracing::info!(purged = report.purged, "deleted_meetings_purged");
            }
            Ok(_) => {}
            Err(error) => tracing::error!(%error, "deleted_meetings_purge_failed"),
        }
    });
}
