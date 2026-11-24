//! A1.2: no feature compares workspace roles itself. Scans `apps/desktop/src`,
//! `plugins` and `crates` (except `crates/aspc`) for comparisons against the
//! role names `owner`, `admin`, `member`, `viewer` and for `Role::Owner` and
//! friends. Chat message roles (`role === "user"`) are not workspace roles
//! and are not matched.
//!
//! Patterns, per line, outside test files:
//! - a quoted role name right after `==`, `===`, `!=`, `!==` or a spaced SQL
//!   ` = `, or right before `==`, `===`, `!=`, `!==`, or `|` inside a Rust
//!   `matches!`;
//! - `Role::Owner|Admin|Member|Viewer` as a whole path segment.

use std::path::{Path, PathBuf};

const ROLES: [&str; 4] = ["owner", "admin", "member", "viewer"];

/// Existing comparisons, allowed with a reason. Each entry must still match
/// something, so the list cannot go stale.
const ALLOWLIST: &[(&str, &str)] = &[
    (
        "apps/desktop/src/settings/team/index.tsx",
        "team members screen mirrors hosted roles to decide which buttons to show; the server enforces [hosted]",
    ),
    (
        "apps/desktop/src/contacts/organization-details.tsx",
        "shows workspace-linking controls for hosted owners/admins; UI only, the server enforces [hosted]",
    ),
    (
        "apps/desktop/src/session-sharing/source.ts",
        "sharing eligibility mirrors the hosted roles of the signed-in account; to move behind aspc with Phase 2b",
    ),
    (
        "apps/desktop/src/settings/team/client.ts",
        "validates the hosted API's role field before display [hosted]",
    ),
    (
        "apps/desktop/src/shared-notes/index.tsx",
        "shared-note capability (viewer/commenter/editor) granted by the hosted share; UI only, the server enforces [hosted]",
    ),
    (
        "apps/desktop/src/shared-notes/cache.ts",
        "validates the hosted share capability before caching [hosted]",
    ),
    (
        "apps/desktop/src/resource-sharing/share-dialog.tsx",
        "resource access kind (owner/team/guest) from the hosted share API; UI only [hosted]",
    ),
    (
        "apps/desktop/src/resource-sharing/library.tsx",
        "resource access kind from the hosted share API; UI only [hosted]",
    ),
    (
        "apps/desktop/src/resource-sharing/client.ts",
        "validates the hosted share API's access kind [hosted]",
    ),
    (
        "apps/desktop/src/folders/folder-editor.tsx",
        "resource access kind from the hosted share API; UI only [hosted]",
    ),
    (
        "apps/desktop/src/auth/cloudsync-credentials.ts",
        "validates the server's workspace projection (personal workspace must be owner)",
    ),
    (
        "apps/desktop/src/auth/cloudsync-workspace-keys.ts",
        "selects which hosted workspaces the server issues E2EE keys for; the server enforces",
    ),
    (
        "crates/db-app/src/cloudsync/projection.rs",
        "validates the server's workspace projection before storing it",
    ),
    (
        "crates/api-sync/src/routes/cloudsync_credentials/grants.rs",
        "hosted API input validation (role must be one of the hosted roles) [hosted]",
    ),
    (
        "crates/api-sync/src/routes/cloudsync_credentials/projection.rs",
        "hosted API projection validation [hosted]",
    ),
];

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = path.file_name().unwrap().to_string_lossy();
        if path.is_dir() {
            if matches!(
                name.as_ref(),
                "node_modules" | "target" | "vendor" | "dist" | "tests" | "gen"
            ) {
                continue;
            }
            collect(&path, out);
        } else if (name.ends_with(".rs") || name.ends_with(".ts") || name.ends_with(".tsx"))
            && !name.contains(".test.")
            && !name.ends_with(".gen.ts")
            && name != "tests.rs"
        {
            out.push(path);
        }
    }
}

fn line_compares_role(line: &str) -> bool {
    for role in ROLES {
        for quote in ['"', '\''] {
            let literal = format!("{quote}{role}{quote}");
            let mut search = 0;
            while let Some(found) = line[search..].find(&literal) {
                let at = search + found;
                let before = line[..at].trim_end();
                let after = line[at + literal.len()..].trim_start();
                // A lone `=` counts only as SQL ` = ` (spaced), not a JSX
                // attribute or a TypeScript type alias.
                let spaced_eq = before.ends_with('=')
                    && !before.ends_with("==")
                    && !before.ends_with("!=")
                    && line[..at].ends_with(' ')
                    && before[..before.len() - 1].ends_with(' ')
                    && !line.trim_start().starts_with("export type")
                    && !line.trim_start().starts_with("type ");
                let before_op = spaced_eq
                    || ["===", "!==", "==", "!="]
                        .iter()
                        .any(|op| before.ends_with(op));
                let after_op = ["===", "!==", "==", "!="]
                    .iter()
                    .any(|op| after.starts_with(op))
                    || (line.contains("matches!")
                        && after.starts_with('|')
                        && !after.starts_with("||"));
                if before_op || after_op {
                    return true;
                }
                search = at + literal.len();
            }
        }
    }
    for variant in ["Owner", "Admin", "Member", "Viewer"] {
        let path = format!("Role::{variant}");
        let mut search = 0;
        while let Some(found) = line[search..].find(&path) {
            let at = search + found;
            let boundary_before = line[..at]
                .chars()
                .last()
                .is_none_or(|c| !c.is_alphanumeric() && c != '_');
            let boundary_after = line[at + path.len()..]
                .chars()
                .next()
                .is_none_or(|c| !c.is_alphanumeric() && c != '_');
            if boundary_before && boundary_after {
                return true;
            }
            search = at + path.len();
        }
    }
    false
}

#[test]
fn no_role_comparisons_outside_aspc() {
    let root = repo_root();
    let mut files = Vec::new();
    for dir in ["apps/desktop/src", "plugins", "crates"] {
        collect(&root.join(dir), &mut files);
    }
    let aspc = root.join("crates/aspc");

    let mut violations = Vec::new();
    let mut allow_hits = vec![0usize; ALLOWLIST.len()];
    for file in &files {
        if file.starts_with(&aspc) {
            continue;
        }
        let relative = file
            .strip_prefix(&root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        let Ok(source) = std::fs::read_to_string(file) else {
            continue;
        };
        // Rust unit tests at the bottom of a file are test code.
        let source = source.split("#[cfg(test)]").next().unwrap_or("");
        for (index, line) in source.lines().enumerate() {
            if !line_compares_role(line) {
                continue;
            }
            if let Some(pos) = ALLOWLIST.iter().position(|(path, _)| *path == relative) {
                allow_hits[pos] += 1;
            } else {
                violations.push(format!("{relative}:{}: {}", index + 1, line.trim()));
            }
        }
    }
    assert!(
        violations.is_empty(),
        "role comparisons outside crates/aspc (use aspc::authorize): {violations:#?}"
    );
    let stale: Vec<_> = ALLOWLIST
        .iter()
        .zip(allow_hits)
        .filter(|(_, hits)| *hits == 0)
        .map(|((path, _), _)| *path)
        .collect();
    assert!(
        stale.is_empty(),
        "allowlist entries with no match: {stale:?}"
    );
}

#[test]
fn matcher_examples() {
    assert!(line_compares_role(r#"if (member.role === "owner") {"#));
    assert!(line_compares_role("AND membership.role = 'owner'"));
    assert!(line_compares_role(
        r#"matches!(r.as_str(), "owner" | "admin")"#
    ));
    assert!(line_compares_role("if ctx.role == Some(Role::Admin) {"));
    assert!(!line_compares_role(r#"if (message.role === "user") {"#));
    assert!(!line_compares_role("GoogleAccessRole::Owner"));
    assert!(!line_compares_role(r#"label: "owner""#));
    assert!(!line_compares_role(r#"a || "admin""#));
    assert!(!line_compares_role(
        r#"export type WorkspaceRole = "owner" | "admin";"#
    ));
    assert!(!line_compares_role(r#"value="viewer""#));
}
