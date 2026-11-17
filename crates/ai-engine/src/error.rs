//! Typed Engine errors. The `code` is stable; the UI maps the pause codes to
//! a calm "AI paused: …" message. Messages never contain keys or meeting
//! content (C3.4, C4.1).

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize, thiserror::Error)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[error("{code}: {message}")]
pub struct EngineError {
    pub code: String,
    pub message: String,
}

pub mod codes {
    /// Network error, provider 5xx or overloaded.
    pub const UNAVAILABLE: &str = "ai.unavailable";
    /// A local daily or monthly quota is used up.
    pub const QUOTA: &str = "ai.quota";
    /// No key in the Keychain (DIRECT mode).
    pub const NO_KEY: &str = "ai.no_key";
    /// Provider 429, or the local per-minute limit.
    pub const RATE_LIMITED: &str = "ai.rate_limited";
    /// Provider rejected the key (401/403).
    pub const INVALID_KEY: &str = "ai.invalid_key";
    /// Provider rejected the request (other 4xx).
    pub const BAD_REQUEST: &str = "ai.bad_request";
    /// The request body could not be parsed or is too large.
    pub const INVALID_REQUEST: &str = "ai.invalid_request";
    /// A path other than the Messages endpoints.
    pub const FORBIDDEN_PATH: &str = "ai.forbidden_path";
    /// PROXY mode is a stub.
    pub const NOT_IMPLEMENTED: &str = "ai.not_implemented";
    /// The caller cancelled.
    pub const CANCELLED: &str = "ai.cancelled";
    /// A settings value outside its allowed range, or a malformed key.
    pub const INVALID_SETTING: &str = "ai.invalid_setting";
    /// Local storage (Keychain, usage file) failed.
    pub const STORAGE: &str = "ai.storage";
}

/// Codes that mean "AI is paused"; the rest of the app keeps working.
pub const PAUSE_CODES: &[&str] = &[
    codes::UNAVAILABLE,
    codes::QUOTA,
    codes::NO_KEY,
    codes::RATE_LIMITED,
    codes::INVALID_KEY,
];

impl EngineError {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.to_string(),
            message: message.into(),
        }
    }

    pub fn is_pause(&self) -> bool {
        PAUSE_CODES.contains(&self.code.as_str())
    }
}

pub type Result<T> = std::result::Result<T, EngineError>;
