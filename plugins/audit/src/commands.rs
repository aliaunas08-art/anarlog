use anlg_aspc::{Action, DataType};
use anlg_audit_log::{EntryPage, NewEntry, VerifyReport, actions, is_identifier};
use tauri::Manager;

use crate::ReaderState;

fn reader<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> Result<anlg_audit_log::AuditReader, String> {
    app.try_state::<ReaderState>()
        .and_then(|state| state.0.clone())
        .ok_or_else(|| "audit.unavailable: the audit log could not be opened".to_string())
}

/// Settings -> Audit: one page of entries, newest first.
#[tauri::command]
#[specta::specta]
pub async fn list_entries<R: tauri::Runtime>(
    aspc_window: tauri::Window<R>,
    app: tauri::AppHandle<R>,
    before_seq: Option<i64>,
    limit: Option<i64>,
) -> Result<EntryPage, String> {
    anlg_aspc::require(&app, &aspc_window, DataType::Settings, Action::Administer)
        .map_err(|denied| denied.to_string())?;
    reader(&app)?
        .list(before_seq, limit.unwrap_or(50))
        .await
        .map_err(|error| error.to_string())
}

/// Settings -> Audit: walks the chain and checks it against the Keychain head.
#[tauri::command]
#[specta::specta]
pub async fn verify<R: tauri::Runtime>(
    aspc_window: tauri::Window<R>,
    app: tauri::AppHandle<R>,
) -> Result<VerifyReport, String> {
    anlg_aspc::require(&app, &aspc_window, DataType::Settings, Action::Administer)
        .map_err(|denied| denied.to_string())?;
    reader(&app)?
        .verify()
        .await
        .map_err(|error| error.to_string())
}

fn checked_id(value: &str) -> Result<&str, String> {
    is_identifier(value)
        .then_some(value)
        .ok_or_else(|| "audit.invalid_field: id".to_string())
}

/// A meeting was opened in a window. Debounced per meeting in Rust.
#[tauri::command]
#[specta::specta]
pub async fn record_meeting_view<R: tauri::Runtime>(
    aspc_window: tauri::Window<R>,
    app: tauri::AppHandle<R>,
    session_id: String,
) -> Result<(), String> {
    anlg_aspc::require(&app, &aspc_window, DataType::Notes, Action::Read)
        .map_err(|denied| denied.to_string())?;
    let session_id = checked_id(&session_id)?;
    anlg_audit_log::record_debounced(
        &app,
        &format!("view:{session_id}"),
        NewEntry::new(actions::MEETING_VIEWED, "meeting", session_id),
    );
    Ok(())
}

const ASK_SCOPES: &[&str] = &["meeting", "library", "folder", "unscoped"];
const MAX_ASK_IDS: usize = 200;

/// An Ask query ran: its scope and the meeting ids it may read. Never the
/// query text (D1.4).
#[tauri::command]
#[specta::specta]
pub async fn record_ask<R: tauri::Runtime>(
    aspc_window: tauri::Window<R>,
    app: tauri::AppHandle<R>,
    scope: String,
    meeting_ids: Vec<String>,
    include_private: bool,
) -> Result<(), String> {
    anlg_aspc::require(&app, &aspc_window, DataType::Notes, Action::Read)
        .map_err(|denied| denied.to_string())?;
    if !ASK_SCOPES.contains(&scope.as_str()) {
        return Err("audit.invalid_field: scope".into());
    }
    if meeting_ids.len() > MAX_ASK_IDS || !meeting_ids.iter().all(|id| is_identifier(id)) {
        return Err("audit.invalid_field: meeting_ids".into());
    }
    anlg_audit_log::record(
        &app,
        NewEntry::new(actions::ASK_QUERY, "ask", "")
            .id("scope", scope)
            .count("meetings", meeting_ids.len() as i64)
            .ids("meeting_ids", meeting_ids)
            .flag("include_private", include_private),
    );
    Ok(())
}

const MAX_SETTING_KEYS: usize = 100;

/// Settings written by the webview (key names only, never values).
#[tauri::command]
#[specta::specta]
pub async fn record_settings_change<R: tauri::Runtime>(
    aspc_window: tauri::Window<R>,
    app: tauri::AppHandle<R>,
    keys: Vec<String>,
) -> Result<(), String> {
    anlg_aspc::require(&app, &aspc_window, DataType::Settings, Action::Edit)
        .map_err(|denied| denied.to_string())?;
    if keys.is_empty() {
        return Ok(());
    }
    if keys.len() > MAX_SETTING_KEYS || !keys.iter().all(|key| is_identifier(key)) {
        return Err("audit.invalid_field: keys".into());
    }
    anlg_audit_log::record(
        &app,
        NewEntry::new(actions::SETTINGS_CHANGED, "settings", "app_settings").ids("keys", keys),
    );
    Ok(())
}
