//! The app lock for the CLI and MCP server (ASPC A2.7, A4.2, A4.4).
//!
//! The desktop app holds its lock state in its own process, so the CLI
//! can't share it. Instead, when the `lock_app` setting is on, the CLI runs
//! the same device authentication itself (Touch ID or the login password)
//! before it reads or writes any data, and fails closed if it can't. The MCP
//! server asks on its first data request and again after the idle relock
//! time (`lock_idle_minutes`) passes with no keyboard or mouse input.
//!
//! This guards against someone using an unlocked Mac's terminal or an AI
//! tool to read notes while the app is locked. It is not a barrier against
//! code running as the user, which can read `app.db` directly (A4.1, C2.3).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use anlg_db_app::app_lock_settings;
use anlg_device_auth::Outcome;

use crate::{Error, Result};

/// The prompt reads "anarlog is trying to <reason>".
pub(crate) const CLI_REASON: &str = "read your Timbre notes from the command line";
pub(crate) const MCP_REASON: &str = "let an AI tool read your Timbre notes";
const NOT_CONFIRMED: &str = "Touch ID or your password wasn't confirmed. Run the command again and confirm the macOS prompt.";
const UNAVAILABLE: &str = "device authentication is not available here (no Touch ID or login password, or no screen to show the prompt on), so the CLI stays locked.";
/// How often the MCP server checks the idle time.
const IDLE_CHECK: Duration = Duration::from_secs(30);

/// Runs device authentication; replaced in tests.
pub(crate) type Authenticator = Arc<dyn Fn(&'static str) -> Outcome + Send + Sync>;
/// Seconds since the last input; replaced in tests.
pub(crate) type IdleSource = Arc<dyn Fn() -> Option<f64> + Send + Sync>;

pub(crate) fn system_authenticator() -> Authenticator {
    Arc::new(anlg_device_auth::authenticate)
}

pub(crate) fn system_idle() -> IdleSource {
    Arc::new(anlg_device_auth::system_idle_seconds)
}

/// The lock setting, read like the desktop app does at startup: if it can't
/// be read (no schema yet), the lock counts as off; the database is then
/// unreadable to the CLI too.
async fn lock_enabled(db: &anlg_db_core::Db) -> bool {
    app_lock_settings::read_lock_setting(db.pool())
        .await
        .unwrap_or(false)
}

async fn run_prompt(authenticate: &Authenticator, reason: &'static str) -> Result<()> {
    let authenticate = Arc::clone(authenticate);
    let outcome = tokio::task::spawn_blocking(move || authenticate(reason))
        .await
        .unwrap_or(Outcome::NotConfirmed);
    match outcome {
        Outcome::Confirmed => Ok(()),
        Outcome::NotConfirmed => Err(Error::Locked(NOT_CONFIRMED)),
        Outcome::Unavailable => Err(Error::Locked(UNAVAILABLE)),
    }
}

/// For one CLI command: if the lock is on, authenticate before going on.
pub(crate) async fn require_unlocked(
    db: &anlg_db_core::Db,
    authenticate: &Authenticator,
) -> Result<bool> {
    if !lock_enabled(db).await {
        return Ok(false);
    }
    run_prompt(authenticate, CLI_REASON).await?;
    Ok(true)
}

/// The MCP server's lock: unlocked by a device authentication, relocked
/// after the idle time.
pub(crate) struct McpLock {
    db: Arc<anlg_db_core::Db>,
    authenticate: Authenticator,
    idle: IdleSource,
    unlocked: AtomicBool,
    prompt: tokio::sync::Mutex<()>,
}

impl McpLock {
    pub(crate) fn new(
        db: Arc<anlg_db_core::Db>,
        authenticate: Authenticator,
        idle: IdleSource,
    ) -> Arc<Self> {
        Arc::new(Self {
            db,
            authenticate,
            idle,
            unlocked: AtomicBool::new(false),
            prompt: tokio::sync::Mutex::new(()),
        })
    }

    /// Relocks if the user has been away for the idle relock time. Called
    /// on a timer and before each request.
    pub(crate) async fn check_idle(&self) {
        if !self.unlocked.load(Ordering::SeqCst) {
            return;
        }
        let minutes = app_lock_settings::read_idle_setting(self.db.pool())
            .await
            .unwrap_or(app_lock_settings::DEFAULT_IDLE_MINUTES);
        if anlg_device_auth::idle_threshold_reached(minutes, (self.idle)()) {
            self.unlocked.store(false, Ordering::SeqCst);
        }
    }

    pub(crate) fn start_idle_watch(self: &Arc<Self>) {
        let lock = Arc::downgrade(self);
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(IDLE_CHECK);
            loop {
                ticker.tick().await;
                let Some(lock) = lock.upgrade() else {
                    return;
                };
                lock.check_idle().await;
            }
        });
    }

    /// Before every data request. Returns true when this call ran a prompt
    /// that succeeded (the caller audit-logs the unlock).
    pub(crate) async fn ensure_unlocked(&self) -> Result<bool> {
        // The setting is read each time, so turning the lock on in the app
        // takes effect without restarting the MCP server.
        if !lock_enabled(&self.db).await {
            return Ok(false);
        }
        self.check_idle().await;
        let _prompt = self.prompt.lock().await;
        if self.unlocked.load(Ordering::SeqCst) {
            return Ok(false);
        }
        run_prompt(&self.authenticate, MCP_REASON).await?;
        self.unlocked.store(true, Ordering::SeqCst);
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use std::sync::atomic::AtomicUsize;

    async fn db_with_lock(lock: Option<bool>, idle_minutes: Option<u32>) -> Arc<anlg_db_core::Db> {
        let db = anlg_db_core::Db::connect_memory_plain().await.unwrap();
        sqlx::raw_sql("CREATE TABLE app_settings (id TEXT PRIMARY KEY NOT NULL, value_json TEXT NOT NULL, updated_at TEXT)")
            .execute(db.pool())
            .await
            .unwrap();
        if let Some(lock) = lock {
            app_lock_settings::write_lock_setting(db.pool(), lock)
                .await
                .unwrap();
        }
        if let Some(minutes) = idle_minutes {
            app_lock_settings::write_idle_setting(db.pool(), minutes)
                .await
                .unwrap();
        }
        Arc::new(db)
    }

    fn counting(outcome: Outcome) -> (Authenticator, Arc<AtomicUsize>) {
        let calls = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&calls);
        let auth: Authenticator = Arc::new(move |_| {
            counter.fetch_add(1, Ordering::SeqCst);
            outcome
        });
        (auth, calls)
    }

    #[tokio::test]
    async fn cli_skips_the_prompt_when_the_lock_is_off() {
        for setting in [None, Some(false)] {
            let db = db_with_lock(setting, None).await;
            let (auth, calls) = counting(Outcome::NotConfirmed);
            assert!(!require_unlocked(&db, &auth).await.unwrap());
            assert_eq!(calls.load(Ordering::SeqCst), 0);
        }
        // No schema at all: treated like the app does, as off.
        let bare = anlg_db_core::Db::connect_memory_plain().await.unwrap();
        let (auth, _) = counting(Outcome::NotConfirmed);
        assert!(!require_unlocked(&bare, &auth).await.unwrap());
    }

    #[tokio::test]
    async fn cli_fails_closed_when_the_lock_is_on() {
        let db = db_with_lock(Some(true), None).await;
        for outcome in [Outcome::NotConfirmed, Outcome::Unavailable] {
            let (auth, calls) = counting(outcome);
            let error = require_unlocked(&db, &auth).await.unwrap_err();
            assert_eq!(error.code(), "locked");
            assert_eq!(error.exit_code(), 5);
            assert_eq!(calls.load(Ordering::SeqCst), 1);
        }
        let (auth, _) = counting(Outcome::Confirmed);
        assert!(require_unlocked(&db, &auth).await.unwrap());
    }

    #[tokio::test]
    async fn mcp_asks_once_then_again_after_the_idle_time() {
        let db = db_with_lock(Some(true), Some(5)).await;
        let (auth, calls) = counting(Outcome::Confirmed);
        let idle_seconds = Arc::new(Mutex::new(Some(0.0)));
        let source = Arc::clone(&idle_seconds);
        let idle: IdleSource = Arc::new(move || *source.lock().unwrap());
        let lock = McpLock::new(db, auth, idle);

        assert!(lock.ensure_unlocked().await.unwrap());
        assert!(!lock.ensure_unlocked().await.unwrap());
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        *idle_seconds.lock().unwrap() = Some(299.0);
        assert!(!lock.ensure_unlocked().await.unwrap());
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        *idle_seconds.lock().unwrap() = Some(300.0);
        lock.check_idle().await;
        *idle_seconds.lock().unwrap() = Some(1.0);
        assert!(lock.ensure_unlocked().await.unwrap());
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn mcp_stays_locked_when_the_prompt_is_refused() {
        let db = db_with_lock(Some(true), None).await;
        let (auth, calls) = counting(Outcome::NotConfirmed);
        let lock = McpLock::new(db, auth, Arc::new(|| Some(0.0)));
        assert_eq!(lock.ensure_unlocked().await.unwrap_err().code(), "locked");
        assert_eq!(lock.ensure_unlocked().await.unwrap_err().code(), "locked");
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn mcp_without_the_lock_never_prompts() {
        let db = db_with_lock(Some(false), None).await;
        let (auth, calls) = counting(Outcome::NotConfirmed);
        let lock = McpLock::new(db, auth, Arc::new(|| Some(1e9)));
        assert!(!lock.ensure_unlocked().await.unwrap());
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }
}
