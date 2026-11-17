//! Google and Microsoft accounts connected straight from the Mac.
//!
//! Sign-in follows RFC 8252 for native apps: the system browser opens the
//! provider's consent page, the provider redirects back to a one-shot server on
//! the loopback interface, and the code is exchanged with PKCE. Refresh tokens
//! are stored in the Keychain; access tokens stay in memory. No app server is
//! involved.

mod loopback;
mod manager;
mod pkce;
mod provider;
mod store;

pub use manager::{Account, AccountKind, AccountManager, replaced_account};
pub use provider::{ClientConfig, Endpoints, Provider};
pub use store::{KeychainStore, MemoryStore, SecretStore};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0} sign-in isn't set up in this build")]
    NotConfigured(&'static str),
    #[error("no connected account with id {0}")]
    UnknownAccount(String),
    #[error("access wasn't granted ({0})")]
    Denied(String),
    #[error("sign-in timed out")]
    TimedOut,
    #[error("sign-in was cancelled")]
    Cancelled,
    #[error("the account needs to be reconnected")]
    ReconnectRequired,
    #[error("calendar access wasn't granted")]
    MissingScope,
    #[error("the provider didn't return a refresh token")]
    NoRefreshToken,
    #[error("the provider didn't say who signed in")]
    NoIdentity,
    #[error("couldn't open the browser: {0}")]
    Browser(String),
    #[error("local sign-in server failed: {0}")]
    Loopback(String),
    #[error("keychain error: {0}")]
    Keychain(String),
    #[error("{0}")]
    Provider(String),
    #[error("network error: {0}")]
    Network(String),
}

impl Error {
    /// Stable code the UI maps to a message.
    pub fn code(&self) -> &'static str {
        match self {
            Error::NotConfigured(_) => "not_configured",
            Error::UnknownAccount(_) => "unknown_account",
            Error::Denied(_) => "denied",
            Error::MissingScope => "missing_scope",
            Error::TimedOut => "timed_out",
            Error::Cancelled => "cancelled",
            Error::ReconnectRequired => "reconnect_required",
            Error::NoRefreshToken | Error::NoIdentity | Error::Provider(_) => "provider",
            Error::Browser(_) => "browser",
            Error::Loopback(_) => "loopback",
            Error::Keychain(_) => "keychain",
            Error::Network(_) => "network",
        }
    }
}

impl From<reqwest::Error> for Error {
    fn from(error: reqwest::Error) -> Self {
        // reqwest's message can include the request URL, which never carries
        // secrets here (codes and tokens go in form bodies), but keep it short.
        Error::Network(error.without_url().to_string())
    }
}
