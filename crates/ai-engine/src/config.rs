//! The ONE place the Engine's endpoint, auth header and mode come from.
//!
//! Feature code never sees an endpoint, a header or a key: it hands the
//! Engine a Messages API body and gets the response back. Switching to the
//! hosted proxy later means implementing [`crate::transport::ProxyUpstream`]
//! and returning [`EngineMode::Proxy`] from [`resolve_engine_config`], plus
//! user sign-in to supply the token. No call site changes.

use url::Url;

/// The pinned model. Every Messages request is sent with this id; whatever
/// model the caller asked for is overwritten (C7.2). Not read from settings.
pub const HAIKU_MODEL: &str = "claude-haiku-5-5";

/// The Messages API version the Engine sends. Requests from the webview may
/// carry their own `anthropic-version`; it is ignored.
pub const ANTHROPIC_API_VERSION: &str = "2026-10-21";

/// DIRECT mode endpoint.
pub const DIRECT_ENDPOINT: &str = "https://api.anthropic.com";

/// Keychain item (service `<bundle id>.ai-engine`) holding the DIRECT mode
/// key. The key is written by `set_api_key` and read only by the Engine.
pub const KEYCHAIN_SERVICE_SUFFIX: &str = "ai-engine";
pub const KEYCHAIN_KEY_ITEM: &str = "anthropic-api-key";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EngineMode {
    /// Calls Anthropic directly with a key from this Mac's Keychain.
    Direct,
    /// Calls a hosted proxy with the signed-in user's token. Stub only.
    Proxy,
}

/// How the auth header is produced.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EngineAuth {
    /// `x-api-key: <key>`, the key read from the Keychain on every request.
    KeychainApiKey {
        header: &'static str,
        item: &'static str,
    },
    /// `authorization: Bearer <token>` with the signed-in user's token.
    /// Not wired: there is no sign-in for the proxy yet.
    UserToken {
        header: &'static str,
        token: Option<String>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EngineConfig {
    pub mode: EngineMode,
    pub endpoint: Url,
    pub auth: EngineAuth,
}

impl EngineConfig {
    pub fn direct() -> Self {
        Self::direct_at(Url::parse(DIRECT_ENDPOINT).expect("static endpoint"))
    }

    /// Crate-private so only tests can point DIRECT mode at a stub server.
    pub(crate) fn direct_at(endpoint: Url) -> Self {
        Self {
            mode: EngineMode::Direct,
            endpoint,
            auth: EngineAuth::KeychainApiKey {
                header: "x-api-key",
                item: KEYCHAIN_KEY_ITEM,
            },
        }
    }

    /// PROXY mode config: endpoint and user token are config fields only.
    pub fn proxy(endpoint: Url, user_token: Option<String>) -> Self {
        Self {
            mode: EngineMode::Proxy,
            endpoint,
            auth: EngineAuth::UserToken {
                header: "authorization",
                token: user_token,
            },
        }
    }

    pub fn endpoint_host(&self) -> String {
        self.endpoint.host_str().unwrap_or_default().to_string()
    }
}

/// Resolves the Engine config. This is the only function that decides the
/// mode, endpoint and auth. Today it always returns DIRECT.
///
/// Pre-publish gate (ASPC_BASE C7.6): before a build is given to other users
/// running on the developer's key, this must return PROXY in that build and
/// DIRECT with a developer key must be impossible.
pub fn resolve_engine_config() -> EngineConfig {
    EngineConfig::direct()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolved_config_is_direct_to_anthropic_with_a_keychain_header() {
        let config = resolve_engine_config();
        assert_eq!(config.mode, EngineMode::Direct);
        assert_eq!(config.endpoint.as_str(), "https://api.anthropic.com/");
        assert_eq!(
            config.auth,
            EngineAuth::KeychainApiKey {
                header: "x-api-key",
                item: KEYCHAIN_KEY_ITEM
            }
        );
    }
}
