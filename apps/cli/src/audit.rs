//! CLI actions in the audit log (ASPC A2.7, D1.4), source `cli`.
//!
//! The CLI only appends ([`anlg_audit_log::open_append_only`]): it never
//! reads or verifies the log and never touches the Keychain head. Recording
//! is best effort: a CLI action is not refused because the log can't be
//! written.

use crate::{Args, db};

pub(crate) async fn record(args: &Args, entry: anlg_audit_log::NewEntry) {
    let Ok(path) = db::resolve_path(args) else {
        return;
    };
    let Some(dir) = path.parent().filter(|dir| dir.is_dir()) else {
        return;
    };
    let entry = entry.source(anlg_audit_log::Source::Cli);
    if let Ok(writer) =
        anlg_audit_log::open_append_only(&dir.join(anlg_audit_log::AUDIT_DB_FILENAME)).await
    {
        let _ = writer.append(entry).await;
    }
}

pub(crate) fn meeting_entry(action: &str, id: &str) -> anlg_audit_log::NewEntry {
    let id = if anlg_audit_log::is_identifier(id) {
        id
    } else {
        ""
    };
    anlg_audit_log::NewEntry::new(action, "meeting", id)
}
