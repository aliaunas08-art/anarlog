//! ASPC D1 audit log.
//!
//! - Its own SQLite file, `audit.db`, next to `app.db`. Nothing else opens it:
//!   `db-app`, `db-core`, the `db` plugin and the webview never get a handle
//!   (`tests/isolation.rs`), and the webview SQL authorizer denies `audit*`
//!   names and `ATTACH`.
//! - Append-only: [`AuditWriter`] only appends; the table's triggers make
//!   `UPDATE` and `DELETE` fail.
//! - Hash chain: `hash = SHA-256(prev_hash ‖ canonical entry)`. The latest
//!   `seq:hash` is also kept in the Keychain ([`head`]), so cutting entries
//!   off the end is detected, not only edits in the middle.
//! - Ids, counts and flags only: [`NewEntry::validate`] refuses values with
//!   whitespace, so meeting content cannot be written (D1.5).
//!
//! [`AuditReader`] (list and verify) is held only by the audit plugin, behind
//! the Settings/Administer guard.

mod entry;
pub mod head;
mod limiter;
#[cfg(feature = "tauri")]
mod tauri_helpers;
#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anlg_local_accounts::SecretStore;
use serde::Serialize;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use sqlx::{Row, SqlitePool};
use tokio::sync::{mpsc, oneshot};

pub use entry::{
    Detail, DetailValue, Entry, GENESIS_HASH, NewEntry, Source, chain_hash, is_identifier,
};
use head::{Head, HeadKeeper, HeadRead};
pub use limiter::{Admit, DenialLimiter};
#[cfg(feature = "tauri")]
pub use tauri_helpers::*;

pub const AUDIT_DB_FILENAME: &str = "audit.db";
/// Keychain service suffix: `<bundle id>.audit-log`.
pub const KEYCHAIN_SERVICE_SUFFIX: &str = "audit-log";

const HEAD_TIMEOUT: Duration = Duration::from_secs(3);
const VERIFY_PAGE: i64 = 1000;
pub const MAX_PAGE: i64 = 200;

/// Action names (D1.4). Entries carry ids and counts only.
pub mod actions {
    pub const APP_UNLOCKED: &str = "app.unlocked";
    pub const MEETING_VIEWED: &str = "meeting.viewed";
    pub const MEETING_EXPORTED: &str = "meeting.exported";
    pub const MEETING_SHARED: &str = "meeting.shared";
    pub const MEETING_DELETED: &str = "meeting.deleted";
    pub const MEETING_PRIVATE_CHANGED: &str = "meeting.private_changed";
    pub const TRANSCRIPT_ACCESSED: &str = "transcript.accessed";
    pub const ASK_QUERY: &str = "ask.query";
    pub const SETTINGS_CHANGED: &str = "settings.changed";
    pub const KEY_CHANGED: &str = "key.changed";
    pub const PERMISSION_DENIED: &str = "permission.denied";
    pub const DATA_DELETED: &str = "data.deleted";
    pub const RETENTION_RUN: &str = "retention.run";
    pub const BACKUP_CREATED: &str = "backup.created";
    pub const BACKUP_RESTORED: &str = "backup.restored";
    /// An MCP tool call or resource read (source `cli`).
    pub const MCP_CALL: &str = "mcp.call";
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("audit.invalid_field: {0}")]
    InvalidField(&'static str),
    #[error("audit.database: {0}")]
    Database(#[from] sqlx::Error),
    #[error("audit.io: {0}")]
    Io(#[from] std::io::Error),
    #[error("audit.closed")]
    Closed,
}

const SCHEMA: &[&str] = &[
    "CREATE TABLE IF NOT EXISTS audit_entries (
        seq INTEGER PRIMARY KEY,
        ts TEXT NOT NULL,
        actor_id TEXT NOT NULL,
        role TEXT NOT NULL,
        workspace_id TEXT NOT NULL,
        action TEXT NOT NULL,
        object_type TEXT NOT NULL,
        object_id TEXT NOT NULL,
        source TEXT NOT NULL CHECK (source IN ('app', 'cli', 'job')),
        device_id TEXT NOT NULL,
        detail TEXT NOT NULL DEFAULT '{}',
        prev_hash TEXT NOT NULL,
        hash TEXT NOT NULL
    ) STRICT",
    "CREATE TRIGGER IF NOT EXISTS audit_entries_no_update
        BEFORE UPDATE ON audit_entries
        BEGIN SELECT RAISE(ABORT, 'audit log is append-only'); END",
    "CREATE TRIGGER IF NOT EXISTS audit_entries_no_delete
        BEFORE DELETE ON audit_entries
        BEGIN SELECT RAISE(ABORT, 'audit log is append-only'); END",
];

#[derive(Debug, Clone, PartialEq, Eq)]
enum HeadState {
    /// The Keychain has not been read yet.
    Pending,
    /// The Keychain head agrees with the log and follows each append.
    Tracking,
    /// The Keychain could not be read; the head is not written (so a
    /// stored anchor is never overwritten blindly).
    Unavailable(String),
    /// The Keychain head disagrees with the log. Frozen as evidence.
    Broken { first_bad_seq: i64, reason: String },
}

enum Queued {
    Entry(NewEntry),
    Barrier(oneshot::Sender<()>),
}

struct Inner {
    pool: SqlitePool,
    head: HeadKeeper,
    head_state: Mutex<HeadState>,
    limiter: DenialLimiter,
    debounce: Mutex<HashMap<String, Instant>>,
    queue: mpsc::UnboundedSender<Queued>,
}

/// Append-only handle. Cloneable; this is what plugins and the CLI get.
#[derive(Clone)]
pub struct AuditWriter {
    inner: Arc<Inner>,
}

/// Read and verify. Only the audit plugin holds one.
#[derive(Clone)]
pub struct AuditReader {
    inner: Arc<Inner>,
}

/// The result of [`AuditReader::verify`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct VerifyReport {
    pub ok: bool,
    pub entries: i64,
    /// The first entry that fails, when `ok` is false.
    pub first_bad_seq: Option<i64>,
    pub reason: Option<String>,
    /// Whether the end of the log was checked against the Keychain head.
    pub head_checked: bool,
    /// Why it was not, or a note about it.
    pub head_note: Option<String>,
}

/// One page of entries, newest first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct EntryPage {
    pub entries: Vec<Entry>,
    pub total: i64,
    /// Pass as `before_seq` for the next (older) page.
    pub next_before_seq: Option<i64>,
}

/// A secret store that never reads or writes: for processes that must not
/// touch the Keychain (the CLI). The head then simply lags, which verify
/// tolerates.
pub struct NoKeychain;

impl SecretStore for NoKeychain {
    fn get(&self, _key: &str) -> Result<Option<String>, anlg_local_accounts::Error> {
        Err(anlg_local_accounts::Error::Keychain(
            "this process does not use the Keychain".into(),
        ))
    }
    fn set(&self, _key: &str, _value: &str) -> Result<(), anlg_local_accounts::Error> {
        Ok(())
    }
    fn delete(&self, _key: &str) -> Result<(), anlg_local_accounts::Error> {
        Ok(())
    }
}

/// Opens (creating if needed) `audit.db` at `path`. Must be called inside a
/// Tokio runtime: it starts the background writer used by
/// [`AuditWriter::submit`]. Does not touch the Keychain; call
/// [`AuditWriter::reconcile_head`] (in the background) for that.
pub async fn open(
    path: &Path,
    head_store: Arc<dyn SecretStore>,
) -> Result<(AuditWriter, AuditReader), Error> {
    let inner = open_inner(path, head_store).await?;
    Ok((
        AuditWriter {
            inner: Arc::clone(&inner),
        },
        AuditReader { inner },
    ))
}

/// For processes that may only append (the CLI): no reader, no Keychain.
pub async fn open_append_only(path: &Path) -> Result<AuditWriter, Error> {
    let inner = open_inner(path, Arc::new(NoKeychain)).await?;
    *inner.head_state.lock().unwrap_or_else(|e| e.into_inner()) =
        HeadState::Unavailable("append-only process".into());
    Ok(AuditWriter { inner })
}

async fn open_inner(path: &Path, head_store: Arc<dyn SecretStore>) -> Result<Arc<Inner>, Error> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let options = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Full)
        .busy_timeout(Duration::from_secs(10));
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await?;
    for statement in SCHEMA {
        sqlx::query(*statement).execute(&pool).await?;
    }
    restrict_permissions(path);

    let (queue, rx) = mpsc::unbounded_channel();
    let inner = Arc::new(Inner {
        pool,
        head: HeadKeeper::spawn(head_store),
        head_state: Mutex::new(HeadState::Pending),
        limiter: DenialLimiter::default(),
        debounce: Mutex::new(HashMap::new()),
        queue,
    });
    tokio::spawn(drain(Arc::downgrade(&inner), rx));
    Ok(inner)
}

#[cfg(unix)]
fn restrict_permissions(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    for suffix in ["", "-wal", "-shm"] {
        let file = std::path::PathBuf::from(format!("{}{suffix}", path.display()));
        if file.exists() {
            let _ = std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600));
        }
    }
}

#[cfg(not(unix))]
fn restrict_permissions(_path: &Path) {}

async fn drain(inner: std::sync::Weak<Inner>, mut rx: mpsc::UnboundedReceiver<Queued>) {
    while let Some(queued) = rx.recv().await {
        let Some(inner) = inner.upgrade() else {
            return;
        };
        match queued {
            Queued::Entry(entry) => {
                let action = entry.action.clone();
                if let Err(error) = append(&inner, entry).await {
                    tracing::error!(%error, %action, "audit_append_failed");
                }
            }
            Queued::Barrier(reply) => {
                let _ = reply.send(());
            }
        }
    }
}

async fn append(inner: &Inner, entry: NewEntry) -> Result<Entry, Error> {
    entry.validate()?;
    let detail = serde_json::to_string(&entry.detail).map_err(|_| Error::InvalidField("detail"))?;
    let mut tx = inner.pool.begin_with("BEGIN IMMEDIATE").await?;
    let last: Option<(i64, String)> =
        sqlx::query_as("SELECT seq, hash FROM audit_entries ORDER BY seq DESC LIMIT 1")
            .fetch_optional(&mut *tx)
            .await?;
    let (last_seq, prev_hash) = last.unwrap_or((0, GENESIS_HASH.to_string()));
    let mut stored = Entry {
        seq: last_seq + 1,
        ts: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        actor_id: entry.actor_id,
        role: entry.role,
        workspace_id: entry.workspace_id,
        action: entry.action,
        object_type: entry.object_type,
        object_id: entry.object_id,
        source: entry.source.as_str().to_string(),
        device_id: entry.device_id,
        detail,
        prev_hash,
        hash: String::new(),
    };
    stored.hash = stored.compute_hash();
    sqlx::query(
        "INSERT INTO audit_entries (seq, ts, actor_id, role, workspace_id, action, object_type,
            object_id, source, device_id, detail, prev_hash, hash)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(stored.seq)
    .bind(&stored.ts)
    .bind(&stored.actor_id)
    .bind(&stored.role)
    .bind(&stored.workspace_id)
    .bind(&stored.action)
    .bind(&stored.object_type)
    .bind(&stored.object_id)
    .bind(&stored.source)
    .bind(&stored.device_id)
    .bind(&stored.detail)
    .bind(&stored.prev_hash)
    .bind(&stored.hash)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    let tracking = matches!(
        *inner.head_state.lock().unwrap_or_else(|e| e.into_inner()),
        HeadState::Tracking
    );
    if tracking {
        inner.head.write(Head {
            seq: stored.seq,
            hash: stored.hash.clone(),
        });
    }
    Ok(stored)
}

async fn last_head(pool: &SqlitePool) -> Result<Option<Head>, Error> {
    Ok(sqlx::query_as::<_, (i64, String)>(
        "SELECT seq, hash FROM audit_entries ORDER BY seq DESC LIMIT 1",
    )
    .fetch_optional(pool)
    .await?
    .map(|(seq, hash)| Head { seq, hash }))
}

/// Compares a Keychain head with the log: `Err((first_bad_seq, reason))`
/// when entries were cut off or rewritten.
async fn check_head(pool: &SqlitePool, head: &Head) -> Result<Result<(), (i64, String)>, Error> {
    let last_seq = last_head(pool).await?.map_or(0, |h| h.seq);
    if head.seq > last_seq {
        return Ok(Err((
            last_seq + 1,
            format!(
                "the log was truncated: entries after {last_seq} are missing (the Keychain head is at {})",
                head.seq
            ),
        )));
    }
    let hash: Option<String> = sqlx::query_scalar("SELECT hash FROM audit_entries WHERE seq = ?")
        .bind(head.seq)
        .fetch_optional(pool)
        .await?;
    if hash.as_deref() != Some(head.hash.as_str()) {
        return Ok(Err((
            head.seq,
            format!(
                "entries up to {} were rewritten: they no longer match the Keychain head",
                head.seq
            ),
        )));
    }
    Ok(Ok(()))
}

impl AuditWriter {
    /// Appends one entry and returns it as stored.
    pub async fn append(&self, entry: NewEntry) -> Result<Entry, Error> {
        append(&self.inner, entry).await
    }

    /// Queues an entry without waiting (for sync callers such as the SQL
    /// authorizer callback). Errors are logged.
    pub fn submit(&self, entry: NewEntry) {
        if self.inner.queue.send(Queued::Entry(entry)).is_err() {
            tracing::error!("audit_queue_closed");
        }
    }

    /// Queues an entry unless one with the same `key` was queued within
    /// `window` (meeting viewed, transcript accessed).
    pub fn submit_debounced(&self, key: &str, window: Duration, entry: NewEntry) -> bool {
        let now = Instant::now();
        {
            let mut seen = self
                .inner
                .debounce
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            if seen.len() > 4096 {
                seen.retain(|_, at| now.duration_since(*at) < window);
            }
            if seen
                .get(key)
                .is_some_and(|at| now.duration_since(*at) < window)
            {
                return false;
            }
            seen.insert(key.to_string(), now);
        }
        self.submit(entry);
        true
    }

    /// Queues a `permission.denied` entry through the rate limiter. `key`
    /// groups identical denials (source, code, data type, action).
    pub fn submit_denial(&self, key: &str, entry: NewEntry) -> bool {
        match self.inner.limiter.admit(key, Instant::now()) {
            Admit::Write { suppressed } => {
                let entry = if suppressed > 0 {
                    entry.count("suppressed", i64::try_from(suppressed).unwrap_or(i64::MAX))
                } else {
                    entry
                };
                self.submit(entry);
                true
            }
            Admit::Drop => false,
        }
    }

    /// Waits until every queued entry is written and the Keychain head
    /// caught up.
    pub async fn flush(&self) {
        let (reply, rx) = oneshot::channel();
        if self.inner.queue.send(Queued::Barrier(reply)).is_ok() {
            let _ = rx.await;
        }
        self.inner.head.barrier(HEAD_TIMEOUT).await;
    }

    /// Reads the Keychain head and decides whether it agrees with the log.
    /// First run (no head): stores the current one. A head that disagrees is
    /// kept untouched as evidence. Never blocks the app: run it in the
    /// background; a Keychain that does not answer leaves the head unverified.
    pub async fn reconcile_head(&self) -> Result<(), Error> {
        let read = self.inner.head.read(HEAD_TIMEOUT).await;
        let pool = &self.inner.pool;
        let state = match read {
            HeadRead::Unavailable(reason) => HeadState::Unavailable(reason),
            HeadRead::Missing => HeadState::Tracking,
            HeadRead::Present(head) => match check_head(pool, &head).await? {
                Ok(()) => HeadState::Tracking,
                Err((first_bad_seq, reason)) => {
                    tracing::error!(first_bad_seq, "audit_chain_head_mismatch");
                    HeadState::Broken {
                        first_bad_seq,
                        reason,
                    }
                }
            },
        };
        let tracking = state == HeadState::Tracking;
        *self
            .inner
            .head_state
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = state;
        if tracking && let Some(head) = last_head(pool).await? {
            self.inner.head.write(head);
        }
        Ok(())
    }
}

impl AuditReader {
    /// Entries newest first, `limit` at most [`MAX_PAGE`].
    pub async fn list(&self, before_seq: Option<i64>, limit: i64) -> Result<EntryPage, Error> {
        let limit = limit.clamp(1, MAX_PAGE);
        let entries: Vec<Entry> =
            sqlx::query_as("SELECT * FROM audit_entries WHERE seq < ? ORDER BY seq DESC LIMIT ?")
                .bind(before_seq.unwrap_or(i64::MAX))
                .bind(limit)
                .fetch_all(&self.inner.pool)
                .await?;
        let total: i64 = sqlx::query_scalar("SELECT count(*) FROM audit_entries")
            .fetch_one(&self.inner.pool)
            .await?;
        let next_before_seq = (entries.len() as i64 == limit)
            .then(|| entries.last().map(|e| e.seq))
            .flatten()
            .filter(|seq| *seq > 1);
        Ok(EntryPage {
            entries,
            total,
            next_before_seq,
        })
    }

    /// Walks the whole chain, then checks its end against the Keychain head.
    /// Reports the first bad entry and why.
    pub async fn verify(&self) -> Result<VerifyReport, Error> {
        let pool = &self.inner.pool;
        let mut expected = 1_i64;
        let mut prev = GENESIS_HASH.to_string();
        let mut entries = 0_i64;
        let broken = |seq: i64, reason: String, entries: i64| VerifyReport {
            ok: false,
            entries,
            first_bad_seq: Some(seq),
            reason: Some(reason),
            head_checked: false,
            head_note: None,
        };
        loop {
            let rows = sqlx::query(
                "SELECT seq, ts, actor_id, role, workspace_id, action, object_type, object_id,
                        source, device_id, detail, prev_hash, hash
                 FROM audit_entries WHERE seq >= ? ORDER BY seq LIMIT ?",
            )
            .bind(expected)
            .bind(VERIFY_PAGE)
            .fetch_all(pool)
            .await?;
            if rows.is_empty() {
                break;
            }
            for row in rows {
                let entry = Entry {
                    seq: row.try_get("seq")?,
                    ts: row.try_get("ts")?,
                    actor_id: row.try_get("actor_id")?,
                    role: row.try_get("role")?,
                    workspace_id: row.try_get("workspace_id")?,
                    action: row.try_get("action")?,
                    object_type: row.try_get("object_type")?,
                    object_id: row.try_get("object_id")?,
                    source: row.try_get("source")?,
                    device_id: row.try_get("device_id")?,
                    detail: row.try_get("detail")?,
                    prev_hash: row.try_get("prev_hash")?,
                    hash: row.try_get("hash")?,
                };
                if entry.seq != expected {
                    return Ok(broken(
                        expected,
                        format!("entry {expected} is missing (deleted)"),
                        entries,
                    ));
                }
                if entry.prev_hash != prev {
                    return Ok(broken(
                        entry.seq,
                        format!(
                            "entry {} does not follow entry {} (removed, reordered or edited)",
                            entry.seq,
                            entry.seq - 1
                        ),
                        entries,
                    ));
                }
                if entry.compute_hash() != entry.hash {
                    return Ok(broken(
                        entry.seq,
                        format!(
                            "entry {} was edited or moved: its hash does not match",
                            entry.seq
                        ),
                        entries,
                    ));
                }
                prev = entry.hash;
                entries += 1;
                expected += 1;
            }
        }

        let frozen = self
            .inner
            .head_state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        if let HeadState::Broken {
            first_bad_seq,
            reason,
        } = frozen
        {
            let mut report = broken(first_bad_seq, reason, entries);
            report.head_checked = true;
            return Ok(report);
        }
        if matches!(frozen, HeadState::Unavailable(ref r) if r == "append-only process") {
            return Ok(ok_unchecked(
                entries,
                "this process does not read the Keychain",
            ));
        }
        match self.inner.head.read(HEAD_TIMEOUT).await {
            HeadRead::Present(head) => match check_head(pool, &head).await? {
                Ok(()) => Ok(VerifyReport {
                    ok: true,
                    entries,
                    first_bad_seq: None,
                    reason: None,
                    head_checked: true,
                    head_note: (head.seq < entries).then(|| {
                        format!(
                            "the Keychain head is at entry {}; later entries are not anchored yet",
                            head.seq
                        )
                    }),
                }),
                Err((seq, reason)) => {
                    let mut report = broken(seq, reason, entries);
                    report.head_checked = true;
                    Ok(report)
                }
            },
            HeadRead::Missing if entries == 0 => Ok(ok_unchecked(entries, "the log is empty")),
            HeadRead::Missing => Ok(ok_unchecked(
                entries,
                "no chain head in the Keychain yet; truncation cannot be ruled out",
            )),
            HeadRead::Unavailable(reason) => Ok(ok_unchecked(
                entries,
                &format!("the Keychain head could not be read: {reason}"),
            )),
        }
    }
}

fn ok_unchecked(entries: i64, note: &str) -> VerifyReport {
    VerifyReport {
        ok: true,
        entries,
        first_bad_seq: None,
        reason: None,
        head_checked: false,
        head_note: Some(note.to_string()),
    }
}
