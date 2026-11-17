//! Rate limit for `permission.denied` entries, so a denial loop (a UI
//! retrying a query while the app is locked, a hostile script) cannot flood
//! the log.
//!
//! Per key (source, code, data type, action): the first denial in a window
//! is logged; later ones in the same window are only counted, and the count
//! is carried on the next logged entry for that key as `suppressed`. On top
//! of that, at most [`GLOBAL_PER_WINDOW`] denial entries are written per
//! window across all keys.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub const WINDOW: Duration = Duration::from_secs(60);
pub const GLOBAL_PER_WINDOW: u32 = 30;
const MAX_KEYS: usize = 512;

#[derive(Default)]
struct KeyState {
    window_start: Option<Instant>,
    suppressed: u64,
}

#[derive(Default)]
struct Inner {
    keys: HashMap<String, KeyState>,
    global_start: Option<Instant>,
    global_count: u32,
    global_suppressed: u64,
}

#[derive(Default)]
pub struct DenialLimiter {
    inner: Mutex<Inner>,
}

/// Whether to write an entry, and how many denials were dropped before it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Admit {
    Write { suppressed: u64 },
    Drop,
}

impl DenialLimiter {
    pub fn admit(&self, key: &str, now: Instant) -> Admit {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if inner
            .global_start
            .is_none_or(|start| now.duration_since(start) >= WINDOW)
        {
            inner.global_start = Some(now);
            inner.global_count = 0;
        }
        if inner.keys.len() > MAX_KEYS {
            inner.keys.clear();
        }
        let state = inner.keys.entry(key.to_string()).or_default();
        let fresh = state
            .window_start
            .is_none_or(|start| now.duration_since(start) >= WINDOW);
        if !fresh {
            state.suppressed += 1;
            return Admit::Drop;
        }
        let suppressed = state.suppressed;
        if inner.global_count >= GLOBAL_PER_WINDOW {
            let state = inner.keys.get_mut(key).expect("inserted above");
            state.suppressed += 1;
            inner.global_suppressed += 1;
            return Admit::Drop;
        }
        let state = inner.keys.get_mut(key).expect("inserted above");
        state.window_start = Some(now);
        state.suppressed = 0;
        inner.global_count += 1;
        Admit::Write { suppressed }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_denial_loop_writes_one_entry_per_window_and_counts_the_rest() {
        let limiter = DenialLimiter::default();
        let start = Instant::now();
        assert_eq!(
            limiter.admit("app:locked", start),
            Admit::Write { suppressed: 0 }
        );
        for i in 1..=1000 {
            assert_eq!(
                limiter.admit("app:locked", start + Duration::from_millis(i)),
                Admit::Drop
            );
        }
        assert_eq!(
            limiter.admit("app:locked", start + WINDOW),
            Admit::Write { suppressed: 1000 }
        );
    }

    #[test]
    fn many_distinct_keys_hit_the_global_cap() {
        let limiter = DenialLimiter::default();
        let now = Instant::now();
        let written = (0..500)
            .filter(|i| matches!(limiter.admit(&format!("k{i}"), now), Admit::Write { .. }))
            .count();
        assert_eq!(written, GLOBAL_PER_WINDOW as usize);
    }
}
