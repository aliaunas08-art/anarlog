//! ASPC D3 retention: "Keep meetings forever" (the default) or "Delete
//! meetings older than N days" (app setting `meeting_retention_days`, 0 =
//! forever). The desktop app runs [`apply_retention`] at startup and every
//! [`RETENTION_INTERVAL`]; it deletes through the D2.2 path (soft delete,
//! then the permanent purge) and never touches a meeting being recorded.

use std::collections::HashSet;
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use specta::Type;
use sqlx::SqlitePool;

pub const RETENTION_SETTING: &str = "meeting_retention_days";
pub const RETENTION_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);
/// Upper bound accepted from the setting (about 100 years).
pub const MAX_RETENTION_DAYS: i64 = 36_500;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct RetentionRun {
    /// `None`: keep meetings forever (nothing was checked).
    pub days: Option<u32>,
    pub deleted: u32,
    pub skipped_recording: u32,
}

/// The configured retention in days, or `None` for "keep forever" (the
/// setting missing, 0, negative or not a number).
pub async fn retention_days(pool: &SqlitePool) -> Result<Option<u32>, sqlx::Error> {
    let raw: Option<String> =
        sqlx::query_scalar("SELECT value_json FROM app_settings WHERE id = ?")
            .bind(RETENTION_SETTING)
            .fetch_optional(pool)
            .await?;
    Ok(raw.and_then(|value| parse_days(&value)))
}

fn parse_days(value_json: &str) -> Option<u32> {
    let value: serde_json::Value = serde_json::from_str(value_json).ok()?;
    let days = value.as_f64()?.floor() as i64;
    (1..=MAX_RETENTION_DAYS)
        .contains(&days)
        .then_some(days as u32)
}

/// Live meetings in the local workspace whose start (or, without one, its
/// creation) is more than `days` before `now`, minus `skip`. A date SQLite
/// can't parse is never treated as old.
pub async fn expired_session_ids(
    pool: &SqlitePool,
    now: DateTime<Utc>,
    days: u32,
    skip: &HashSet<String>,
) -> Result<Vec<String>, sqlx::Error> {
    let Some(scope) = anlg_db_app::local_workspace_scope(pool).await? else {
        return Ok(Vec::new());
    };
    let cutoff = (now - chrono::Duration::days(i64::from(days)))
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let ids: Vec<String> = sqlx::query_scalar(
        "SELECT id FROM sessions
         WHERE deleted_at IS NULL
           AND workspace_id IN (?, ?)
           AND julianday(COALESCE(NULLIF(started_at, ''), created_at)) < julianday(?)
         ORDER BY created_at",
    )
    .bind(scope.sql_values()[0])
    .bind(scope.sql_values()[1])
    .bind(cutoff)
    .fetch_all(pool)
    .await?;
    Ok(ids.into_iter().filter(|id| !skip.contains(id)).collect())
}

/// Soft-deletes the meetings with one tombstone time; returns those deleted.
pub async fn soft_delete_for_retention(
    pool: &SqlitePool,
    ids: &[String],
    now: DateTime<Utc>,
) -> Result<Vec<String>, String> {
    let tombstone = now.to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let mut deleted = Vec::new();
    for id in ids {
        let row = crate::deletion::soft_delete_session(
            pool,
            crate::deletion::TombstoneSessionRequest {
                session_id: id.clone(),
                tombstone: tombstone.clone(),
            },
        )
        .await?;
        if row.is_some() {
            deleted.push(id.clone());
        }
    }
    Ok(deleted)
}

/// One retention run: finds meetings past the retention period (skipping
/// `recording`, the meetings being captured or finalized), deletes them
/// through the D2.2 path and writes `retention.run` with the count.
pub async fn apply_retention<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    now: DateTime<Utc>,
    recording: &HashSet<String>,
) -> Result<RetentionRun, String> {
    use tauri::Manager;

    let runtime = app
        .try_state::<tauri_plugin_db::ManagedState>()
        .map(|state| state.inner().clone())
        .ok_or_else(|| "database is not ready yet".to_string())?;
    let Some(days) = retention_days(runtime.pool())
        .await
        .map_err(|e| e.to_string())?
    else {
        return Ok(RetentionRun::default());
    };
    let all = expired_session_ids(runtime.pool(), now, days, &HashSet::new())
        .await
        .map_err(|e| e.to_string())?;
    let skipped_recording = all.iter().filter(|id| recording.contains(*id)).count() as u32;
    let candidates: Vec<String> = all
        .into_iter()
        .filter(|id| !recording.contains(id))
        .collect();
    let deleted = {
        let _guard = runtime.synced_write_guard().await;
        soft_delete_for_retention(runtime.pool(), &candidates, now).await?
    };
    let report = crate::purge::purge_sessions(app, &deleted, anlg_audit_log::Source::Job).await?;
    let run = RetentionRun {
        days: Some(days),
        deleted: deleted.len() as u32,
        skipped_recording,
    };
    anlg_audit_log::record_job(
        app,
        anlg_audit_log::NewEntry::new(anlg_audit_log::actions::RETENTION_RUN, "meetings", "")
            .count("days", i64::from(days))
            .count("deleted", i64::from(run.deleted))
            .count("purged", i64::from(report.purged))
            .count("skipped_recording", i64::from(skipped_recording)),
    );
    Ok(run)
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use anlg_db_core::Db;
    use chrono::TimeZone;

    use super::*;

    async fn test_db() -> Db {
        let db = Db::connect_memory_plain().await.unwrap();
        anlg_db_app::prepare_schema(&db).await.unwrap();
        db
    }

    #[test]
    fn retention_setting_parses_to_days_or_forever() {
        assert_eq!(parse_days("30"), Some(30));
        assert_eq!(parse_days("30.9"), Some(30));
        for forever in ["0", "-5", "\"30\"", "null", "{}", "99999999"] {
            assert_eq!(parse_days(forever), None, "{forever}");
        }
    }

    /// D3.3: with a fake clock, meetings past N days are deleted (through the
    /// permanent delete) and newer ones, and the one being recorded, are kept.
    #[tokio::test]
    async fn old_meetings_are_deleted_and_newer_or_recording_ones_kept() {
        let db = test_db().await;
        let pool = db.pool();
        let now = Utc.with_ymd_and_hms(2026, 11, 16, 12, 0, 0).unwrap();
        for (id, started_at, created_at) in [
            (
                "old-1",
                "2026-09-01T10:00:00.000Z",
                "2026-09-01T10:00:00.000Z",
            ),
            ("old-2", "", "2026-10-01T09:00:00.000Z"),
            (
                "old-recording",
                "2026-08-01T10:00:00.000Z",
                "2026-08-01T10:00:00.000Z",
            ),
            (
                "new-1",
                "2026-11-10T10:00:00.000Z",
                "2026-11-10T10:00:00.000Z",
            ),
            (
                "edge",
                "2026-10-17T12:00:01.000Z",
                "2026-10-17T12:00:01.000Z",
            ),
            (
                "offset",
                "2026-09-01T10:00:00+02:00",
                "2026-11-15T00:00:00.000Z",
            ),
            ("unparseable", "someday", "2026-11-15T00:00:00.000Z"),
        ] {
            sqlx::query(
                "INSERT INTO sessions (id, title, started_at, created_at) VALUES (?, 'x', ?, ?)",
            )
            .bind(id)
            .bind(started_at)
            .bind(created_at)
            .execute(pool)
            .await
            .unwrap();
            sqlx::query("INSERT INTO session_documents (id, session_id, kind, body) VALUES (?, ?, 'note', 'body')")
                .bind(id)
                .bind(id)
                .execute(pool)
                .await
                .unwrap();
        }
        sqlx::query("INSERT INTO app_settings (id, value_json) VALUES (?, '30')")
            .bind(RETENTION_SETTING)
            .execute(pool)
            .await
            .unwrap();

        let days = retention_days(pool).await.unwrap().unwrap();
        assert_eq!(days, 30);
        let recording: HashSet<String> = ["old-recording".to_string()].into();
        let mut expired = expired_session_ids(pool, now, days, &recording)
            .await
            .unwrap();
        expired.sort();
        assert_eq!(expired, vec!["offset", "old-1", "old-2"]);

        let deleted = soft_delete_for_retention(pool, &expired, now)
            .await
            .unwrap();
        let (purged, _) = crate::purge::purge_sessions_in_db(pool, &deleted)
            .await
            .unwrap();
        assert_eq!(purged.len(), 3);

        let mut left: Vec<String> = sqlx::query_scalar("SELECT id FROM sessions ORDER BY id")
            .fetch_all(pool)
            .await
            .unwrap();
        left.sort();
        assert_eq!(left, vec!["edge", "new-1", "old-recording", "unparseable"]);
        let notes: i64 = sqlx::query_scalar("SELECT count(*) FROM session_documents WHERE session_id IN ('old-1', 'old-2', 'offset')")
            .fetch_one(pool)
            .await
            .unwrap();
        assert_eq!(notes, 0);

        // A day later the edge meeting is past the period too.
        let later = now + chrono::Duration::days(1);
        assert_eq!(
            expired_session_ids(pool, later, days, &recording)
                .await
                .unwrap(),
            vec!["edge"]
        );
    }

    #[tokio::test]
    async fn keep_forever_deletes_nothing() {
        let db = test_db().await;
        assert_eq!(retention_days(db.pool()).await.unwrap(), None);
        sqlx::query("INSERT INTO app_settings (id, value_json) VALUES (?, '0')")
            .bind(RETENTION_SETTING)
            .execute(db.pool())
            .await
            .unwrap();
        assert_eq!(retention_days(db.pool()).await.unwrap(), None);
    }
}
