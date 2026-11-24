//! What the Engine forwards: only the Messages endpoints, with the model
//! forced to [`HAIKU_MODEL`] and the fields the Engine owns rewritten.

use serde_json::{Map, Value};

use crate::config::HAIKU_MODEL;
use crate::error::{EngineError, Result, codes};

pub const MESSAGES_PATH: &str = "/v1/messages";
pub const COUNT_TOKENS_PATH: &str = "/v1/messages/count_tokens";
/// Every path the Engine forwards. Anything else is rejected before any
/// network or Keychain access.
pub const ALLOWED_PATHS: &[&str] = &[MESSAGES_PATH, COUNT_TOKENS_PATH];

/// Hard cap on output tokens per call (cost protection). A caller asking
/// for more is clamped.
pub const MAX_OUTPUT_TOKENS: u64 = 16_384;
/// Body size cap. Enhance can attach note images as base64.
pub const MAX_BODY_BYTES: usize = 24 * 1024 * 1024;

/// Beta headers the Engine passes on. Everything else is dropped, so a
/// caller can't turn on server-side tools (code execution, web fetch,
/// computer use, MCP) through the gateway.
pub const ALLOWED_BETAS: &[&str] = &["structured-outputs-2025-11-13", "pdfs-2024-09-25"];

/// Top-level fields removed from every request: sampling parameters are
/// rejected by the pinned model and API version, and the rest would reach
/// services outside the Messages call.
const STRIPPED_FIELDS: &[&str] = &[
    "temperature",
    "top_p",
    "top_k",
    "mcp_servers",
    "container",
    "service_tier",
    "speed",
    "inference_geo",
];

#[derive(Debug, Clone, PartialEq)]
pub struct PreparedRequest {
    pub path: &'static str,
    pub body: Value,
    pub betas: Vec<String>,
    pub streaming: bool,
}

impl PreparedRequest {
    pub fn is_messages(&self) -> bool {
        self.path == MESSAGES_PATH
    }
}

pub fn allowed_path(path: &str) -> Result<&'static str> {
    ALLOWED_PATHS
        .iter()
        .copied()
        .find(|allowed| *allowed == path)
        .ok_or_else(|| EngineError::new(codes::FORBIDDEN_PATH, "Only the Messages API is allowed."))
}

/// Validates and rewrites a caller's request. The caller's `model` is
/// always replaced (never rejected) so feature code keeps working when it
/// passes any id.
pub fn prepare(path: &str, body: &str, betas: &[String]) -> Result<PreparedRequest> {
    let path = allowed_path(path)?;
    if body.len() > MAX_BODY_BYTES {
        return Err(EngineError::new(
            codes::INVALID_REQUEST,
            "The request is too large.",
        ));
    }
    let mut object: Map<String, Value> = match serde_json::from_str(body) {
        Ok(Value::Object(object)) => object,
        _ => {
            return Err(EngineError::new(
                codes::INVALID_REQUEST,
                "The request body must be a JSON object.",
            ));
        }
    };

    object.insert("model".into(), Value::String(HAIKU_MODEL.into()));
    for field in STRIPPED_FIELDS {
        object.remove(*field);
    }

    let streaming = object
        .get("stream")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if path == MESSAGES_PATH {
        let requested = object.get("max_tokens").and_then(Value::as_u64);
        let max_tokens = requested
            .unwrap_or(MAX_OUTPUT_TOKENS)
            .clamp(1, MAX_OUTPUT_TOKENS);
        object.insert("max_tokens".into(), Value::from(max_tokens));
    } else {
        object.remove("max_tokens");
        object.remove("stream");
    }

    let betas = betas
        .iter()
        .flat_map(|value| value.split(','))
        .map(str::trim)
        .filter(|beta| ALLOWED_BETAS.contains(beta))
        .map(str::to_string)
        .collect();

    Ok(PreparedRequest {
        path,
        body: Value::Object(object),
        betas,
        streaming: streaming && path == MESSAGES_PATH,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_is_overwritten_and_owned_fields_are_rewritten() {
        let body = r#"{"model":"claude-opus-5-5","max_tokens":64000,"temperature":0,
            "top_p":0.9,"mcp_servers":[{}],"messages":[{"role":"user","content":"hi"}],"stream":true}"#;
        let prepared = prepare(
            MESSAGES_PATH,
            body,
            &["computer-use-2025-01-24,structured-outputs-2025-11-13".into()],
        )
        .unwrap();
        assert_eq!(prepared.body["model"], HAIKU_MODEL);
        assert_eq!(prepared.body["max_tokens"], MAX_OUTPUT_TOKENS);
        assert!(prepared.body.get("temperature").is_none());
        assert!(prepared.body.get("top_p").is_none());
        assert!(prepared.body.get("mcp_servers").is_none());
        assert!(prepared.streaming);
        assert_eq!(
            prepared.betas,
            vec!["structured-outputs-2025-11-13".to_string()]
        );
    }

    #[test]
    fn a_missing_model_is_added() {
        let prepared = prepare(MESSAGES_PATH, r#"{"messages":[]}"#, &[]).unwrap();
        assert_eq!(prepared.body["model"], HAIKU_MODEL);
    }

    #[test]
    fn other_paths_and_bodies_are_rejected() {
        for path in [
            "/v1/complete",
            "/v1/models",
            "/v1/files",
            "/v1/messages/batches",
            "/v1/messages/../files",
            "",
        ] {
            let error = prepare(path, "{}", &[]).unwrap_err();
            assert_eq!(error.code, codes::FORBIDDEN_PATH, "{path}");
        }
        assert_eq!(
            prepare(MESSAGES_PATH, "[1]", &[]).unwrap_err().code,
            codes::INVALID_REQUEST
        );
        assert_eq!(
            prepare(MESSAGES_PATH, "nope", &[]).unwrap_err().code,
            codes::INVALID_REQUEST
        );
    }

    #[test]
    fn count_tokens_drops_generation_fields() {
        let prepared = prepare(
            COUNT_TOKENS_PATH,
            r#"{"model":"x","max_tokens":5,"stream":true,"messages":[]}"#,
            &[],
        )
        .unwrap();
        assert_eq!(prepared.body["model"], HAIKU_MODEL);
        assert!(prepared.body.get("max_tokens").is_none());
        assert!(!prepared.streaming);
    }
}
