//! Path checks for commands that take a filesystem path from the webview
//! (ASPC A3.6).
//!
//! A path is accepted when it resolves (no `..`, symlinks resolved on the
//! existing part) inside one of the app's own roots, or when the user picked it
//! in a native dialog opened by Rust moments ago. Dialog picks are single-use
//! and expire.

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::Error;

const GRANT_TTL: Duration = Duration::from_secs(10 * 60);

/// File types the webview may write inside the app's roots without a dialog.
pub(crate) const ROOT_WRITE_EXTENSIONS: &[&str] = &["md", "txt", "org", "pdf"];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum GrantKind {
    Read,
    Write,
}

#[derive(Default)]
pub struct PathGrants {
    grants: Mutex<HashMap<(GrantKind, PathBuf), Instant>>,
}

impl PathGrants {
    pub(crate) fn record(&self, kind: GrantKind, path: &Path) -> Result<PathBuf, Error> {
        let resolved = resolve(path)?;
        let mut grants = self.grants.lock().unwrap();
        let now = Instant::now();
        grants.retain(|_, granted_at| now.duration_since(*granted_at) < GRANT_TTL);
        grants.insert((kind, resolved.clone()), now);
        Ok(resolved)
    }

    /// Consumes a matching grant. Returns false if there was none or it expired.
    pub(crate) fn consume(&self, kind: GrantKind, resolved: &Path) -> bool {
        let mut grants = self.grants.lock().unwrap();
        match grants.remove(&(kind, resolved.to_path_buf())) {
            Some(granted_at) => granted_at.elapsed() < GRANT_TTL,
            None => false,
        }
    }
}

/// Resolve an absolute path for checking: refuses `..`, resolves symlinks on
/// the longest existing ancestor, and appends the not-yet-existing rest.
pub(crate) fn resolve(path: &Path) -> Result<PathBuf, Error> {
    if !path.is_absolute() {
        return Err(Error::Path(format!(
            "path must be absolute: {}",
            path.display()
        )));
    }
    if path
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(Error::PathForbidden(path.to_path_buf()));
    }

    let mut existing = path.to_path_buf();
    let mut rest = Vec::new();
    while !existing.exists() {
        let Some(name) = existing.file_name().map(|name| name.to_os_string()) else {
            return Err(Error::Path(format!("invalid path: {}", path.display())));
        };
        rest.push(name);
        if !existing.pop() {
            return Err(Error::Path(format!("invalid path: {}", path.display())));
        }
    }
    let mut resolved = existing.canonicalize()?;
    for name in rest.into_iter().rev() {
        resolved.push(name);
    }
    Ok(resolved)
}

pub(crate) fn is_under_any(resolved: &Path, roots: &[PathBuf]) -> bool {
    roots.iter().any(|root| resolved.starts_with(root))
}

pub(crate) fn has_extension(path: &Path, allowed: &[&str]) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            allowed
                .iter()
                .any(|allowed| allowed.eq_ignore_ascii_case(extension))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_rejects_parent_components_and_relative_paths() {
        let dir = tempfile::tempdir().unwrap();
        let escape = dir.path().join("vault/../../etc/passwd");
        assert!(matches!(resolve(&escape), Err(Error::PathForbidden(_))));
        assert!(resolve(Path::new("relative/file.md")).is_err());
    }

    #[test]
    fn resolve_follows_symlinks_out_of_a_root() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("vault");
        let outside = dir.path().join("outside");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&outside, root.join("link")).unwrap();
        let roots = vec![root.canonicalize().unwrap()];

        let inside = resolve(&root.join("new/dir/note.md")).unwrap();
        assert!(is_under_any(&inside, &roots));
        #[cfg(unix)]
        {
            let through_link = resolve(&root.join("link/evil.md")).unwrap();
            assert!(!is_under_any(&through_link, &roots));
        }
    }

    #[test]
    fn grants_are_single_use() {
        let dir = tempfile::tempdir().unwrap();
        let grants = PathGrants::default();
        let path = dir.path().join("export.md");
        let resolved = grants.record(GrantKind::Write, &path).unwrap();
        assert!(!grants.consume(GrantKind::Read, &resolved));
        assert!(grants.consume(GrantKind::Write, &resolved));
        assert!(!grants.consume(GrantKind::Write, &resolved));
    }

    #[test]
    fn root_writes_are_limited_to_document_extensions() {
        assert!(has_extension(Path::new("/a/b.MD"), ROOT_WRITE_EXTENSIONS));
        assert!(!has_extension(
            Path::new("/a/b.command"),
            ROOT_WRITE_EXTENSIONS
        ));
        assert!(!has_extension(Path::new("/a/b"), ROOT_WRITE_EXTENSIONS));
    }
}
