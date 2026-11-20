//! Recording from Tauri plugins: the writer and the actor come from Tauri
//! state (`AuditWriter`, `Arc<AspcState>`). Never blocks and never fails
//! the caller: a missing writer (tests, early startup) only logs.

use std::sync::Arc;
use std::time::Duration;

use anlg_aspc::{AccessContext, AspcState, Denied};
use tauri::Manager;

use crate::{AuditWriter, NewEntry, Source, actions};

/// How long a repeated "meeting viewed" / "transcript accessed" for the same
/// meeting is folded into the first one.
pub const VIEW_DEBOUNCE: Duration = Duration::from_secs(5 * 60);

/// Fills the actor fields from an access context.
pub fn with_context(mut entry: NewEntry, ctx: &AccessContext) -> NewEntry {
    entry.actor_id = ctx.actor_id.clone();
    entry.role = ctx.role.map(|r| r.as_str().to_string()).unwrap_or_default();
    entry.workspace_id = ctx.workspace_id.clone();
    entry.device_id = ctx.device_id.clone();
    entry
}

fn writer_and_context<R: tauri::Runtime, M: Manager<R>>(
    app: &M,
) -> Option<(AuditWriter, Option<AccessContext>)> {
    let writer = app.try_state::<AuditWriter>()?.inner().clone();
    let ctx = app
        .try_state::<Arc<AspcState>>()
        .map(|state| state.context(None));
    Some((writer, ctx))
}

fn prepared<R: tauri::Runtime, M: Manager<R>>(
    app: &M,
    entry: NewEntry,
) -> Option<(AuditWriter, NewEntry)> {
    let Some((writer, ctx)) = writer_and_context(app) else {
        tracing::debug!(action = %entry.action, "audit_writer_missing");
        return None;
    };
    let entry = match ctx {
        Some(ctx) => with_context(entry, &ctx),
        None => entry,
    };
    Some((writer, entry))
}

/// Queues an entry as the local actor, source `app`.
pub fn record<R: tauri::Runtime, M: Manager<R>>(app: &M, entry: NewEntry) {
    if let Some((writer, entry)) = prepared(app, entry) {
        writer.submit(entry);
    }
}

/// Like [`record`], as a background job (source `job`).
pub fn record_job<R: tauri::Runtime, M: Manager<R>>(app: &M, entry: NewEntry) {
    record(app, entry.source(Source::Job));
}

/// Like [`record`], folded with identical `key`s within [`VIEW_DEBOUNCE`].
pub fn record_debounced<R: tauri::Runtime, M: Manager<R>>(app: &M, key: &str, entry: NewEntry) {
    if let Some((writer, entry)) = prepared(app, entry) {
        writer.submit_debounced(key, VIEW_DEBOUNCE, entry);
    }
}

/// `transcript.accessed` for one meeting, debounced.
pub fn record_transcript_access<R: tauri::Runtime, M: Manager<R>>(
    app: &M,
    session_id: &str,
    via: &str,
) {
    record_debounced(
        app,
        &format!("transcript:{session_id}"),
        NewEntry::new(actions::TRANSCRIPT_ACCESSED, "meeting", session_id).id("via", via),
    );
}

/// The `permission.denied` entry for a denial and its rate-limit key.
/// `subject` names what was denied (`plugin:command` or `sql:<table>`).
pub fn denial_entry(
    ctx: &AccessContext,
    denied: &Denied,
    kind: &str,
    subject: &str,
) -> (String, NewEntry) {
    let data = format!("{:?}", denied.data).to_ascii_lowercase();
    let action = format!("{:?}", denied.action).to_ascii_lowercase();
    let key = format!("{kind}:{}:{data}:{action}", denied.code.as_str());
    let subject = if crate::is_identifier(subject) {
        subject.to_string()
    } else {
        String::new()
    };
    let mut entry = NewEntry::new(actions::PERMISSION_DENIED, kind, subject)
        .id("code", denied.code.as_str())
        .id("data", data)
        .id("access", action);
    if let Some(window) = ctx.window.as_deref().filter(|w| crate::is_identifier(w)) {
        entry = entry.id("window", window);
    }
    (key, with_context(entry, ctx))
}
