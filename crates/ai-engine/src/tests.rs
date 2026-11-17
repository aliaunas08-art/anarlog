//! Engine tests against a stub HTTP server. Fake keys only; nothing here
//! reaches the real Anthropic API.

use std::sync::Arc;

use anlg_local_accounts::{MemoryStore, SecretStore};
use chrono::NaiveDate;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

use super::*;
use crate::config::{EngineConfig, KEYCHAIN_KEY_ITEM};

const FAKE_KEY: &str = "sk-ant-test-0000000000000000";

const SSE: &str = "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"m\",\"model\":\"claude-haiku-5-5\",\"usage\":{\"input_tokens\":11,\"output_tokens\":1}}}\n\n\
event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hi\"}}\n\n\
event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":5}}\n\n";

fn day() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 11, 16).unwrap()
}

fn engine_at(server: &MockServer, key: Option<&str>) -> Engine {
    let store = MemoryStore::default();
    if let Some(key) = key {
        store.set(KEYCHAIN_KEY_ITEM, key).unwrap();
    }
    let config = EngineConfig::direct_at(url::Url::parse(&server.uri()).unwrap());
    Engine::new(config, Arc::new(store), None).with_clock(day)
}

fn messages(body: &str) -> EngineRequest {
    EngineRequest {
        request_id: "r1".into(),
        path: "/v1/messages".into(),
        body: body.into(),
        betas: vec![],
    }
}

async fn run(engine: &Engine, request: EngineRequest) -> (Result<()>, Vec<EngineEvent>) {
    let mut events = Vec::new();
    let result = engine.request(request, |event| events.push(event)).await;
    (result, events)
}

#[tokio::test]
async fn a_request_for_another_model_is_sent_with_the_pinned_model() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .and(header("x-api-key", FAKE_KEY))
        .and(header(
            "anthropic-version",
            crate::config::ANTHROPIC_API_VERSION,
        ))
        .respond_with(ResponseTemplate::new(200).set_body_raw(SSE, "text/event-stream"))
        .expect(1)
        .mount(&server)
        .await;
    let engine = engine_at(&server, Some(FAKE_KEY));

    let (result, events) = run(
        &engine,
        messages(r#"{"model":"claude-opus-5-5","max_tokens":10,"temperature":0.2,"stream":true,"messages":[{"role":"user","content":"hi"}]}"#),
    )
    .await;
    result.unwrap();

    let received: Vec<Request> = server.received_requests().await.unwrap();
    assert_eq!(received.len(), 1);
    let body: serde_json::Value = received[0].body_json().unwrap();
    assert_eq!(body["model"], HAIKU_MODEL);
    assert!(body.get("temperature").is_none());
    // The key is only in the header, never in the URL.
    assert!(!received[0].url.as_str().contains(FAKE_KEY));

    assert!(matches!(events[0], EngineEvent::Head { status: 200, .. }));
    let text: String = events
        .iter()
        .filter_map(|e| match e {
            EngineEvent::Chunk { data } => Some(data.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(text, SSE);
    assert_eq!(events.last(), Some(&EngineEvent::End));

    let status = engine.status();
    assert_eq!(
        status.today,
        UsageCounts {
            calls: 1,
            input_tokens: 11,
            output_tokens: 5
        }
    );
    assert_eq!(status.month.calls, 1);
    assert!(status.pause.is_none());
}

#[tokio::test]
async fn forbidden_paths_never_reach_the_network_or_the_keychain() {
    let server = MockServer::start().await;
    let engine = engine_at(&server, Some(FAKE_KEY));
    for bad in [
        "/v1/complete",
        "/v1/models",
        "/v1/files",
        "/v1/messages/batches",
    ] {
        let mut request = messages("{}");
        request.path = bad.into();
        let (result, events) = run(&engine, request).await;
        assert_eq!(result.unwrap_err().code, codes::FORBIDDEN_PATH);
        assert!(events.is_empty());
    }
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn missing_key_pauses_with_ai_no_key() {
    let server = MockServer::start().await;
    let engine = engine_at(&server, None);
    let (result, _) = run(&engine, messages(r#"{"messages":[]}"#)).await;
    assert_eq!(result.unwrap_err().code, codes::NO_KEY);
    assert_eq!(engine.status().pause.unwrap().code, codes::NO_KEY);
    assert!(server.received_requests().await.unwrap().is_empty());

    engine.set_api_key(FAKE_KEY.into()).await.unwrap();
    assert!(engine.status().pause.is_none());
    assert!(engine.key_status().await.unwrap().present);
}

#[tokio::test]
async fn provider_errors_map_to_pause_codes_and_never_leak_the_key() {
    for (status, code) in [
        (500, codes::UNAVAILABLE),
        (529, codes::UNAVAILABLE),
        (429, codes::RATE_LIMITED),
        (401, codes::INVALID_KEY),
        (400, codes::BAD_REQUEST),
    ] {
        let server = MockServer::start().await;
        let body = format!(
            r#"{{"type":"error","error":{{"type":"x","message":"echo {FAKE_KEY} Bearer abc"}}}}"#
        );
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(status).set_body_string(body))
            .mount(&server)
            .await;
        let engine = engine_at(&server, Some(FAKE_KEY));
        let (result, events) = run(&engine, messages(r#"{"messages":[]}"#)).await;
        let error = result.unwrap_err();
        assert_eq!(error.code, code, "status {status}");
        assert!(!error.message.contains(FAKE_KEY), "{}", error.message);
        assert!(!error.message.contains("abc"), "{}", error.message);
        assert!(events.is_empty());
        // Failed calls don't count as usage.
        assert_eq!(engine.status().today.calls, 0);
    }
}

#[tokio::test]
async fn network_error_is_ai_unavailable() {
    // wiremock pools servers, so use a port nothing listens on.
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let uri = format!("http://127.0.0.1:{port}");
    let store = MemoryStore::default();
    store.set(KEYCHAIN_KEY_ITEM, FAKE_KEY).unwrap();
    let engine = Engine::new(
        EngineConfig::direct_at(url::Url::parse(&uri).unwrap()),
        Arc::new(store),
        None,
    );
    let (result, _) = run(&engine, messages(r#"{"messages":[]}"#)).await;
    assert_eq!(result.unwrap_err().code, codes::UNAVAILABLE);
}

#[tokio::test]
async fn quotas_and_rate_limit_stop_calls_before_the_network() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            r#"{"content":[],"usage":{"input_tokens":1,"output_tokens":1}}"#,
            "application/json",
        ))
        .mount(&server)
        .await;
    let engine = engine_at(&server, Some(FAKE_KEY));
    engine
        .set_quotas(Quotas {
            daily_calls: 2,
            per_minute: 60,
            ..Quotas::default()
        })
        .unwrap();
    for _ in 0..2 {
        run(&engine, messages(r#"{"messages":[]}"#))
            .await
            .0
            .unwrap();
    }
    let (result, _) = run(&engine, messages(r#"{"messages":[]}"#)).await;
    assert_eq!(result.unwrap_err().code, codes::QUOTA);
    assert_eq!(server.received_requests().await.unwrap().len(), 2);
    assert_eq!(engine.status().pause.unwrap().code, codes::QUOTA);

    let engine = engine_at(&server, Some(FAKE_KEY));
    engine
        .set_quotas(Quotas {
            per_minute: 1,
            ..Quotas::default()
        })
        .unwrap();
    run(&engine, messages(r#"{"messages":[]}"#))
        .await
        .0
        .unwrap();
    let (result, _) = run(&engine, messages(r#"{"messages":[]}"#)).await;
    assert_eq!(result.unwrap_err().code, codes::RATE_LIMITED);

    assert_eq!(
        engine
            .set_quotas(Quotas {
                per_minute: 0,
                ..Quotas::default()
            })
            .unwrap_err()
            .code,
        codes::INVALID_SETTING
    );
}

#[tokio::test]
async fn proxy_mode_is_a_stub() {
    let store: Arc<dyn SecretStore> = Arc::new(MemoryStore::default());
    let config = EngineConfig::proxy(
        url::Url::parse("https://proxy.invalid").unwrap(),
        Some("t".into()),
    );
    let engine = Engine::new(config, store, None);
    let (result, _) = run(&engine, messages(r#"{"messages":[]}"#)).await;
    assert_eq!(result.unwrap_err().code, codes::NOT_IMPLEMENTED);
}

#[tokio::test]
async fn malformed_keys_are_refused_and_never_echoed() {
    let server = MockServer::start().await;
    let engine = engine_at(&server, None);
    for bad in [
        "",
        "hello",
        "sk-ant-short",
        "sk-ant-has space inside-000000",
    ] {
        let error = engine.set_api_key(bad.into()).await.unwrap_err();
        assert_eq!(error.code, codes::INVALID_SETTING);
        if !bad.is_empty() {
            assert!(!error.message.contains(bad));
        }
    }
    assert!(!engine.key_status().await.unwrap().present);
    engine.set_api_key(FAKE_KEY.into()).await.unwrap();
    engine.clear_api_key().await.unwrap();
    assert!(!engine.key_status().await.unwrap().present);
}

#[tokio::test]
async fn cancel_stops_a_request() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_delay(std::time::Duration::from_secs(5)))
        .mount(&server)
        .await;
    let engine = Arc::new(engine_at(&server, Some(FAKE_KEY)));
    let runner = Arc::clone(&engine);
    let handle =
        tokio::spawn(async move { runner.request(messages(r#"{"messages":[]}"#), |_| {}).await });
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    engine.cancel("r1");
    let result = tokio::time::timeout(std::time::Duration::from_secs(2), handle)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(result.unwrap_err().code, codes::CANCELLED);
}
