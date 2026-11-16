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
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

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

/// Policy and lock state for webview SQL. One instance is shared by every
/// webview execution path of a database.
#[derive(Debug, Default)]
pub struct WebviewSqlAuthorizer {
    locked: AtomicBool,
}

impl WebviewSqlAuthorizer {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// While locked, every webview statement is denied. Phase 2 wires this to
    /// the app lock; the default is unlocked.
    pub fn set_locked(&self, locked: bool) {
        self.locked.store(locked, Ordering::SeqCst);
    }

    pub fn is_locked(&self) -> bool {
        self.locked.load(Ordering::SeqCst)
    }

    fn decide(&self, request: &AuthRequest<'_>) -> Decision {
        if self.is_locked() {
            return Decision::Deny;
        }
        decide(request)
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
        if policy.is_locked() {
            return Err(sqlx::Error::Protocol(
                "webview SQL is locked: not authorized".to_string(),
            ));
        }
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
    async fn denies_everything_when_locked() {
        let pool = test_pool().await;
        let policy = WebviewSqlAuthorizer::new();
        policy.set_locked(true);
        assert!(is_not_authorized(run(&pool, &policy, "SELECT 1").await));
        assert!(is_not_authorized(
            run(&pool, &policy, "SELECT * FROM notes").await
        ));
        policy.set_locked(false);
        run(&pool, &policy, "SELECT * FROM notes").await.unwrap();
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
