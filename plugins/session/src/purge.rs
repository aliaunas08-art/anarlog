//! ASPC D2.2 permanent delete, after the 5-second undo window.
//!
//! The webview soft-deletes a meeting at once (undo stays possible), and
//! when the undo window closes it calls `session:purge_deleted_session`. A
//! sweep at startup (and in the retention job) finishes any purge a quit
//! interrupted and applies deletions that arrived from other devices: a
//! meeting deleted just before quitting is permanently deleted on the next
//! launch, never restored.
//!
//! Per meeting, in one transaction on a connection with `secure_delete` on:
//! every row that refers to it (or, for a synced meeting, blank tombstones;
//! see `anlg_db_app::purge_deleted_session`). Then a WAL checkpoint
//! (TRUNCATE), its vault folder (audio, recovery chunks, attachments), its
//! cache files, and its search documents (segments rewritten).

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use specta::Type;
use sqlx::SqlitePool;

/// The undo window the webview shows, plus margin: the sweep never purges a
/// meeting deleted more recently than this.
pub const UNDO_WINDOW_MARGIN: Duration = Duration::from_secs(60);

const CHECKPOINT_ATTEMPTS: u32 = 5;
const CHECKPOINT_RETRY: Duration = Duration::from_millis(200);
const CACHE_SCAN_DEPTH: usize = 4;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct PurgeReport {
    /// Meetings whose rows were removed (or blanked) by this run.
    pub purged: u32,
    /// Of those, the ones kept as blank sync tombstones.
    pub tombstones: u32,
    pub rows_deleted: u32,
    pub files_removed: u32,
    /// Whether the WAL was checkpointed and truncated.
    pub wal_truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PurgedSession {
    pub id: String,
    pub purge: anlg_db_app::SessionPurge,
}

/// Database part: one transaction per meeting with `secure_delete` on, then
/// a TRUNCATE checkpoint. Returns the meetings that were soft-deleted (the
/// others are skipped: still alive, inside the undo window, or unknown).
pub async fn purge_sessions_in_db(
    pool: &SqlitePool,
    ids: &[String],
) -> Result<(Vec<PurgedSession>, bool), String> {
    if ids.is_empty() {
        return Ok((Vec::new(), false));
    }
    let mut connection = pool.acquire().await.map_err(|e| e.to_string())?;
    let previous: i64 = sqlx::query_scalar("PRAGMA secure_delete")
        .fetch_one(&mut *connection)
        .await
        .map_err(|e| e.to_string())?;
    sqlx::query("PRAGMA secure_delete = ON")
        .execute(&mut *connection)
        .await
        .map_err(|e| e.to_string())?;

    let result = async {
        let mut purged = Vec::new();
        let mut plan = None;
        for id in ids {
            let mut tx = sqlx::Connection::begin_with(&mut *connection, "BEGIN IMMEDIATE")
                .await
                .map_err(|e| e.to_string())?;
            if plan.is_none() {
                plan = Some(
                    anlg_db_app::PurgePlan::load(&mut tx)
                        .await
                        .map_err(|e| e.to_string())?,
                );
            }
            let outcome = anlg_db_app::purge_deleted_session_with(
                &mut tx,
                id,
                plan.as_ref().expect("loaded above"),
            )
            .await
            .map_err(|e| e.to_string())?;
            tx.commit().await.map_err(|e| e.to_string())?;
            if let Some(purge) = outcome {
                purged.push(PurgedSession {
                    id: id.clone(),
                    purge,
                });
            }
        }
        let changed = purged.iter().any(|p| p.purge.changed());
        let truncated = if changed {
            checkpoint(&mut connection).await
        } else {
            false
        };
        Ok::<_, String>((purged, truncated))
    }
    .await;

    let restore = if previous == 0 {
        "PRAGMA secure_delete = OFF"
    } else {
        "PRAGMA secure_delete = ON"
    };
    let _ = sqlx::query(restore).execute(&mut *connection).await;
    result
}

/// `PRAGMA wal_checkpoint(TRUNCATE)`, retried while readers hold the WAL.
async fn checkpoint(connection: &mut sqlx::SqliteConnection) -> bool {
    for attempt in 0..CHECKPOINT_ATTEMPTS {
        match sqlx::query_as::<_, (i64, i64, i64)>("PRAGMA wal_checkpoint(TRUNCATE)")
            .fetch_one(&mut *connection)
            .await
        {
            Ok((0, _, _)) => return true,
            Ok(_) => {}
            Err(error) => {
                tracing::warn!(%error, "purge_wal_checkpoint_failed");
                return false;
            }
        }
        if attempt + 1 < CHECKPOINT_ATTEMPTS {
            tokio::time::sleep(CHECKPOINT_RETRY).await;
        }
    }
    tracing::warn!("purge_wal_checkpoint_busy");
    false
}

/// Removes every vault folder named after one of the meetings under
/// `sessions_dir` (audio, `audio-recovery/` chunks and attachments live
/// there), and every cache entry whose name contains one of the ids (the
/// waveform cache `audio-peaks/<id>.json`). The trees are walked once.
pub fn purge_session_files(
    sessions_dir: Option<&Path>,
    cache_dir: Option<&Path>,
    ids: &HashSet<String>,
) -> u32 {
    if ids.is_empty() {
        return 0;
    }
    let mut removed = 0;
    if let Some(dir) = sessions_dir {
        let mut found = Vec::new();
        find_named_dirs(dir, ids, &mut found);
        for path in found {
            match std::fs::remove_dir_all(&path) {
                Ok(()) => removed += 1,
                Err(error) => tracing::warn!(%error, "purge_session_dir_failed"),
            }
        }
    }
    if let Some(dir) = cache_dir {
        let mut found = Vec::new();
        find_cache_entries(dir, ids, CACHE_SCAN_DEPTH, &mut found);
        for path in found {
            let result = if path.is_dir() {
                std::fs::remove_dir_all(&path)
            } else {
                std::fs::remove_file(&path)
            };
            match result {
                Ok(()) => removed += 1,
                Err(error) => tracing::warn!(%error, "purge_cache_entry_failed"),
            }
        }
    }
    removed
}

fn find_named_dirs(dir: &Path, ids: &HashSet<String>, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        // Never follow symlinks out of the vault.
        if !file_type.is_dir() {
            continue;
        }
        let path = entry.path();
        if entry
            .file_name()
            .to_str()
            .is_some_and(|name| ids.contains(name))
        {
            found.push(path);
        } else {
            find_named_dirs(&path, ids, found);
        }
    }
}

fn find_cache_entries(dir: &Path, ids: &HashSet<String>, depth: usize, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_symlink() {
            continue;
        }
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if ids.iter().any(|id| name.contains(id.as_str())) {
            found.push(entry.path());
        } else if file_type.is_dir() && depth > 0 {
            find_cache_entries(&entry.path(), ids, depth - 1, found);
        }
    }
}

/// Session ids are UUID-like; anything else is refused so a crafted id can
/// never match a broad file name.
pub fn is_purgeable_id(id: &str) -> bool {
    (8..=128).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

#[cfg(test)]
mod tests;

/// Purges these soft-deleted meetings everywhere (database, search index,
/// vault, caches) and records each one in the audit log (ids and counts
/// only). Meetings that are not soft-deleted, or whose id is malformed, are
/// skipped.
pub async fn purge_sessions<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    ids: &[String],
    source: anlg_audit_log::Source,
) -> Result<PurgeReport, String> {
    use tauri::Manager;
    use tauri_plugin_settings::SettingsPluginExt;
    use tauri_plugin_tantivy::TantivyPluginExt;

    let ids: Vec<String> = ids
        .iter()
        .filter(|id| is_purgeable_id(id))
        .cloned()
        .collect();
    if ids.is_empty() {
        return Ok(PurgeReport::default());
    }
    let runtime = app
        .try_state::<tauri_plugin_db::ManagedState>()
        .map(|state| state.inner().clone())
        .ok_or_else(|| "database is not ready yet".to_string())?;
    let (purged, wal_truncated) = {
        let _guard = runtime.synced_write_guard().await;
        purge_sessions_in_db(runtime.pool(), &ids).await?
    };
    if purged.is_empty() {
        return Ok(PurgeReport::default());
    }

    let purged_ids: Vec<String> = purged.iter().map(|p| p.id.clone()).collect();
    if let Err(error) = app.tantivy().purge_sessions(&purged_ids).await {
        tracing::warn!(%error, "purge_search_index_failed");
    }
    let sessions_dir = app
        .settings()
        .vault_base()
        .ok()
        .map(|base| base.join("sessions").into_std_path_buf());
    let cache_dir = app.path().app_cache_dir().ok();
    let id_set: HashSet<String> = purged_ids.iter().cloned().collect();
    let files_removed = tokio::task::spawn_blocking(move || {
        purge_session_files(sessions_dir.as_deref(), cache_dir.as_deref(), &id_set)
    })
    .await
    .unwrap_or(0);

    let mut report = PurgeReport {
        files_removed,
        wal_truncated,
        ..PurgeReport::default()
    };
    for session in &purged {
        if !session.purge.changed() {
            continue;
        }
        report.purged += 1;
        report.tombstones += u32::from(session.purge.kept_tombstone);
        report.rows_deleted += u32::try_from(session.purge.rows_deleted).unwrap_or(u32::MAX);
        anlg_audit_log::record(
            app,
            anlg_audit_log::NewEntry::new(
                anlg_audit_log::actions::MEETING_DELETED,
                "meeting",
                session.id.as_str(),
            )
            .source(source)
            .flag("sync_tombstone", session.purge.kept_tombstone)
            .count("rows_deleted", session.purge.rows_deleted as i64)
            .count("rows_blanked", session.purge.rows_blanked as i64),
        );
    }
    if report.purged > 0 || files_removed > 0 {
        anlg_audit_log::record(
            app,
            anlg_audit_log::NewEntry::new(anlg_audit_log::actions::DATA_DELETED, "meetings", "")
                .source(source)
                .count("meetings", i64::from(report.purged))
                .count("files", i64::from(files_removed))
                .flag("wal_truncated", wal_truncated),
        );
    }
    Ok(report)
}

/// The sweep: every soft-deleted meeting whose undo window is over (a quit
/// during the window, or a deletion that arrived from another device).
pub async fn purge_expired_soft_deletes<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    now: chrono::DateTime<chrono::Utc>,
    source: anlg_audit_log::Source,
) -> Result<PurgeReport, String> {
    use tauri::Manager;

    let runtime = app
        .try_state::<tauri_plugin_db::ManagedState>()
        .map(|state| state.inner().clone())
        .ok_or_else(|| "database is not ready yet".to_string())?;
    let before = (now - chrono::Duration::from_std(UNDO_WINDOW_MARGIN).unwrap_or_default())
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let ids = anlg_db_app::soft_deleted_session_ids(runtime.pool(), &before)
        .await
        .map_err(|e| e.to_string())?;
    purge_sessions(app, &ids, source).await
}
