//! Builds the local access context at startup (A1.3) and persists the app
//! lock setting. Runs in Rust before any window exists.
//!
//! The actor and workspace are the data model's own local identity
//! (`anlg_db_app::local_content_identity`: the CloudSync binding's workspace,
//! and the owner id `insert_session` writes). Phase 2a used a Keychain actor
//! id and a new `local-workspace-…` row instead; rows stamped with those ids
//! would make CloudSync's claim fail (`ForeignWorkspace`) and would not be
//! rekeyed to the account on sign-in, so Phase 2b (C1) replaced them.

use std::sync::Arc;

use anlg_aspc::AspcState;
use anlg_db_core::Db;

pub use anlg_db_app::app_lock_settings::{
    DEFAULT_IDLE_MINUTES, IDLE_MINUTE_CHOICES, IDLE_SETTING_ID, LOCK_SETTING_ID,
};

/// The local actor and workspace ids, or empty strings (every check denies)
/// when the database has no binding yet. On a fresh install the schema is not
/// ready when the app starts; the db plugin sets the identity once it is
/// (`tauri_plugin_db` startup, after the C1 backfill).
pub async fn local_identity(db: &Db) -> (String, String) {
    match anlg_db_app::local_content_identity(db.pool()).await {
        Ok(Some(identity)) => (identity.owner_user_id, identity.workspace_id),
        Ok(None) => {
            tracing::info!("aspc_identity_pending_schema");
            (String::new(), String::new())
        }
        Err(error) => {
            tracing::info!(%error, "aspc_identity_pending_schema");
            (String::new(), String::new())
        }
    }
}

pub async fn read_lock_setting(db: &Db) -> Result<bool, sqlx::Error> {
    anlg_db_app::app_lock_settings::read_lock_setting(db.pool()).await
}

pub async fn write_lock_setting(db: &Db, enabled: bool) -> Result<(), sqlx::Error> {
    anlg_db_app::app_lock_settings::write_lock_setting(db.pool(), enabled).await
}

pub async fn read_idle_setting(db: &Db) -> Result<u32, sqlx::Error> {
    anlg_db_app::app_lock_settings::read_idle_setting(db.pool()).await
}

pub async fn write_idle_setting(db: &Db, minutes: u32) -> Result<(), sqlx::Error> {
    anlg_db_app::app_lock_settings::write_idle_setting(db.pool(), minutes).await
}

/// Startup: the local Owner of the local workspace, with the lock state
/// from the stored setting (locked when it is on). If the setting can't be
/// read the lock is treated as off and the error is logged; the database it
/// lives in is then unreadable too, so nothing is exposed by it.
pub async fn bootstrap(db: &Db) -> Arc<AspcState> {
    let (actor_id, workspace_id) = local_identity(db).await;
    let lock_enabled = read_lock_setting(db).await.unwrap_or_else(|error| {
        tracing::error!(%error, "aspc_lock_setting_read_failed");
        false
    });
    Arc::new(AspcState::local_owner(
        actor_id,
        workspace_id,
        anlg_host::fingerprint(),
        lock_enabled,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn test_db() -> Db {
        let db = Db::connect_memory_plain().await.unwrap();
        sqlx::raw_sql(
            "CREATE TABLE app_settings (
               id TEXT PRIMARY KEY NOT NULL, value_json TEXT NOT NULL DEFAULT 'null',
               created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
               updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')));
             CREATE TABLE local_library_connections (
               account_user_id TEXT PRIMARY KEY NOT NULL,
               library_workspace_id TEXT NOT NULL DEFAULT '',
               active INTEGER NOT NULL DEFAULT 0);",
        )
        .execute(db.pool())
        .await
        .unwrap();
        db
    }

    async fn bind(db: &Db, workspace_id: &str) {
        sqlx::query(
            "INSERT INTO app_settings (id, value_json) VALUES ('cloudsync_workspace_binding', ?)",
        )
        .bind(format!(
            "{{\"workspace_id\":\"{workspace_id}\",\"account_user_id\":null}}"
        ))
        .execute(db.pool())
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn identity_comes_from_the_cloudsync_binding() {
        let db = test_db().await;
        bind(&db, "local-binding").await;
        assert_eq!(
            local_identity(&db).await,
            ("local-binding".to_string(), "local-binding".to_string())
        );
    }

    #[tokio::test]
    async fn no_binding_or_schema_means_no_identity() {
        let db = test_db().await;
        assert_eq!(local_identity(&db).await, (String::new(), String::new()));
        let bare = Db::connect_memory_plain().await.unwrap();
        assert_eq!(local_identity(&bare).await, (String::new(), String::new()));
        let state = bootstrap(&bare).await;
        assert!(state.scope().is_none());
    }

    #[tokio::test]
    async fn lock_setting_round_trip_and_legacy() {
        let db = test_db().await;
        assert!(!read_lock_setting(&db).await.unwrap());
        sqlx::query("INSERT INTO app_settings (id, value_json) VALUES ('legacy_settings_document', '{\"general\":{\"lock_app\":true}}')")
            .execute(db.pool())
            .await
            .unwrap();
        assert!(read_lock_setting(&db).await.unwrap());
        write_lock_setting(&db, false).await.unwrap();
        assert!(!read_lock_setting(&db).await.unwrap());
        write_lock_setting(&db, true).await.unwrap();
        assert!(read_lock_setting(&db).await.unwrap());
    }

    #[tokio::test]
    async fn idle_setting_defaults_and_round_trips() {
        let db = test_db().await;
        assert_eq!(read_idle_setting(&db).await.unwrap(), DEFAULT_IDLE_MINUTES);
        write_idle_setting(&db, 5).await.unwrap();
        assert_eq!(read_idle_setting(&db).await.unwrap(), 5);
        write_idle_setting(&db, 0).await.unwrap();
        assert_eq!(read_idle_setting(&db).await.unwrap(), 0);
        // A value the screen never offers falls back to the default.
        sqlx::query("UPDATE app_settings SET value_json = '7' WHERE id = 'lock_idle_minutes'")
            .execute(db.pool())
            .await
            .unwrap();
        assert_eq!(read_idle_setting(&db).await.unwrap(), DEFAULT_IDLE_MINUTES);
        sqlx::query("UPDATE app_settings SET value_json = '\"x\"' WHERE id = 'lock_idle_minutes'")
            .execute(db.pool())
            .await
            .unwrap();
        assert_eq!(read_idle_setting(&db).await.unwrap(), DEFAULT_IDLE_MINUTES);
    }

    #[tokio::test]
    async fn bootstrap_starts_locked_when_the_setting_is_on() {
        let db = test_db().await;
        bind(&db, "local-binding").await;
        let state = bootstrap(&db).await;
        assert!(state.is_unlocked());
        let ctx = state.context(None);
        assert_eq!(ctx.role, Some(anlg_aspc::Role::Owner));
        assert_eq!(ctx.workspace_id, "local-binding");

        write_lock_setting(&db, true).await.unwrap();
        let state = bootstrap(&db).await;
        assert!(!state.is_unlocked());
        assert_eq!(state.context(None).workspace_id, ctx.workspace_id);
    }
}
