//! D1.1: the app's normal data code has no handle to `audit.db`.
//!
//! - `db-app`, `db-core` and the `db` plugin neither depend on this crate nor
//!   name the file.
//! - The CLI may only append (`open_append_only`); it never opens a reader.
//!
//! The webview side (the `db` plugin's connection cannot ATTACH or read
//! `audit*`) is tested in `crates/db-core/src/webview_sql.rs`
//! (`webview_sql_cannot_reach_the_audit_log`).

use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap()
        .to_path_buf()
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path
                .file_name()
                .is_some_and(|n| n == "target" || n == "node_modules")
            {
                continue;
            }
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

fn normal_dependencies(manifest: &Path) -> String {
    let text = std::fs::read_to_string(manifest).unwrap();
    let mut out = String::new();
    let mut in_deps = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_deps = trimmed == "[dependencies]"
                || trimmed.ends_with(".dependencies]") && !trimmed.contains("dev-");
        } else if in_deps {
            out.push_str(trimmed);
            out.push('\n');
        }
    }
    out
}

#[test]
fn app_data_code_has_no_handle_to_the_audit_log() {
    let root = root();
    for component in ["crates/db-app", "crates/db-core", "plugins/db"] {
        let dir = root.join(component);
        let deps = normal_dependencies(&dir.join("Cargo.toml"));
        assert!(
            !deps.contains("audit-log") && !deps.contains("audit_log"),
            "{component} depends on the audit log"
        );
        let mut files = Vec::new();
        rust_files(&dir.join("src"), &mut files);
        assert!(!files.is_empty(), "{component} has no sources");
        for file in files {
            let text = std::fs::read_to_string(&file).unwrap();
            // Unit tests may name the file to prove it is unreachable.
            let text = text
                .split("#[cfg(test)]\nmod tests")
                .next()
                .unwrap_or_default();
            assert!(
                !text.contains("anlg_audit_log") && !text.contains("audit.db\""),
                "{} reaches the audit log",
                file.display()
            );
        }
    }
}

#[test]
fn the_cli_can_only_append() {
    let mut files = Vec::new();
    rust_files(&root().join("apps/cli/src"), &mut files);
    let mut appends = 0;
    for file in files {
        let text = std::fs::read_to_string(&file).unwrap();
        for forbidden in [
            "AuditReader",
            "anlg_audit_log::open(",
            ".verify()",
            "reconcile_head",
        ] {
            assert!(
                !(text.contains("anlg_audit_log") && text.contains(forbidden)),
                "{} uses {forbidden}",
                file.display()
            );
        }
        appends += text.matches("anlg_audit_log::open_append_only").count();
    }
    assert!(appends > 0, "the CLI no longer records its actions");
}
