//! A2.2: the registry in `src/commands.rs` matches the commands the app and
//! every plugin actually register, and guarded commands call the guard.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use aspc::commands::{COMMANDS, Guard};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Names inside `collect_commands![...]` / `generate_handler![...]`.
fn registered_commands(source: &str) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for marker in ["collect_commands!", "generate_handler!"] {
        let mut rest = source;
        while let Some(start) = rest.find(marker) {
            rest = &rest[start + marker.len()..];
            let Some(open) = rest.find('[') else { break };
            let Some(close) = rest[open..].find(']') else {
                break;
            };
            let body = &rest[open + 1..open + close];
            rest = &rest[open + close..];
            let body: String = body
                .lines()
                .map(|line| line.split("//").next().unwrap_or(""))
                .collect::<Vec<_>>()
                .join("\n");
            let mut cleaned = String::new();
            let mut depth = 0usize;
            let mut in_attr = false;
            let chars: Vec<char> = body.chars().collect();
            let mut i = 0;
            while i < chars.len() {
                let c = chars[i];
                if !in_attr && c == '#' && chars.get(i + 1) == Some(&'[') {
                    in_attr = true;
                } else if in_attr && c == ']' {
                    in_attr = false;
                } else if !in_attr {
                    match c {
                        '<' => depth += 1,
                        '>' => depth = depth.saturating_sub(1),
                        _ if depth == 0 => cleaned.push(c),
                        _ => {}
                    }
                }
                i += 1;
            }
            for item in cleaned.split(',') {
                let item = item.trim().trim_end_matches(':');
                if let Some(name) = item.rsplit("::").next().filter(|n| !n.is_empty()) {
                    names.insert(name.to_string());
                }
            }
        }
    }
    names
}

fn real_commands() -> BTreeSet<(String, String)> {
    let root = repo_root();
    let mut all = BTreeSet::new();
    for entry in std::fs::read_dir(root.join("plugins")).unwrap() {
        let dir = entry.unwrap().path();
        let lib = dir.join("src/lib.rs");
        let Ok(source) = std::fs::read_to_string(&lib) else {
            continue;
        };
        let plugin = dir.file_name().unwrap().to_string_lossy().to_string();
        for name in registered_commands(&source) {
            all.insert((plugin.clone(), name));
        }
    }
    let app = std::fs::read_to_string(root.join("apps/desktop/src-tauri/src/lib.rs")).unwrap();
    for name in registered_commands(&app) {
        all.insert(("app".to_string(), name));
    }
    all
}

#[test]
fn registry_matches_real_commands() {
    let real = real_commands();
    assert!(real.len() > 400, "parsed only {} commands", real.len());
    let listed: BTreeSet<(String, String)> = COMMANDS
        .iter()
        .map(|e| (e.plugin.to_string(), e.name.to_string()))
        .collect();
    assert_eq!(listed.len(), COMMANDS.len(), "duplicate registry entries");

    let missing: Vec<_> = real.difference(&listed).collect();
    let stale: Vec<_> = listed.difference(&real).collect();
    assert!(
        missing.is_empty() && stale.is_empty(),
        "add to crates/aspc/src/commands.rs: {missing:?}\nremove (no longer exist): {stale:?}"
    );
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// The body of `fn name` up to the first line containing a guard call or
/// the 30th line, whichever comes first.
fn function_heads(files: &[PathBuf], name: &str) -> Vec<String> {
    let mut heads = Vec::new();
    for file in files {
        let source = std::fs::read_to_string(file).unwrap();
        let lines: Vec<&str> = source.lines().collect();
        for (index, line) in lines.iter().enumerate() {
            let trimmed = line.trim_start();
            let is_fn = [
                "pub async fn ",
                "pub(crate) async fn ",
                "async fn ",
                "pub fn ",
                "pub(crate) fn ",
                "fn ",
            ]
            .iter()
            .any(|prefix| {
                trimmed.strip_prefix(prefix).is_some_and(|rest| {
                    rest.strip_prefix(name)
                        .is_some_and(|after| after.starts_with('<') || after.starts_with('('))
                })
            });
            let is_command = index >= 3
                && lines[index.saturating_sub(4)..index]
                    .iter()
                    .any(|l| l.contains("tauri::command"));
            if is_fn && is_command {
                heads.push(lines[index..(index + 30).min(lines.len())].join("\n"));
            }
        }
    }
    heads
}

#[test]
fn guarded_commands_call_the_guard() {
    let root = repo_root();
    let mut problems = Vec::new();
    for entry in COMMANDS.iter().filter(|e| e.is_guarded()) {
        let dir = if entry.plugin == "app" {
            root.join("apps/desktop/src-tauri/src")
        } else {
            root.join("plugins").join(entry.plugin).join("src")
        };
        let mut files = Vec::new();
        rust_files(&dir, &mut files);
        let heads = function_heads(&files, entry.name);
        let call = match entry.guard {
            Guard::Enforced => "aspc::require(",
            Guard::LockExempt(_) => "aspc::require_ignoring_lock(",
            _ => unreachable!(),
        };
        if heads.is_empty() || !heads.iter().all(|head| head.contains(call)) {
            problems.push(format!("{}:{} (expected {call})", entry.plugin, entry.name));
        }
    }
    assert!(problems.is_empty(), "guard missing: {problems:#?}");
}

/// A2.3: each guarded command's guard call names the same `(DataType,
/// Action)` as its registry entry, so the guard-logic test in
/// `state::tests::guard_denies_locked_viewer_and_wrong_window` (which runs
/// every registry entry through the guard) covers what the command really
/// enforces. Also ratchets the Pending count (155 on 2026-11-18, after the
/// Recording batch 3) so it cannot grow.
#[test]
fn guarded_commands_require_their_registry_permission() {
    use aspc::commands::Access;

    let root = repo_root();
    let mut problems = Vec::new();
    let mut guarded = 0usize;
    for entry in COMMANDS.iter().filter(|e| e.is_guarded()) {
        guarded += 1;
        let Access::Data(data, action) = entry.access else {
            problems.push(format!(
                "{}:{} guarded but Public",
                entry.plugin, entry.name
            ));
            continue;
        };
        let dir = if entry.plugin == "app" {
            root.join("apps/desktop/src-tauri/src")
        } else {
            root.join("plugins").join(entry.plugin).join("src")
        };
        let mut files = Vec::new();
        rust_files(&dir, &mut files);
        let call = match entry.guard {
            Guard::Enforced => "aspc::require(",
            Guard::LockExempt(_) => "aspc::require_ignoring_lock(",
            _ => unreachable!(),
        };
        let want_data = format!("DataType::{data:?}");
        let want_action = format!("Action::{action:?}");
        for head in function_heads(&files, entry.name) {
            let Some(at) = head.find(call) else {
                problems.push(format!("{}:{} no {call}", entry.plugin, entry.name));
                continue;
            };
            // The guard call's arguments: up to the closing `)?` / `)` line.
            let args: String = head[at..].lines().take(8).collect::<Vec<_>>().join(" ");
            if !args.contains(&want_data) || !args.contains(&want_action) {
                problems.push(format!(
                    "{}:{} guard call does not name {want_data}, {want_action}: {args}",
                    entry.plugin, entry.name
                ));
            }
        }
    }
    assert!(problems.is_empty(), "{problems:#?}");

    let pending = COMMANDS
        .iter()
        .filter(|e| e.guard == Guard::Pending)
        .count();
    eprintln!(
        "registry: {} commands, {guarded} guarded, {pending} Pending",
        COMMANDS.len()
    );
    // Ratchet: new commands must not be added unguarded.
    assert!(
        pending <= 155,
        "Pending count went up to {pending}: guard the new command"
    );
}
