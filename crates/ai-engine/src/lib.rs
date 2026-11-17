//! Efficiency Engine (ASPC C7): the single gateway every AI call goes
//! through.
//!
//! - One config location: [`config::resolve_engine_config`] (DIRECT today,
//!   PROXY as a stub).
//! - Model lock: every request is sent with [`config::HAIKU_MODEL`],
//!   whatever the caller asked for.
//! - Only the Messages endpoints are forwarded ([`request::ALLOWED_PATHS`]).
//! - Local usage counter, daily/monthly quotas and a per-minute limit
//!   ([`usage`]).
//! - Typed pause errors ([`error::codes`]) so AI features pause cleanly
//!   while recording, transcripts and notes keep working.
//! - The key lives in the Keychain and is read only here; it is never
//!   returned, logged or put in a URL.

pub mod config;
pub mod error;
pub mod redact;
pub mod request;
pub mod transport;
pub mod usage;

mod stream;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use anlg_local_accounts::SecretStore;
use chrono::NaiveDate;
use futures_util::StreamExt;
use tokio_util::sync::CancellationToken;

pub use config::{EngineConfig, EngineMode, HAIKU_MODEL, resolve_engine_config};
pub use error::{EngineError, codes};
pub use usage::{Quotas, UsageCounts};

use config::{EngineAuth, KEYCHAIN_KEY_ITEM};
use error::Result;
use request::PreparedRequest;
use stream::{UsageParser, Utf8Chunker};
use transport::{DirectUpstream, ProxyUpstream, Upstream};
use usage::{RateLimiter, UsageBook, UsageFiles};

/// Events streamed back to the caller for one request.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum EngineEvent {
    /// The provider answered 2xx. Sent once, before any chunk.
    #[serde(rename_all = "camelCase")]
    Head { status: u16, content_type: String },
    /// Response body text, in order.
    Chunk { data: String },
    /// The body was relayed completely. Not sent after an error.
    End,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct EngineRequest {
    /// Caller-chosen id, used by `cancel`.
    pub request_id: String,
    pub path: String,
    /// Messages API JSON body.
    pub body: String,
    /// `anthropic-beta` values the caller wants; filtered by the Engine.
    #[serde(default)]
    pub betas: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct PauseInfo {
    pub code: String,
    pub message: String,
    /// RFC 3339.
    pub at: String,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct EngineStatus {
    pub mode: EngineMode,
    pub model: String,
    pub endpoint_host: String,
    pub today: UsageCounts,
    pub month: UsageCounts,
    pub quotas: Quotas,
    /// The last pause reason, cleared by the next successful call.
    pub pause: Option<PauseInfo>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct KeyStatus {
    /// Whether a key is stored. The key itself is never returned.
    pub present: bool,
}

struct State {
    book: UsageBook,
    quotas: Quotas,
    limiter: RateLimiter,
    pause: Option<PauseInfo>,
}

type Clock = Box<dyn Fn() -> NaiveDate + Send + Sync>;

pub struct Engine {
    config: EngineConfig,
    direct: DirectUpstream,
    proxy: ProxyUpstream,
    keys: Arc<dyn SecretStore>,
    files: UsageFiles,
    state: Mutex<State>,
    cancels: Mutex<HashMap<String, CancellationToken>>,
    today: Clock,
}

const MAX_ERROR_BODY: usize = 64 * 1024;
const MAX_JSON_BODY: usize = 32 * 1024 * 1024;

impl Engine {
    /// `data_dir`: where `usage.json` and `quotas.json` live (None keeps
    /// them in memory only).
    pub fn new(
        config: EngineConfig,
        keys: Arc<dyn SecretStore>,
        data_dir: Option<std::path::PathBuf>,
    ) -> Self {
        let files = UsageFiles::new(data_dir);
        let state = State {
            book: files.load_usage(),
            quotas: files.load_quotas(),
            limiter: RateLimiter::default(),
            pause: None,
        };
        Self {
            direct: DirectUpstream::new(config.clone()),
            proxy: ProxyUpstream::new(config.clone()),
            config,
            keys,
            files,
            state: Mutex::new(state),
            cancels: Mutex::new(HashMap::new()),
            today: Box::new(|| chrono::Local::now().date_naive()),
        }
    }

    #[cfg(test)]
    fn with_clock(mut self, today: impl Fn() -> NaiveDate + Send + Sync + 'static) -> Self {
        self.today = Box::new(today);
        self
    }

    pub fn status(&self) -> EngineStatus {
        let today = (self.today)();
        let state = self.state.lock().unwrap();
        EngineStatus {
            mode: self.config.mode,
            model: HAIKU_MODEL.to_string(),
            endpoint_host: self.config.endpoint_host(),
            today: state.book.day(today),
            month: state.book.month(today),
            quotas: state.quotas,
            pause: state.pause.clone(),
        }
    }

    pub fn set_quotas(&self, quotas: Quotas) -> Result<EngineStatus> {
        quotas.validate()?;
        self.files.save_quotas(&quotas)?;
        self.state.lock().unwrap().quotas = quotas;
        Ok(self.status())
    }

    pub async fn key_status(&self) -> Result<KeyStatus> {
        Ok(KeyStatus {
            present: self.read_key().await?.is_some(),
        })
    }

    /// Stores the DIRECT mode key. The value is validated and never echoed.
    pub async fn set_api_key(&self, key: String) -> Result<KeyStatus> {
        let key = key.trim().to_string();
        let well_formed = key.starts_with("sk-ant-")
            && (20..=512).contains(&key.len())
            && key.bytes().all(|b| b.is_ascii_graphic());
        if !well_formed {
            return Err(EngineError::new(
                codes::INVALID_SETTING,
                "That doesn't look like an Anthropic API key (it starts with sk-ant-).",
            ));
        }
        let keys = Arc::clone(&self.keys);
        blocking(move || keys.set(KEYCHAIN_KEY_ITEM, &key)).await?;
        self.clear_pause_if(&[codes::NO_KEY, codes::INVALID_KEY]);
        Ok(KeyStatus { present: true })
    }

    pub async fn clear_api_key(&self) -> Result<KeyStatus> {
        let keys = Arc::clone(&self.keys);
        blocking(move || keys.delete(KEYCHAIN_KEY_ITEM)).await?;
        Ok(KeyStatus { present: false })
    }

    pub fn cancel(&self, request_id: &str) {
        if let Some(token) = self.cancels.lock().unwrap().remove(request_id) {
            token.cancel();
        }
    }

    /// Runs one request. Returns after the response body has been fully
    /// streamed through `emit`. Errors before the head carry no partial
    /// output; an error after the head ends the stream.
    pub async fn request(
        &self,
        input: EngineRequest,
        emit: impl FnMut(EngineEvent) + Send,
    ) -> Result<()> {
        let token = CancellationToken::new();
        self.cancels
            .lock()
            .unwrap()
            .insert(input.request_id.clone(), token.clone());
        let result = tokio::select! {
            biased;
            _ = token.cancelled() => Err(EngineError::new(codes::CANCELLED, "cancelled")),
            result = self.run(&input, emit) => result,
        };
        self.cancels.lock().unwrap().remove(&input.request_id);
        match &result {
            Ok(()) => self.clear_pause_if(error::PAUSE_CODES),
            Err(error) if error.is_pause() => self.set_pause(error),
            Err(_) => {}
        }
        if let Err(error) = &result {
            tracing::info!(code = %error.code, "ai_engine_request_failed");
        }
        result
    }

    async fn run(
        &self,
        input: &EngineRequest,
        mut emit: impl FnMut(EngineEvent) + Send,
    ) -> Result<()> {
        let prepared = request::prepare(&input.path, &input.body, &input.betas)?;
        let today = (self.today)();
        {
            let mut state = self.state.lock().unwrap();
            if prepared.is_messages() {
                state.book.check(today, &state.quotas)?;
            }
            let per_minute = state.quotas.per_minute;
            state.limiter.try_acquire(Instant::now(), per_minute)?;
        }

        let response = match &self.config.auth {
            EngineAuth::KeychainApiKey { .. } => {
                let key = self
                    .read_key()
                    .await?
                    .ok_or_else(|| EngineError::new(codes::NO_KEY, "no API key is set"))?;
                let response = self.direct.send(&prepared, &key).await?;
                return self.relay(prepared, response, &key, today, &mut emit).await;
            }
            EngineAuth::UserToken { token, .. } => {
                self.proxy
                    .send(&prepared, token.as_deref().unwrap_or_default())
                    .await?
            }
        };
        self.relay(prepared, response, "", today, &mut emit).await
    }

    async fn relay(
        &self,
        prepared: PreparedRequest,
        response: reqwest::Response,
        secret: &str,
        today: NaiveDate,
        emit: &mut (impl FnMut(EngineEvent) + Send),
    ) -> Result<()> {
        let status = response.status();
        if !status.is_success() {
            let body = read_capped(response, MAX_ERROR_BODY).await;
            return Err(map_status(status.as_u16(), &body, secret));
        }

        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("application/json")
            .to_string();
        emit(EngineEvent::Head {
            status: status.as_u16(),
            content_type,
        });

        let mut chunker = Utf8Chunker::default();
        let mut parser = UsageParser::new(prepared.streaming);
        let mut stream = response.bytes_stream();
        let mut outcome = Ok(());
        while let Some(next) = stream.next().await {
            match next {
                Ok(bytes) => {
                    if let Some(text) = chunker.push(&bytes) {
                        parser.feed(&text, MAX_JSON_BODY);
                        emit(EngineEvent::Chunk { data: text });
                    }
                }
                Err(_) => {
                    outcome = Err(EngineError::new(
                        codes::UNAVAILABLE,
                        "the AI service stopped responding",
                    ));
                    break;
                }
            }
        }
        if let Some(text) = chunker.finish() {
            parser.feed(&text, MAX_JSON_BODY);
            emit(EngineEvent::Chunk { data: text });
        }

        if outcome.is_ok() {
            emit(EngineEvent::End);
        }
        if prepared.is_messages() {
            let (input_tokens, output_tokens) = parser.totals();
            self.record(
                today,
                UsageCounts {
                    calls: 1,
                    input_tokens,
                    output_tokens,
                },
            );
        }
        outcome
    }

    fn record(&self, date: NaiveDate, counts: UsageCounts) {
        let book = {
            let mut state = self.state.lock().unwrap();
            state.book.record(date, counts);
            state.book.clone()
        };
        if let Err(error) = self.files.save_usage(&book) {
            tracing::warn!(code = %error.code, "ai_engine_usage_not_saved");
        }
    }

    async fn read_key(&self) -> Result<Option<String>> {
        let keys = Arc::clone(&self.keys);
        blocking(move || keys.get(KEYCHAIN_KEY_ITEM)).await
    }

    fn set_pause(&self, error: &EngineError) {
        self.state.lock().unwrap().pause = Some(PauseInfo {
            code: error.code.clone(),
            message: error.message.clone(),
            at: chrono::Utc::now().to_rfc3339(),
        });
    }

    fn clear_pause_if(&self, codes: &[&str]) {
        let mut state = self.state.lock().unwrap();
        if state
            .pause
            .as_ref()
            .is_some_and(|pause| codes.contains(&pause.code.as_str()))
        {
            state.pause = None;
        }
    }
}

async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> std::result::Result<T, anlg_local_accounts::Error> + Send + 'static,
) -> Result<T> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|_| EngineError::new(codes::STORAGE, "Keychain task failed"))?
        .map_err(|_| EngineError::new(codes::STORAGE, "The Keychain couldn't be read or written."))
}

async fn read_capped(response: reqwest::Response, cap: usize) -> String {
    let mut out = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(Ok(bytes)) = stream.next().await {
        let room = cap.saturating_sub(out.len());
        out.extend_from_slice(&bytes[..bytes.len().min(room)]);
        if out.len() >= cap {
            break;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn map_status(status: u16, body: &str, secret: &str) -> EngineError {
    let provider_message = serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| v["error"]["message"].as_str().map(str::to_string))
        .map(|m| {
            let mut m = redact::redact(&m, &[secret]);
            m.truncate(300);
            m
        });
    match status {
        429 => EngineError::new(
            codes::RATE_LIMITED,
            "the AI service is rate limiting requests",
        ),
        401 | 403 => EngineError::new(codes::INVALID_KEY, "the API key was rejected"),
        500..=599 => EngineError::new(codes::UNAVAILABLE, "the AI service is unavailable"),
        _ => EngineError::new(
            codes::BAD_REQUEST,
            provider_message
                .unwrap_or_else(|| format!("the AI service rejected the request ({status})")),
        ),
    }
}

#[cfg(test)]
mod tests;
