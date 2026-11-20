//! Device authentication (Touch ID or the login password) and the system
//! idle time, for code that runs outside the desktop app: the CLI and its MCP
//! server (ASPC A2.7, A4.4). The desktop app runs its own prompt on the main
//! thread through `tauri-plugin-local-auth`.

use std::time::Duration;

#[cfg(target_os = "macos")]
mod macos;

/// How long a prompt may stay open before it counts as not confirmed.
pub const PROMPT_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Confirmed,
    /// Cancelled, failed, timed out, or no window server to show it on.
    NotConfirmed,
    /// No Touch ID and no login password, or an unsupported platform.
    Unavailable,
}

/// Shows the system prompt "… is trying to `reason`" and blocks until the
/// user answers or [`PROMPT_TIMEOUT`] passes.
pub fn authenticate(reason: &str) -> Outcome {
    let reason = reason.trim();
    if reason.is_empty() {
        return Outcome::NotConfirmed;
    }
    #[cfg(target_os = "macos")]
    {
        macos::authenticate(reason)
    }
    #[cfg(not(target_os = "macos"))]
    {
        Outcome::Unavailable
    }
}

/// Seconds since the last keyboard, mouse or trackpad input anywhere in the
/// login session (not only in this process), or `None` where the platform
/// can't say.
pub fn system_idle_seconds() -> Option<f64> {
    #[cfg(target_os = "macos")]
    {
        macos::system_idle_seconds()
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

/// True when an idle relock set to `minutes` (0 is off) should fire after
/// `idle_seconds` without input. An unknown idle time never fires.
pub fn idle_threshold_reached(minutes: u32, idle_seconds: Option<f64>) -> bool {
    minutes > 0 && idle_seconds.is_some_and(|idle| idle >= f64::from(minutes) * 60.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_threshold() {
        assert!(!idle_threshold_reached(0, Some(1e9)), "0 is off");
        assert!(!idle_threshold_reached(5, None), "unknown idle never locks");
        assert!(!idle_threshold_reached(5, Some(299.0)));
        assert!(idle_threshold_reached(5, Some(300.0)));
        assert!(idle_threshold_reached(1, Some(61.5)));
    }

    #[test]
    fn empty_reason_is_refused_without_a_prompt() {
        assert_eq!(authenticate("  "), Outcome::NotConfirmed);
    }
}
