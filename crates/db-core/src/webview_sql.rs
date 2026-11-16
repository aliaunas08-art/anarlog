//! SQLite authorizer for SQL that arrives from the webview (ASPC A2.4).
//!
//! The webview is untrusted, but the desktop frontend still talks to SQLite
//! through raw SQL (`db:execute*`, live queries). Every such statement runs on
//! a pool connection that has this authorizer installed for the duration of
//! the statement, and the authorizer is removed before the connection goes back
//! to the pool, so internal Rust paths (migrations, CloudSync, importers) are
//! never affected.
//!
//! If a webview statement is cancelled while the authorizer is installed, a
//! task clears the authorizer (after the in-flight statement finishes) before
//! the connection goes back to the pool; if that fails the connection is
//! closed and the policy leaked so the callback's context stays valid.

use std::ffi::{CStr, c_char, c_int, c_void};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};

use anlg_aspc as aspc;
use aspc::{AccessContext, Action, DataType};

use libsqlite3_sys as ffi;
use sqlx::pool::PoolConnection;
use sqlx::{Sqlite, SqliteConnection, SqlitePool};

/// Read-only pragmas the webview may run without an argument.
const READ_ONLY_PRAGMAS: &[&str] = &[
    "data_version",
    "schema_version",
    "user_version",
    "foreign_keys",
    "journal_mode",
    "page_count",
    "page_size",
    "freelist_count",
];

/// Introspection pragmas whose argument names a table or index (not a value to set).
const INTROSPECTION_PRAGMAS: &[&str] = &[
    "table_info",
    "table_xinfo",
    "table_list",
    "index_list",
    "index_info",
    "index_xinfo",
    "foreign_key_list",
];

const DENIED_FUNCTIONS: &[&str] = &["load_extension", "fts3_tokenizer"];

/// Tables the lock screen may read while the app is locked: the settings the
/// shared boot path needs (theme, language, the `lock_app` flag) to render
/// the lock screen. Reads only; every write is denied while locked.
const LOCK_SCREEN_READABLE: &[&str] = &[
    "app_settings",
    "synced_preferences",
    "sqlite_master",
    "sqlite_schema",
    "sqlite_temp_master",
];

/// The data type of each app table (A2.4). Unknown tables, including
/// CloudSync's `*_cloudsync` shadow tables written by triggers, fall back to
/// `Notes`, the content type every role that can use the app reads.
pub fn table_data_type(table: &str) -> DataType {
    let table = table.to_ascii_lowercase();
    match table.as_str() {
        "transcripts" | "transcript_live_deltas" | "transcript_live_state" => DataType::Transcript,
        "local_recording_import_jobs"
        | "local_recording_import_worker"
        | "voiceprint_candidates"
        | "voiceprint_exemplars"
        | "voiceprint_exemplars_next" => DataType::Recording,
        "action_items" => DataType::Actions,
        "app_settings"
        | "synced_preferences"
        | "managed_app_settings"
        | "templates"
        | "calendars"
        | "workspaces"
        | "workspace_memberships"
        | "storage_migration_state" => DataType::Settings,
        "api_keys" | "webhook_endpoints" => DataType::Keys,
        _ if table.starts_with("e2ee_") => DataType::Keys,
        _ => DataType::Notes,
    }
}

/// Policy, access context and lock state for webview SQL. One instance is
/// shared by every webview execution path of a database.
#[derive(Debug, Default)]
pub struct WebviewSqlAuthorizer {
    locked: AtomicBool,
    /// Set by the app at startup. Until then (and in tests that never set
    /// it) only the static policy applies, which is the Phase 1 behaviour.
    context: RwLock<Option<AccessContext>>,
}

impl WebviewSqlAuthorizer {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// Mirrors the Rust-held app lock (A4.2). While locked, every webview
    /// statement is denied except reads of [`LOCK_SCREEN_READABLE`].
    /// Default: unlocked.
    pub fn set_locked(&self, locked: bool) {
        self.locked.store(locked, Ordering::SeqCst);
    }

    pub fn is_locked(&self) -> bool {
        self.locked.load(Ordering::SeqCst)
    }

    /// The actor whose rights every webview statement is checked against.
    /// `window` is ignored here (the window is checked by the `db:*`
    /// command guard) and `unlocked` comes from [`Self::set_locked`].
    pub fn set_context(&self, mut context: AccessContext) {
        context.window = None;
        *self.context.write().unwrap_or_else(|e| e.into_inner()) = Some(context);
    }

    fn decide(&self, request: &AuthRequest<'_>) -> Decision {
        if self.is_locked() {
            return decide_locked(request);
        }
        if decide(request) == Decision::Deny {
            return Decision::Deny;
        }
        let Some((data, action)) = data_access(request) else {
            return Decision::Allow;
        };
        let guard = self.context.read().unwrap_or_else(|e| e.into_inner());
        let Some(context) = guard.as_ref() else {
            return Decision::Allow;
        };
        let mut context = context.clone();
        context.unlocked = true;
        match aspc::authorize(&context, &context.workspace_id, data, action) {
            Ok(_) => Decision::Allow,
            Err(_) => Decision::Deny,
        }
    }
}

/// Reads map to `Read`; inserts, updates and deletes to `Edit`.
fn data_access(request: &AuthRequest<'_>) -> Option<(DataType, Action)> {
    let action = match request.action {
        ffi::SQLITE_READ => Action::Read,
        ffi::SQLITE_INSERT | ffi::SQLITE_UPDATE | ffi::SQLITE_DELETE => Action::Edit,
        _ => return None,
    };
    let table = request.arg1?;
    Some((table_data_type(table), action))
}

fn decide_locked(request: &AuthRequest<'_>) -> Decision {
    match request.action {
        ffi::SQLITE_SELECT => Decision::Allow,
        ffi::SQLITE_READ if eq_any(request.arg1, LOCK_SCREEN_READABLE) => Decision::Allow,
        ffi::SQLITE_FUNCTION if !eq_any(request.arg2, DENIED_FUNCTIONS) => Decision::Allow,
        _ => Decision::Deny,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Decision {
    Allow,
    Deny,
}

#[derive(Debug)]
pub(crate) struct AuthRequest<'a> {
    pub action: c_int,
    pub arg1: Option<&'a str>,
    pub arg2: Option<&'a str>,
}

fn is_audit_table(name: Option<&str>) -> bool {
    name.is_some_and(|name| name.len() >= 5 && name[..5].eq_ignore_ascii_case("audit"))
}

fn is_sqlite_internal_table(name: Option<&str>) -> bool {
    name.is_some_and(|name| name.len() >= 7 && name[..7].eq_ignore_ascii_case("sqlite_"))
}

fn eq_any(name: Option<&str>, list: &[&str]) -> bool {
    name.is_some_and(|name| list.iter().any(|item| item.eq_ignore_ascii_case(name)))
}

pub(crate) fn decide(request: &AuthRequest<'_>) -> Decision {
    use Decision::{Allow, Deny};

    match request.action {
        ffi::SQLITE_SELECT | ffi::SQLITE_RECURSIVE => Allow,
        ffi::SQLITE_TRANSACTION | ffi::SQLITE_SAVEPOINT => Allow,
        ffi::SQLITE_READ => {
            if is_audit_table(request.arg1) {
                Deny
            } else {
                Allow
            }
        }
        ffi::SQLITE_INSERT | ffi::SQLITE_UPDATE | ffi::SQLITE_DELETE => {
            if is_audit_table(request.arg1) || is_sqlite_internal_table(request.arg1) {
                Deny
            } else {
                Allow
            }
        }
        ffi::SQLITE_FUNCTION => {
            if eq_any(request.arg2, DENIED_FUNCTIONS) {
                Deny
            } else {
                Allow
            }
        }
        ffi::SQLITE_PRAGMA => {
            let allowed = (eq_any(request.arg1, READ_ONLY_PRAGMAS) && request.arg2.is_none())
                || eq_any(request.arg1, INTROSPECTION_PRAGMAS);
            if allowed { Allow } else { Deny }
        }
        // No schema changes from the webview. CloudSync's change triggers only
        // SELECT extension functions and write its own `*_cloudsync` tables,
        // which the arms above allow.
        ffi::SQLITE_CREATE_INDEX
        | ffi::SQLITE_CREATE_TABLE
        | ffi::SQLITE_CREATE_TEMP_INDEX
        | ffi::SQLITE_CREATE_TEMP_TABLE
        | ffi::SQLITE_CREATE_TEMP_TRIGGER
        | ffi::SQLITE_CREATE_TEMP_VIEW
        | ffi::SQLITE_CREATE_TRIGGER
        | ffi::SQLITE_CREATE_VIEW
        | ffi::SQLITE_DROP_INDEX
        | ffi::SQLITE_DROP_TABLE
        | ffi::SQLITE_DROP_TEMP_INDEX
        | ffi::SQLITE_DROP_TEMP_TABLE
        | ffi::SQLITE_DROP_TEMP_TRIGGER
        | ffi::SQLITE_DROP_TEMP_VIEW
        | ffi::SQLITE_DROP_TRIGGER
        | ffi::SQLITE_DROP_VIEW
        | ffi::SQLITE_ALTER_TABLE
        | ffi::SQLITE_REINDEX
        | ffi::SQLITE_ANALYZE
        | ffi::SQLITE_CREATE_VTABLE
        | ffi::SQLITE_DROP_VTABLE
        | ffi::SQLITE_ATTACH
        | ffi::SQLITE_DETACH => Deny,
        // Fail closed on anything unknown (including SQLITE_COPY).
        _ => Deny,
    }
}

unsafe fn opt_str<'a>(value: *const c_char) -> Option<&'a str> {
    if value.is_null() {
        None
    } else {
        // SAFETY: SQLite passes NUL-terminated strings valid for the callback.
        unsafe { CStr::from_ptr(value) }.to_str().ok()
    }
}

unsafe extern "C" fn authorizer_callback(
    context: *mut c_void,
    action: c_int,
    arg1: *const c_char,
    arg2: *const c_char,
    _database: *const c_char,
    _accessor: *const c_char,
) -> c_int {
    let result = std::panic::catch_unwind(|| {
        // SAFETY: `context` is an `Arc<WebviewSqlAuthorizer>` pointer that is
        // kept alive while installed (see `AuthorizedConnection`).
        let policy = unsafe { &*(context as *const WebviewSqlAuthorizer) };
        let request = AuthRequest {
            action,
            arg1: unsafe { opt_str(arg1) },
            arg2: unsafe { opt_str(arg2) },
        };
        policy.decide(&request)
    });
    match result {
        Ok(Decision::Allow) => ffi::SQLITE_OK,
        _ => ffi::SQLITE_DENY,
    }
}

/// A pool connection with the webview authorizer installed.
///
/// Call [`AuthorizedConnection::release`] when done. Dropping it without
/// releasing clears the authorizer in a background task.
pub struct AuthorizedConnection {
    connection: Option<PoolConnection<Sqlite>>,
    policy: Arc<WebviewSqlAuthorizer>,
    installed: bool,
}

impl AuthorizedConnection {
    pub async fn acquire(
        pool: &SqlitePool,
        policy: &Arc<WebviewSqlAuthorizer>,
    ) -> Result<Self, sqlx::Error> {
        let connection = pool.acquire().await?;
        let mut this = Self {
            connection: Some(connection),
            policy: Arc::clone(policy),
            installed: false,
        };
        this.install().await?;
        Ok(this)
    }

    async fn install(&mut self) -> Result<(), sqlx::Error> {
        let connection = self.connection.as_mut().expect("connection present");
        let mut handle = connection.lock_handle().await?;
        let raw = handle.as_raw_handle().as_ptr();
        let context = Arc::as_ptr(&self.policy) as *mut c_void;
        // Mark installed before the call so a failure path still closes the
        // connection rather than returning a half-configured handle.
        self.installed = true;
        // SAFETY: the handle is locked away from the worker thread, and the
        // context outlives the installation (released or leaked on drop).
        let rc = unsafe { ffi::sqlite3_set_authorizer(raw, Some(authorizer_callback), context) };
        if rc != ffi::SQLITE_OK {
            return Err(sqlx::Error::Protocol(format!(
                "sqlite3_set_authorizer failed: {rc}"
            )));
        }
        Ok(())
    }

    pub fn connection(&mut self) -> &mut SqliteConnection {
        self.connection.as_mut().expect("connection present")
    }

    /// Remove the authorizer and return the connection to the pool.
    pub async fn release(mut self) -> Result<(), sqlx::Error> {
        let connection = self.connection.as_mut().expect("connection present");
        clear_authorizer(connection).await?;
        self.installed = false;
        Ok(())
    }
}

async fn clear_authorizer(connection: &mut SqliteConnection) -> Result<(), sqlx::Error> {
    let mut handle = connection.lock_handle().await?;
    let raw = handle.as_raw_handle().as_ptr();
    // SAFETY: handle locked; clearing the authorizer takes no context.
    let rc = unsafe { ffi::sqlite3_set_authorizer(raw, None, std::ptr::null_mut()) };
    drop(handle);
    if rc != ffi::SQLITE_OK {
        return Err(sqlx::Error::Protocol(format!(
            "sqlite3_set_authorizer reset failed: {rc}"
        )));
    }
    Ok(())
}

impl Drop for AuthorizedConnection {
    fn drop(&mut self) {
        if !self.installed {
            return;
        }
        let Some(mut connection) = self.connection.take() else {
            return;
        };
        let policy = Arc::clone(&self.policy);
        // Cancelled mid-statement. The worker finishes the in-flight statement
        // (still under the authorizer, with `policy` kept alive here), then the
        // authorizer is cleared before the connection returns to the pool. If
        // that is impossible, the connection is closed instead.
        match tokio::runtime::Handle::try_current() {
            Ok(runtime) => {
                runtime.spawn(async move {
                    if let Err(error) = clear_authorizer(&mut connection).await {
                        tracing::error!(%error, "webview_sql_authorizer_reset_failed");
                        connection.close_on_drop();
                        std::mem::forget(policy);
                    }
                });
            }
            Err(_) => {
                tracing::error!("webview_sql_cancelled_without_async_runtime");
                connection.close_on_drop();
                std::mem::forget(policy);
            }
        }
    }
}

/// Run `f` on a connection with the webview authorizer installed, and always
/// release it afterwards (also when `f` returns an error).
pub async fn with_authorized_connection<T, E, F>(
    pool: &SqlitePool,
    policy: &Arc<WebviewSqlAuthorizer>,
    f: F,
) -> Result<T, E>
where
    E: From<sqlx::Error>,
    F: for<'c> FnOnce(
        &'c mut SqliteConnection,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<T, E>> + Send + 'c>,
    >,
{
    let mut connection = AuthorizedConnection::acquire(pool, policy).await?;
    let result = f(connection.connection()).await;
    connection.release().await?;
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn test_pool() -> SqlitePool {
        let db = crate::Db::connect_memory_plain().await.unwrap();
        let pool = db.pool().clone();
        std::mem::forget(db);
        sqlx::query(
            "CREATE TABLE notes (id TEXT PRIMARY KEY, body TEXT);
             CREATE TABLE app_settings (id TEXT PRIMARY KEY, value_json TEXT);
             CREATE TABLE synced_preferences (id TEXT PRIMARY KEY, value_json TEXT);
             CREATE TABLE webhook_endpoints (id TEXT PRIMARY KEY, secret TEXT);
             INSERT INTO app_settings VALUES ('lock_app', 'true');
             CREATE TABLE audit_x (id INTEGER PRIMARY KEY, entry TEXT);
             INSERT INTO audit_x (entry) VALUES ('secret');",
        )
        .execute(&pool)
        .await
        .unwrap();
        pool
    }

    async fn run(
        pool: &SqlitePool,
        policy: &Arc<WebviewSqlAuthorizer>,
        sql: &str,
    ) -> Result<(), sqlx::Error> {
        let sql = sql.to_string();
        with_authorized_connection(pool, policy, move |conn| {
            Box::pin(async move {
                sqlx::raw_sql(sqlx::AssertSqlSafe(sql))
                    .execute(conn)
                    .await?;
                Ok::<_, sqlx::Error>(())
            })
        })
        .await
    }

    fn is_not_authorized(result: Result<(), sqlx::Error>) -> bool {
        match result {
            Err(error) => {
                let message = error.to_string();
                message.contains("not authorized") || message.contains("prohibited")
            }
            Ok(()) => false,
        }
    }

    #[tokio::test]
    async fn denies_dangerous_statements() {
        let pool = test_pool().await;
        let policy = WebviewSqlAuthorizer::new();
        for sql in [
            "ATTACH DATABASE ':memory:' AS other",
            "SELECT load_extension('/tmp/evil')",
            "PRAGMA writable_schema=1",
            "PRAGMA writable_schema",
            "SELECT * FROM audit_x",
            "SELECT entry FROM AUDIT_X",
            "INSERT INTO audit_x (entry) VALUES ('x')",
            "CREATE TABLE evil (id INTEGER)",
            "DROP TABLE notes",
            "ALTER TABLE notes ADD COLUMN extra TEXT",
            "SELECT 1; ATTACH DATABASE ':memory:' AS other",
        ] {
            assert!(
                is_not_authorized(run(&pool, &policy, sql).await),
                "expected denial for {sql}"
            );
        }
        // SQLite refuses this itself even before the authorizer's own denial.
        assert!(
            run(
                &pool,
                &policy,
                "UPDATE sqlite_master SET sql = '' WHERE name = 'notes'"
            )
            .await
            .is_err()
        );
        let internal = AuthRequest {
            action: ffi::SQLITE_UPDATE,
            arg1: Some("sqlite_schema"),
            arg2: Some("sql"),
        };
        assert_eq!(decide(&internal), Decision::Deny);
    }

    #[tokio::test]
    async fn allows_normal_app_queries() {
        let pool = test_pool().await;
        let policy = WebviewSqlAuthorizer::new();
        run(
            &pool,
            &policy,
            "INSERT INTO notes (id, body) VALUES ('a', 'hello')",
        )
        .await
        .unwrap();
        run(
            &pool,
            &policy,
            "UPDATE notes SET body = 'bye' WHERE id = 'a'",
        )
        .await
        .unwrap();
        run(
            &pool,
            &policy,
            "SELECT n.id, count(*) FROM notes n GROUP BY n.id",
        )
        .await
        .unwrap();
        run(&pool, &policy, "WITH RECURSIVE c(x) AS (SELECT 1 UNION ALL SELECT x + 1 FROM c WHERE x < 3) SELECT * FROM c")
            .await
            .unwrap();
        run(&pool, &policy, "PRAGMA data_version").await.unwrap();
        run(
            &pool,
            &policy,
            "SELECT name FROM pragma_table_info('notes')",
        )
        .await
        .unwrap();
        run(&pool, &policy, "DELETE FROM notes WHERE id = 'a'")
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn denies_everything_but_lock_screen_settings_when_locked() {
        let pool = test_pool().await;
        let policy = WebviewSqlAuthorizer::new();
        policy.set_locked(true);
        for sql in [
            "SELECT * FROM notes",
            "SELECT id FROM notes WHERE id = 'a'",
            "INSERT INTO notes (id, body) VALUES ('b', 'x')",
            "INSERT INTO app_settings (id, value_json) VALUES ('lock_app', 'false')",
            "UPDATE app_settings SET value_json = 'false'",
            "DELETE FROM synced_preferences",
            "SELECT s.value_json, n.body FROM app_settings s JOIN notes n",
        ] {
            assert!(is_not_authorized(run(&pool, &policy, sql).await), "{sql}");
        }
        run(
            &pool,
            &policy,
            "SELECT id, value_json, 0 AS source_rank FROM app_settings UNION ALL SELECT id, value_json, 1 FROM synced_preferences ORDER BY id, source_rank",
        )
        .await
        .unwrap();
        run(
            &pool,
            &policy,
            "EXPLAIN QUERY PLAN SELECT id FROM app_settings",
        )
        .await
        .unwrap();
        policy.set_locked(false);
        run(&pool, &policy, "SELECT * FROM notes").await.unwrap();
    }

    fn context(role: Option<aspc::Role>) -> AccessContext {
        AccessContext {
            actor_id: "actor".into(),
            role,
            workspace_id: "ws".into(),
            device_id: "device".into(),
            window: Some("main".into()),
            unlocked: true,
        }
    }

    #[tokio::test]
    async fn applies_the_role_per_table() {
        let pool = test_pool().await;
        let policy = WebviewSqlAuthorizer::new();

        policy.set_context(context(Some(aspc::Role::Owner)));
        run(
            &pool,
            &policy,
            "INSERT INTO notes (id, body) VALUES ('o', 'x')",
        )
        .await
        .unwrap();
        run(&pool, &policy, "SELECT value_json FROM app_settings")
            .await
            .unwrap();

        policy.set_context(context(Some(aspc::Role::Viewer)));
        run(&pool, &policy, "SELECT body FROM notes").await.unwrap();
        for sql in [
            "INSERT INTO notes (id, body) VALUES ('v', 'x')",
            "UPDATE notes SET body = 'y'",
            "DELETE FROM notes",
            "SELECT value_json FROM app_settings",
            "SELECT secret FROM webhook_endpoints",
        ] {
            assert!(
                is_not_authorized(run(&pool, &policy, sql).await),
                "viewer: {sql}"
            );
        }

        policy.set_context(context(None));
        assert!(is_not_authorized(
            run(&pool, &policy, "SELECT body FROM notes").await
        ));

        let mut empty_workspace = context(Some(aspc::Role::Owner));
        empty_workspace.workspace_id.clear();
        policy.set_context(empty_workspace);
        assert!(is_not_authorized(
            run(&pool, &policy, "SELECT body FROM notes").await
        ));
    }

    #[test]
    fn maps_tables_to_data_types() {
        assert_eq!(table_data_type("transcripts"), DataType::Transcript);
        assert_eq!(table_data_type("SESSIONS"), DataType::Notes);
        assert_eq!(table_data_type("action_items"), DataType::Actions);
        assert_eq!(table_data_type("app_settings"), DataType::Settings);
        assert_eq!(table_data_type("webhook_endpoints"), DataType::Keys);
        assert_eq!(table_data_type("e2ee_local_device"), DataType::Keys);
        assert_eq!(table_data_type("voiceprint_exemplars"), DataType::Recording);
        assert_eq!(table_data_type("sessions_cloudsync"), DataType::Notes);
        assert_eq!(table_data_type("something_new"), DataType::Notes);
    }

    #[tokio::test]
    async fn internal_queries_are_unaffected_after_release() {
        let pool = test_pool().await;
        let policy = WebviewSqlAuthorizer::new();
        assert!(is_not_authorized(
            run(&pool, &policy, "SELECT * FROM audit_x").await
        ));
        let entry: String = sqlx::query_scalar("SELECT entry FROM audit_x")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(entry, "secret");
        sqlx::query("PRAGMA user_version = 3")
            .execute(&pool)
            .await
            .unwrap();
    }

    #[cfg_attr(
        not(any(target_os = "macos", target_os = "linux", target_os = "windows")),
        ignore
    )]
    #[tokio::test]
    async fn cloudsync_tables_accept_authorized_webview_writes() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("cloudsync.db");
        let db = crate::Db::open(crate::DbOpenOptions {
            storage: crate::DbStorage::Local(&db_path),
            cloudsync_enabled: true,
            journal_mode_wal: true,
            foreign_keys: true,
            max_connections: Some(1),
        })
        .await
        .unwrap();
        sqlx::query(
            "CREATE TABLE items (id TEXT PRIMARY KEY NOT NULL, value TEXT NOT NULL DEFAULT '')",
        )
        .execute(db.pool())
        .await
        .unwrap();
        db.cloudsync_init("items", None, None).await.unwrap();
        let before: i64 = sqlx::query_scalar("SELECT cloudsync_db_version()")
            .fetch_one(db.pool())
            .await
            .unwrap();

        let policy = WebviewSqlAuthorizer::new();
        run(
            db.pool(),
            &policy,
            "INSERT INTO items (id, value) VALUES ('a', 'one')",
        )
        .await
        .unwrap();
        run(
            db.pool(),
            &policy,
            "UPDATE items SET value = 'two' WHERE id = 'a'",
        )
        .await
        .unwrap();
        run(db.pool(), &policy, "DELETE FROM items WHERE id = 'a'")
            .await
            .unwrap();

        let after: i64 = sqlx::query_scalar("SELECT cloudsync_db_version()")
            .fetch_one(db.pool())
            .await
            .unwrap();
        assert!(after > before, "CloudSync should record webview writes");
        let tracked: i64 = sqlx::query_scalar("SELECT count(*) FROM items_cloudsync")
            .fetch_one(db.pool())
            .await
            .unwrap();
        assert!(tracked > 0);
    }

    #[test]
    fn unknown_action_is_denied() {
        let request = AuthRequest {
            action: 9_999,
            arg1: None,
            arg2: None,
        };
        assert_eq!(decide(&request), Decision::Deny);
        let copy = AuthRequest {
            action: ffi::SQLITE_COPY,
            arg1: None,
            arg2: None,
        };
        assert_eq!(decide(&copy), Decision::Deny);
    }
}
