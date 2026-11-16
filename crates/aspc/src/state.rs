use std::sync::{Mutex, RwLock};

use crate::lock::{AuthResult, LockMachine, LockSnapshot, UnlockOutcome};
use crate::{AccessContext, Role};

type LockListener = Box<dyn Fn(LockSnapshot) + Send + Sync>;
type IdentityListener = Box<dyn Fn(&AccessContext) + Send + Sync>;

/// The process-wide access state: who the local actor is and the lock.
/// The desktop app manages one `Arc<AspcState>` as Tauri state.
pub struct AspcState {
    base: RwLock<AccessContext>,
    lock: Mutex<LockMachine>,
    listeners: Mutex<Vec<LockListener>>,
    identity_listeners: Mutex<Vec<IdentityListener>>,
}

impl AspcState {
    /// `base.window` and `base.unlocked` are ignored: the window comes from
    /// each call and `unlocked` from the lock.
    pub fn new(mut base: AccessContext, lock_enabled: bool) -> Self {
        base.window = None;
        base.unlocked = false;
        Self {
            base: RwLock::new(base),
            lock: Mutex::new(LockMachine::new(lock_enabled)),
            listeners: Mutex::new(Vec::new()),
            identity_listeners: Mutex::new(Vec::new()),
        }
    }

    /// The local Owner of a workspace (A1.3).
    pub fn local_owner(
        actor_id: String,
        workspace_id: String,
        device_id: String,
        lock_enabled: bool,
    ) -> Self {
        Self::new(
            AccessContext {
                actor_id,
                role: Some(Role::Owner),
                workspace_id,
                device_id,
                window: None,
                unlocked: false,
            },
            lock_enabled,
        )
    }

    fn machine(&self) -> std::sync::MutexGuard<'_, LockMachine> {
        self.lock.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The context for a call from `window` (or `None` for window-less checks).
    pub fn context(&self, window: Option<&str>) -> AccessContext {
        let mut ctx = self.base.read().unwrap_or_else(|e| e.into_inner()).clone();
        ctx.window = window.map(str::to_owned);
        ctx.unlocked = self.machine().is_unlocked();
        ctx
    }

    /// The local workspace scope (C1.2), or `None` (fail closed) while the
    /// context has no workspace.
    pub fn scope(&self) -> Option<crate::WorkspaceScope> {
        crate::WorkspaceScope::from_context(&self.context(None))
    }

    /// Replaces the actor and workspace (C1). The desktop app calls this once
    /// the schema is ready and the local identity is known; until then a
    /// fresh install has an empty workspace and every check denies.
    /// Listeners run only when something changed.
    pub fn set_identity(&self, actor_id: String, workspace_id: String) {
        let changed = {
            let mut base = self.base.write().unwrap_or_else(|e| e.into_inner());
            let changed = base.actor_id != actor_id || base.workspace_id != workspace_id;
            base.actor_id = actor_id;
            base.workspace_id = workspace_id;
            changed
        };
        if changed {
            let ctx = self.context(None);
            for listener in self
                .identity_listeners
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .iter()
            {
                listener(&ctx);
            }
        }
    }

    /// Registers a callback run after every identity change.
    pub fn on_identity_change(&self, listener: impl Fn(&AccessContext) + Send + Sync + 'static) {
        self.identity_listeners
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(Box::new(listener));
    }

    pub fn lock_snapshot(&self) -> LockSnapshot {
        self.machine().snapshot()
    }

    pub fn is_unlocked(&self) -> bool {
        self.machine().is_unlocked()
    }

    /// Registers a callback run after every lock state change, and once
    /// immediately with the current state.
    pub fn on_lock_change(&self, listener: impl Fn(LockSnapshot) + Send + Sync + 'static) {
        listener(self.lock_snapshot());
        self.listeners
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(Box::new(listener));
    }

    fn notify(&self, snapshot: LockSnapshot) {
        for listener in self
            .listeners
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
        {
            listener(snapshot);
        }
    }

    fn mutate<T>(&self, f: impl FnOnce(&mut LockMachine) -> T) -> T {
        let (result, before, after) = {
            let mut machine = self.machine();
            let before = machine.snapshot();
            let result = f(&mut machine);
            (result, before, machine.snapshot())
        };
        if before != after {
            self.notify(after);
        }
        result
    }

    pub fn lock(&self) -> bool {
        self.mutate(LockMachine::lock)
    }

    pub fn begin_unlock(&self) -> Option<u64> {
        self.machine().begin_unlock()
    }

    pub fn finish_unlock(&self, epoch: u64, result: AuthResult) -> UnlockOutcome {
        self.mutate(|m| m.finish_unlock(epoch, result))
    }

    pub fn set_lock_enabled(&self, enabled: bool, auth: AuthResult) -> bool {
        self.mutate(|m| m.set_enabled(enabled, auth))
    }

    pub fn apply_stored_lock_setting(&self, enabled: bool) -> bool {
        self.mutate(|m| m.apply_stored_setting(enabled))
    }
}

/// What `require` does, without Tauri: fails closed with no state.
#[cfg_attr(not(feature = "tauri"), allow(dead_code))]
pub(crate) fn check(
    state: Option<&AspcState>,
    window: &str,
    data: crate::DataType,
    action: crate::Action,
    check_lock: bool,
) -> Result<crate::Grant, crate::Denied> {
    let Some(state) = state else {
        return Err(crate::Denied::new(crate::DenyCode::NoContext, data, action));
    };
    let mut ctx = state.context(Some(window));
    if !check_lock {
        ctx.unlocked = true;
    }
    crate::authorize(&ctx, &ctx.workspace_id, data, action)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    use super::*;
    use crate::{Action, DataType, DenyCode, authorize};

    #[test]
    fn listeners_follow_the_lock_and_context_reflects_it() {
        let state = AspcState::local_owner("a".into(), "w".into(), "d".into(), true);
        let locked = Arc::new(AtomicBool::new(false));
        let seen = Arc::clone(&locked);
        state.on_lock_change(move |s| seen.store(!s.unlocked, Ordering::SeqCst));
        assert!(locked.load(Ordering::SeqCst));

        let ctx = state.context(Some("main"));
        assert_eq!(
            authorize(&ctx, "w", DataType::Notes, Action::Read)
                .unwrap_err()
                .code,
            DenyCode::Locked
        );

        let epoch = state.begin_unlock().unwrap();
        assert_eq!(
            state.finish_unlock(epoch, AuthResult::Success),
            UnlockOutcome::Unlocked
        );
        assert!(!locked.load(Ordering::SeqCst));
        assert!(
            authorize(
                &state.context(Some("main")),
                "w",
                DataType::Notes,
                Action::Read
            )
            .is_ok()
        );

        assert!(state.lock());
        assert!(locked.load(Ordering::SeqCst));
    }

    /// A2.3: the guard denies a locked context, a Viewer and a wrong window,
    /// for every guarded (data, action) in the registry.
    #[test]
    fn guard_denies_locked_viewer_and_wrong_window() {
        use crate::commands::{Access, COMMANDS};

        let locked = AspcState::local_owner("a".into(), "w".into(), "d".into(), true);
        let mut viewer_ctx = locked.context(None);
        viewer_ctx.role = Some(Role::Viewer);
        let viewer = AspcState::new(viewer_ctx, false);
        let owner = AspcState::local_owner("a".into(), "w".into(), "d".into(), false);

        let mut guarded = 0;
        for entry in COMMANDS {
            let Access::Data(data, action) = entry.access else {
                continue;
            };
            guarded += 1;
            let name = format!("{}:{}", entry.plugin, entry.name);

            assert_eq!(
                check(Some(&locked), "main", data, action, true)
                    .unwrap_err()
                    .code,
                DenyCode::Locked,
                "{name} while locked"
            );
            let viewer_result = check(Some(&viewer), "main", data, action, true);
            if action == Action::Read && data.is_content() {
                assert!(viewer_result.is_ok(), "{name} viewer read");
            } else {
                assert_eq!(
                    viewer_result.unwrap_err().code,
                    DenyCode::RoleDenied,
                    "{name} viewer"
                );
            }
            assert_eq!(
                check(Some(&owner), "dictation-overlay", data, action, true)
                    .unwrap_err()
                    .code,
                DenyCode::WindowDenied,
                "{name} wrong window"
            );
            assert!(
                check(Some(&owner), "main", data, action, true).is_ok(),
                "{name} owner in main"
            );
            assert_eq!(
                check(None, "main", data, action, true).unwrap_err().code,
                DenyCode::NoContext
            );
        }
        assert!(guarded > 100);

        // The composer never reaches Keys.
        for action in Action::ALL {
            assert_eq!(
                check(Some(&owner), "composer", DataType::Keys, action, true)
                    .unwrap_err()
                    .code,
                DenyCode::WindowDenied
            );
        }
        // The SQL commands skip only the lock check.
        assert!(check(Some(&locked), "main", DataType::Notes, Action::Read, false).is_ok());
    }

    #[test]
    fn identity_change_updates_context_scope_and_listeners() {
        let state = AspcState::local_owner("a".into(), String::new(), "d".into(), false);
        // Fresh install before the schema is ready: no workspace, fail closed.
        assert!(state.scope().is_none());
        assert_eq!(
            check(Some(&state), "main", DataType::Notes, Action::Read, true)
                .unwrap_err()
                .code,
            DenyCode::WrongWorkspace
        );
        let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = Arc::clone(&seen);
        state.on_identity_change(move |ctx| sink.lock().unwrap().push(ctx.workspace_id.clone()));
        state.set_identity("owner".into(), "w1".into());
        state.set_identity("owner".into(), "w1".into());
        assert_eq!(*seen.lock().unwrap(), vec!["w1".to_string()]);
        assert_eq!(state.scope().unwrap().workspace_id(), "w1");
        assert_eq!(state.context(None).actor_id, "owner");
        assert!(check(Some(&state), "main", DataType::Notes, Action::Read, true).is_ok());
    }
}
