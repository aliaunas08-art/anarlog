//! Data at rest on this Mac (ASPC A4.1, C2.1).
//!
//! - The app data directory is `0700` and the database, audit and store files
//!   in it are `0600`; [`harden_app_data`] checks and corrects that at startup.
//! - The database is not encrypted by the app (SQLCipher is a deferred gate,
//!   ASPC_BASE.md "Decisions"), so the app checks that FileVault is on and
//!   warns if it isn't ([`filevault_status`]).
//!
//! Nothing here reads file contents; callers log only paths and modes.

use std::path::{Path, PathBuf};

/// Whether FileVault protects the startup disk.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[cfg_attr(feature = "serde", serde(rename_all = "lowercase"))]
pub enum FileVault {
    On,
    Off,
    /// `fdesetup` is missing, failed, or printed something unexpected (also
    /// while encryption or decryption is in progress).
    Unknown,
}

/// Parses `fdesetup status` output (`FileVault is On.` / `FileVault is Off.`).
pub fn parse_fdesetup_status(stdout: &str) -> FileVault {
    let first = stdout.lines().next().unwrap_or_default().trim();
    match first {
        "FileVault is On." => FileVault::On,
        "FileVault is Off." => FileVault::Off,
        _ => FileVault::Unknown,
    }
}

/// Runs `/usr/bin/fdesetup status` (no admin rights needed). Blocking; call
/// it off the main thread. `Unknown` on other platforms.
pub fn filevault_status() -> FileVault {
    #[cfg(target_os = "macos")]
    {
        match std::process::Command::new("/usr/bin/fdesetup")
            .arg("status")
            .output()
        {
            Ok(output) if output.status.success() => {
                parse_fdesetup_status(&String::from_utf8_lossy(&output.stdout))
            }
            _ => FileVault::Unknown,
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        FileVault::Unknown
    }
}

pub const DIR_MODE: u32 = 0o700;
pub const FILE_MODE: u32 = 0o600;

/// One corrected permission: the path and its mode before and after.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModeChange {
    pub path: PathBuf,
    pub from: u32,
    pub to: u32,
}

/// Files directly in the app data directory that hold user data or settings:
/// SQLite databases with their `-wal`/`-shm`/`-journal` files (`app.db`,
/// `audit.db`) and JSON stores (`store.json`, `settings.json`, …).
pub fn is_protected_file_name(name: &str) -> bool {
    let base = ["-wal", "-shm", "-journal"]
        .iter()
        .find_map(|suffix| name.strip_suffix(suffix))
        .unwrap_or(name);
    base.ends_with(".db") || base.ends_with(".sqlite") || name.ends_with(".json")
}

/// Makes `dir` `0700` and every protected file directly in it `0600`, only
/// tightening (a mode that is already stricter is left alone). Symlinks are
/// never followed. Returns what changed; errors on single files are skipped
/// (the caller logs the result).
#[cfg(unix)]
pub fn harden_app_data(dir: &Path) -> std::io::Result<Vec<ModeChange>> {
    use std::os::unix::fs::PermissionsExt;

    let mut changes = Vec::new();
    let tighten = |path: &Path, wanted: u32, changes: &mut Vec<ModeChange>| {
        let Ok(meta) = std::fs::symlink_metadata(path) else {
            return;
        };
        if meta.file_type().is_symlink() {
            return;
        }
        let mode = meta.permissions().mode() & 0o7777;
        // Only clear bits beyond `wanted` (group/other access, setuid, …).
        let next = mode & wanted;
        if next != mode
            && std::fs::set_permissions(path, std::fs::Permissions::from_mode(next)).is_ok()
        {
            changes.push(ModeChange {
                path: path.to_path_buf(),
                from: mode,
                to: next,
            });
        }
    };

    let meta = std::fs::symlink_metadata(dir)?;
    if !meta.is_dir() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "app data path is not a directory",
        ));
    }
    tighten(dir, DIR_MODE, &mut changes);
    for entry in std::fs::read_dir(dir)?.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if !file_type.is_file() {
            continue;
        }
        let name = entry.file_name();
        if is_protected_file_name(&name.to_string_lossy()) {
            tighten(&entry.path(), FILE_MODE, &mut changes);
        }
    }
    Ok(changes)
}

#[cfg(not(unix))]
pub fn harden_app_data(_dir: &Path) -> std::io::Result<Vec<ModeChange>> {
    Ok(Vec::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_fdesetup_output() {
        assert_eq!(parse_fdesetup_status("FileVault is On.\n"), FileVault::On);
        assert_eq!(parse_fdesetup_status("FileVault is Off.\n"), FileVault::Off);
        assert_eq!(
            parse_fdesetup_status("Encryption in progress: Percent completed = 12\n"),
            FileVault::Unknown
        );
        assert_eq!(parse_fdesetup_status(""), FileVault::Unknown);
    }

    #[test]
    fn protected_names_cover_databases_and_stores() {
        for name in [
            "app.db",
            "app.db-wal",
            "app.db-shm",
            "audit.db",
            "audit.db-journal",
            "store.json",
        ] {
            assert!(is_protected_file_name(name), "{name}");
        }
        for name in ["launch.lock", "AGENTS.md", "notes.txt"] {
            assert!(!is_protected_file_name(name), "{name}");
        }
    }

    /// A4.1: permissions are checked on startup and corrected.
    #[cfg(unix)]
    #[test]
    fn startup_tightens_the_data_dir_and_its_files() {
        use std::os::unix::fs::PermissionsExt;

        let dir = std::env::temp_dir().join(format!(
            "aspc-protection-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(dir.join("sessions")).unwrap();
        let set = |path: &Path, mode: u32| {
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap()
        };
        let mode = |path: &Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o7777;
        set(&dir, 0o755);
        for (name, m) in [
            ("app.db", 0o644),
            ("app.db-wal", 0o644),
            ("audit.db", 0o600),
            ("store.json", 0o664),
            ("launch.lock", 0o644),
        ] {
            std::fs::write(dir.join(name), b"x").unwrap();
            set(&dir.join(name), m);
        }

        let changes = harden_app_data(&dir).unwrap();

        assert_eq!(mode(&dir), 0o700);
        assert_eq!(mode(&dir.join("app.db")), 0o600);
        assert_eq!(mode(&dir.join("app.db-wal")), 0o600);
        assert_eq!(mode(&dir.join("audit.db")), 0o600);
        assert_eq!(mode(&dir.join("store.json")), 0o600);
        assert_eq!(mode(&dir.join("launch.lock")), 0o644);
        assert_eq!(changes.len(), 4, "{changes:?}");
        assert!(changes.iter().all(|c| c.to & 0o077 == 0));
        // Idempotent.
        assert!(harden_app_data(&dir).unwrap().is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
