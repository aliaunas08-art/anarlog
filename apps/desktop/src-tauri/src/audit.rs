//! ASPC D1: opens `audit.db` next to `app.db` and connects the denial
//! sources (the command guard and the webview SQL authorizer) to it.

use std::sync::Arc;

use anlg_audit_log::{AuditReader, AuditWriter, NewEntry, actions};
use anlg_local_accounts::KeychainStore;
use tauri::Manager;

pub async fn open(identifier: &str) -> Option<(AuditWriter, AuditReader)> {
    let dir = crate::db::desktop_db_dir(identifier)?;
    let path = dir.join(anlg_audit_log::AUDIT_DB_FILENAME);
    let keychain = Arc::new(KeychainStore::new(format!(
        "{identifier}.{}",
        anlg_audit_log::KEYCHAIN_SERVICE_SUFFIX
    )));
    match anlg_audit_log::open(&path, keychain).await {
        Ok(pair) => Some(pair),
        Err(error) => {
            tracing::error!(%error, "audit_log_open_failed");
            None
        }
    }
}

pub fn wire(
    app: &tauri::AppHandle<tauri::Wry>,
    access_state: &Arc<anlg_aspc::AspcState>,
    writer: &AuditWriter,
) {
    let denials = writer.clone();
    access_state.on_denial(move |ctx, denied| {
        let (key, entry) = anlg_audit_log::denial_entry(ctx, denied, "command", "");
        denials.submit_denial(&key, entry);
    });

    if let Some(runtime) = app.try_state::<tauri_plugin_db::ManagedState>() {
        let access_app = app.clone();
        runtime.set_content_access_observer(move |kind, id| {
            if kind == "transcript" && anlg_audit_log::is_identifier(id) {
                anlg_audit_log::record_transcript_access(&access_app, id, "get_meeting_transcript");
            }
        });
        let sql_denials = writer.clone();
        let state = Arc::clone(access_state);
        runtime.set_webview_sql_denial_sink(move |denial| {
            let ctx = state.context(None);
            let table = denial.table.clone().unwrap_or_default();
            let mut entry = NewEntry::new(
                actions::PERMISSION_DENIED,
                "sql",
                if anlg_audit_log::is_identifier(&table) {
                    table.clone()
                } else {
                    String::new()
                },
            )
            .id("code", denial.code);
            if let Some((data, action)) = denial.access {
                entry = entry
                    .id("data", format!("{data:?}").to_ascii_lowercase())
                    .id("access", format!("{action:?}").to_ascii_lowercase());
            }
            let key = format!("sql:{}:{table}", denial.code);
            sql_denials.submit_denial(&key, anlg_audit_log::with_context(entry, &ctx));
        });
    }

    // The Keychain may be slow or prompt; never wait on it here.
    let head = writer.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(error) = head.reconcile_head().await {
            tracing::error!(%error, "audit_head_reconcile_failed");
        }
    });
}
