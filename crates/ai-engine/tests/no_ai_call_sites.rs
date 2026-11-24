//! C7.1: no AI call site exists outside the Efficiency Engine.
//!
//! - `apps/desktop/src`: no model-provider SDK import outside
//!   `src/ai/engine/`, and no provider API URL outside the allowlist.
//! - Every local crate the desktop app depends on (resolved from the
//!   Cargo manifests, normal and build dependencies): no provider API host
//!   outside `crates/ai-engine` and `plugins/ai-engine`, except the
//!   allowlist.
//!
//! Every allowlist entry says why, and an entry that no longer matches
//! anything fails the test, so the list can't go stale.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use regex::Regex;

/// Model-provider API hosts (LLM endpoints).
const PROVIDER_HOSTS: &[&str] = &[
    "api.anthropic.com",
    "api.openai.com",
    "openrouter.ai",
    "generativelanguage.googleapis.com",
    "aiplatform.googleapis.com",
    "openai.azure.com",
    "services.ai.azure.com",
    "api.mistral.ai",
    "api.groq.com",
    "api.x.ai",
    "api.deepseek.com",
    "api.together.xyz",
    "api.together.ai",
    "api.fireworks.ai",
    "api.cohere.ai",
    "api.cohere.com",
    "api.cerebras.ai",
    "api.moonshot.ai",
    "api.kimi.com",
    "api.venice.ai",
    "api.githubcopilot.com",
    "chatgpt.com",
    "dashscope-intl.aliyuncs.com",
    "api.z.ai",
    "api.siliconflow.com",
    "bedrock-runtime",
    "bedrock-mantle",
];

/// Paths (relative to the repo root, prefix match) that may contain
/// provider hosts, with the reason.
const HOST_ALLOWLIST: &[(&str, &str)] = &[
    (
        "apps/desktop/src/settings/ai/stt/",
        "speech-to-text provider endpoints and docs links; transcription runs in Rust (owhisper-client), not through an LLM",
    ),
    (
        "crates/owhisper-client/",
        "speech-to-text provider adapters (transcription is outside C7)",
    ),
    (
        "crates/listener-core/src/actors/session/types.rs",
        "speech-to-text base URLs in unit-test fixtures",
    ),
    (
        "crates/listener2-core/src/batch/",
        "speech-to-text base URLs in unit-test fixtures",
    ),
    (
        "plugins/misc/src/http_origins.rs",
        "an example origin in the origin-approval unit test; it makes no call",
    ),
];

/// The gateway itself.
const GATEWAY: &[&str] = &["crates/ai-engine/", "plugins/ai-engine/"];

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

fn files(dir: &Path, extensions: &[&str], out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if path.is_dir() {
            if name != "node_modules" && name != "target" && !name.starts_with('.') {
                files(&path, extensions, out);
            }
        } else if path
            .extension()
            .is_some_and(|e| extensions.iter().any(|x| e == *x))
        {
            out.push(path);
        }
    }
}

fn host_regex() -> Regex {
    let hosts = PROVIDER_HOSTS
        .iter()
        .map(|h| regex::escape(h))
        .collect::<Vec<_>>()
        .join("|");
    Regex::new(&format!(
        r"(?:https?|wss?)://(?:[a-z0-9*{{}}.\-]+\.)?(?:{hosts})"
    ))
    .unwrap()
}

fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap()
        .to_string_lossy()
        .replace('\\', "/")
}

fn is_test_file(rel: &str) -> bool {
    rel.contains(".test.") || rel.contains("/tests/") || rel.ends_with("/tests.rs")
}

/// Returns `(path, offending text)` for host hits outside the gateway,
/// split into allowed and not allowed, plus which allowlist entries matched.
fn scan_hosts(root: &Path, paths: &[PathBuf], used: &mut BTreeSet<&'static str>) -> Vec<String> {
    let re = host_regex();
    let mut problems = Vec::new();
    for path in paths {
        let rel = relative(root, path);
        if is_test_file(&rel) || GATEWAY.iter().any(|g| rel.starts_with(g)) {
            continue;
        }
        let source = std::fs::read_to_string(path).unwrap_or_default();
        for found in re.find_iter(&source) {
            match HOST_ALLOWLIST
                .iter()
                .find(|(prefix, _)| rel.starts_with(prefix))
            {
                Some((prefix, _)) => {
                    used.insert(prefix);
                }
                None => problems.push(format!("{rel}: {}", found.as_str())),
            }
        }
    }
    problems
}

#[test]
fn desktop_frontend_calls_no_provider_directly() {
    let root = repo_root();
    let src = root.join("apps/desktop/src");
    let mut paths = Vec::new();
    files(&src, &["ts", "tsx", "js", "jsx", "mjs"], &mut paths);
    assert!(paths.len() > 100, "scanned too few files: {}", paths.len());

    let import =
        Regex::new(r#"(?:from\s+|import\s*\(\s*|require\s*\(\s*)["']([^"']+)["']"#).unwrap();
    let mut problems = Vec::new();
    for path in &paths {
        let rel = relative(&root, path);
        if is_test_file(&rel) {
            continue;
        }
        let in_engine = rel.starts_with("apps/desktop/src/ai/engine/");
        let source = std::fs::read_to_string(path).unwrap();
        for capture in import.captures_iter(&source) {
            let module = &capture[1];
            let provider_sdk = (module.starts_with("@ai-sdk/") && module != "@ai-sdk/react")
                || module.starts_with("@openrouter/")
                || module.starts_with("@anthropic-ai/")
                || module.starts_with("@mistralai/")
                || module.starts_with("@google/generative-ai")
                || ["openai", "ollama", "groq-sdk", "cohere-ai"].contains(&module);
            let allowed = in_engine && module == "@ai-sdk/anthropic";
            if provider_sdk && !allowed {
                problems.push(format!("{rel}: imports {module}"));
            }
        }
    }

    let mut used = BTreeSet::new();
    problems.extend(scan_hosts(&root, &paths, &mut used));
    assert!(
        problems.is_empty(),
        "AI call sites outside the Efficiency Engine:\n{}",
        problems.join("\n")
    );
    assert!(
        used.contains("apps/desktop/src/settings/ai/stt/"),
        "stale allowlist entry: apps/desktop/src/settings/ai/stt/"
    );
}

/// Local crates the desktop app depends on, from the Cargo manifests.
fn desktop_dependency_dirs(root: &Path) -> BTreeSet<PathBuf> {
    let workspace: toml::Value =
        toml::from_str(&std::fs::read_to_string(root.join("Cargo.toml")).unwrap()).unwrap();
    let workspace_paths: BTreeMap<String, PathBuf> = workspace["workspace"]["dependencies"]
        .as_table()
        .unwrap()
        .iter()
        .filter_map(|(name, spec)| {
            spec.get("path")
                .and_then(|p| p.as_str())
                .map(|p| (name.clone(), root.join(p)))
        })
        .collect();

    let mut seen = BTreeSet::new();
    let mut queue = vec![root.join("apps/desktop/src-tauri")];
    while let Some(dir) = queue.pop() {
        let Ok(dir) = dir.canonicalize() else {
            continue;
        };
        if !seen.insert(dir.clone()) {
            continue;
        }
        let manifest: toml::Value =
            toml::from_str(&std::fs::read_to_string(dir.join("Cargo.toml")).unwrap()).unwrap();
        let mut tables = Vec::new();
        for key in ["dependencies", "build-dependencies"] {
            if let Some(table) = manifest.get(key).and_then(|t| t.as_table()) {
                tables.push(table.clone());
            }
        }
        if let Some(targets) = manifest.get("target").and_then(|t| t.as_table()) {
            for target in targets.values() {
                for key in ["dependencies", "build-dependencies"] {
                    if let Some(table) = target.get(key).and_then(|t| t.as_table()) {
                        tables.push(table.clone());
                    }
                }
            }
        }
        for table in tables {
            for (name, spec) in table {
                if let Some(path) = spec.get("path").and_then(|p| p.as_str()) {
                    queue.push(dir.join(path));
                } else if spec.get("workspace").and_then(|w| w.as_bool()) == Some(true)
                    && let Some(path) = workspace_paths.get(&name)
                {
                    queue.push(path.clone());
                }
            }
        }
    }
    seen
}

#[test]
fn desktop_rust_dependencies_call_no_provider_outside_the_engine() {
    let root = repo_root();
    let dirs = desktop_dependency_dirs(&root);
    let rel_dirs: BTreeSet<String> = dirs.iter().map(|d| relative(&root, d)).collect();
    assert!(rel_dirs.contains("plugins/ai-engine"), "{rel_dirs:?}");
    assert!(rel_dirs.contains("crates/ai-engine"));
    // The hosted LLM proxy server is not part of the desktop app.
    assert!(!rel_dirs.contains("crates/llm-proxy"));

    let mut paths = Vec::new();
    for dir in &dirs {
        files(&dir.join("src"), &["rs"], &mut paths);
    }
    let mut used = BTreeSet::new();
    let problems = scan_hosts(&root, &paths, &mut used);
    assert!(
        problems.is_empty(),
        "provider hosts outside the Efficiency Engine:\n{}",
        problems.join("\n")
    );
    let stale: Vec<&str> = HOST_ALLOWLIST
        .iter()
        .map(|(prefix, _)| *prefix)
        .filter(|prefix| !prefix.starts_with("apps/") && !used.contains(prefix))
        .collect();
    assert!(stale.is_empty(), "stale allowlist entries: {stale:?}");
}
