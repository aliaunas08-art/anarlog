//! The app lock state machine (A4.2–A4.4). Pure, so every path is unit
//! tested; the desktop app holds one instance in [`crate::AspcState`].
//!
//! Rules:
//! - With the lock off, the app is always unlocked.
//! - With the lock on, the app starts locked.
//! - Only a successful device authentication that Rust itself ran can
//!   unlock ([`LockMachine::finish_unlock`] with [`AuthResult::Success`]);
//!   there is no API that unlocks from a boolean sent by the webview.
//! - A lock that happens while an authentication prompt is open wins: the
//!   prompt's later success does not unlock (epoch check).
//! - If device authentication is unavailable, the app stays locked (A4.3).

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuthResult {
    Success,
    Failed,
    Unavailable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnlockOutcome {
    Unlocked,
    AlreadyUnlocked,
    /// Authentication failed or was cancelled.
    StillLocked,
    /// The app locked again while the prompt was open.
    Relocked,
    /// Device authentication is not available: the app stays locked.
    Unavailable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct LockSnapshot {
    pub enabled: bool,
    pub unlocked: bool,
}

#[derive(Debug)]
pub struct LockMachine {
    enabled: bool,
    unlocked: bool,
    epoch: u64,
}

impl LockMachine {
    pub fn new(enabled: bool) -> Self {
        Self {
            enabled,
            unlocked: !enabled,
            epoch: 0,
        }
    }

    pub fn snapshot(&self) -> LockSnapshot {
        LockSnapshot {
            enabled: self.enabled,
            unlocked: self.is_unlocked(),
        }
    }

    pub fn is_unlocked(&self) -> bool {
        !self.enabled || self.unlocked
    }

    /// Locks if the lock is on. Returns true when the state changed.
    pub fn lock(&mut self) -> bool {
        if !self.enabled {
            return false;
        }
        self.epoch += 1;
        let changed = self.unlocked;
        self.unlocked = false;
        changed
    }

    /// Call before showing the device-authentication prompt. `None` means
    /// there is nothing to unlock.
    pub fn begin_unlock(&self) -> Option<u64> {
        if self.is_unlocked() {
            None
        } else {
            Some(self.epoch)
        }
    }

    pub fn finish_unlock(&mut self, epoch: u64, result: AuthResult) -> UnlockOutcome {
        if self.is_unlocked() {
            return UnlockOutcome::AlreadyUnlocked;
        }
        match result {
            AuthResult::Unavailable => UnlockOutcome::Unavailable,
            AuthResult::Failed => UnlockOutcome::StillLocked,
            AuthResult::Success if epoch != self.epoch => UnlockOutcome::Relocked,
            AuthResult::Success => {
                self.unlocked = true;
                UnlockOutcome::Unlocked
            }
        }
    }

    /// Turns the lock on or off after a device authentication Rust ran.
    /// Without a successful authentication nothing changes. Turning it on
    /// keeps the current session unlocked: the user just authenticated.
    pub fn set_enabled(&mut self, enabled: bool, auth: AuthResult) -> bool {
        if auth != AuthResult::Success || !self.is_unlocked() {
            return false;
        }
        self.enabled = enabled;
        self.unlocked = true;
        true
    }

    /// Updates the setting from storage (startup, or an external change)
    /// without authentication. It can only make the app *more* locked:
    /// turning the lock on locks; turning it off is ignored while locked.
    pub fn apply_stored_setting(&mut self, enabled: bool) -> bool {
        match (self.enabled, enabled) {
            (false, true) => {
                self.enabled = true;
                self.epoch += 1;
                self.unlocked = false;
                true
            }
            (true, false) if self.is_unlocked() => {
                self.enabled = false;
                true
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lock_off_is_always_unlocked() {
        let mut m = LockMachine::new(false);
        assert!(m.is_unlocked());
        assert!(!m.lock());
        assert!(m.is_unlocked());
        assert_eq!(m.begin_unlock(), None);
    }

    #[test]
    fn lock_on_starts_locked_and_unlocks_only_on_success() {
        let mut m = LockMachine::new(true);
        assert!(!m.is_unlocked());
        let epoch = m.begin_unlock().unwrap();
        assert_eq!(
            m.finish_unlock(epoch, AuthResult::Failed),
            UnlockOutcome::StillLocked
        );
        assert!(!m.is_unlocked());
        assert_eq!(
            m.finish_unlock(epoch, AuthResult::Success),
            UnlockOutcome::Unlocked
        );
        assert!(m.is_unlocked());
        assert_eq!(
            m.finish_unlock(epoch, AuthResult::Success),
            UnlockOutcome::AlreadyUnlocked
        );
    }

    #[test]
    fn unavailable_auth_stays_locked() {
        let mut m = LockMachine::new(true);
        let epoch = m.begin_unlock().unwrap();
        assert_eq!(
            m.finish_unlock(epoch, AuthResult::Unavailable),
            UnlockOutcome::Unavailable
        );
        assert!(!m.is_unlocked());
    }

    #[test]
    fn lock_during_prompt_wins() {
        let mut m = LockMachine::new(true);
        let epoch = m.begin_unlock().unwrap();
        m.lock();
        assert_eq!(
            m.finish_unlock(epoch, AuthResult::Success),
            UnlockOutcome::Relocked
        );
        assert!(!m.is_unlocked());
        let epoch = m.begin_unlock().unwrap();
        assert_eq!(
            m.finish_unlock(epoch, AuthResult::Success),
            UnlockOutcome::Unlocked
        );
    }

    #[test]
    fn relock_after_unlock() {
        let mut m = LockMachine::new(true);
        let epoch = m.begin_unlock().unwrap();
        m.finish_unlock(epoch, AuthResult::Success);
        assert!(m.lock());
        assert!(!m.is_unlocked());
        assert!(!m.lock());
    }

    #[test]
    fn changing_the_setting_needs_auth() {
        let mut m = LockMachine::new(false);
        assert!(!m.set_enabled(true, AuthResult::Failed));
        assert!(!m.set_enabled(true, AuthResult::Unavailable));
        assert!(!m.snapshot().enabled);
        assert!(m.set_enabled(true, AuthResult::Success));
        assert_eq!(
            m.snapshot(),
            LockSnapshot {
                enabled: true,
                unlocked: true
            }
        );
        m.lock();
        // Locked: cannot turn the lock off, even with a successful auth,
        // without unlocking first.
        assert!(!m.set_enabled(false, AuthResult::Success));
        let epoch = m.begin_unlock().unwrap();
        m.finish_unlock(epoch, AuthResult::Success);
        assert!(m.set_enabled(false, AuthResult::Success));
        assert_eq!(
            m.snapshot(),
            LockSnapshot {
                enabled: false,
                unlocked: true
            }
        );
    }

    #[test]
    fn stored_setting_can_only_tighten_while_locked() {
        let mut m = LockMachine::new(true);
        assert!(!m.apply_stored_setting(false));
        assert!(!m.is_unlocked());

        let mut m = LockMachine::new(false);
        assert!(m.apply_stored_setting(true));
        assert!(!m.is_unlocked());

        let mut m = LockMachine::new(true);
        let epoch = m.begin_unlock().unwrap();
        m.finish_unlock(epoch, AuthResult::Success);
        assert!(m.apply_stored_setting(false));
        assert_eq!(
            m.snapshot(),
            LockSnapshot {
                enabled: false,
                unlocked: true
            }
        );
    }
}
