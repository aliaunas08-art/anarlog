//! CLI actions in the audit log (ASPC A2.7, D1.4), source `cli`.
//!
//! The CLI only appends ([`anlg_audit_log::open_append_only`]): it never
//! reads or verifies the log and never touches the Keychain head. Recording
//! is best effort: a CLI action is not refused because the log can't be
//! written.

use crate::{Args, db};

/// `audit.db` next to the database the CLI uses, if that folder exists.
pub(crate) fn audit_path(args: &Args) -> Option<std::path::PathBuf> {
    let path = db::resolve_path(args).ok()?;
    let dir = path.parent().filter(|dir| dir.is_dir())?;
    Some(dir.join(anlg_audit_log::AUDIT_DB_FILENAME))
}

pub(crate) async fn record(args: &Args, entry: anlg_audit_log::NewEntry) {
    let Some(path) = audit_path(args) else {
        return;
    };
    let entry = entry.source(anlg_audit_log::Source::Cli);
    if let Ok(writer) = anlg_audit_log::open_append_only(&path).await {
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
