//! The Rust-held app lock (A4.2–A4.4): unlock through device
//! authentication run here, relock on window hide and sleep, and the event
//! every window listens to.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use anlg_aspc::{AspcState, AuthResult, LockSnapshot, UnlockOutcome};
use tauri::{Manager, Runtime};
use tauri_specta::Event;

use crate::{Error, LocalAuthPluginExt};

/// Fixed in Rust so the webview can't choose the system prompt's text.
const UNLOCK_REASON: &str = "open";
const CHANGE_SETTING_REASON: &str = "change lock settings";
/// Time the webview gets to flush pending writes before a relock lands.
pub const RELOCK_GRACE: Duration = Duration::from_secs(5);
/// While a recording runs, a pending relock is retried at this interval.
const BUSY_RETRY: Duration = Duration::from_secs(15);

#[derive(Clone, Debug, serde::Serialize, specta::Type, tauri_specta::Event)]
#[serde(rename_all = "camelCase")]
pub struct AppLockChanged {
    pub enabled: bool,
    pub unlocked: bool,
}

impl From<LockSnapshot> for AppLockChanged {
    fn from(value: LockSnapshot) -> Self {
        Self {
            enabled: value.enabled,
            unlocked: value.unlocked,
        }
    }
}

type BusyCheck = Arc<dyn Fn() -> Pin<Box<dyn Future<Output = bool> + Send>> + Send + Sync>;

/// Plugin-private state: serialises unlock prompts and holds relock hooks.
#[derive(Default)]
pub(crate) struct LockRuntime {
    unlock: tokio::sync::Mutex<()>,
    relock_generation: AtomicU64,
    busy: std::sync::Mutex<Option<BusyCheck>>,
}

pub(crate) fn aspc_state<R: Runtime, M: Manager<R>>(app: &M) -> Result<Arc<AspcState>, String> {
    app.try_state::<Arc<AspcState>>()
        .map(|state| Arc::clone(state.inner()))
        .ok_or_else(|| "aspc.no_context: access state is not initialised".to_string())
}

fn auth_result(result: Result<bool, Error>) -> AuthResult {
    match result {
        Ok(true) => AuthResult::Success,
        Ok(false) => AuthResult::Failed,
        Err(Error::Unavailable) => AuthResult::Unavailable,
        Err(error) => {
            tracing::warn!(%error, "device_auth_failed");
            AuthResult::Failed
        }
    }
}

async fn run_device_auth<R: Runtime>(
    app: &tauri::AppHandle<R>,
    reason: &'static str,
) -> AuthResult {
    let handle = app.clone();
    let result =
        tauri::async_runtime::spawn_blocking(move || handle.local_auth().authenticate(reason))
            .await
            .map_err(|error| Error::Platform(error.to_string()))
            .and_then(|result| result);
    auth_result(result)
}

pub const UNAVAILABLE_MESSAGE: &str = "aspc.auth_unavailable: device authentication is not available on this Mac, so the app stays locked. Set a login password (or Touch ID) in System Settings, then try again.";

pub(crate) async fn unlock<R: Runtime>(app: &tauri::AppHandle<R>) -> Result<LockSnapshot, String> {
    let state = aspc_state(app)?;
    let runtime = app.state::<LockRuntime>();
    let _prompt = runtime.unlock.lock().await;
    let Some(epoch) = state.begin_unlock() else {
        return Ok(state.lock_snapshot());
    };
    let result = run_device_auth(app, UNLOCK_REASON).await;
    match state.finish_unlock(epoch, result) {
        UnlockOutcome::Unavailable => Err(UNAVAILABLE_MESSAGE.to_string()),
        UnlockOutcome::Unlocked => {
            anlg_audit_log::record(
                app,
                anlg_audit_log::NewEntry::new(anlg_audit_log::actions::APP_UNLOCKED, "app", ""),
            );
            Ok(state.lock_snapshot())
        }
        _ => Ok(state.lock_snapshot()),
    }
}

pub(crate) async fn set_enabled<R: Runtime>(
    app: &tauri::AppHandle<R>,
    enabled: bool,
) -> Result<LockSnapshot, String> {
    let state = aspc_state(app)?;
    let runtime = app.state::<LockRuntime>();
    let _prompt = runtime.unlock.lock().await;
    let result = run_device_auth(app, CHANGE_SETTING_REASON).await;
    match result {
        AuthResult::Success => {}
        AuthResult::Failed => return Ok(state.lock_snapshot()),
        AuthResult::Unavailable => return Err(UNAVAILABLE_MESSAGE.to_string()),
    }
    let db = app
        .try_state::<anlg_db_core::Db>()
        .ok_or_else(|| "database is not available".to_string())?;
    crate::access::write_lock_setting(&db, enabled)
        .await
        .map_err(|error| error.to_string())?;
    state.set_lock_enabled(enabled, AuthResult::Success);
    anlg_audit_log::record(
        app,
        anlg_audit_log::NewEntry::new(
            anlg_audit_log::actions::SETTINGS_CHANGED,
            "settings",
            "app_settings",
        )
        .ids("keys", vec!["lock_app".into()])
        .flag("enabled", enabled),
    );
    Ok(state.lock_snapshot())
}

/// Emits [`AppLockChanged`] to every window on each change.
pub(crate) fn forward_lock_events<R: Runtime>(app: &tauri::AppHandle<R>) {
    let Ok(state) = aspc_state(app) else {
        return;
    };
    let handle = app.clone();
    state.on_lock_change(move |snapshot| {
        if let Err(error) = AppLockChanged::from(snapshot).emit(&handle) {
            tracing::warn!(%error, "app_lock_event_failed");
        }
    });
}

/// Hooks for the app to tell the lock when relocking must wait (a recording
/// is running).
pub trait AppLockExt<R: Runtime> {
    fn set_relock_busy_check<F, Fut>(&self, check: F)
    where
        F: Fn() -> Fut + Send + Sync + 'static,
        Fut: Future<Output = bool> + Send + 'static;

    /// Relock after [`RELOCK_GRACE`], waiting while the busy check says a
    /// recording is running, and only if `still_applies` is still true then
    /// (for a hidden window: it is still hidden). A newer request replaces
    /// an older one.
    fn request_relock(&self, delay: Duration, still_applies: impl Fn() -> bool + Send + 'static);
}

impl<R: Runtime> AppLockExt<R> for tauri::AppHandle<R> {
    fn set_relock_busy_check<F, Fut>(&self, check: F)
    where
        F: Fn() -> Fut + Send + Sync + 'static,
        Fut: Future<Output = bool> + Send + 'static,
    {
        let runtime = self.state::<LockRuntime>();
        let check: BusyCheck = Arc::new(move || Box::pin(check()));
        *runtime.busy.lock().unwrap_or_else(|e| e.into_inner()) = Some(check);
    }

    fn request_relock(&self, delay: Duration, still_applies: impl Fn() -> bool + Send + 'static) {
        let Ok(state) = aspc_state(self) else {
            return;
        };
        if !state.lock_snapshot().enabled {
            return;
        }
        let app = self.clone();
        let generation = self
            .state::<LockRuntime>()
            .relock_generation
            .fetch_add(1, Ordering::SeqCst)
            + 1;
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(delay).await;
            loop {
                let runtime = app.state::<LockRuntime>();
                if runtime.relock_generation.load(Ordering::SeqCst) != generation {
                    return;
                }
                if !state.is_unlocked() || !still_applies() {
                    return;
                }
                let busy = runtime
                    .busy
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .clone();
                let is_busy = match busy {
                    Some(check) => check().await,
                    None => false,
                };
                if !is_busy {
                    state.lock();
                    return;
                }
                tokio::time::sleep(BUSY_RETRY).await;
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_platform_results() {
        assert_eq!(auth_result(Ok(true)), AuthResult::Success);
        assert_eq!(auth_result(Ok(false)), AuthResult::Failed);
        assert_eq!(
            auth_result(Err(Error::Unavailable)),
            AuthResult::Unavailable
        );
        assert_eq!(
            auth_result(Err(Error::Platform("x".into()))),
            AuthResult::Failed
        );
    }
}
