//! Back up and restore the library (ASPC C5).
//!
//! A backup is one uncompressed tar archive with:
//! - `manifest.json`: format version, creation time, app version and counts
//!   (no content);
//! - `app.db`: a consistent snapshot of the database (`VACUUM INTO`);
//! - `sessions/…`: the vault's meeting folders (audio, attachments, notes).
//!
//! It never contains the audit log (its chain head is in this Mac's Keychain)
//! or any Keychain item (C3).
//!
//! Restoring is two steps so the open database is never replaced under the
//! running app: [`stage_restore`] checks the archive and unpacks it into
//! `<app data>/restore-pending/`; on the next launch, before the database is
//! opened, [`apply_pending_restore`] moves the current `app.db` and
//! `sessions/` aside (kept, never deleted) and puts the restored ones in
//! place.

use std::fs::File;
use std::io::{BufReader, BufWriter};
use std::path::{Component, Path, PathBuf};

use sqlx::SqlitePool;

pub const BACKUP_FORMAT: u32 = 1;
pub const BACKUP_EXTENSION: &str = "anarlogbackup";
const MANIFEST: &str = "manifest.json";
const DATABASE: &str = "app.db";
const SESSIONS: &str = "sessions";
const PENDING_DIR: &str = "restore-pending";
const PENDING_MARKER: &str = "restore.json";
const MAX_ENTRIES: usize = 2_000_000;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct BackupManifest {
    pub format: u32,
    pub created_at: String,
    pub app_version: String,
    pub meetings: i64,
    pub files: u64,
}

#[derive(Debug, Clone, serde::Serialize, specta::Type, PartialEq, Eq)]
pub struct BackupSummary {
    pub meetings: i64,
    pub files: u64,
    pub bytes: u64,
}

#[derive(Debug, Clone, serde::Serialize, specta::Type, PartialEq, Eq)]
pub struct RestoreSummary {
    pub meetings: i64,
    pub files: u64,
    pub created_at: String,
    /// The restore is applied when the app next starts.
    pub restart_required: bool,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct PendingRestore {
    /// Where the vault's `sessions/` folder lives (set by Rust from the
    /// settings when the restore was staged).
    vault_sessions_dir: Option<PathBuf>,
}

/// What [`apply_pending_restore`] moved aside.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppliedRestore {
    pub previous_database: Option<PathBuf>,
    pub previous_sessions: Option<PathBuf>,
}

fn stamp() -> String {
    chrono::Utc::now().format("%Y%m%dT%H%M%SZ").to_string()
}

fn io(context: &str) -> impl Fn(std::io::Error) -> String + '_ {
    move |error| format!("backup.io: {context}: {error}")
}

/// Every regular file under `root`, relative to it. Symlinks are skipped.
fn collect_files(root: &Path) -> Result<Vec<PathBuf>, String> {
    let mut files = Vec::new();
    let mut stack = vec![PathBuf::new()];
    while let Some(relative) = stack.pop() {
        let dir = root.join(&relative);
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(io("read sessions")(error)),
        };
        for entry in entries {
            let entry = entry.map_err(io("read sessions"))?;
            let file_type = entry.file_type().map_err(io("read sessions"))?;
            let child = relative.join(entry.file_name());
            if file_type.is_dir() {
                stack.push(child);
            } else if file_type.is_file() {
                files.push(child);
            }
        }
    }
    files.sort();
    Ok(files)
}

async fn count_meetings(pool: &SqlitePool) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM sessions WHERE deleted_at IS NULL OR deleted_at = ''")
        .fetch_one(pool)
        .await
        .unwrap_or(0)
}

#[cfg(unix)]
fn make_private(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
}

#[cfg(not(unix))]
fn make_private(_path: &Path) {}

/// Writes a backup of the database behind `pool` and of `sessions_dir` to
/// `out` (replaced atomically). The archive is `0600`.
pub async fn write_backup(
    pool: &SqlitePool,
    sessions_dir: Option<&Path>,
    out: &Path,
    app_version: &str,
) -> Result<BackupSummary, String> {
    let parent = out
        .parent()
        .ok_or_else(|| "backup.invalid_path".to_string())?;
    let work = tempfile::Builder::new()
        .prefix(".anarlog-backup-")
        .tempdir_in(parent)
        .map_err(io("create work folder"))?;
    let snapshot = work.path().join(DATABASE);
    let snapshot_str = snapshot
        .to_str()
        .ok_or_else(|| "backup.invalid_path".to_string())?
        .to_string();
    // A consistent snapshot while the app keeps running (one read
    // transaction); free pages are not copied.
    sqlx::query("VACUUM INTO ?")
        .bind(&snapshot_str)
        .execute(pool)
        .await
        .map_err(|error| format!("backup.database: {error}"))?;
    let meetings = count_meetings(pool).await;

    let sessions_dir = sessions_dir.map(Path::to_path_buf);
    let out = out.to_path_buf();
    let app_version = app_version.to_string();
    let work_path = work.path().to_path_buf();
    let summary = tokio::task::spawn_blocking(move || -> Result<BackupSummary, String> {
        let files = match &sessions_dir {
            Some(dir) => collect_files(dir)?,
            None => Vec::new(),
        };
        let manifest = BackupManifest {
            format: BACKUP_FORMAT,
            created_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            app_version,
            meetings,
            files: files.len() as u64,
        };
        let manifest_path = work_path.join(MANIFEST);
        std::fs::write(
            &manifest_path,
            serde_json::to_vec_pretty(&manifest).map_err(|e| e.to_string())?,
        )
        .map_err(io("write manifest"))?;

        let partial = work_path.join("archive.partial");
        {
            let file = File::create(&partial).map_err(io("create archive"))?;
            make_private(&partial);
            let mut builder = tar::Builder::new(BufWriter::new(file));
            builder.follow_symlinks(false);
            builder
                .append_path_with_name(&manifest_path, MANIFEST)
                .map_err(io("write archive"))?;
            builder
                .append_path_with_name(&snapshot, DATABASE)
                .map_err(io("write archive"))?;
            if let Some(dir) = &sessions_dir {
                for relative in &files {
                    builder
                        .append_path_with_name(
                            dir.join(relative),
                            Path::new(SESSIONS).join(relative),
                        )
                        .map_err(io("write archive"))?;
                }
            }
            builder
                .into_inner()
                .and_then(|writer| writer.into_inner().map_err(|e| e.into_error()))
                .and_then(|file| file.sync_all())
                .map_err(io("finish archive"))?;
        }
        std::fs::rename(&partial, &out).map_err(io("move archive"))?;
        let bytes = std::fs::metadata(&out).map(|m| m.len()).unwrap_or(0);
        Ok(BackupSummary {
            meetings,
            files: manifest.files,
            bytes,
        })
    })
    .await
    .map_err(|error| format!("backup.task: {error}"))??;
    drop(work);
    Ok(summary)
}

/// The archive path inside the backup, if it is one this format allows:
/// `manifest.json`, `app.db`, or a plain relative path under `sessions/`.
fn allowed_entry(path: &Path) -> Option<PathBuf> {
    let mut parts = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => parts.push(part.to_os_string()),
            Component::CurDir => {}
            _ => return None,
        }
    }
    let path: PathBuf = parts.iter().collect();
    let first = parts.first()?.to_str()?;
    match (first, parts.len()) {
        (MANIFEST | DATABASE, 1) => Some(path),
        (SESSIONS, _) => Some(path),
        _ => None,
    }
}

async fn check_database(path: &Path) -> Result<(), String> {
    use sqlx::ConnectOptions;
    let mut connection = sqlx::sqlite::SqliteConnectOptions::new()
        .filename(path)
        .read_only(true)
        .connect()
        .await
        .map_err(|error| format!("backup.invalid: database: {error}"))?;
    let integrity: String = sqlx::query_scalar("PRAGMA integrity_check")
        .fetch_one(&mut connection)
        .await
        .map_err(|error| format!("backup.invalid: database: {error}"))?;
    if integrity != "ok" {
        return Err("backup.invalid: database integrity check failed".into());
    }
    let has_sessions: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'sessions'",
    )
    .fetch_one(&mut connection)
    .await
    .map_err(|error| format!("backup.invalid: database: {error}"))?;
    if has_sessions == 0 {
        return Err("backup.invalid: not an Anarlog library".into());
    }
    Ok(())
}

/// Checks `archive` and unpacks it into `<app_data_dir>/restore-pending/`
/// for [`apply_pending_restore`] on the next launch. Refuses anything but a
/// format-1 backup with a sound database: unknown or absolute paths, `..`,
/// links and special files fail the whole restore. Nothing in the current
/// library changes here.
pub async fn stage_restore(
    archive: &Path,
    app_data_dir: &Path,
    vault_sessions_dir: Option<&Path>,
) -> Result<RestoreSummary, String> {
    let staging = tempfile::Builder::new()
        .prefix(".restore-staging-")
        .tempdir_in(app_data_dir)
        .map_err(io("create staging folder"))?;
    let staging_path = staging.path().to_path_buf();
    let archive = archive.to_path_buf();
    let files = tokio::task::spawn_blocking(move || -> Result<u64, String> {
        let file = File::open(&archive).map_err(io("open backup"))?;
        let mut tar = tar::Archive::new(BufReader::new(file));
        let mut files = 0u64;
        for (index, entry) in tar.entries().map_err(io("read backup"))?.enumerate() {
            if index >= MAX_ENTRIES {
                return Err("backup.invalid: too many entries".into());
            }
            let mut entry = entry.map_err(io("read backup"))?;
            let raw = entry.path().map_err(io("read backup"))?.into_owned();
            let Some(relative) = allowed_entry(&raw) else {
                return Err("backup.invalid: unexpected entry".into());
            };
            let target = staging_path.join(&relative);
            match entry.header().entry_type() {
                tar::EntryType::Regular => {
                    if let Some(parent) = target.parent() {
                        std::fs::create_dir_all(parent).map_err(io("unpack backup"))?;
                    }
                    let mut out = File::create(&target).map_err(io("unpack backup"))?;
                    std::io::copy(&mut entry, &mut out).map_err(io("unpack backup"))?;
                    make_private(&target);
                    if relative.starts_with(SESSIONS) {
                        files += 1;
                    }
                }
                tar::EntryType::Directory => {
                    std::fs::create_dir_all(&target).map_err(io("unpack backup"))?;
                }
                _ => return Err("backup.invalid: links and special files are refused".into()),
            }
        }
        Ok(files)
    })
    .await
    .map_err(|error| format!("backup.task: {error}"))??;

    let manifest: BackupManifest = std::fs::read(staging.path().join(MANIFEST))
        .map_err(|_| "backup.invalid: missing manifest".to_string())
        .and_then(|bytes| {
            serde_json::from_slice(&bytes).map_err(|_| "backup.invalid: bad manifest".to_string())
        })?;
    if manifest.format != BACKUP_FORMAT {
        return Err(format!(
            "backup.unsupported_format: {} (this app reads {BACKUP_FORMAT})",
            manifest.format
        ));
    }
    let database = staging.path().join(DATABASE);
    if !database.is_file() {
        return Err("backup.invalid: missing database".into());
    }
    check_database(&database).await?;

    std::fs::write(
        staging.path().join(PENDING_MARKER),
        serde_json::to_vec(&PendingRestore {
            vault_sessions_dir: vault_sessions_dir.map(Path::to_path_buf),
        })
        .map_err(|e| e.to_string())?,
    )
    .map_err(io("write restore marker"))?;

    let pending = app_data_dir.join(PENDING_DIR);
    if pending.exists() {
        std::fs::remove_dir_all(&pending).map_err(io("replace earlier staged restore"))?;
    }
    let staged = staging.keep();
    std::fs::rename(&staged, &pending).map_err(io("stage restore"))?;
    Ok(RestoreSummary {
        meetings: manifest.meetings,
        files,
        created_at: manifest.created_at,
        restart_required: true,
    })
}

fn move_aside(path: &Path, stamp: &str) -> Result<Option<PathBuf>, String> {
    if std::fs::symlink_metadata(path).is_err() {
        return Ok(None);
    }
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "restore.invalid_path".to_string())?;
    let target = path.with_file_name(format!("{name}.{stamp}.pre-restore"));
    std::fs::rename(path, &target).map_err(io("move current library aside"))?;
    Ok(Some(target))
}

fn move_dir(from: &Path, to: &Path) -> Result<(), String> {
    if std::fs::rename(from, to).is_ok() {
        return Ok(());
    }
    // Another volume (a custom vault folder): copy, then remove the staged copy.
    for relative in collect_files(from)? {
        let target = to.join(&relative);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(io("restore sessions"))?;
        }
        std::fs::copy(from.join(&relative), &target).map_err(io("restore sessions"))?;
    }
    std::fs::remove_dir_all(from).map_err(io("clean staged sessions"))
}

/// Applies a restore staged by [`stage_restore`]. Call before the database
/// is opened. The current `app.db` (with `-wal`/`-shm`) and `sessions/` are
/// renamed to `*.<time>.pre-restore`, never deleted. `Ok(None)` when nothing
/// is staged.
pub fn apply_pending_restore(app_data_dir: &Path) -> Result<Option<AppliedRestore>, String> {
    let pending = app_data_dir.join(PENDING_DIR);
    let marker = pending.join(PENDING_MARKER);
    let Ok(bytes) = std::fs::read(&marker) else {
        return Ok(None);
    };
    let plan: PendingRestore =
        serde_json::from_slice(&bytes).map_err(|_| "restore.invalid_marker".to_string())?;
    let stamp = stamp();

    let database = app_data_dir.join(DATABASE);
    let previous_database = move_aside(&database, &stamp)?;
    for suffix in ["-wal", "-shm"] {
        move_aside(&app_data_dir.join(format!("{DATABASE}{suffix}")), &stamp)?;
    }
    std::fs::rename(pending.join(DATABASE), &database).map_err(io("restore database"))?;
    make_private(&database);

    let mut previous_sessions = None;
    let staged_sessions = pending.join(SESSIONS);
    if let Some(sessions_dir) = plan.vault_sessions_dir {
        previous_sessions = move_aside(&sessions_dir, &stamp)?;
        if staged_sessions.is_dir() {
            if let Some(parent) = sessions_dir.parent() {
                std::fs::create_dir_all(parent).map_err(io("restore sessions"))?;
            }
            move_dir(&staged_sessions, &sessions_dir)?;
        } else {
            std::fs::create_dir_all(&sessions_dir).map_err(io("restore sessions"))?;
        }
    }
    std::fs::remove_dir_all(&pending).map_err(io("clean staged restore"))?;
    Ok(Some(AppliedRestore {
        previous_database,
        previous_sessions,
    }))
}

#[cfg(test)]
mod tests;
