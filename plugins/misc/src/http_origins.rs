//! User-approved extra hosts for the `http` plugin scope (ASPC A3.3).
//!
//! The static scope in `capabilities/*.json` lists the provider, calendar and
//! app hosts the frontend uses. A user can point a provider at their own base
//! URL (custom OpenAI-compatible server, Azure resource, LAN model server).
//! For those, the webview asks Rust to allow the origin; Rust shows a native
//! confirmation naming the exact origin, and only on "Allow" adds a runtime
//! capability scoped to that origin and remembers it in the app data
//! directory (not writable through any webview command).

use std::collections::BTreeSet;
use std::path::PathBuf;

use tauri::Manager;

const STORE_FILE: &str = "approved-http-origins.json";
/// Windows that make provider calls. Overlays never get extra hosts.
const WINDOWS: &[&str] = &["main", "note-*", "composer"];

/// The canonical origin (`scheme://host[:port]`) for a URL the user may
/// approve, or why it can't be approved.
pub(crate) fn approvable_origin(url: &str) -> Result<String, String> {
    let parsed = url::Url::parse(url.trim()).map_err(|_| "invalid URL".to_string())?;
    let host = parsed.host().ok_or_else(|| "URL has no host".to_string())?;
    match parsed.scheme() {
        "https" => {}
        // Plain http only for a model server on the local network (C2.2);
        // loopback is already in the static scope.
        "http" if is_lan_host(&host) => {}
        "http" => return Err("only https:// URLs can be allowed".to_string()),
        _ => return Err("only https:// URLs can be allowed".to_string()),
    }
    let origin = parsed.origin().ascii_serialization();
    if origin == "null" {
        return Err("URL has no origin".to_string());
    }
    Ok(origin)
}

fn is_lan_host(host: &url::Host<&str>) -> bool {
    match host {
        url::Host::Ipv4(ip) => ip.is_private() || ip.is_link_local(),
        url::Host::Ipv6(ip) => {
            let first = ip.segments()[0];
            (first & 0xfe00) == 0xfc00 || (first & 0xffc0) == 0xfe80
        }
        url::Host::Domain(domain) => domain.to_ascii_lowercase().ends_with(".local"),
    }
}

fn store_path<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Option<PathBuf> {
    app.path()
        .app_data_dir()
        .ok()
        .map(|dir| dir.join(STORE_FILE))
}

fn load<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> BTreeSet<String> {
    store_path(app)
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|content| serde_json::from_str::<BTreeSet<String>>(&content).ok())
        .unwrap_or_default()
        .into_iter()
        .filter(|origin| approvable_origin(origin).as_deref() == Ok(origin.as_str()))
        .collect()
}

fn save<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    origins: &BTreeSet<String>,
) -> Result<(), String> {
    let path = store_path(app).ok_or_else(|| "no app data directory".to_string())?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let content = serde_json::to_string_pretty(origins).map_err(|e| e.to_string())?;
    std::fs::write(path, content).map_err(|e| e.to_string())
}

fn capability_id(origin: &str) -> String {
    let sanitized: String = origin
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    format!("user-http-origin-{sanitized}")
}

fn apply<R: tauri::Runtime>(app: &tauri::AppHandle<R>, origin: &str) -> Result<(), String> {
    let capability = tauri::ipc::CapabilityBuilder::new(capability_id(origin))
        .windows(WINDOWS.iter().copied())
        .permission_scoped(
            "http:default",
            vec![serde_json::json!({ "url": format!("{origin}/*") })],
            vec![],
        );
    app.add_capability(capability).map_err(|e| e.to_string())
}

/// Re-applies every remembered origin. Called once at startup.
pub(crate) fn restore<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    for origin in load(app) {
        if let Err(error) = apply(app, &origin) {
            tracing::warn!(%error, "http_origin_restore_failed");
        }
    }
}

/// Asks the user to allow `url`'s origin and, if they do, adds it to the
/// `http` scope. Returns whether the origin is allowed afterwards.
pub(crate) async fn request<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    window: &tauri::WebviewWindow<R>,
    url: &str,
) -> Result<bool, String> {
    use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};

    let origin = approvable_origin(url)?;
    let mut origins = load(app);
    if origins.contains(&origin) {
        return Ok(true);
    }

    let unencrypted = origin.starts_with("http://");
    let message = format!(
        "Notes wants to connect to {origin}.\n\nThis address was set as an AI provider or server URL. Allow it only if you entered it yourself: Notes will send meeting text and API keys to it.{}",
        if unencrypted {
            "\n\nThis connection is not encrypted."
        } else {
            ""
        }
    );
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .message(message)
        .title("Allow connection?")
        .kind(MessageDialogKind::Warning)
        .buttons(MessageDialogButtons::OkCancelCustom(
            "Allow".to_string(),
            "Don't Allow".to_string(),
        ))
        .parent(window)
        .show(move |allowed| {
            let _ = tx.send(allowed);
        });
    if !rx.await.unwrap_or(false) {
        return Ok(false);
    }

    apply(app, &origin)?;
    origins.insert(origin);
    save(app, &origins)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::approvable_origin;

    #[test]
    fn only_https_or_lan_http_origins_are_approvable() {
        assert_eq!(
            approvable_origin("https://my-resource.openai.azure.com/openai/v1").as_deref(),
            Ok("https://my-resource.openai.azure.com")
        );
        assert_eq!(
            approvable_origin("http://192.168.1.20:11434/v1").as_deref(),
            Ok("http://192.168.1.20:11434")
        );
        assert_eq!(
            approvable_origin("http://studio.local:1234").as_deref(),
            Ok("http://studio.local:1234")
        );
        assert!(approvable_origin("http://example.com/v1").is_err());
        assert!(approvable_origin("http://8.8.8.8/v1").is_err());
        assert!(approvable_origin("file:///etc/passwd").is_err());
        assert!(approvable_origin("not a url").is_err());
    }
}
