use std::collections::HashMap;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use wiremock::matchers::{body_string_contains, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::*;
use crate::store::MemoryStore;

fn overrides(server: &MockServer) -> Overrides {
    let mut map = HashMap::new();
    map.insert(
        Provider::Google,
        (
            ClientConfig {
                client_id: "client-1".into(),
                client_secret: Some("desktop-secret".into()),
                tenant: String::new(),
            },
            Endpoints {
                authorize: format!("{}/authorize", server.uri()),
                token: format!("{}/token", server.uri()),
                userinfo: format!("{}/userinfo", server.uri()),
                revoke: Some(format!("{}/revoke", server.uri())),
                redirect_host: "127.0.0.1",
            },
        ),
    );
    map
}

/// Plays the browser: reads the consent URL, then follows the redirect the
/// provider would send with a code.
fn browser(code: &'static str) -> impl FnOnce(&str) -> Result<(), String> {
    move |consent: &str| {
        let consent = url::Url::parse(consent).unwrap();
        let query: HashMap<_, _> = consent.query_pairs().into_owned().collect();
        assert_eq!(query["client_id"], "client-1");
        assert_eq!(query["code_challenge_method"], "S256");
        assert_eq!(query["access_type"], "offline");
        assert!(query["scope"].contains("calendar.readonly"));
        let redirect = url::Url::parse(&query["redirect_uri"]).unwrap();
        let port = redirect.port().unwrap();
        let state = query["state"].clone();
        tokio::spawn(async move {
            let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
                .await
                .unwrap();
            let request = format!("GET /?code={code}&state={state} HTTP/1.1\r\nHost: x\r\n\r\n");
            stream.write_all(request.as_bytes()).await.unwrap();
            let mut response = String::new();
            let _ = stream.read_to_string(&mut response).await;
        });
        Ok(())
    }
}

async fn mount_sign_in(server: &MockServer) {
    Mock::given(method("POST"))
        .and(path("/token"))
        .and(body_string_contains("grant_type=authorization_code"))
        .and(body_string_contains("code=the-code"))
        .and(body_string_contains("code_verifier="))
        .and(body_string_contains("client_secret=desktop-secret"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "access_token": "access-1",
            "expires_in": 3600,
            "refresh_token": "refresh-1",
            "token_type": "Bearer"
        })))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/userinfo"))
        .and(header("authorization", "Bearer access-1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "sub": "1234", "email": "sam@example.com", "name": "Sam Lee"
        })))
        .mount(server)
        .await;
}

#[tokio::test]
async fn connects_stores_the_refresh_token_and_serves_the_access_token() {
    let server = MockServer::start().await;
    mount_sign_in(&server).await;
    let manager = AccountManager::with_overrides(MemoryStore::default(), overrides(&server));

    let account = manager
        .connect(Provider::Google, browser("the-code"))
        .await
        .unwrap();
    assert_eq!(account.id, "google-1234");
    assert_eq!(account.email.as_deref(), Some("sam@example.com"));
    assert_eq!(manager.list().unwrap(), vec![account.clone()]);
    assert_eq!(
        manager.store.get("refresh/google-1234").unwrap().as_deref(),
        Some("refresh-1")
    );
    // The index holds no secrets.
    let index = manager.store.get(INDEX_KEY).unwrap().unwrap();
    assert!(!index.contains("refresh-1") && !index.contains("access-1"));

    assert_eq!(
        manager.access_token("google-1234").await.unwrap(),
        "access-1"
    );
}

#[tokio::test]
async fn refreshes_an_expired_token_and_keeps_a_rotated_refresh_token() {
    let server = MockServer::start().await;
    mount_sign_in(&server).await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .and(body_string_contains("grant_type=refresh_token"))
        .and(body_string_contains("refresh_token=refresh-1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "access_token": "access-2", "expires_in": 3600, "refresh_token": "refresh-2"
        })))
        .expect(1)
        .mount(&server)
        .await;
    let manager = AccountManager::with_overrides(MemoryStore::default(), overrides(&server));
    manager
        .connect(Provider::Google, browser("the-code"))
        .await
        .unwrap();

    manager.forget_access_token("google-1234");
    let (a, b) = tokio::join!(
        manager.access_token("google-1234"),
        manager.access_token("google-1234")
    );
    assert_eq!(a.unwrap(), "access-2");
    assert_eq!(b.unwrap(), "access-2");
    assert_eq!(
        manager.store.get("refresh/google-1234").unwrap().as_deref(),
        Some("refresh-2")
    );
}

#[tokio::test]
async fn a_rejected_refresh_token_marks_the_account_for_reconnecting() {
    let server = MockServer::start().await;
    mount_sign_in(&server).await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .and(body_string_contains("grant_type=refresh_token"))
        .respond_with(ResponseTemplate::new(400).set_body_json(serde_json::json!({
            "error": "invalid_grant", "error_description": "Token has been expired or revoked."
        })))
        .mount(&server)
        .await;
    let manager = AccountManager::with_overrides(MemoryStore::default(), overrides(&server));
    manager
        .connect(Provider::Google, browser("the-code"))
        .await
        .unwrap();
    manager.forget_access_token("google-1234");

    let error = manager.access_token("google-1234").await.unwrap_err();
    assert_eq!(error.code(), "reconnect_required");
    assert!(manager.get("google-1234").unwrap().needs_reconnect);
}

#[tokio::test]
async fn disconnect_revokes_and_removes_everything() {
    let server = MockServer::start().await;
    mount_sign_in(&server).await;
    Mock::given(method("POST"))
        .and(path("/revoke"))
        .and(body_string_contains("token=refresh-1"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server)
        .await;
    let manager = AccountManager::with_overrides(MemoryStore::default(), overrides(&server));
    manager
        .connect(Provider::Google, browser("the-code"))
        .await
        .unwrap();

    manager.disconnect("google-1234").await.unwrap();
    assert!(manager.list().unwrap().is_empty());
    assert!(manager.store.get("refresh/google-1234").unwrap().is_none());
    assert_eq!(
        manager
            .access_token("google-1234")
            .await
            .unwrap_err()
            .code(),
        "unknown_account"
    );
}

#[tokio::test]
async fn denied_consent_and_missing_configuration_are_reported() {
    let server = MockServer::start().await;
    let manager = AccountManager::with_overrides(MemoryStore::default(), overrides(&server));
    let deny = |consent: &str| {
        let consent = url::Url::parse(consent).unwrap();
        let query: HashMap<_, _> = consent.query_pairs().into_owned().collect();
        let port = url::Url::parse(&query["redirect_uri"])
            .unwrap()
            .port()
            .unwrap();
        let state = query["state"].clone();
        tokio::spawn(async move {
            let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
                .await
                .unwrap();
            let request =
                format!("GET /?error=access_denied&state={state} HTTP/1.1\r\nHost: x\r\n\r\n");
            stream.write_all(request.as_bytes()).await.unwrap();
        });
        Ok(())
    };
    let error = manager.connect(Provider::Google, deny).await.unwrap_err();
    assert_eq!(error.code(), "denied");
    assert!(manager.list().unwrap().is_empty());

    let error = manager
        .connect(Provider::Microsoft, |_| Ok(()))
        .await
        .unwrap_err();
    assert_eq!(error.code(), "not_configured");
    assert_eq!(manager.configured_providers(), vec![Provider::Google]);
}

#[tokio::test]
async fn starting_a_new_sign_in_cancels_the_waiting_one() {
    let server = MockServer::start().await;
    let manager = std::sync::Arc::new(AccountManager::with_overrides(
        MemoryStore::default(),
        overrides(&server),
    ));
    let waiting = {
        let manager = manager.clone();
        tokio::spawn(async move { manager.connect(Provider::Google, |_| Ok(())).await })
    };
    tokio::time::sleep(Duration::from_millis(100)).await;
    manager.cancel_connect();
    let error = waiting.await.unwrap().unwrap_err();
    assert_eq!(error.code(), "cancelled");
}

#[tokio::test]
async fn an_account_without_calendar_access_is_not_saved() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "access_token": "access-1", "expires_in": 3600, "refresh_token": "refresh-1",
            "scope": "openid https://www.googleapis.com/auth/userinfo.email"
        })))
        .mount(&server)
        .await;
    let manager = AccountManager::with_overrides(MemoryStore::default(), overrides(&server));
    let error = manager
        .connect(Provider::Google, browser("the-code"))
        .await
        .unwrap_err();
    assert_eq!(error.code(), "missing_scope");
    assert!(manager.list().unwrap().is_empty());
    assert!(manager.store.get("refresh/google-1234").unwrap().is_none());
}
