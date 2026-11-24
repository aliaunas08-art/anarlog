use sqlx::{Row, Sqlite, Transaction};

pub(crate) const CAPTURE_USAGE_SESSION_PREDICATE: &str = "CASE WHEN json_valid(value_json) THEN
       json_extract(value_json,'$.sessionId')=?1
       AND id='capture_usage:'||?1||':'||json_extract(value_json,'$.transcriptId')
     ELSE 0 END";

pub(crate) fn capture_stop_session_predicate() -> String {
    format!(
        "id IN (SELECT 'capture_stop:'||?1||':'||id FROM transcripts WHERE session_id=?1
          UNION SELECT 'capture_stop:'||?1||':'||json_extract(value_json,'$.transcriptId')
          FROM app_settings WHERE ({CAPTURE_USAGE_SESSION_PREDICATE}))"
    )
}

/// Physical local deletion after the caller has locked the database and
/// verified the exported snapshot. Refuse replicas and shared/cloud content;
/// their deletion requires the owning sync protocol rather than a local purge.
pub async fn purge_local_session(
    tx: &mut Transaction<'_, Sqlite>,
    id: &str,
) -> Result<(), sqlx::Error> {
    let cloud_disabled: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM app_settings WHERE id='cloud_sync_enabled' AND value_json='false')").fetch_one(&mut **tx).await?;
    let cloud_attachment: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM session_attachments WHERE session_id=? AND cloud_object_key<>'')").bind(id).fetch_one(&mut **tx).await?;
    let shared: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM shared_session_cache WHERE session_id=?)")
            .bind(id)
            .fetch_one(&mut **tx)
            .await?;
    let replicas: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM e2ee_local_state WHERE (table_name='sessions' AND row_id=?) OR (table_name='transcripts' AND row_id IN (SELECT id FROM transcripts WHERE session_id=?)) OR (table_name='session_documents' AND row_id IN (SELECT id FROM session_documents WHERE session_id=?)) OR (table_name='session_attachments' AND row_id IN (SELECT id FROM session_attachments WHERE session_id=?)))")
        .bind(id).bind(id).bind(id).bind(id).fetch_one(&mut **tx).await?;
    if !cloud_disabled || cloud_attachment || shared || replicas {
        return Err(sqlx::Error::Protocol(
            "local purge requires unsynced local content".into(),
        ));
    }
    let plan = PurgePlan::load(tx).await?;
    delete_local_rows(tx, id, &plan, &[]).await?;
    delete_session_row(tx, id).await?;
    Ok(())
}

/// Rows every purge removes, plus every row in a table with a `session_id`
/// column except `keep_tables` (the synced tables whose rows stay as blank
/// tombstones). Only names enumerated by SQLite and quoted as identifiers are
/// used.
async fn delete_local_rows(
    tx: &mut Transaction<'_, Sqlite>,
    id: &str,
    plan: &PurgePlan,
    keep_tables: &[&str],
) -> Result<u64, sqlx::Error> {
    let mut deleted = 0;
    // Resolve exact owners while transcript rows still exist. Prefix matching
    // would include another session whose ID contains this ID plus a colon.
    deleted += sqlx::query(sqlx::AssertSqlSafe(format!(
        "DELETE FROM app_settings WHERE ({})",
        capture_stop_session_predicate()
    )))
    .bind(id)
    .execute(&mut **tx)
    .await?
    .rows_affected();
    // Live partials are keyed by transcript, not session. Removing only the
    // final transcript would leave the original words in this journal.
    deleted += sqlx::query("DELETE FROM transcript_live_state WHERE transcript_id IN (SELECT id FROM transcripts WHERE session_id=?)")
        .bind(id).execute(&mut **tx).await?.rows_affected();
    deleted += sqlx::query("DELETE FROM voiceprint_exemplars WHERE source_session_id=?")
        .bind(id)
        .execute(&mut **tx)
        .await?
        .rows_affected();
    // "This meeting" Ask threads about the meeting (local chat, never synced
    // through the replica). A group only goes when nothing else is left in it.
    const ASK_ABOUT: &str = "json_valid(metadata_json)
        AND json_extract(metadata_json, '$.askMeetingId') = ?1";
    deleted += sqlx::query(sqlx::AssertSqlSafe(format!(
        "DELETE FROM chat_groups WHERE id IN (
            SELECT chat_group_id FROM chat_messages WHERE {ASK_ABOUT}
        ) AND NOT EXISTS (
            SELECT 1 FROM chat_messages AS other
            WHERE other.chat_group_id = chat_groups.id
              AND NOT (json_valid(other.metadata_json)
                AND json_extract(other.metadata_json, '$.askMeetingId') = ?1)
        )"
    )))
    .bind(id)
    .execute(&mut **tx)
    .await?
    .rows_affected();
    deleted += sqlx::query(sqlx::AssertSqlSafe(format!(
        "DELETE FROM chat_messages WHERE {ASK_ABOUT}"
    )))
    .bind(id)
    .execute(&mut **tx)
    .await?
    .rows_affected();
    // This includes document history, proposals, consent evidence, share
    // caches and local attachment jobs.
    for quoted in &plan.session_tables {
        if keep_tables
            .iter()
            .any(|keep| quoted.trim_matches('"') == *keep)
        {
            continue;
        }
        deleted += sqlx::query(sqlx::AssertSqlSafe(format!(
            "DELETE FROM {quoted} WHERE session_id=?"
        )))
        .bind(id)
        .execute(&mut **tx)
        .await?
        .rows_affected();
    }
    deleted += sqlx::query("DELETE FROM entity_mentions WHERE (source_type='session' AND source_id=?) OR (target_type='session' AND target_id=?)").bind(id).bind(id).execute(&mut **tx).await?.rows_affected();
    deleted += sqlx::query("DELETE FROM app_settings WHERE id=?")
        .bind(format!("auto_enhance_pending:{id}"))
        .execute(&mut **tx)
        .await?
        .rows_affected();
    Ok(deleted)
}

async fn delete_session_row(
    tx: &mut Transaction<'_, Sqlite>,
    id: &str,
) -> Result<u64, sqlx::Error> {
    Ok(sqlx::query("DELETE FROM sessions WHERE id=?")
        .bind(id)
        .execute(&mut **tx)
        .await?
        .rows_affected())
}

/// The tables with a `session_id` column (quoted identifiers, enumerated
/// from SQLite, `e2ee_*` excluded), read once per purge run.
#[derive(Debug, Clone)]
pub struct PurgePlan {
    session_tables: Vec<String>,
}

impl PurgePlan {
    pub async fn load(tx: &mut Transaction<'_, Sqlite>) -> Result<Self, sqlx::Error> {
        let tables: Vec<String> = sqlx::query_scalar(
            "SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'",
        )
        .fetch_all(&mut **tx)
        .await?;
        let mut session_tables = Vec::new();
        for table in tables {
            if table.starts_with("e2ee_") {
                continue;
            }
            let quoted = format!("\"{}\"", table.replace('"', "\"\""));
            let columns = sqlx::query(sqlx::AssertSqlSafe(format!("PRAGMA table_info({quoted})")))
                .fetch_all(&mut **tx)
                .await?;
            if columns
                .iter()
                .any(|r| r.get::<String, _>("name") == "session_id")
            {
                session_tables.push(quoted);
            }
        }
        Ok(Self { session_tables })
    }
}

/// What [`purge_deleted_session`] did.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SessionPurge {
    /// The meeting was known to sync, so a blank tombstone was kept.
    pub kept_tombstone: bool,
    pub rows_deleted: u64,
    pub rows_blanked: u64,
}

impl SessionPurge {
    pub fn changed(&self) -> bool {
        self.rows_deleted > 0 || self.rows_blanked > 0
    }
}

/// The synced session tables whose rows stay as blank tombstones when a
/// synced meeting is purged: the E2EE replica propagates their `deleted_at`
/// to other devices, and they replace the server copy with blank content.
const SYNCED_SESSION_TABLES: &[&str] = &[
    "session_documents",
    "transcripts",
    "session_participants",
    "session_tags",
    "action_items",
    "session_attachments",
];

/// Columns a tombstone keeps: identity, ownership, timestamps, the deletion
/// marker itself, and what attachment sync needs to remove the stored blob.
/// `deletion_context` is cleared too: its observations can hold content (an
/// attachment's name), and without it no device restores the meeting when an
/// older edit arrives, which is what a permanent delete means.
const TOMBSTONE_COLUMNS: &[&str] = &[
    "id",
    "workspace_id",
    "owner_user_id",
    "session_id",
    "kind",
    "created_at",
    "updated_at",
    "deleted_at",
    "storage_kind",
    "cloud_object_key",
    "sha256",
    "size_bytes",
];

/// Whether sync has seen the meeting: an E2EE replica state for it or one
/// of its rows, a cloud attachment, or a shared copy.
pub async fn session_is_synced(
    tx: &mut Transaction<'_, Sqlite>,
    id: &str,
) -> Result<bool, sqlx::Error> {
    let cloud_attachment: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM session_attachments WHERE session_id=? AND cloud_object_key<>'')").bind(id).fetch_one(&mut **tx).await?;
    let shared: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM shared_session_cache WHERE session_id=?)")
            .bind(id)
            .fetch_one(&mut **tx)
            .await?;
    let mut replica_tables = String::from("(table_name='sessions' AND row_id=?1)");
    for table in SYNCED_SESSION_TABLES {
        replica_tables.push_str(&format!(
            " OR (table_name='{table}' AND row_id IN (SELECT id FROM {table} WHERE session_id=?1))"
        ));
    }
    let replicas: bool = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT EXISTS(SELECT 1 FROM e2ee_local_state WHERE {replica_tables})"
    )))
    .bind(id)
    .fetch_one(&mut **tx)
    .await?;
    Ok(cloud_attachment || shared || replicas)
}

/// ASPC D2.2: permanently deletes a meeting that is already soft-deleted
/// (its undo window is over), in the caller's transaction.
///
/// - Not known to sync: every row that refers to it is deleted, including
///   the `sessions` row.
/// - Known to sync: the `sessions` row and its synced child rows stay as
///   tombstones with every content column reset to its default, and are
///   queued for upload, so the server copy is overwritten with blank content
///   and other devices delete the meeting too. Everything else is deleted.
///
/// Returns `None` when the meeting doesn't exist or isn't soft-deleted.
/// Running it again on a purged tombstone changes nothing. The caller runs
/// `secure_delete` and the WAL checkpoint, and removes files and index
/// documents.
pub async fn purge_deleted_session(
    tx: &mut Transaction<'_, Sqlite>,
    id: &str,
) -> Result<Option<SessionPurge>, sqlx::Error> {
    let plan = PurgePlan::load(tx).await?;
    purge_deleted_session_with(tx, id, &plan).await
}

/// [`purge_deleted_session`] with a [`PurgePlan`] shared across meetings
/// (the startup sweep and retention).
pub async fn purge_deleted_session_with(
    tx: &mut Transaction<'_, Sqlite>,
    id: &str,
    plan: &PurgePlan,
) -> Result<Option<SessionPurge>, sqlx::Error> {
    let deleted_at: Option<String> = sqlx::query_scalar(
        "SELECT deleted_at FROM sessions WHERE id = ? AND deleted_at IS NOT NULL",
    )
    .bind(id)
    .fetch_optional(&mut **tx)
    .await?;
    let Some(deleted_at) = deleted_at else {
        return Ok(None);
    };
    if !session_is_synced(tx, id).await? {
        let rows_deleted =
            delete_local_rows(tx, id, plan, &[]).await? + delete_session_row(tx, id).await?;
        return Ok(Some(SessionPurge {
            kept_tombstone: false,
            rows_deleted,
            rows_blanked: 0,
        }));
    }
    // Blank first: blanking a document body records its old body as a
    // version, which the delete below then removes.
    let mut rows_blanked = 0;
    for table in SYNCED_SESSION_TABLES {
        rows_blanked += blank_rows(tx, table, "session_id", id, &deleted_at).await?;
    }
    rows_blanked += blank_rows(tx, "sessions", "id", id, &deleted_at).await?;
    let rows_deleted = delete_local_rows(tx, id, plan, SYNCED_SESSION_TABLES).await?;
    Ok(Some(SessionPurge {
        kept_tombstone: true,
        rows_deleted,
        rows_blanked,
    }))
}

/// Resets every content column of `table`'s rows matching `key = id` to its
/// default and marks them deleted, under the E2EE apply guard so the local
/// edit triggers (restore-on-edit, content versions) don't fire, then queues
/// the rows for upload as the sync apply path does.
async fn blank_rows(
    tx: &mut Transaction<'_, Sqlite>,
    table: &str,
    key: &str,
    id: &str,
    deleted_at: &str,
) -> Result<u64, sqlx::Error> {
    let columns = sqlx::query(sqlx::AssertSqlSafe(format!("PRAGMA table_info({table})")))
        .fetch_all(&mut **tx)
        .await?;
    let mut assignments = Vec::new();
    let mut differs = Vec::new();
    for column in &columns {
        let name: String = column.get("name");
        if TOMBSTONE_COLUMNS.contains(&name.as_str()) {
            continue;
        }
        let declared: String = column.get("type");
        let not_null: i64 = column.get("notnull");
        let default: Option<String> = column.get("dflt_value");
        let blank = match default {
            Some(default) => default,
            None if not_null == 0 => "NULL".to_string(),
            None if declared.eq_ignore_ascii_case("INTEGER")
                || declared.eq_ignore_ascii_case("REAL") =>
            {
                "0".to_string()
            }
            None if declared.eq_ignore_ascii_case("BLOB") => "X''".to_string(),
            None => "''".to_string(),
        };
        // A default like `(strftime(...))` isn't stable; compare such
        // columns only for being set at all.
        if blank.starts_with('(') {
            differs.push(format!("\"{name}\" IS NOT NULL AND \"{name}\" <> ''"));
        } else {
            differs.push(format!("\"{name}\" IS NOT {blank}"));
        }
        assignments.push(format!("\"{name}\" = {blank}"));
    }
    if assignments.is_empty() {
        return Ok(0);
    }
    let pending = format!(
        "{key} = ?1 AND ({} OR deleted_at IS NULL)",
        differs.join(" OR ")
    );
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "INSERT OR IGNORE INTO e2ee_apply_guard (workspace_id, table_name, row_id)
         SELECT workspace_id, '{table}', id FROM {table} WHERE {pending}"
    )))
    .bind(id)
    .execute(&mut **tx)
    .await?;
    let changed: Vec<(String, String)> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT workspace_id, id FROM {table} WHERE {pending}"
    )))
    .bind(id)
    .fetch_all(&mut **tx)
    .await?;
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "UPDATE {table} SET {}, deleted_at = COALESCE(deleted_at, ?2) WHERE {pending}",
        assignments.join(", ")
    )))
    .bind(id)
    .bind(deleted_at)
    .execute(&mut **tx)
    .await?;
    for (workspace_id, row_id) in &changed {
        sqlx::query(
            "INSERT INTO e2ee_dirty_rows (workspace_id, table_name, row_id)
             VALUES (?, ?, ?)
             ON CONFLICT(workspace_id, table_name, row_id)
             DO UPDATE SET generation = generation + 1",
        )
        .bind(workspace_id)
        .bind(table)
        .bind(row_id)
        .execute(&mut **tx)
        .await?;
        sqlx::query(
            "DELETE FROM e2ee_apply_guard WHERE workspace_id = ? AND table_name = ? AND row_id = ?",
        )
        .bind(workspace_id)
        .bind(table)
        .bind(row_id)
        .execute(&mut **tx)
        .await?;
    }
    Ok(changed.len() as u64)
}

/// ASPC D2.2: ids of soft-deleted meetings whose undo window is over
/// (`deleted_at` at or before `before`), for the sweep that finishes
/// deletions interrupted by a quit and applies deletions that arrived from
/// other devices.
pub async fn soft_deleted_session_ids(
    pool: &sqlx::SqlitePool,
    before: &str,
) -> Result<Vec<String>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT id FROM sessions WHERE deleted_at IS NOT NULL AND deleted_at <= ? ORDER BY deleted_at",
    )
    .bind(before)
    .fetch_all(pool)
    .await
}

#[cfg(test)]
mod tests {
    use anlg_db_core::Db;
    use sqlx::{Column, Row, SqlitePool};

    use super::*;

    const MARKER: &str = "ZEBRA-PURGE-7731";

    async fn test_db() -> Db {
        let db = Db::connect_memory_plain().await.unwrap();
        crate::prepare_schema(&db).await.unwrap();
        db
    }

    /// Every text value in every table that contains the marker.
    async fn marker_hits(pool: &SqlitePool) -> Vec<String> {
        let tables: Vec<String> = sqlx::query_scalar(
            "SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'",
        )
        .fetch_all(pool)
        .await
        .unwrap();
        let mut hits = Vec::new();
        for table in tables {
            let rows = sqlx::query(sqlx::AssertSqlSafe(format!("SELECT * FROM \"{table}\"")))
                .fetch_all(pool)
                .await
                .unwrap_or_default();
            for row in rows {
                for (index, column) in row.columns().iter().enumerate() {
                    if let Ok(Some(text)) = row.try_get::<Option<String>, _>(index)
                        && text.contains(MARKER)
                    {
                        hits.push(format!("{table}.{}", column.name()));
                    }
                }
            }
        }
        hits
    }

    async fn seed_meeting(pool: &SqlitePool, id: &str) {
        let m = MARKER;
        for statement in [
            format!(
                "INSERT INTO sessions (id, title, event_json, metadata_json) VALUES ('{id}', 'Title {m}', '{{\"title\":\"{m}\"}}', '{{\"x\":\"{m}\"}}')"
            ),
            format!(
                "INSERT INTO session_documents (id, session_id, kind, body_format, body) VALUES ('{id}', '{id}', 'note', 'markdown', 'Note {m}')"
            ),
            format!("UPDATE session_documents SET body = 'Edited note {m}' WHERE id = '{id}'"),
            format!(
                "INSERT INTO session_documents (id, session_id, kind, body_format, body) VALUES ('{id}-chat', '{id}', 'meeting_chat', 'markdown', 'Chat {m}')"
            ),
            format!(
                "INSERT INTO transcripts (id, session_id, words_json, memo) VALUES ('{id}-t', '{id}', '[{{\"text\":\"{m}\"}}]', 'memo {m}')"
            ),
            format!("INSERT INTO transcript_live_state (transcript_id) VALUES ('{id}-t')"),
            format!(
                "INSERT INTO transcript_live_deltas (id, transcript_id, sequence, delta_json) VALUES ('{id}-d', '{id}-t', 0, '{{\"new_words\":[\"{m}\"]}}')"
            ),
            format!(
                "INSERT INTO session_participants (id, session_id, human_id, display_name) VALUES ('{id}-p', '{id}', 'h1', 'Person {m}')"
            ),
            format!(
                "INSERT INTO action_items (id, session_id, text) VALUES ('{id}-a', '{id}', 'Do {m}')"
            ),
            format!(
                "INSERT INTO session_attachments (id, session_id, filename, relative_path) VALUES ('{id}-f', '{id}', 'file-{m}.pdf', 'attachments/file-{m}.pdf')"
            ),
            format!(
                "INSERT INTO session_proposals (id, session_id, proposed_markdown) VALUES ('{id}-prop', '{id}', 'Proposal {m}')"
            ),
            format!("INSERT INTO chat_groups (id, title) VALUES ('{id}-g', 'Ask {m}')"),
            format!(
                "INSERT INTO chat_messages (id, chat_group_id, content, metadata_json) VALUES ('{id}-msg', '{id}-g', 'Answer {m}', '{{\"askScope\":\"meeting\",\"askMeetingId\":\"{id}\"}}')"
            ),
            format!(
                "INSERT INTO entity_mentions (id, source_type, source_id, target_type, target_id) VALUES ('{id}-em', 'session', '{id}', 'human', 'h1')"
            ),
            format!(
                "INSERT INTO app_settings (id, value_json) VALUES ('auto_enhance_pending:{id}', '\"{m}\"')"
            ),
        ] {
            sqlx::query(sqlx::AssertSqlSafe(statement.clone()))
                .execute(pool)
                .await
                .unwrap_or_else(|error| panic!("{statement}: {error}"));
        }
    }

    async fn soft_delete(pool: &SqlitePool, id: &str) {
        let mut conn = pool.acquire().await.unwrap();
        for table in SYNCED_SESSION_TABLES {
            crate::update_session_child_tombstone(
                &mut conn,
                table,
                id,
                "2026-11-16T00:00:00Z",
                false,
            )
            .await
            .unwrap();
        }
        crate::update_sessions_tombstone(&mut conn, id, "2026-11-16T00:00:00Z", false)
            .await
            .unwrap();
    }

    async fn purge(pool: &SqlitePool, id: &str) -> Option<SessionPurge> {
        let mut tx = pool.begin().await.unwrap();
        let result = purge_deleted_session(&mut tx, id).await.unwrap();
        tx.commit().await.unwrap();
        result
    }

    #[tokio::test]
    async fn an_unsynced_meeting_leaves_no_row_behind() {
        let db = test_db().await;
        seed_meeting(db.pool(), "m1").await;
        seed_meeting(db.pool(), "keep").await;
        sqlx::query("UPDATE sessions SET title = 'Other' WHERE id = 'keep'")
            .execute(db.pool())
            .await
            .unwrap();
        // Not soft-deleted yet: nothing happens (the undo window).
        assert_eq!(purge(db.pool(), "m1").await, None);
        soft_delete(db.pool(), "m1").await;
        let outcome = purge(db.pool(), "m1").await.unwrap();
        assert!(!outcome.kept_tombstone);
        assert!(outcome.rows_deleted > 10, "{outcome:?}");

        let hits = marker_hits(db.pool()).await;
        assert!(hits.iter().all(|hit| !hit.contains("m1")), "{hits:?}");
        let remaining: i64 = sqlx::query_scalar(
            "SELECT (SELECT count(*) FROM sessions WHERE id = 'm1')
                  + (SELECT count(*) FROM transcript_live_deltas WHERE transcript_id = 'm1-t')
                  + (SELECT count(*) FROM session_document_versions WHERE session_id = 'm1')
                  + (SELECT count(*) FROM chat_messages WHERE id = 'm1-msg')
                  + (SELECT count(*) FROM chat_groups WHERE id = 'm1-g')",
        )
        .fetch_one(db.pool())
        .await
        .unwrap();
        assert_eq!(remaining, 0);
        // The other meeting keeps its content.
        let kept: i64 =
            sqlx::query_scalar("SELECT count(*) FROM session_documents WHERE session_id = 'keep'")
                .fetch_one(db.pool())
                .await
                .unwrap();
        assert_eq!(kept, 2);
    }

    #[tokio::test]
    async fn a_synced_meeting_keeps_only_a_blank_tombstone_and_queues_it() {
        let db = test_db().await;
        seed_meeting(db.pool(), "m2").await;
        let workspace: String =
            sqlx::query_scalar("SELECT workspace_id FROM sessions WHERE id = 'm2'")
                .fetch_one(db.pool())
                .await
                .unwrap();
        sqlx::query(
            "INSERT INTO e2ee_local_state (record_id, workspace_id, table_name, row_id, field_name)
             VALUES ('r1', ?, 'sessions', 'm2', 'title')",
        )
        .bind(&workspace)
        .execute(db.pool())
        .await
        .unwrap();
        soft_delete(db.pool(), "m2").await;
        sqlx::query("DELETE FROM e2ee_dirty_rows")
            .execute(db.pool())
            .await
            .unwrap();

        let outcome = purge(db.pool(), "m2").await.unwrap();
        assert!(outcome.kept_tombstone);
        assert!(outcome.rows_blanked >= 7, "{outcome:?}");
        assert_eq!(marker_hits(db.pool()).await, Vec::<String>::new());

        let (title, deleted_at, deletion_context): (String, Option<String>, String) =
            sqlx::query_as(
                "SELECT title, deleted_at, deletion_context FROM sessions WHERE id = 'm2'",
            )
            .fetch_one(db.pool())
            .await
            .unwrap();
        assert_eq!(title, "");
        assert!(deleted_at.is_some());
        assert_eq!(deletion_context, "");
        // Every blanked synced row is queued so the server copy is replaced
        // and other devices delete it too.
        let queued: Vec<String> = sqlx::query_scalar(
            "SELECT table_name || ':' || row_id FROM e2ee_dirty_rows ORDER BY 1",
        )
        .fetch_all(db.pool())
        .await
        .unwrap();
        for expected in [
            "sessions:m2",
            "transcripts:m2-t",
            "session_documents:m2",
            "session_attachments:m2-f",
        ] {
            assert!(queued.contains(&expected.to_string()), "{queued:?}");
        }
        let guards: i64 = sqlx::query_scalar("SELECT count(*) FROM e2ee_apply_guard")
            .fetch_one(db.pool())
            .await
            .unwrap();
        assert_eq!(guards, 0);
        let undeleted: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM transcripts WHERE session_id = 'm2' AND deleted_at IS NULL",
        )
        .fetch_one(db.pool())
        .await
        .unwrap();
        assert_eq!(undeleted, 0);

        // A second run (the startup sweep) changes nothing and queues nothing.
        sqlx::query("DELETE FROM e2ee_dirty_rows")
            .execute(db.pool())
            .await
            .unwrap();
        let again = purge(db.pool(), "m2").await.unwrap();
        assert!(!again.changed(), "{again:?}");
        let queued: i64 = sqlx::query_scalar("SELECT count(*) FROM e2ee_dirty_rows")
            .fetch_one(db.pool())
            .await
            .unwrap();
        assert_eq!(queued, 0);
    }
}
