//! Builds the local access context at startup (A1.3) and persists the app
//! lock setting. Runs in Rust before any window exists.

use std::sync::Arc;
use std::time::Duration;

use anlg_aspc::AspcState;
use anlg_db_core::Db;
use anlg_local_accounts::{KeychainStore, SecretStore};

const ACTOR_ID_KEY: &str = "local-actor-id";
const KEYCHAIN_TIMEOUT: Duration = Duration::from_secs(3);
pub const LOCK_SETTING_ID: &str = "lock_app";
const LEGACY_SETTINGS_ID: &str = "legacy_settings_document";

/// Keychain service for the access context's items.
pub fn keychain_service(identifier: &str) -> String {
    format!("{identifier}.access")
}

/// The stable local actor id: read from the Keychain, or created once and
/// stored there. Returns `None` if the Keychain is unusable.
pub fn load_or_create_actor_id(store: &dyn SecretStore) -> Option<String> {
    match store.get(ACTOR_ID_KEY) {
        Ok(Some(id)) if !id.trim().is_empty() => Some(id),
        Ok(_) => {
            let id = format!("local-{}", uuid::Uuid::new_v4());
            match store.set(ACTOR_ID_KEY, &id) {
                Ok(()) => Some(id),
                Err(error) => {
                    tracing::warn!(%error, "aspc_actor_id_store_failed");
                    None
                }
            }
        }
        Err(error) => {
            tracing::warn!(%error, "aspc_actor_id_read_failed");
            None
        }
    }
}

/// The Keychain can block on an access prompt (dev builds with ad-hoc
/// signatures). Startup waits at most `KEYCHAIN_TIMEOUT`, then uses an id
/// for this launch only.
fn actor_id_from_keychain(identifier: &str) -> String {
    let service = keychain_service(identifier);
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let store = KeychainStore::new(service);
        let _ = tx.send(load_or_create_actor_id(&store));
    });
    match rx.recv_timeout(KEYCHAIN_TIMEOUT) {
        Ok(Some(id)) => id,
        _ => {
            tracing::warn!("aspc_actor_id_ephemeral");
            format!("ephemeral-{}", uuid::Uuid::new_v4())
        }
    }
}

/// Reuses the device's personal workspace, else creates one owned by the
/// actor. Inserts a row; never changes the schema.
pub async fn ensure_personal_workspace(db: &Db, actor_id: &str) -> Result<String, sqlx::Error> {
    let existing: Option<String> = sqlx::query_scalar(
        "SELECT id FROM workspaces
         WHERE kind = 'personal' AND deleted_at IS NULL AND id != ''
         ORDER BY (owner_user_id = ?) DESC, created_at, id
         LIMIT 1",
    )
    .bind(actor_id)
    .fetch_optional(db.pool())
    .await?;
    if let Some(id) = existing {
        return Ok(id);
    }
    let id = format!("local-workspace-{}", uuid::Uuid::new_v4());
    sqlx::query(
        "INSERT INTO workspaces (id, owner_user_id, kind, name) VALUES (?, ?, 'personal', 'Personal')",
    )
    .bind(&id)
    .bind(actor_id)
    .execute(db.pool())
    .await?;
    Ok(id)
}

fn parse_bool(value_json: &str) -> Option<bool> {
    match serde_json::from_str::<serde_json::Value>(value_json).ok()? {
        serde_json::Value::Bool(value) => Some(value),
        _ => None,
    }
}

/// Reads the `lock_app` setting the way the frontend does: the direct
/// `app_settings` row, else the legacy settings document's
/// `general.lock_app`, else off.
pub async fn read_lock_setting(db: &Db) -> Result<bool, sqlx::Error> {
    let rows: Vec<(String, String)> =
        sqlx::query_as("SELECT id, value_json FROM app_settings WHERE id IN (?, ?)")
            .bind(LOCK_SETTING_ID)
            .bind(LEGACY_SETTINGS_ID)
            .fetch_all(db.pool())
            .await?;
    let direct = rows
        .iter()
        .find(|(id, _)| id == LOCK_SETTING_ID)
        .and_then(|(_, value)| parse_bool(value));
    if let Some(value) = direct {
        return Ok(value);
    }
    let legacy = rows
        .iter()
        .find(|(id, _)| id == LEGACY_SETTINGS_ID)
        .and_then(|(_, value)| serde_json::from_str::<serde_json::Value>(value).ok())
        .and_then(|doc| doc.get("general")?.get(LOCK_SETTING_ID)?.as_bool());
    Ok(legacy.unwrap_or(false))
}

pub async fn write_lock_setting(db: &Db, enabled: bool) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO app_settings (id, value_json) VALUES (?, ?)
         ON CONFLICT(id) DO UPDATE SET
           value_json = excluded.value_json,
           updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
    )
    .bind(LOCK_SETTING_ID)
    .bind(if enabled { "true" } else { "false" })
    .execute(db.pool())
    .await?;
    Ok(())
}

/// Startup: the local Owner of the personal workspace, with the lock state
/// from the stored setting (locked when it is on). If the setting can't be
/// read the lock is treated as off and the error is logged; the database it
/// lives in is then unreadable too, so nothing is exposed by it.
pub async fn bootstrap(db: &Db, identifier: &str) -> Arc<AspcState> {
    let identifier = identifier.to_string();
    let actor_id = tokio::task::spawn_blocking(move || actor_id_from_keychain(&identifier))
        .await
        .unwrap_or_else(|_| format!("ephemeral-{}", uuid::Uuid::new_v4()));
    bootstrap_with_actor(db, actor_id).await
}

pub async fn bootstrap_with_actor(db: &Db, actor_id: String) -> Arc<AspcState> {
    let workspace_id = match ensure_personal_workspace(db, &actor_id).await {
        Ok(id) => id,
        Err(error) => {
            // An empty workspace id makes every check deny (fail closed).
            tracing::error!(%error, "aspc_personal_workspace_failed");
            String::new()
        }
    };
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
    use anlg_local_accounts::MemoryStore;

    async fn test_db() -> Db {
        let db = Db::connect_memory_plain().await.unwrap();
        sqlx::raw_sql(
            "CREATE TABLE workspaces (
               id TEXT PRIMARY KEY NOT NULL, owner_user_id TEXT NOT NULL DEFAULT '',
               kind TEXT NOT NULL DEFAULT 'personal', name TEXT NOT NULL DEFAULT '',
               created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
               updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
               deleted_at TEXT);
             CREATE TABLE app_settings (
               id TEXT PRIMARY KEY NOT NULL, value_json TEXT NOT NULL DEFAULT 'null',
               created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
               updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')));",
        )
        .execute(db.pool())
        .await
        .unwrap();
        db
    }

    #[test]
    fn actor_id_is_created_once() {
        let store = MemoryStore::default();
        let first = load_or_create_actor_id(&store).unwrap();
        assert!(first.starts_with("local-"));
        assert_eq!(load_or_create_actor_id(&store).unwrap(), first);
    }

    #[tokio::test]
    async fn personal_workspace_is_reused_or_created() {
        let db = test_db().await;
        let created = ensure_personal_workspace(&db, "actor").await.unwrap();
        assert_eq!(
            ensure_personal_workspace(&db, "actor").await.unwrap(),
            created
        );
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM workspaces")
            .fetch_one(db.pool())
            .await
            .unwrap();
        assert_eq!(count, 1);

        let db = test_db().await;
        sqlx::query("INSERT INTO workspaces (id, owner_user_id, kind) VALUES ('shared-1', 'x', 'shared'), ('account-1', 'user', 'personal')")
            .execute(db.pool())
            .await
            .unwrap();
        assert_eq!(
            ensure_personal_workspace(&db, "actor").await.unwrap(),
            "account-1"
        );
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
    async fn bootstrap_starts_locked_when_the_setting_is_on() {
        let db = test_db().await;
        let state = bootstrap_with_actor(&db, "actor".into()).await;
        assert!(state.is_unlocked());
        let ctx = state.context(None);
        assert_eq!(ctx.role, Some(anlg_aspc::Role::Owner));
        assert!(!ctx.workspace_id.is_empty());

        write_lock_setting(&db, true).await.unwrap();
        let state = bootstrap_with_actor(&db, "actor".into()).await;
        assert!(!state.is_unlocked());
        assert_eq!(state.context(None).workspace_id, ctx.workspace_id);
    }
}
