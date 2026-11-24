//! Local usage counter, quotas and the per-minute rate limit (C7.3).
//!
//! Stored as small JSON files in the app data directory (`ai-engine/`).
//! They hold counts only, never meeting content, so no schema change and no
//! Keychain. The webview can read them through `status` and change quotas
//! only through `set_quotas`, which validates every value in Rust.

use std::collections::{BTreeMap, VecDeque};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use chrono::{Datelike, NaiveDate};

use crate::error::{EngineError, Result, codes};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct UsageCounts {
    pub calls: u32,
    pub input_tokens: u32,
    pub output_tokens: u32,
}

impl UsageCounts {
    pub fn tokens(&self) -> u64 {
        u64::from(self.input_tokens) + u64::from(self.output_tokens)
    }

    fn add(&mut self, other: UsageCounts) {
        self.calls = self.calls.saturating_add(other.calls);
        self.input_tokens = self.input_tokens.saturating_add(other.input_tokens);
        self.output_tokens = self.output_tokens.saturating_add(other.output_tokens);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct Quotas {
    pub daily_calls: u32,
    pub daily_tokens: u32,
    pub monthly_calls: u32,
    pub monthly_tokens: u32,
    pub per_minute: u32,
}

/// Allowed ranges, enforced in Rust. `(min, max)`.
pub const DAILY_CALLS_RANGE: (u32, u32) = (1, 2_000);
pub const DAILY_TOKENS_RANGE: (u32, u32) = (10_000, 20_000_000);
pub const MONTHLY_CALLS_RANGE: (u32, u32) = (1, 50_000);
pub const MONTHLY_TOKENS_RANGE: (u32, u32) = (100_000, 400_000_000);
pub const PER_MINUTE_RANGE: (u32, u32) = (1, 60);

impl Default for Quotas {
    fn default() -> Self {
        Self {
            daily_calls: 200,
            daily_tokens: 2_000_000,
            monthly_calls: 3_000,
            monthly_tokens: 30_000_000,
            per_minute: 20,
        }
    }
}

impl Quotas {
    pub fn validate(&self) -> Result<()> {
        let checks = [
            ("daily_calls", self.daily_calls, DAILY_CALLS_RANGE),
            ("daily_tokens", self.daily_tokens, DAILY_TOKENS_RANGE),
            ("monthly_calls", self.monthly_calls, MONTHLY_CALLS_RANGE),
            ("monthly_tokens", self.monthly_tokens, MONTHLY_TOKENS_RANGE),
            ("per_minute", self.per_minute, PER_MINUTE_RANGE),
        ];
        for (name, value, (min, max)) in checks {
            if value < min || value > max {
                return Err(EngineError::new(
                    codes::INVALID_SETTING,
                    format!("{name} must be between {min} and {max}."),
                ));
            }
        }
        if self.daily_calls > self.monthly_calls || self.daily_tokens > self.monthly_tokens {
            return Err(EngineError::new(
                codes::INVALID_SETTING,
                "A daily limit can't be higher than the monthly limit.",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct UsageBook {
    /// `YYYY-MM-DD` → counts. Only the last 62 days are kept.
    #[serde(default)]
    pub days: BTreeMap<String, UsageCounts>,
    /// `YYYY-MM` → counts. Only the last 13 months are kept.
    #[serde(default)]
    pub months: BTreeMap<String, UsageCounts>,
}

pub fn day_key(date: NaiveDate) -> String {
    date.format("%Y-%m-%d").to_string()
}

pub fn month_key(date: NaiveDate) -> String {
    format!("{:04}-{:02}", date.year(), date.month())
}

impl UsageBook {
    pub fn day(&self, date: NaiveDate) -> UsageCounts {
        self.days.get(&day_key(date)).copied().unwrap_or_default()
    }

    pub fn month(&self, date: NaiveDate) -> UsageCounts {
        self.months
            .get(&month_key(date))
            .copied()
            .unwrap_or_default()
    }

    pub fn record(&mut self, date: NaiveDate, counts: UsageCounts) {
        self.days.entry(day_key(date)).or_default().add(counts);
        self.months.entry(month_key(date)).or_default().add(counts);
        while self.days.len() > 62 {
            let first = self.days.keys().next().cloned().unwrap();
            self.days.remove(&first);
        }
        while self.months.len() > 13 {
            let first = self.months.keys().next().cloned().unwrap();
            self.months.remove(&first);
        }
    }

    /// Fails with `ai.quota` when any daily or monthly cap is reached.
    pub fn check(&self, date: NaiveDate, quotas: &Quotas) -> Result<()> {
        let day = self.day(date);
        let month = self.month(date);
        let reason = if day.calls >= quotas.daily_calls {
            Some("today's AI call limit is reached")
        } else if day.tokens() >= u64::from(quotas.daily_tokens) {
            Some("today's AI token limit is reached")
        } else if month.calls >= quotas.monthly_calls {
            Some("this month's AI call limit is reached")
        } else if month.tokens() >= u64::from(quotas.monthly_tokens) {
            Some("this month's AI token limit is reached")
        } else {
            None
        };
        match reason {
            Some(reason) => Err(EngineError::new(codes::QUOTA, reason)),
            None => Ok(()),
        }
    }
}

/// Reads and writes the JSON files under `dir`.
pub struct UsageFiles {
    dir: Option<PathBuf>,
}

impl UsageFiles {
    pub fn new(dir: Option<PathBuf>) -> Self {
        Self { dir }
    }

    fn path(&self, name: &str) -> Option<PathBuf> {
        self.dir.as_ref().map(|dir| dir.join(name))
    }

    pub fn load_usage(&self) -> UsageBook {
        self.path("usage.json")
            .and_then(|path| std::fs::read_to_string(path).ok())
            .and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_default()
    }

    pub fn load_quotas(&self) -> Quotas {
        self.path("quotas.json")
            .and_then(|path| std::fs::read_to_string(path).ok())
            .and_then(|json| serde_json::from_str::<Quotas>(&json).ok())
            .filter(|quotas| quotas.validate().is_ok())
            .unwrap_or_default()
    }

    pub fn save_usage(&self, book: &UsageBook) -> Result<()> {
        self.write("usage.json", book)
    }

    pub fn save_quotas(&self, quotas: &Quotas) -> Result<()> {
        self.write("quotas.json", quotas)
    }

    fn write<T: serde::Serialize>(&self, name: &str, value: &T) -> Result<()> {
        let Some(path) = self.path(name) else {
            return Ok(());
        };
        let storage = |e: std::io::Error| EngineError::new(codes::STORAGE, e.to_string());
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(storage)?;
        }
        let json = serde_json::to_vec_pretty(value)
            .map_err(|e| EngineError::new(codes::STORAGE, e.to_string()))?;
        let tmp = tmp_path(&path);
        std::fs::write(&tmp, json).map_err(storage)?;
        std::fs::rename(&tmp, &path).map_err(storage)
    }
}

fn tmp_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".tmp");
    path.with_file_name(name)
}

/// Sliding one-minute window shared by every window of the app.
#[derive(Default)]
pub struct RateLimiter {
    starts: VecDeque<Instant>,
}

impl RateLimiter {
    pub fn try_acquire(&mut self, now: Instant, per_minute: u32) -> Result<()> {
        while self
            .starts
            .front()
            .is_some_and(|start| now.duration_since(*start) >= Duration::from_secs(60))
        {
            self.starts.pop_front();
        }
        if self.starts.len() >= per_minute as usize {
            return Err(EngineError::new(
                codes::RATE_LIMITED,
                "too many AI requests in the last minute",
            ));
        }
        self.starts.push_back(now);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    #[test]
    fn quota_ranges_are_enforced() {
        assert!(Quotas::default().validate().is_ok());
        let bad = [
            Quotas {
                daily_calls: 0,
                ..Quotas::default()
            },
            Quotas {
                daily_calls: 2_001,
                ..Quotas::default()
            },
            Quotas {
                per_minute: 61,
                ..Quotas::default()
            },
            Quotas {
                daily_tokens: 9_999,
                ..Quotas::default()
            },
            Quotas {
                monthly_tokens: 400_000_001,
                ..Quotas::default()
            },
            Quotas {
                daily_calls: 500,
                monthly_calls: 100,
                ..Quotas::default()
            },
        ];
        for quotas in bad {
            assert_eq!(
                quotas.validate().unwrap_err().code,
                codes::INVALID_SETTING,
                "{quotas:?}"
            );
        }
    }

    #[test]
    fn daily_and_monthly_caps_block_with_ai_quota() {
        let quotas = Quotas {
            daily_calls: 2,
            monthly_calls: 3,
            ..Quotas::default()
        };
        let mut book = UsageBook::default();
        let one = UsageCounts {
            calls: 1,
            input_tokens: 10,
            output_tokens: 5,
        };
        book.record(date("2026-11-15"), one);
        book.record(date("2026-11-16"), one);
        assert!(book.check(date("2026-11-16"), &quotas).is_ok());
        book.record(date("2026-11-16"), one);
        assert_eq!(
            book.check(date("2026-11-16"), &quotas).unwrap_err().code,
            codes::QUOTA
        );
        // New day, same month: the monthly cap (3) still blocks.
        assert_eq!(
            book.check(date("2026-11-17"), &quotas).unwrap_err().code,
            codes::QUOTA
        );
        // New month: clear.
        assert!(book.check(date("2026-12-01"), &quotas).is_ok());
        assert_eq!(book.month(date("2026-11-30")).input_tokens, 30);
    }

    #[test]
    fn rate_limiter_allows_n_per_minute() {
        let mut limiter = RateLimiter::default();
        let start = Instant::now();
        assert!(limiter.try_acquire(start, 2).is_ok());
        assert!(limiter.try_acquire(start, 2).is_ok());
        assert_eq!(
            limiter.try_acquire(start, 2).unwrap_err().code,
            codes::RATE_LIMITED
        );
        assert!(
            limiter
                .try_acquire(start + Duration::from_secs(61), 2)
                .is_ok()
        );
    }

    #[test]
    fn files_round_trip_and_bad_quotas_fall_back_to_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let files = UsageFiles::new(Some(dir.path().join("ai-engine")));
        let mut book = UsageBook::default();
        book.record(
            date("2026-11-16"),
            UsageCounts {
                calls: 1,
                input_tokens: 3,
                output_tokens: 4,
            },
        );
        files.save_usage(&book).unwrap();
        assert_eq!(files.load_usage(), book);
        std::fs::write(dir.path().join("ai-engine/quotas.json"), r#"{"daily_calls":0,"daily_tokens":1,"monthly_calls":1,"monthly_tokens":1,"per_minute":1}"#).unwrap();
        assert_eq!(files.load_quotas(), Quotas::default());
    }
}
