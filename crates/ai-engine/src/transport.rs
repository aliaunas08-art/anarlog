//! The two upstreams behind [`EngineConfig`]. Both take the same prepared
//! request; only how it is authenticated and where it goes differ.

use crate::config::{ANTHROPIC_API_VERSION, EngineAuth, EngineConfig};
use crate::error::{EngineError, Result, codes};
use crate::request::PreparedRequest;

/// One outbound call. `credential` is the key (DIRECT) or user token (PROXY).
pub trait Upstream {
    fn send(
        &self,
        request: &PreparedRequest,
        credential: &str,
    ) -> impl std::future::Future<Output = Result<reqwest::Response>> + Send;
}

/// DIRECT mode: `POST {endpoint}{path}` with the key in the configured header.
/// The key goes only to this host, only in a header, never in the URL (C3.3).
pub struct DirectUpstream {
    client: reqwest::Client,
    config: EngineConfig,
}

impl DirectUpstream {
    pub fn new(config: EngineConfig) -> Self {
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(std::time::Duration::from_secs(15))
            .build()
            .unwrap_or_default();
        Self { client, config }
    }
}

impl Upstream for DirectUpstream {
    async fn send(&self, request: &PreparedRequest, credential: &str) -> Result<reqwest::Response> {
        let EngineAuth::KeychainApiKey { header, .. } = &self.config.auth else {
            return Err(EngineError::new(
                codes::NOT_IMPLEMENTED,
                "DIRECT mode needs a Keychain key.",
            ));
        };
        let url = self
            .config
            .endpoint
            .join(request.path)
            .map_err(|_| EngineError::new(codes::INVALID_REQUEST, "bad endpoint"))?;
        let mut builder = self
            .client
            .post(url)
            .header(*header, credential)
            .header("anthropic-version", ANTHROPIC_API_VERSION)
            .header("content-type", "application/json")
            .json(&request.body);
        if !request.betas.is_empty() {
            builder = builder.header("anthropic-beta", request.betas.join(","));
        }
        builder.send().await.map_err(|error| {
            tracing::warn!(
                timeout = error.is_timeout(),
                connect = error.is_connect(),
                "ai_engine_network_error"
            );
            EngineError::new(codes::UNAVAILABLE, "the AI service can't be reached")
        })
    }
}

/// PROXY mode: a stub. It returns `ai.not_implemented` for every call.
///
/// To implement it (and nothing else needs to change):
/// 1. **Endpoint:** the hosted proxy's base URL, set in [`EngineConfig::proxy`]
///    by [`crate::config::resolve_engine_config`]. The proxy exposes the same
///    `/v1/messages` (and `/v1/messages/count_tokens`) paths and response
///    format as Anthropic, so the frontend client and usage parsing are
///    unchanged.
/// 2. **User sign-in token:** the signed-in user's token, sent as
///    `authorization: Bearer <token>` (the `UserToken` auth in the config).
///    It comes from the app's sign-in, not from a key on this Mac.
/// 3. **Server-side spend caps:** the proxy holds the developer key and
///    enforces per-user quotas, spend caps and billing alerts (C7.3
///    **[hosted]**); the local quotas stay as a first line.
pub struct ProxyUpstream {
    #[allow(dead_code)]
    config: EngineConfig,
}

impl ProxyUpstream {
    pub fn new(config: EngineConfig) -> Self {
        Self { config }
    }
}

impl Upstream for ProxyUpstream {
    async fn send(
        &self,
        _request: &PreparedRequest,
        _credential: &str,
    ) -> Result<reqwest::Response> {
        Err(EngineError::new(
            codes::NOT_IMPLEMENTED,
            "The hosted AI proxy isn't available in this build.",
        ))
    }
}
