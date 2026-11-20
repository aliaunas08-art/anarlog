//! The Tauri command guard (A2.3): data commands call
//! `aspc::require(&app, &window, data, action)?` as their first line.

use std::sync::Arc;

use tauri::Manager;

use crate::{Action, AspcState, DataType, Denied, Grant};

/// The label of the window or webview that invoked a command.
pub trait CallerLabel {
    fn caller_label(&self) -> &str;
}

impl<R: tauri::Runtime> CallerLabel for tauri::Window<R> {
    fn caller_label(&self) -> &str {
        self.label()
    }
}

impl<R: tauri::Runtime> CallerLabel for tauri::WebviewWindow<R> {
    fn caller_label(&self) -> &str {
        self.label()
    }
}

impl<R: tauri::Runtime> CallerLabel for tauri::Webview<R> {
    fn caller_label(&self) -> &str {
        self.label()
    }
}

impl CallerLabel for str {
    fn caller_label(&self) -> &str {
        self
    }
}

fn state<R: tauri::Runtime, M: Manager<R>>(app: &M) -> Option<Arc<AspcState>> {
    app.try_state::<Arc<AspcState>>()
        .map(|s| Arc::clone(s.inner()))
}

/// Checks the calling window's context against the matrix, the window
/// policy and the lock. Fails closed when the app has no access state.
pub fn require<R: tauri::Runtime, M: Manager<R>>(
    app: &M,
    window: &(impl CallerLabel + ?Sized),
    data: DataType,
    action: Action,
) -> Result<Grant, Denied> {
    crate::state::check(
        state(app).as_deref(),
        window.caller_label(),
        data,
        action,
        true,
    )
}

/// Like [`require`] but without the lock check. Only for commands whose lock
/// enforcement happens deeper and must allow a narrow lock-screen path: the
/// webview SQL commands, where the SQL authorizer denies everything except
/// the settings reads the lock screen needs.
pub fn require_ignoring_lock<R: tauri::Runtime, M: Manager<R>>(
    app: &M,
    window: &(impl CallerLabel + ?Sized),
    data: DataType,
    action: Action,
) -> Result<Grant, Denied> {
    crate::state::check(
        state(app).as_deref(),
        window.caller_label(),
        data,
        action,
        false,
    )
}
