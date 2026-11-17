use std::future::Future;

use bytes::Bytes;
use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;
use sqlx::SqlitePool;

pub const EVENT_MEETING_COMPLETED: &str = "meeting.completed";
pub const EVENT_NOTE_ENHANCED: &str = "note.enhanced";
pub const EVENT_TEST: &str = "webhook.test";
pub const KNOWN_EVENTS: &[&str] = &[EVENT_MEETING_COMPLETED, EVENT_NOTE_ENHANCED];

const DELIVERY_TIMEOUT_SECS: u64 = 10;
const RETRY_DELAYS_SECS: &[u64] = &[0, 5, 30];
const MAX_STATUS_LEN: usize = 200;
const MAX_CONCURRENT_DELIVERIES: usize = 4;
const MAX_CONCURRENT_FANOUTS: usize = 4;
const MAX_PENDING_FANOUTS: usize = 32;
const MAX_PENDING_TEST_DELIVERIES: usize = 32;
const FANOUT_BUSY_ERROR: &str = "webhook fanout is busy; try again";
const DELIVERY_BUSY_ERROR: &str = "webhook delivery is busy; try again";

pub(crate) const MAX_WEBHOOK_ENDPOINTS: usize = 64;

static CREATE_ENDPOINT_LOCK: std::sync::OnceLock<tokio::sync::Mutex<()>> =
    std::sync::OnceLock::new();

static DELIVERY_SLOTS: std::sync::OnceLock<std::sync::Arc<tokio::sync::Semaphore>> =
    std::sync::OnceLock::new();
static DELIVERY_WAITING_SLOTS: std::sync::OnceLock<std::sync::Arc<tokio::sync::Semaphore>> =
    std::sync::OnceLock::new();
static FANOUT_SLOTS: std::sync::OnceLock<std::sync::Arc<tokio::sync::Semaphore>> =
    std::sync::OnceLock::new();
static FANOUT_WAITING_SLOTS: std::sync::OnceLock<std::sync::Arc<tokio::sync::Semaphore>> =
    std::sync::OnceLock::new();

fn delivery_slots() -> std::sync::Arc<tokio::sync::Semaphore> {
    DELIVERY_SLOTS
        .get_or_init(|| std::sync::Arc::new(tokio::sync::Semaphore::new(MAX_CONCURRENT_DELIVERIES)))
        .clone()
}

fn fanout_slots() -> std::sync::Arc<tokio::sync::Semaphore> {
    FANOUT_SLOTS
        .get_or_init(|| std::sync::Arc::new(tokio::sync::Semaphore::new(MAX_CONCURRENT_FANOUTS)))
        .clone()
}

fn delivery_waiting_slots() -> std::sync::Arc<tokio::sync::Semaphore> {
    DELIVERY_WAITING_SLOTS
        .get_or_init(|| {
            std::sync::Arc::new(tokio::sync::Semaphore::new(MAX_PENDING_TEST_DELIVERIES))
        })
        .clone()
}

fn fanout_waiting_slots() -> std::sync::Arc<tokio::sync::Semaphore> {
    FANOUT_WAITING_SLOTS
        .get_or_init(|| std::sync::Arc::new(tokio::sync::Semaphore::new(MAX_PENDING_FANOUTS)))
        .clone()
}

async fn acquire_with_waiting_tier(
    active: std::sync::Arc<tokio::sync::Semaphore>,
    waiting: std::sync::Arc<tokio::sync::Semaphore>,
    busy_error: &'static str,
) -> Result<tokio::sync::OwnedSemaphorePermit, String> {
    let waiting_permit = waiting
        .try_acquire_owned()
        .map_err(|_| busy_error.to_string())?;
    let active_permit = active
        .acquire_owned()
        .await
        .map_err(|_| busy_error.to_string())?;
    drop(waiting_permit);
    Ok(active_permit)
}

async fn acquire_fanout_slot() -> Result<tokio::sync::OwnedSemaphorePermit, String> {
    acquire_with_waiting_tier(fanout_slots(), fanout_waiting_slots(), FANOUT_BUSY_ERROR).await
}

async fn acquire_test_delivery_slot() -> Result<tokio::sync::OwnedSemaphorePermit, String> {
    acquire_with_waiting_tier(
        delivery_slots(),
        delivery_waiting_slots(),
        DELIVERY_BUSY_ERROR,
    )
    .await
}

#[cfg(test)]
pub fn sign_payload(secret: &str, body: &str) -> String {
    sign_payload_bytes(secret, body.as_bytes())
}

fn sign_payload_bytes(secret: &str, body: &[u8]) -> String {
    let mut mac = <Hmac<Sha256> as KeyInit>::new_from_slice(secret.as_bytes())
        .expect("hmac accepts keys of any length");
    mac.update(body);
    mac.finalize()
        .into_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn subscribes(endpoint: &anlg_db_app::WebhookEndpointRow, event: &str) -> bool {
    let events: Vec<String> = serde_json::from_str(&endpoint.events_json).unwrap_or_default();
    events.is_empty() || events.iter().any(|subscribed| subscribed == event)
}

fn envelope(event: &str, data: serde_json::Value) -> String {
    serde_json::json!({
        "id": format!("evt_{}", uuid::Uuid::new_v4().simple()),
        "event": event,
        "created_at": chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        "data": data,
    })
    .to_string()
}

async fn meeting_payload(pool: &SqlitePool, meeting_id: &str) -> Result<serde_json::Value, String> {
    let export = crate::commands::scoped_meeting_export(pool, meeting_id.to_string())
        .await
        .map_err(|error| error.to_string())?;
    let transcript_text = export
        .transcripts
        .iter()
        .map(|transcript| transcript.text.trim())
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n");
    Ok(serde_json::json!({
        "meeting": export.meeting,
        "transcript_text": transcript_text,
    }))
}

pub async fn create_endpoint(
    pool: &SqlitePool,
    url: &str,
    events: &[String],
) -> Result<crate::CreatedWebhook, String> {
    let _guard = CREATE_ENDPOINT_LOCK
        .get_or_init(|| tokio::sync::Mutex::new(()))
        .lock()
        .await;
    let url = url.trim();
    let parsed = parse_webhook_url(url)?;
    if let Some(unknown) = events
        .iter()
        .find(|event| !KNOWN_EVENTS.contains(&event.as_str()))
    {
        return Err(format!(
            "unknown event '{unknown}'; known events: {}",
            KNOWN_EVENTS.join(", ")
        ));
    }
    let endpoint_count = anlg_db_app::list_webhook_endpoints(pool)
        .await
        .map_err(|error| error.to_string())?
        .len();
    if endpoint_count >= MAX_WEBHOOK_ENDPOINTS {
        return Err(format!(
            "at most {MAX_WEBHOOK_ENDPOINTS} webhook endpoints can be configured"
        ));
    }

    resolve_public_addrs(&parsed).await?;

    let events_json = serde_json::to_string(events).map_err(|error| error.to_string())?;
    let secret = anlg_db_app::generate_webhook_secret();
    let row = anlg_db_app::insert_webhook_endpoint(
        pool,
        &uuid::Uuid::new_v4().to_string(),
        url,
        &secret,
        &events_json,
    )
    .await
    .map_err(|error| error.to_string())?;

    Ok(crate::CreatedWebhook {
        info: crate::WebhookInfo::from(row),
        secret,
    })
}

/// ASPC C6.4: whether a meeting is marked Private. Fails closed: a meeting
/// whose flag can't be read counts as Private, so it is not sent anywhere.
pub(crate) async fn is_private_meeting(pool: &SqlitePool, meeting_id: &str) -> bool {
    match sqlx::query_scalar::<_, i64>("SELECT private FROM sessions WHERE id = ?")
        .bind(meeting_id)
        .fetch_optional(pool)
        .await
    {
        Ok(Some(flag)) => flag != 0,
        Ok(None) => false,
        Err(error) => {
            tracing::warn!("[local-api] could not read the private flag: {error}");
            true
        }
    }
}

/// Fans an event out to every active endpoint subscribed to it. Deliveries run
/// in background tasks; the returned count is the number of endpoints targeted.
pub async fn dispatch_event(
    pool: &SqlitePool,
    event: &str,
    meeting_id: &str,
) -> Result<usize, String> {
    // ASPC C6.4: a Private meeting is never sent to a webhook.
    if is_private_meeting(pool, meeting_id).await {
        return Ok(0);
    }
    let fanout_permit = acquire_fanout_slot().await?;
    let mut endpoints = anlg_db_app::list_active_webhook_endpoints(pool)
        .await
        .map_err(|error| error.to_string())?
        .into_iter()
        .filter(|endpoint| subscribes(endpoint, event))
        .collect::<Vec<_>>();
    if endpoints.len() > MAX_WEBHOOK_ENDPOINTS {
        tracing::warn!(
            "[local-api] limiting webhook fanout from {} to {} endpoints",
            endpoints.len(),
            MAX_WEBHOOK_ENDPOINTS
        );
        endpoints.truncate(MAX_WEBHOOK_ENDPOINTS);
    }
    if endpoints.is_empty() {
        return Ok(0);
    }

    let body = Bytes::from(envelope(event, meeting_payload(pool, meeting_id).await?));
    let targeted = endpoints.len();
    let pool = pool.clone();
    let event = std::sync::Arc::<str>::from(event);
    let delivery_slots = delivery_slots();
    tokio::spawn(async move {
        let _fanout_permit = fanout_permit;
        run_bounded(endpoints, MAX_CONCURRENT_DELIVERIES, move |endpoint| {
            let pool = pool.clone();
            let event = event.clone();
            let body = body.clone();
            let delivery_slots = delivery_slots.clone();
            async move {
                let Ok(_permit) = delivery_slots.acquire_owned().await else {
                    return;
                };
                deliver_with_retry(&pool, &endpoint, &event, &body).await;
            }
        })
        .await;
    });
    Ok(targeted)
}

async fn run_bounded<T, F, Fut>(items: impl IntoIterator<Item = T>, limit: usize, run: F)
where
    T: Send + 'static,
    F: Fn(T) -> Fut + Clone + Send + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    let mut items = items.into_iter();
    let mut tasks = tokio::task::JoinSet::new();

    loop {
        while tasks.len() < limit {
            let Some(item) = items.next() else {
                break;
            };
            tasks.spawn(run.clone()(item));
        }

        let Some(result) = tasks.join_next().await else {
            break;
        };
        if let Err(error) = result {
            tracing::warn!("[local-api] webhook delivery task failed: {error}");
        }
    }
}

pub async fn send_test(
    pool: &SqlitePool,
    endpoint: &anlg_db_app::WebhookEndpointRow,
) -> Result<crate::WebhookDelivery, String> {
    let _permit = acquire_test_delivery_slot().await?;
    let body = Bytes::from(envelope(
        EVENT_TEST,
        serde_json::json!({ "message": "This is a test delivery." }),
    ));
    let delivery_id = format!("dlv_{}", uuid::Uuid::new_v4().simple());
    let status = deliver_once(endpoint, EVENT_TEST, &body, &delivery_id).await;
    let delivered = status.delivered;
    let status_text = truncate_status(&status.status);
    if let Err(error) = anlg_db_app::record_webhook_delivery(pool, &endpoint.id, &status_text).await
    {
        tracing::warn!("[local-api] failed to record webhook delivery: {error}");
    }
    Ok(crate::WebhookDelivery {
        delivered,
        status: status_text,
    })
}

struct DeliveryOutcome {
    delivered: bool,
    status: String,
}

async fn deliver_with_retry(
    pool: &SqlitePool,
    endpoint: &anlg_db_app::WebhookEndpointRow,
    event: &str,
    body: &Bytes,
) {
    let delivery_id = format!("dlv_{}", uuid::Uuid::new_v4().simple());
    let mut outcome = DeliveryOutcome {
        delivered: false,
        status: "not attempted".to_string(),
    };
    for delay in RETRY_DELAYS_SECS {
        if *delay > 0 {
            tokio::time::sleep(std::time::Duration::from_secs(*delay)).await;
        }
        outcome = deliver_once(endpoint, event, body, &delivery_id).await;
        if outcome.delivered {
            break;
        }
    }

    if !outcome.delivered {
        tracing::warn!(
            "[local-api] webhook delivery to {} failed: {}",
            endpoint.url,
            outcome.status
        );
    }
    let status_text = truncate_status(&outcome.status);
    if let Err(error) = anlg_db_app::record_webhook_delivery(pool, &endpoint.id, &status_text).await
    {
        tracing::warn!("[local-api] failed to record webhook delivery: {error}");
    }
}

/// Webhook URLs must be `https://` and resolve only to public addresses
/// (ASPC B2.2). There is no user override yet.
pub(crate) fn parse_webhook_url(url: &str) -> Result<reqwest::Url, String> {
    let parsed =
        reqwest::Url::parse(url).map_err(|_| "url must start with https://".to_string())?;
    if parsed.scheme() != "https" {
        return Err("url must start with https://".to_string());
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err("url must not contain credentials".to_string());
    }
    match parsed.host() {
        Some(url::Host::Domain(domain))
            if domain.eq_ignore_ascii_case("localhost")
                || domain.to_ascii_lowercase().ends_with(".localhost") =>
        {
            Err("url must not point at this computer".to_string())
        }
        Some(_) => Ok(parsed),
        None => Err("url must include a host".to_string()),
    }
}

fn is_public_ip(ip: std::net::IpAddr) -> bool {
    use std::net::IpAddr;

    match ip {
        IpAddr::V4(v4) => {
            let [a, b, ..] = v4.octets();
            !(v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.is_broadcast()
                || v4.is_multicast()
                || (a == 192 && b == 0 && v4.octets()[2] == 2)
                || (a == 198 && b == 51 && v4.octets()[2] == 100)
                || (a == 203 && b == 0 && v4.octets()[2] == 113)
                || a == 0
                || (a == 100 && (64..128).contains(&b))
                || (a == 192 && b == 0 && v4.octets()[2] == 0)
                || (a == 198 && (b == 18 || b == 19))
                || a >= 240)
        }
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return is_public_ip(IpAddr::V4(v4));
            }
            let first = v6.segments()[0];
            !(v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                || (first & 0xfe00) == 0xfc00
                || (first & 0xffc0) == 0xfe80
                || (first == 0x2001 && v6.segments()[1] == 0x0db8)
                || (first == 0x0064 && v6.segments()[1] == 0xff9b))
        }
    }
}

/// Resolves the URL's host and fails unless every address is public, so a
/// name can't smuggle in loopback, link-local or private ranges.
pub(crate) async fn resolve_public_addrs(
    url: &reqwest::Url,
) -> Result<Vec<std::net::SocketAddr>, String> {
    let port = url.port_or_known_default().unwrap_or(443);
    let addrs: Vec<std::net::SocketAddr> = match url.host() {
        Some(url::Host::Ipv4(ip)) => vec![(ip, port).into()],
        Some(url::Host::Ipv6(ip)) => vec![(ip, port).into()],
        Some(url::Host::Domain(domain)) => tokio::net::lookup_host((domain, port))
            .await
            .map_err(|error| format!("could not resolve webhook host: {error}"))?
            .collect(),
        None => return Err("url must include a host".to_string()),
    };
    if addrs.is_empty() {
        return Err("could not resolve webhook host".to_string());
    }
    if let Some(blocked) = addrs.iter().find(|addr| !is_public_ip(addr.ip())) {
        return Err(format!(
            "webhook host resolves to a non-public address ({})",
            blocked.ip()
        ));
    }
    Ok(addrs)
}

async fn deliver_once(
    endpoint: &anlg_db_app::WebhookEndpointRow,
    event: &str,
    body: &Bytes,
    delivery_id: &str,
) -> DeliveryOutcome {
    // Re-check on every delivery: rows can be written without
    // `create_endpoint`, and DNS can change after creation. The checked
    // addresses are pinned so the connection can't be rebound, and redirects
    // are not followed.
    let checked = match parse_webhook_url(&endpoint.url) {
        Ok(url) => resolve_public_addrs(&url).await.map(|addrs| (url, addrs)),
        Err(error) => Err(error),
    };
    let (url, addrs) = match checked {
        Ok(checked) => checked,
        Err(error) => {
            return DeliveryOutcome {
                delivered: false,
                status: format!("blocked: {error}"),
            };
        }
    };
    let mut builder = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(DELIVERY_TIMEOUT_SECS))
        .redirect(reqwest::redirect::Policy::none());
    if let Some(domain) = url.domain() {
        builder = builder.resolve_to_addrs(domain, &addrs);
    }
    let client = match builder.build() {
        Ok(client) => client,
        Err(error) => {
            return DeliveryOutcome {
                delivered: false,
                status: format!("error: {error}"),
            };
        }
    };

    let response = client
        .post(url)
        .header("content-type", "application/json")
        .header("x-anarlog-event", event)
        .header("x-anarlog-delivery", delivery_id)
        .header(
            "x-anarlog-timestamp",
            chrono::Utc::now().timestamp().to_string(),
        )
        .header(
            "x-anarlog-signature",
            format!(
                "sha256={}",
                sign_payload_bytes(&endpoint.secret, body.as_ref())
            ),
        )
        .body(body.clone())
        .send()
        .await;

    match response {
        Ok(response) => DeliveryOutcome {
            delivered: response.status().is_success(),
            status: response.status().to_string(),
        },
        Err(error) => DeliveryOutcome {
            delivered: false,
            status: format!("error: {error}"),
        },
    }
}

fn truncate_status(status: &str) -> String {
    status.chars().take(MAX_STATUS_LEN).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    #[test]
    fn signature_matches_reference_hmac() {
        // Precomputed with python hmac: key=whsec_test, message={"a":1}
        assert_eq!(
            sign_payload("whsec_test", "{\"a\":1}"),
            "51426af50a41dd7ff2cd3f116594734766d4018d15d6fb07169aee5d2959adf5"
        );
    }

    #[test]
    fn empty_event_list_subscribes_to_everything() {
        let endpoint = anlg_db_app::WebhookEndpointRow {
            id: "webhook-1".to_string(),
            url: "https://example.com".to_string(),
            secret: "whsec_x".to_string(),
            events_json: "[]".to_string(),
            active: true,
            created_at: String::new(),
            last_delivery_at: None,
            last_delivery_status: String::new(),
        };
        assert!(subscribes(&endpoint, EVENT_NOTE_ENHANCED));

        let scoped = anlg_db_app::WebhookEndpointRow {
            events_json: "[\"meeting.completed\"]".to_string(),
            ..endpoint
        };
        assert!(subscribes(&scoped, EVENT_MEETING_COMPLETED));
        assert!(!subscribes(&scoped, EVENT_NOTE_ENHANCED));
    }

    #[tokio::test]
    async fn dispatch_without_subscribed_endpoints_is_a_no_op() {
        let db = anlg_db_core::Db::connect_memory_plain().await.unwrap();
        anlg_db_app::prepare_schema(&db).await.unwrap();
        anlg_db_app::insert_webhook_endpoint(
            db.pool(),
            "webhook-1",
            "https://example.com",
            "whsec_test",
            "[\"note.enhanced\"]",
        )
        .await
        .unwrap();

        let targeted = dispatch_event(db.pool(), EVENT_MEETING_COMPLETED, "missing-meeting")
            .await
            .unwrap();

        assert_eq!(targeted, 0);
    }

    /// C6.4: a Private meeting is neither sent to webhooks nor written by
    /// the Markdown export automation.
    #[tokio::test]
    async fn private_meetings_are_not_dispatched_or_auto_exported() {
        let db = anlg_db_core::Db::connect_memory_plain().await.unwrap();
        anlg_db_app::prepare_schema(&db).await.unwrap();
        anlg_db_app::insert_webhook_endpoint(
            db.pool(),
            "webhook-1",
            "https://example.com",
            "whsec_test",
            "[]",
        )
        .await
        .unwrap();
        let dir = std::env::temp_dir().join(format!("anlg-private-export-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for (id, value) in [
            ("automation_markdown_export_enabled", "true".to_string()),
            (
                "automation_markdown_export_directory",
                serde_json::Value::String(dir.display().to_string()).to_string(),
            ),
        ] {
            sqlx::query("INSERT OR REPLACE INTO app_settings (id, value_json) VALUES (?, ?)")
                .bind(id)
                .bind(value)
                .execute(db.pool())
                .await
                .unwrap();
        }
        sqlx::query(
            "INSERT INTO sessions (id, title, private, workspace_id)
             SELECT 'secret', 'Secret meeting', 1,
               json_extract(value_json, '$.workspace_id')
             FROM app_settings WHERE id = 'cloudsync_workspace_binding'",
        )
        .execute(db.pool())
        .await
        .unwrap();

        let targeted = dispatch_event(db.pool(), EVENT_MEETING_COMPLETED, "secret")
            .await
            .unwrap();
        assert_eq!(targeted, 0);
        crate::commands::run_markdown_export_automation(db.pool(), "secret").await;
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn endpoint_creation_stops_at_the_fanout_limit() {
        let db = anlg_db_core::Db::connect_memory_plain().await.unwrap();
        anlg_db_app::prepare_schema(&db).await.unwrap();

        for index in 0..MAX_WEBHOOK_ENDPOINTS {
            anlg_db_app::insert_webhook_endpoint(
                db.pool(),
                &format!("webhook-{index}"),
                "https://example.com",
                "whsec_test",
                "[]",
            )
            .await
            .unwrap();
        }

        let error = create_endpoint(db.pool(), "https://example.com", &[])
            .await
            .unwrap_err();

        assert_eq!(
            error,
            format!("at most {MAX_WEBHOOK_ENDPOINTS} webhook endpoints can be configured")
        );
        assert_eq!(
            anlg_db_app::list_webhook_endpoints(db.pool())
                .await
                .unwrap()
                .len(),
            MAX_WEBHOOK_ENDPOINTS
        );
    }

    #[test]
    fn public_addresses_are_allowed() {
        for ip in ["8.8.8.8", "1.1.1.1", "2606:4700:4700::1111"] {
            assert!(is_public_ip(ip.parse().unwrap()), "{ip}");
        }
        for ip in [
            "0.0.0.0",
            "100.100.1.1",
            "198.18.0.1",
            "::",
            "64:ff9b::7f00:1",
        ] {
            assert!(!is_public_ip(ip.parse().unwrap()), "{ip}");
        }
    }

    #[tokio::test]
    async fn endpoint_creation_rejects_bad_urls_and_unknown_events() {
        let db = anlg_db_core::Db::connect_memory_plain().await.unwrap();
        anlg_db_app::prepare_schema(&db).await.unwrap();

        assert_eq!(
            create_endpoint(db.pool(), "example.com", &[])
                .await
                .unwrap_err(),
            "url must start with https://"
        );
        for blocked in [
            "http://example.com/hook",
            "https://localhost/hook",
            "https://127.0.0.1/hook",
            "https://10.0.0.5/hook",
            "https://172.16.4.1/hook",
            "https://192.168.1.10/hook",
            "https://169.254.169.254/latest/meta-data",
            "https://100.64.0.1/hook",
            "https://[::1]/hook",
            "https://[fe80::1]/hook",
            "https://[fd00::1]/hook",
            "https://[::ffff:127.0.0.1]/hook",
            "https://user:pass@8.8.8.8/hook",
        ] {
            assert!(
                create_endpoint(db.pool(), blocked, &[]).await.is_err(),
                "{blocked} should be rejected"
            );
        }
        assert!(
            create_endpoint(db.pool(), "https://example.com", &["nope".to_string()])
                .await
                .unwrap_err()
                .starts_with("unknown event 'nope'")
        );
        assert!(
            anlg_db_app::list_webhook_endpoints(db.pool())
                .await
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn bounded_runner_limits_concurrency_without_dropping_items() {
        let active = Arc::new(AtomicUsize::new(0));
        let completed = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));

        run_bounded(0..100, 4, {
            let active = active.clone();
            let completed = completed.clone();
            let peak = peak.clone();
            move |_| {
                let active = active.clone();
                let completed = completed.clone();
                let peak = peak.clone();
                async move {
                    let current = active.fetch_add(1, Ordering::SeqCst) + 1;
                    peak.fetch_max(current, Ordering::SeqCst);
                    tokio::task::yield_now().await;
                    active.fetch_sub(1, Ordering::SeqCst);
                    completed.fetch_add(1, Ordering::SeqCst);
                }
            }
        })
        .await;

        assert_eq!(completed.load(Ordering::SeqCst), 100);
        assert!(peak.load(Ordering::SeqCst) <= 4);
    }

    #[tokio::test]
    async fn bounded_admission_queues_a_limited_waiting_tier() {
        let active = Arc::new(tokio::sync::Semaphore::new(1));
        let waiting = Arc::new(tokio::sync::Semaphore::new(2));
        let active_guard = active.clone().acquire_owned().await.unwrap();

        let first = tokio::spawn(acquire_with_waiting_tier(
            active.clone(),
            waiting.clone(),
            "busy",
        ));
        tokio::time::timeout(std::time::Duration::from_secs(1), async {
            while waiting.available_permits() != 1 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        let second = tokio::spawn(acquire_with_waiting_tier(
            active.clone(),
            waiting.clone(),
            "busy",
        ));
        tokio::time::timeout(std::time::Duration::from_secs(1), async {
            while waiting.available_permits() != 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();

        assert_eq!(
            acquire_with_waiting_tier(active.clone(), waiting.clone(), "busy")
                .await
                .unwrap_err(),
            "busy"
        );

        drop(active_guard);
        let first_permit = tokio::time::timeout(std::time::Duration::from_secs(1), first)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        drop(first_permit);
        let second_permit = tokio::time::timeout(std::time::Duration::from_secs(1), second)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        drop(second_permit);

        assert_eq!(active.available_permits(), 1);
        assert_eq!(waiting.available_permits(), 2);
    }

    #[tokio::test]
    async fn cancelled_admission_releases_its_waiting_slot() {
        let active = Arc::new(tokio::sync::Semaphore::new(1));
        let waiting = Arc::new(tokio::sync::Semaphore::new(1));
        let active_guard = active.clone().acquire_owned().await.unwrap();
        let waiter = tokio::spawn(acquire_with_waiting_tier(
            active.clone(),
            waiting.clone(),
            "busy",
        ));
        tokio::time::timeout(std::time::Duration::from_secs(1), async {
            while waiting.available_permits() != 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();

        waiter.abort();
        assert!(waiter.await.unwrap_err().is_cancelled());
        assert_eq!(waiting.available_permits(), 1);
        drop(active_guard);
    }
}
