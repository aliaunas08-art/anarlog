//! ASPC D2.2 sweep and D3 retention, at startup and every 6 hours.
//!
//! - Sweep: permanently deletes every meeting that was soft-deleted and whose
//!   undo window is over: one deleted just before a quit (the delete happens
//!   on the next launch; it is never restored), and deletions that arrived
//!   from other devices.
//! - Retention: "Delete meetings older than N days" through the same delete
//!   path, skipping meetings being recorded or finalized. Each run with
//!   retention on writes `retention.run` with its count.

use std::collections::HashSet;

use tauri::AppHandle;
use tauri_plugin_session::retention::RETENTION_INTERVAL;

pub fn spawn(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        crate::search_index::wait_for_tantivy(&app).await;
        if !crate::search_index::wait_for_database_ready(&app).await {
            return;
        }
        let mut interval = tokio::time::interval(RETENTION_INTERVAL);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            interval.tick().await;
            run_once(&app).await;
        }
    });
}

async fn run_once(app: &AppHandle) {
    let now = chrono::Utc::now();
    match tauri_plugin_session::purge::purge_expired_soft_deletes(
        app,
        now,
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

    let Some(recording) = recording_sessions(app).await else {
        tracing::warn!("retention_skipped_capture_state_unknown");
        return;
    };
    match tauri_plugin_session::retention::apply_retention(app, now, &recording).await {
        Ok(run) if run.deleted > 0 => {
            tracing::info!(deleted = run.deleted, "retention_deleted_meetings");
        }
        Ok(_) => {}
        Err(error) => tracing::error!(%error, "retention_run_failed"),
    }
}

/// The meetings being captured or finalized. `None` when the capture state
/// can't be read: retention then waits for the next run rather than risk
/// deleting a meeting that is being recorded.
async fn recording_sessions(app: &AppHandle) -> Option<HashSet<String>> {
    use tauri_plugin_transcription::ListenerPluginExt;

    let snapshot = app.listener().get_capture_snapshot().await.ok()?;
    let mut ids: HashSet<String> = snapshot.finalizing_session_ids.into_iter().collect();
    ids.extend(snapshot.active_session_id);
    ids.extend(snapshot.live_segments_session_id);
    Some(ids)
}
