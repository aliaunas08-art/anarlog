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

/// ASPC C1: content tables that carry `workspace_id`. Must match
/// `anlg_db_app::WORKSPACE_CONTENT_TABLES` (checked by a db-app test).
pub const WORKSPACE_SCOPED_TABLES: &[&str] = &[
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

/// One query: does any content row belong to a workspace other than the
/// local one (the CloudSync binding) or `''`? Without a binding every
/// non-empty workspace counts as foreign (fail closed). Reads the binding in
/// the same snapshot as the rows.
fn foreign_workspace_census_sql(tables: &[&str]) -> String {
    let local = "COALESCE((SELECT json_extract(value_json, '$.workspace_id') \
                 FROM app_settings WHERE id = 'cloudsync_workspace_binding'), char(0))";
    let checks = tables
        .iter()
        .map(|table| {
            format!("EXISTS(SELECT 1 FROM {table} WHERE workspace_id NOT IN ('', {local}))")
        })
        .collect::<Vec<_>>()
        .join(" OR ");
    format!("SELECT {checks}")
}

/// The workspace-scoped tables a statement names (a superset is fine: each
/// extra table only costs census time; a table it reaches without naming it,
/// through a view, is denied and the read is retried after a full census).
pub fn referenced_scoped_tables(sql: &str) -> Vec<&'static str> {
    let lower = sql.to_ascii_lowercase();
    let bytes = lower.as_bytes();
    let is_ident = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
    WORKSPACE_SCOPED_TABLES
        .iter()
        .copied()
        .filter(|table| {
            lower.match_indices(table).any(|(start, _)| {
                let end = start + table.len();
                (start == 0 || !is_ident(bytes[start - 1]))
                    && (end == bytes.len() || !is_ident(bytes[end]))
            })
        })
        .collect()
}

/// Whether the webview's own statement only reads. Anything else runs in a
/// `BEGIN IMMEDIATE` transaction. A misclassified write in a read
/// transaction fails with `SQLITE_BUSY_SNAPSHOT` if the database changed, and
/// is still checked by the post-write census.
pub fn is_read_only_sql(sql: &str) -> bool {
    let first = sql
        .trim_start()
        .split(|c: char| c.is_whitespace() || c == '(')
        .next()
        .unwrap_or_default();
    ["select", "explain", "pragma", "values"]
        .iter()
        .any(|keyword| first.eq_ignore_ascii_case(keyword))
}

/// Whether a table's rows are denied while another workspace is present:
/// every table except settings, keys (ciphertext and key material the
/// role check already limits) and SQLite's own schema tables.
fn denied_while_multi_workspace(table: Option<&str>) -> bool {
    let Some(table) = table else {
        return true;
    };
    // Schema only: SQLite's tables and the `pragma_*` table-valued
    // functions (the PRAGMA itself is checked by the static policy).
    if is_sqlite_internal_table(Some(table))
        || (table.len() > 7 && table[..7].eq_ignore_ascii_case("pragma_"))
    {
        return false;
    }
    !matches!(table_data_type(table), DataType::Settings | DataType::Keys)
}

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

/// One denied webview statement step, reported to the denial sink (the
/// desktop app writes it to the audit log as `permission.denied`, D1.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SqlDenial {
    /// `aspc.*` code for role and lock denials, or `sql.policy` /
    /// `sql.workspace` for the static policy and the C1 guard.
    pub code: &'static str,
    pub table: Option<String>,
    pub access: Option<(DataType, Action)>,
}

type DenialSink = Arc<dyn Fn(SqlDenial) + Send + Sync>;

#[derive(Default)]
struct DenialSinkSlot(RwLock<Option<DenialSink>>);

impl std::fmt::Debug for DenialSinkSlot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("DenialSinkSlot")
    }
}

/// Policy, access context and lock state for webview SQL. One instance is
/// shared by every webview execution path of a database.
#[derive(Debug, Default)]
pub struct WebviewSqlAuthorizer {
    /// Called for every denial. Runs inside the SQLite authorizer callback,
    /// so it must only queue (the audit writer's `submit`), never block.
    denial_sink: DenialSinkSlot,
    locked: AtomicBool,
    /// Set by the app at startup. Until then (and in tests that never set
    /// it) only the static policy applies, which is the Phase 1 behaviour.
    context: RwLock<Option<AccessContext>>,
    /// C1 interim single-workspace guard (see [`WorkspaceGuard`]). Off by
    /// default; the app turns it on.
    workspace_guard: AtomicBool,
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

    /// Turns on the C1 single-workspace guard for every later webview
    /// statement: content tables are readable and writable only while every
    /// content row belongs to the local workspace (or none), checked in the
    /// statement's own transaction.
    pub fn enable_workspace_guard(&self) {
        self.workspace_guard.store(true, Ordering::SeqCst);
    }

    pub fn workspace_guard_enabled(&self) -> bool {
        self.workspace_guard.load(Ordering::SeqCst)
    }

    /// Installs the denial sink (see [`SqlDenial`]).
    pub fn set_denial_sink(&self, sink: impl Fn(SqlDenial) + Send + Sync + 'static) {
        *self
            .denial_sink
            .0
            .write()
            .unwrap_or_else(|e| e.into_inner()) = Some(Arc::new(sink));
    }

    fn decide(&self, request: &AuthRequest<'_>, connection: &ConnectionState) -> Decision {
        let (decision, code) = self.decide_with_code(request, connection);
        if decision == Decision::Deny {
            let denial = SqlDenial {
                code,
                table: request.arg1.map(str::to_owned),
                access: data_access(request),
            };
            // During a read's first, partial-census attempt a denial is only
            // provisional: the caller retries after a full census. It reaches
            // the sink only if no retry clears it (see `WorkspaceGuard`).
            let mut deferred = connection
                .deferred_denials
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            match deferred.as_mut() {
                Some(buffer) => buffer.push(denial),
                None => {
                    drop(deferred);
                    self.report_denial(denial);
                }
            }
        }
        decision
    }

    fn report_denial(&self, denial: SqlDenial) {
        let sink = self
            .denial_sink
            .0
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        if let Some(sink) = sink {
            sink(denial);
        }
    }

    fn decide_with_code(
        &self,
        request: &AuthRequest<'_>,
        connection: &ConnectionState,
    ) -> (Decision, &'static str) {
        if self.is_locked() {
            return (decide_locked(request), aspc::DenyCode::Locked.as_str());
        }
        if decide(request) == Decision::Deny {
            return (Decision::Deny, "sql.policy");
        }
        if connection.guarded {
            // Only Rust opens and ends the guard's transaction, so the census
            // and the statement always share one snapshot.
            if request.action == ffi::SQLITE_TRANSACTION
                && !connection.transaction_control.load(Ordering::SeqCst)
            {
                return (Decision::Deny, "sql.policy");
            }
            if matches!(
                request.action,
                ffi::SQLITE_READ | ffi::SQLITE_INSERT | ffi::SQLITE_UPDATE | ffi::SQLITE_DELETE
            ) && denied_while_multi_workspace(request.arg1)
                && !connection.allows_content(request.arg1)
            {
                return (Decision::Deny, "sql.workspace");
            }
        }
        let Some((data, action)) = data_access(request) else {
            return (Decision::Allow, "");
        };
        let guard = self.context.read().unwrap_or_else(|e| e.into_inner());
        let Some(context) = guard.as_ref() else {
            return (Decision::Allow, "");
        };
        let mut context = context.clone();
        context.unlocked = true;
        match aspc::authorize(&context, &context.workspace_id, data, action) {
            Ok(_) => (Decision::Allow, ""),
            Err(denied) => (Decision::Deny, denied.code.as_str()),
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
        // SAFETY: `context` is an `Arc<ConnectionState>` pointer that is
        // kept alive while installed (see `AuthorizedConnection`).
        let connection = unsafe { &*(context as *const ConnectionState) };
        let request = AuthRequest {
            action,
            arg1: unsafe { opt_str(arg1) },
            arg2: unsafe { opt_str(arg2) },
        };
        connection.policy.decide(&request, connection)
    });
    match result {
        Ok(Decision::Allow) => ffi::SQLITE_OK,
        _ => ffi::SQLITE_DENY,
    }
}

/// Per-connection authorizer state: the shared policy plus the C1 guard's
/// result for the statement running now.
#[derive(Debug)]
pub struct ConnectionState {
    policy: Arc<WebviewSqlAuthorizer>,
    guarded: bool,
    /// Content tables denied. Starts `true` when guarded (deny until the
    /// census proves one workspace) and is kept `true` while another
    /// workspace is present.
    content_denied: AtomicBool,
    /// Allows `BEGIN`/`COMMIT`/`ROLLBACK`; set only around Rust's own.
    transaction_control: AtomicBool,
    /// After a read's partial census: only these content tables are allowed
    /// (every other content table is denied). `None`: all, after a full
    /// census.
    allowed_only: std::sync::Mutex<Option<Vec<&'static str>>>,
    /// `Some` while a read runs its first attempt after a partial census:
    /// denials are held here instead of reaching the denial sink. They are
    /// provisional (judged against a partial census), so they are never
    /// reported: a full census before the retry discards them and the retry
    /// reports its own, and a first attempt that is cancelled (the live query
    /// was dropped while the user moved on) or not retried discards them on
    /// release. A real denial always resurfaces in the retry.
    deferred_denials: std::sync::Mutex<Option<Vec<SqlDenial>>>,
}

impl ConnectionState {
    fn defer_denials(&self) {
        *self
            .deferred_denials
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = Some(Vec::new());
    }

    fn discard_deferred_denials(&self) {
        *self
            .deferred_denials
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = None;
    }

    fn allows_content(&self, table: Option<&str>) -> bool {
        if self.content_denied.load(Ordering::SeqCst) {
            return false;
        }
        let allowed = self.allowed_only.lock().unwrap_or_else(|e| e.into_inner());
        match (allowed.as_ref(), table) {
            (None, _) => true,
            (Some(list), Some(table)) => list.iter().any(|t| t.eq_ignore_ascii_case(table)),
            (Some(_), None) => false,
        }
    }
}

/// C1 interim guard for webview SQL (the single-workspace mode, ASPC_AUDIT
/// "C1 decision"). The authorizer cannot filter rows, so webview SQL reaches
/// content only while the device holds a single workspace:
///
/// 1. Rust opens the statement's transaction (`BEGIN`, or `BEGIN IMMEDIATE`
///    for writes); the webview cannot issue `BEGIN`/`COMMIT` itself.
/// 2. [`WorkspaceGuard::check`] runs the census in that transaction. If any
///    content row belongs to another workspace, every content table is
///    denied for the statement.
/// 3. After a statement that changed rows, [`WorkspaceGuard::check_writes`]
///    runs the census again before commit; a write that introduced another
///    workspace is rolled back.
#[derive(Clone, Debug)]
pub struct WorkspaceGuard {
    state: Arc<ConnectionState>,
}

impl WorkspaceGuard {
    pub fn allow_transaction_control(&self, allowed: bool) {
        self.state
            .transaction_control
            .store(allowed, Ordering::SeqCst);
    }

    fn set_allowed_only(&self, tables: Option<Vec<&'static str>>) {
        *self
            .state
            .allowed_only
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = tables;
    }

    /// Whether the last census covered only some tables.
    pub fn is_partial(&self) -> bool {
        self.state
            .allowed_only
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_some()
    }

    async fn foreign_workspace_present(&self, connection: &mut SqliteConnection) -> bool {
        self.foreign_workspace_present_in(connection, WORKSPACE_SCOPED_TABLES)
            .await
    }

    async fn foreign_workspace_present_in(
        &self,
        connection: &mut SqliteConnection,
        tables: &[&str],
    ) -> bool {
        self.set_allowed_only(None);
        self.state.content_denied.store(false, Ordering::SeqCst);
        let result: Result<bool, sqlx::Error> =
            sqlx::query_scalar(sqlx::AssertSqlSafe(foreign_workspace_census_sql(tables)))
                .fetch_one(&mut *connection)
                .await;
        let foreign = match result {
            Ok(foreign) => foreign,
            Err(error) => {
                tracing::error!(%error, "webview_sql_workspace_census_failed");
                true
            }
        };
        if foreign {
            self.log_foreign_tables(connection, tables).await;
        }
        self.state.content_denied.store(true, Ordering::SeqCst);
        foreign
    }

    /// Names (never rows) of the tables that hold another workspace's rows,
    /// so a locked-out webview can be diagnosed from the log.
    async fn log_foreign_tables(&self, connection: &mut SqliteConnection, tables: &[&str]) {
        let mut found = Vec::new();
        for table in tables {
            let foreign: Result<bool, sqlx::Error> =
                sqlx::query_scalar(sqlx::AssertSqlSafe(foreign_workspace_census_sql(&[table])))
                    .fetch_one(&mut *connection)
                    .await;
            if !matches!(foreign, Ok(false)) {
                found.push(*table);
            }
        }
        tracing::warn!(tables = ?found, "webview_sql_denied_multiple_workspaces");
    }

    /// Runs the census and allows content tables only if every content row
    /// is in the local workspace (or unassigned). Never fails: an error
    /// leaves content denied.
    ///
    /// Called before the retry of a read whose partial-census attempt was
    /// denied: that attempt's denials are dropped, so only the retry's own
    /// (final) denials reach the denial sink.
    pub async fn check(&self, connection: &mut SqliteConnection) {
        self.state.discard_deferred_denials();
        let foreign = self.foreign_workspace_present(connection).await;
        self.state.content_denied.store(foreign, Ordering::SeqCst);
    }

    /// For a read: runs the census only on the content tables the statement
    /// names and allows only those. A statement that names none gets no
    /// content at all. If the read then hits a denied table (a view, or a
    /// table without `workspace_id`), the caller runs [`Self::check`] in the
    /// same transaction and retries.
    pub async fn check_read(&self, connection: &mut SqliteConnection, sql: &str) {
        // The first attempt's denials are provisional: `check` before a retry,
        // or releasing the connection, drops them (see `deferred_denials`).
        self.state.defer_denials();
        let tables = referenced_scoped_tables(sql);
        if tables.is_empty() {
            self.state.content_denied.store(true, Ordering::SeqCst);
            self.set_allowed_only(None);
            return;
        }
        let foreign = self.foreign_workspace_present_in(connection, &tables).await;
        self.state.content_denied.store(foreign, Ordering::SeqCst);
        self.set_allowed_only(if foreign { None } else { Some(tables) });
    }

    /// Rows changed by this connection so far (`total_changes()`).
    pub async fn changes(&self, connection: &mut SqliteConnection) -> Result<i64, sqlx::Error> {
        sqlx::query_scalar("SELECT total_changes()")
            .fetch_one(connection)
            .await
    }

    /// After the webview's statements: if they changed rows and another
    /// workspace is now present, returns an error so the caller rolls back.
    pub async fn check_writes(
        &self,
        connection: &mut SqliteConnection,
        changes_before: i64,
    ) -> Result<(), sqlx::Error> {
        let before = self.state.content_denied.load(Ordering::SeqCst);
        if self.changes(connection).await? == changes_before {
            return Ok(());
        }
        let foreign = self.foreign_workspace_present(connection).await;
        self.state
            .content_denied
            .store(before || foreign, Ordering::SeqCst);
        if foreign {
            return Err(sqlx::Error::Protocol(
                "aspc.wrong_workspace: not authorized (a webview write may not create another workspace's rows)".into(),
            ));
        }
        Ok(())
    }
}

/// A pool connection with the webview authorizer installed.
///
/// Call [`AuthorizedConnection::release`] when done. Dropping it without
/// releasing clears the authorizer in a background task.
pub struct AuthorizedConnection {
    connection: Option<PoolConnection<Sqlite>>,
    state: Arc<ConnectionState>,
    installed: bool,
}

impl AuthorizedConnection {
    pub async fn acquire(
        pool: &SqlitePool,
        policy: &Arc<WebviewSqlAuthorizer>,
    ) -> Result<Self, sqlx::Error> {
        let connection = pool.acquire().await?;
        let guarded = policy.workspace_guard_enabled();
        let mut this = Self {
            connection: Some(connection),
            state: Arc::new(ConnectionState {
                policy: Arc::clone(policy),
                guarded,
                content_denied: AtomicBool::new(guarded),
                transaction_control: AtomicBool::new(false),
                allowed_only: std::sync::Mutex::new(None),
                deferred_denials: std::sync::Mutex::new(None),
            }),
            installed: false,
        };
        this.install().await?;
        Ok(this)
    }

    async fn install(&mut self) -> Result<(), sqlx::Error> {
        let connection = self.connection.as_mut().expect("connection present");
        let mut handle = connection.lock_handle().await?;
        let raw = handle.as_raw_handle().as_ptr();
        let context = Arc::as_ptr(&self.state) as *mut c_void;
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

    /// The C1 guard for this connection, when the policy has it on.
    pub fn workspace_guard(&self) -> Option<WorkspaceGuard> {
        self.state.guarded.then(|| WorkspaceGuard {
            state: Arc::clone(&self.state),
        })
    }

    pub fn connection(&mut self) -> &mut SqliteConnection {
        self.connection.as_mut().expect("connection present")
    }

    /// Remove the authorizer and return the connection to the pool.
    pub async fn release(mut self) -> Result<(), sqlx::Error> {
        self.state.discard_deferred_denials();
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
        self.state.discard_deferred_denials();
        let Some(mut connection) = self.connection.take() else {
            return;
        };
        let policy = Arc::clone(&self.state);
        // A cancelled transaction's queued ROLLBACK must get through.
        policy.transaction_control.store(true, Ordering::SeqCst);
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

    /// D1.1: the db plugin's webview connection cannot reach `audit.db`,
    /// even when the file sits next to `app.db` and the statement names it.
    #[tokio::test]
    async fn webview_sql_cannot_reach_the_audit_log() {
        let dir = tempfile::tempdir().unwrap();
        let audit_path = dir.path().join("audit.db");
        let audit = SqlitePool::connect_with(
            sqlx::sqlite::SqliteConnectOptions::new()
                .filename(&audit_path)
                .create_if_missing(true),
        )
        .await
        .unwrap();
        sqlx::query("CREATE TABLE audit_entries (seq INTEGER PRIMARY KEY, object_id TEXT)")
            .execute(&audit)
            .await
            .unwrap();
        audit.close().await;

        let pool = test_pool().await;
        let policy = WebviewSqlAuthorizer::new();
        let path = audit_path.display().to_string().replace('\'', "''");
        for sql in [
            format!("ATTACH DATABASE '{path}' AS audit"),
            format!("ATTACH DATABASE 'file:{path}?mode=ro' AS a"),
            "SELECT * FROM audit_entries".to_string(),
            "DELETE FROM audit_entries".to_string(),
        ] {
            assert!(
                run(&pool, &policy, &sql).await.is_err(),
                "{sql} reached the audit log"
            );
        }
        // No second database is attached to the connection.
        let databases: Vec<String> = sqlx::query_scalar("SELECT name FROM pragma_database_list")
            .fetch_all(&pool)
            .await
            .unwrap();
        assert!(
            databases
                .iter()
                .all(|name| name == "main" || name == "temp")
        );
    }

    /// C4.2: a denied webview statement that carries meeting content (a
    /// marker in a literal) leaves it out of the error and the audit denial.
    #[tokio::test]
    async fn denials_never_carry_the_statement_text() {
        const MARKER: &str = "zqxcontentmarker4417";
        let pool = test_pool().await;
        let policy = WebviewSqlAuthorizer::new();
        let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = Arc::clone(&seen);
        policy.set_denial_sink(move |denial| sink.lock().unwrap().push(denial));
        policy.set_locked(true);
        let locked = run(
            &pool,
            &policy,
            &format!("UPDATE notes SET body = '{MARKER} notes' WHERE id = 'x'"),
        )
        .await;
        policy.set_locked(false);
        let audit = run(
            &pool,
            &policy,
            &format!("INSERT INTO audit_x (entry) VALUES ('{MARKER} title')"),
        )
        .await;
        for result in [locked, audit] {
            let error = result.unwrap_err().to_string();
            assert!(error.contains("not authorized"), "{error}");
            assert!(!error.contains(MARKER), "{error}");
        }
        let seen = seen.lock().unwrap();
        assert!(!seen.is_empty());
        assert!(!format!("{seen:?}").contains(MARKER));
    }

    #[tokio::test]
    async fn denials_reach_the_denial_sink() {
        let pool = test_pool().await;
        let policy = WebviewSqlAuthorizer::new();
        let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = Arc::clone(&seen);
        policy.set_denial_sink(move |denial| sink.lock().unwrap().push(denial));
        policy.set_locked(true);
        assert!(is_not_authorized(
            run(&pool, &policy, "SELECT * FROM notes").await
        ));
        policy.set_locked(false);
        assert!(is_not_authorized(
            run(&pool, &policy, "SELECT * FROM audit_x").await
        ));
        run(&pool, &policy, "SELECT * FROM notes").await.unwrap();
        let seen = seen.lock().unwrap();
        assert!(
            seen.iter()
                .any(|d| d.code == "aspc.locked" && d.table.as_deref() == Some("notes"))
        );
        assert!(
            seen.iter()
                .any(|d| d.code == "sql.policy" && d.table.as_deref() == Some("audit_x"))
        );
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
