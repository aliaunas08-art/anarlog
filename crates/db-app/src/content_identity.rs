//! ASPC C1: the device's local workspace and owner ids, the startup backfill
//! that gives every unassigned content row those ids, and the scope that Rust
//! reads filter by.
//!
//! The data model already has a local workspace: the CloudSync binding's
//! `workspace_id` (a UUID made on first run; the account id once a signed-in
//! account claims the device). New rows written by Rust take it from the
//! binding (`insert_session`), and CloudSync's claim moves `''` rows and rows
//! of that workspace into the account. The local owner id follows the same
//! rule as `insert_session`: the active local library's workspace, else the
//! binding's workspace. Using these ids (and not new ones) keeps the claim
//! working: a row with any other workspace id would make it fail with
//! `ForeignWorkspace`.

use anlg_aspc::WorkspaceScope;
use sqlx::{Sqlite, SqlitePool, Transaction};

use crate::cloudsync::CLOUDSYNC_WORKSPACE_BINDING_ID;

/// SQL for the local workspace id, for create paths that must not write `''`
/// (C1.1). Same source as `insert_session`.
pub const LOCAL_WORKSPACE_ID_SQL: &str = "(SELECT json_extract(value_json, '$.workspace_id') FROM app_settings WHERE id = 'cloudsync_workspace_binding')";

/// Content tables with a `workspace_id` that holds the device's own rows.
/// Not here: CloudSync/e2ee bookkeeping, `shared_session_cache` (other
/// people's notes, keyed by the sharer's workspace), `workspaces` and
/// `workspace_memberships` (the cloud projection), and the enterprise outbox.
pub const WORKSPACE_CONTENT_TABLES: &[&str] = &[
    "action_items",
    "chat_groups",
    "chat_messages",
    "daily_notes",
    "entity_mentions",
    "folder_attachments",
    "folders",
    "humans",
    "organizations",
    "session_attachments",
    "session_document_versions",
    "session_documents",
    "session_participants",
    "session_proposals",
    "session_tags",
    "sessions",
    "synced_preferences",
    "tags",
    "transcripts",
    "voiceprint_candidates",
    "voiceprint_exemplars",
];

/// The tables in [`WORKSPACE_CONTENT_TABLES`] that also have `owner_user_id`.
/// Each is in CloudSync's `USER_ID_REFERENCES`, so the claim rekeys it.
pub const OWNER_CONTENT_TABLES: &[&str] = &[
    "chat_groups",
    "chat_messages",
    "daily_notes",
    "entity_mentions",
    "humans",
    "organizations",
    "session_participants",
    "session_tags",
    "sessions",
    "tags",
    "transcripts",
];

/// Content tables outside CloudSync's E2EE domain. The claim moves their rows
/// into the account workspace too, so they never end up in a stale one.
pub const NON_E2EE_CONTENT_TABLES: &[&str] = &[
    "chat_groups",
    "chat_messages",
    "entity_mentions",
    "folder_attachments",
    "session_document_versions",
    "session_proposals",
    "voiceprint_candidates",
    "voiceprint_exemplars",
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContentIdentity {
    pub workspace_id: String,
    pub owner_user_id: String,
}

impl ContentIdentity {
    /// The local scope, or `None` if the workspace id is empty.
    pub fn scope(&self) -> Option<WorkspaceScope> {
        WorkspaceScope::local(self.workspace_id.clone())
    }
}

/// The local workspace and owner ids. `None` before the binding exists (an
/// unmigrated or never-started database): callers must fail closed.
pub async fn local_content_identity(
    pool: &SqlitePool,
) -> Result<Option<ContentIdentity>, sqlx::Error> {
    let row: Option<(Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT json_extract(binding.value_json, '$.workspace_id'),
                (SELECT library_workspace_id FROM local_library_connections
                 WHERE active = 1 AND library_workspace_id <> '')
         FROM app_settings AS binding
         WHERE binding.id = ?",
    )
    .bind(CLOUDSYNC_WORKSPACE_BINDING_ID)
    .fetch_optional(pool)
    .await?;
    let Some((Some(workspace_id), library)) = row else {
        return Ok(None);
    };
    let workspace_id = workspace_id.trim().to_string();
    if workspace_id.is_empty() {
        return Ok(None);
    }
    let owner_user_id = library
        .map(|id| id.trim().to_string())
        .filter(|id| !id.is_empty())
        .unwrap_or_else(|| workspace_id.clone());
    Ok(Some(ContentIdentity {
        workspace_id,
        owner_user_id,
    }))
}

/// The scope for Rust reads of local content (C1.2). Resolved per call
/// because the binding changes when an account claims the device.
pub async fn local_workspace_scope(
    pool: &SqlitePool,
) -> Result<Option<WorkspaceScope>, sqlx::Error> {
    Ok(local_content_identity(pool)
        .await?
        .and_then(|identity| identity.scope()))
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ContentIdentityBackfill {
    /// Rows changed, per table (tables with no change are left out).
    pub updated: Vec<(String, u64)>,
}

impl ContentIdentityBackfill {
    pub fn total(&self) -> u64 {
        self.updated.iter().map(|(_, count)| count).sum()
    }
}

/// C1.1 backfill: every content row whose `workspace_id` is `''` gets the
/// local workspace id, and every row of the local workspace (or `''`) whose
/// `owner_user_id` is `''` gets the local owner id. Rows of any other
/// workspace are never touched. Idempotent: a second run changes nothing.
///
/// Sync: the E2EE dirty-row triggers are suppressed for exactly these rows
/// (with `e2ee_apply_guard`, as sync's own apply does), so the backfill queues
/// no uploads. A `''` row was never uploaded (only workspaces with keys are),
/// and it is uploaded with its next real edit. The search index is requeued
/// for changed sessions, which the workspace field in the index needs.
pub async fn backfill_content_identity(
    pool: &SqlitePool,
    identity: &ContentIdentity,
) -> Result<ContentIdentityBackfill, sqlx::Error> {
    if identity.workspace_id.trim().is_empty() || identity.owner_user_id.trim().is_empty() {
        return Err(sqlx::Error::Protocol(
            "content identity backfill needs a workspace and an owner id".into(),
        ));
    }
    let mut transaction = pool.begin_with("BEGIN IMMEDIATE").await?;
    let mut report = ContentIdentityBackfill::default();
    for table in WORKSPACE_CONTENT_TABLES {
        let count = backfill_table(&mut transaction, table, identity).await?;
        if count > 0 {
            report.updated.push(((*table).to_string(), count));
        }
    }
    transaction.commit().await?;
    Ok(report)
}

fn needs_backfill_predicate(table: &str) -> &'static str {
    if OWNER_CONTENT_TABLES.contains(&table) {
        "(workspace_id = '' OR (owner_user_id = '' AND workspace_id = ?1))"
    } else {
        "(workspace_id = '' AND ?1 IS NOT NULL)"
    }
}

async fn backfill_table(
    transaction: &mut Transaction<'_, Sqlite>,
    table: &str,
    identity: &ContentIdentity,
) -> Result<u64, sqlx::Error> {
    // `table` comes from the constant lists above, never from input.
    let predicate = needs_backfill_predicate(table);
    let guarded = crate::E2EE_DOMAIN_TABLES.contains(&table);
    if guarded {
        // The update triggers check the guard for the old and the new
        // workspace id; guard both.
        for workspace in ["workspace_id", "?1"] {
            sqlx::query(sqlx::AssertSqlSafe(format!(
                "INSERT OR IGNORE INTO e2ee_apply_guard (workspace_id, table_name, row_id)
                 SELECT {workspace}, '{table}', id FROM {table} WHERE {predicate}"
            )))
            .bind(&identity.workspace_id)
            .execute(&mut **transaction)
            .await?;
        }
    }
    let owner_set = if OWNER_CONTENT_TABLES.contains(&table) {
        ", owner_user_id = CASE WHEN owner_user_id = '' THEN ?2 ELSE owner_user_id END"
    } else {
        ""
    };
    let mut update = sqlx::query(sqlx::AssertSqlSafe(format!(
        "UPDATE {table}
         SET workspace_id = CASE WHEN workspace_id = '' THEN ?1 ELSE workspace_id END{owner_set}
         WHERE {predicate}"
    )))
    .bind(&identity.workspace_id);
    if !owner_set.is_empty() {
        update = update.bind(&identity.owner_user_id);
    }
    let result = update.execute(&mut **transaction).await?;
    if guarded {
        // Guards live only inside a transaction; any left here are stale.
        sqlx::query("DELETE FROM e2ee_apply_guard WHERE table_name = ?")
            .bind(table)
            .execute(&mut **transaction)
            .await?;
    }
    Ok(result.rows_affected())
}

/// Rows with an empty `workspace_id` or (in tables that have it) an empty
/// `owner_user_id`, per table. Used by the C1.1 tests and diagnostics.
pub async fn count_unassigned_content_rows(
    pool: &SqlitePool,
) -> Result<Vec<(String, i64)>, sqlx::Error> {
    let mut counts = Vec::new();
    for table in WORKSPACE_CONTENT_TABLES {
        let owner = if OWNER_CONTENT_TABLES.contains(table) {
            " OR owner_user_id = ''"
        } else {
            ""
        };
        let count: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT count(*) FROM {table} WHERE workspace_id = ''{owner}"
        )))
        .fetch_one(pool)
        .await?;
        if count > 0 {
            counts.push(((*table).to_string(), count));
        }
    }
    Ok(counts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use anlg_db_core::Db;

    async fn test_db() -> Db {
        let db = Db::open(anlg_db_core::DbOpenOptions {
            storage: anlg_db_core::DbStorage::Memory,
            cloudsync_enabled: false,
            journal_mode_wal: true,
            foreign_keys: true,
            max_connections: Some(1),
        })
        .await
        .unwrap();
        crate::prepare_schema(&db).await.unwrap();
        db
    }

    /// Voiceprint tables already reject an empty workspace (CHECK).
    fn seedable_tables() -> impl Iterator<Item = &'static str> {
        WORKSPACE_CONTENT_TABLES
            .iter()
            .copied()
            .filter(|table| !table.starts_with("voiceprint_"))
    }

    async fn seed(db: &Db, local: &str) {
        for table in seedable_tables() {
            for (id, workspace) in [("unassigned", ""), ("local", local), ("foreign", "team-ws")] {
                sqlx::query(sqlx::AssertSqlSafe(format!(
                    "INSERT INTO {table} (id, workspace_id) VALUES (?, ?)"
                )))
                .bind(format!("{table}-{id}"))
                .bind(workspace)
                .execute(db.pool())
                .await
                .unwrap();
            }
        }
    }

    async fn ids(db: &Db, table: &str, id: &str) -> (String, Option<String>) {
        let owner = if OWNER_CONTENT_TABLES.contains(&table) {
            "owner_user_id"
        } else {
            "NULL"
        };
        sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "SELECT workspace_id, {owner} FROM {table} WHERE id = ?"
        )))
        .bind(format!("{table}-{id}"))
        .fetch_one(db.pool())
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn identity_is_the_cloudsync_binding_and_library_owner() {
        let db = test_db().await;
        let binding = crate::ensure_cloudsync_workspace_binding(db.pool())
            .await
            .unwrap();
        let identity = local_content_identity(db.pool()).await.unwrap().unwrap();
        assert_eq!(identity.workspace_id, binding);
        assert_eq!(identity.owner_user_id, binding);
        assert_eq!(
            local_workspace_scope(db.pool()).await.unwrap().unwrap(),
            WorkspaceScope::local(binding.clone()).unwrap()
        );

        sqlx::query(
            "INSERT INTO local_library_connections (account_user_id, library_workspace_id, active)
             VALUES ('account', 'library-ws', 1)",
        )
        .execute(db.pool())
        .await
        .unwrap();
        let identity = local_content_identity(db.pool()).await.unwrap().unwrap();
        assert_eq!(identity.workspace_id, binding);
        assert_eq!(identity.owner_user_id, "library-ws");

        sqlx::query("DELETE FROM app_settings WHERE id = ?")
            .bind(CLOUDSYNC_WORKSPACE_BINDING_ID)
            .execute(db.pool())
            .await
            .unwrap();
        assert!(local_content_identity(db.pool()).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn backfill_assigns_unassigned_rows_only_and_is_idempotent() {
        let db = test_db().await;
        let local = crate::ensure_cloudsync_workspace_binding(db.pool())
            .await
            .unwrap();
        seed(&db, &local).await;
        // Trigger output from seeding is not the backfill's; start clean.
        sqlx::query("DELETE FROM e2ee_dirty_rows")
            .execute(db.pool())
            .await
            .unwrap();
        let identity = ContentIdentity {
            workspace_id: local.clone(),
            owner_user_id: local.clone(),
        };

        let report = backfill_content_identity(db.pool(), &identity)
            .await
            .unwrap();
        assert!(report.total() > 0);

        for table in seedable_tables() {
            let has_owner = OWNER_CONTENT_TABLES.contains(&table);
            let owner = has_owner.then(|| local.clone());
            assert_eq!(
                ids(&db, table, "unassigned").await,
                (local.clone(), owner.clone()),
                "{table} unassigned row"
            );
            assert_eq!(
                ids(&db, table, "local").await,
                (local.clone(), owner),
                "{table} local row gets an owner"
            );
            // Another workspace's row is never rewritten, owner included.
            assert_eq!(
                ids(&db, table, "foreign").await,
                ("team-ws".to_string(), has_owner.then(String::new)),
                "{table} foreign row"
            );
        }
        assert!(
            count_unassigned_content_rows(db.pool())
                .await
                .unwrap()
                .iter()
                .all(
                    |(table, count)| OWNER_CONTENT_TABLES.contains(&table.as_str()) && *count == 1
                )
        );

        // No sync churn: no dirty rows queued, no guards left behind.
        let dirty: i64 = sqlx::query_scalar("SELECT count(*) FROM e2ee_dirty_rows")
            .fetch_one(db.pool())
            .await
            .unwrap();
        assert_eq!(dirty, 0);
        let guards: i64 = sqlx::query_scalar("SELECT count(*) FROM e2ee_apply_guard")
            .fetch_one(db.pool())
            .await
            .unwrap();
        assert_eq!(guards, 0);

        let again = backfill_content_identity(db.pool(), &identity)
            .await
            .unwrap();
        assert_eq!(again.total(), 0);
    }

    #[tokio::test]
    async fn backfill_refuses_an_empty_identity() {
        let db = test_db().await;
        let empty = ContentIdentity {
            workspace_id: String::new(),
            owner_user_id: "owner".into(),
        };
        assert!(backfill_content_identity(db.pool(), &empty).await.is_err());
    }

    #[tokio::test]
    async fn claim_still_works_after_the_backfill_and_moves_every_content_table() {
        let db = test_db().await;
        let local = crate::ensure_cloudsync_workspace_binding(db.pool())
            .await
            .unwrap();
        for table in seedable_tables() {
            sqlx::query(sqlx::AssertSqlSafe(format!(
                "INSERT INTO {table} (id, workspace_id) VALUES (?, '')"
            )))
            .bind(format!("{table}-unassigned"))
            .execute(db.pool())
            .await
            .unwrap();
        }
        let identity = local_content_identity(db.pool()).await.unwrap().unwrap();
        backfill_content_identity(db.pool(), &identity)
            .await
            .unwrap();

        crate::claim_cloudsync_workspace(db.pool(), "account-1")
            .await
            .unwrap();

        let identity = local_content_identity(db.pool()).await.unwrap().unwrap();
        assert_eq!(identity.workspace_id, "account-1");
        for table in seedable_tables() {
            let (workspace, owner) = ids(&db, table, "unassigned").await;
            assert_eq!(workspace, "account-1", "{table} moved by the claim");
            if let Some(owner) = owner {
                assert_ne!(owner, local, "{table} owner rekeyed by the claim");
            }
        }
    }
}

#[cfg(test)]
mod create_path_tests {
    use super::*;
    use anlg_db_core::Db;

    const LEGACY_DEFAULT_USER: &str = "00000000-0000-0000-0000-000000000000";

    /// C1.1: rows created through the normal Rust create paths, on a device
    /// that has a local identity, never have an empty workspace or owner, and
    /// are in the local workspace.
    #[tokio::test]
    async fn rust_create_paths_never_write_empty_ids() {
        let db = Db::connect_memory_plain().await.unwrap();
        crate::prepare_schema(&db).await.unwrap();
        let identity = local_content_identity(db.pool()).await.unwrap().unwrap();
        let now = "2026-11-16T00:00:00.000Z";
        let mut connection = db.pool().acquire().await.unwrap();
        let conn = &mut *connection;

        crate::insert_session(conn, "s1", "Planning", LEGACY_DEFAULT_USER, "", "", now)
            .await
            .unwrap();
        crate::insert_empty_session_note(conn, "s1", "{}", now)
            .await
            .unwrap();
        crate::upsert_session_owner_human(conn, "s1", now)
            .await
            .unwrap();
        crate::insert_owner_session_participant(conn, "p-owner", "s1", now)
            .await
            .unwrap();
        crate::create_human(
            conn,
            "h1",
            LEGACY_DEFAULT_USER,
            "Alice",
            "a@example.com",
            now,
        )
        .await
        .unwrap();
        crate::create_organization(conn, "o1", LEGACY_DEFAULT_USER, "ACME", now)
            .await
            .unwrap();
        crate::insert_manual_session_participant(conn, "p-alice", "s1", "h1", "manual", now)
            .await
            .unwrap();
        crate::upsert_tag(conn, &identity.owner_user_id, "launch", now)
            .await
            .unwrap();
        crate::upsert_session_tag(conn, "s1", &identity.owner_user_id, "launch", now)
            .await
            .unwrap();
        crate::insert_note_attachment(
            conn,
            "att-1",
            "a.png",
            "a.png",
            "image/png",
            1,
            "sha",
            "src-1",
            "s1",
        )
        .await
        .unwrap();
        crate::insert_session_audio_attachment(
            conn,
            "audio-1",
            "audio.wav",
            "audio/wav",
            1,
            "sha",
            "s1",
        )
        .await
        .unwrap();
        crate::insert_folder_if_missing(conn, "f1", "Projects")
            .await
            .unwrap();
        crate::insert_folder_attachment(
            conn,
            "fa-1",
            "Projects",
            "b.pdf",
            "b.pdf",
            "application/pdf",
            1,
            "sha",
            "src-2",
        )
        .await
        .unwrap();
        crate::insert_pending_session_proposal(
            conn,
            "prop-1",
            "s1",
            "summary_replace",
            "doc",
            "",
            "old",
            "new",
            "cli",
        )
        .await
        .unwrap();
        crate::insert_batch_transcript(
            conn,
            &crate::BatchTranscriptInsert {
                id: "t-batch".into(),
                session_id: "s1".into(),
                owner_user_id: identity.owner_user_id.clone(),
                created_at: now.into(),
                started_at_ms: 0.0,
                memo: String::new(),
                provider: "local".into(),
                model: "m".into(),
                words_json: "[]".into(),
                speaker_hints_json: "[]".into(),
            },
            &crate::BatchTranscriptReplace {
                replace_session: false,
                replace_transcript_id: None,
            },
        )
        .await
        .unwrap();
        drop(connection);
        crate::append_live_transcript_deltas(
            db.pool(),
            &crate::LiveTranscriptInsert {
                id: "t-live".into(),
                session_id: "s1".into(),
                owner_user_id: identity.owner_user_id.clone(),
                created_at: now.into(),
                started_at_ms: 0,
                memo: String::new(),
                provider: None,
                model: None,
            },
            &[],
        )
        .await
        .unwrap();

        assert_eq!(
            count_unassigned_content_rows(db.pool()).await.unwrap(),
            Vec::<(String, i64)>::new(),
            "no content row has an empty workspace_id or owner_user_id"
        );
        for table in WORKSPACE_CONTENT_TABLES {
            let foreign: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
                "SELECT count(*) FROM {table} WHERE workspace_id <> ?"
            )))
            .bind(&identity.workspace_id)
            .fetch_one(db.pool())
            .await
            .unwrap();
            assert_eq!(foreign, 0, "{table} rows are in the local workspace");
        }
        let created: i64 = sqlx::query_scalar(
            "SELECT (SELECT count(*) FROM sessions) + (SELECT count(*) FROM transcripts)
                  + (SELECT count(*) FROM session_proposals) + (SELECT count(*) FROM tags)",
        )
        .fetch_one(db.pool())
        .await
        .unwrap();
        assert!(created >= 5, "the create paths wrote rows ({created})");
    }
}
