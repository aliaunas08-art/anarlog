//! Google and Outlook calendars read straight from the provider with the
//! user's own access token (accounts connected on the Mac), instead of through
//! the app server.

use std::time::Duration;

use anlg_calendar_interface::{CalendarEvent, CalendarListItem, CalendarProviderType, EventFilter};
use anlg_google_calendar::{EventOrderBy, EventType, GoogleCalendarClient};
use anlg_outlook_calendar::OutlookCalendarClient;

use crate::{Error, convert};

/// Connection ids of the form `local:<account id>` are accounts connected on
/// this Mac; any other id is a server-side connection.
pub const LOCAL_CONNECTION_PREFIX: &str = "local:";

pub fn local_account_id(connection_id: &str) -> Option<&str> {
    connection_id
        .strip_prefix(LOCAL_CONNECTION_PREFIX)
        .filter(|id| !id.is_empty())
}

const GOOGLE_API: &str = "https://www.googleapis.com";
const GRAPH_API: &str = "https://graph.microsoft.com/v1.0";
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
/// Google pages; stop well past any real calendar to avoid looping.
const MAX_GOOGLE_PAGES: usize = 100;

#[derive(Debug, thiserror::Error)]
#[error("provider returned {status}")]
struct StatusError {
    status: u16,
}

struct BearerHttpClient {
    base_url: String,
    http: reqwest::Client,
}

impl BearerHttpClient {
    fn new(base_url: &str, access_token: &str) -> Result<Self, Error> {
        let mut headers = reqwest::header::HeaderMap::new();
        let mut auth: reqwest::header::HeaderValue = format!("Bearer {access_token}").parse()?;
        auth.set_sensitive(true);
        headers.insert(reqwest::header::AUTHORIZATION, auth);
        let http = reqwest::Client::builder()
            .default_headers(headers)
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(REQUEST_TIMEOUT)
            .build()?;
        Ok(Self {
            base_url: base_url.to_string(),
            http,
        })
    }

    async fn send(
        &self,
        request: reqwest::RequestBuilder,
    ) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
        let response = request.send().await.map_err(|e| e.without_url())?;
        let status = response.status();
        if !status.is_success() {
            return Err(Box::new(StatusError {
                status: status.as_u16(),
            }));
        }
        Ok(response.bytes().await?.to_vec())
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }
}

impl anlg_http::HttpClient for BearerHttpClient {
    async fn get(&self, path: &str) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
        self.send(self.http.get(self.url(path))).await
    }

    async fn post(
        &self,
        path: &str,
        body: Vec<u8>,
        content_type: &str,
    ) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
        let request = self
            .http
            .post(self.url(path))
            .header(reqwest::header::CONTENT_TYPE, content_type)
            .body(body);
        self.send(request).await
    }

    async fn put(
        &self,
        path: &str,
        body: Vec<u8>,
    ) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
        self.send(self.http.put(self.url(path)).body(body)).await
    }

    async fn patch(
        &self,
        path: &str,
        body: Vec<u8>,
    ) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
        self.send(self.http.patch(self.url(path)).body(body)).await
    }

    async fn delete(
        &self,
        path: &str,
    ) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
        self.send(self.http.delete(self.url(path))).await
    }
}

fn provider_error(error: impl std::fmt::Display, source: Option<&StatusError>) -> Error {
    match source {
        Some(StatusError { status: 401 }) => Error::Unauthorized,
        _ => Error::Api(error.to_string()),
    }
}

fn google_error(error: anlg_google_calendar::Error) -> Error {
    let status = match &error {
        anlg_google_calendar::Error::Http(inner) => inner.downcast_ref::<StatusError>(),
        _ => None,
    };
    provider_error(&error, status)
}

fn outlook_error(error: anlg_outlook_calendar::Error) -> Error {
    let status = match &error {
        anlg_outlook_calendar::Error::Http(inner) => inner.downcast_ref::<StatusError>(),
        _ => None,
    };
    provider_error(&error, status)
}

pub async fn list_calendars(
    provider: CalendarProviderType,
    access_token: &str,
) -> Result<Vec<CalendarListItem>, Error> {
    list_calendars_at(provider, access_token, None).await
}

pub async fn list_events(
    provider: CalendarProviderType,
    access_token: &str,
    filter: EventFilter,
) -> Result<Vec<CalendarEvent>, Error> {
    list_events_at(provider, access_token, filter, None).await
}

fn base_url(provider: CalendarProviderType, override_url: Option<&str>) -> Result<String, Error> {
    if let Some(url) = override_url {
        return Ok(url.to_string());
    }
    match provider {
        CalendarProviderType::Google => Ok(GOOGLE_API.to_string()),
        CalendarProviderType::Outlook => Ok(GRAPH_API.to_string()),
        CalendarProviderType::Apple => Err(Error::UnsupportedOperation {
            operation: "direct_access",
            provider,
        }),
    }
}

async fn list_calendars_at(
    provider: CalendarProviderType,
    access_token: &str,
    override_url: Option<&str>,
) -> Result<Vec<CalendarListItem>, Error> {
    let http = BearerHttpClient::new(&base_url(provider, override_url)?, access_token)?;
    match provider {
        CalendarProviderType::Google => {
            let response = GoogleCalendarClient::new(http)
                .list_calendars()
                .await
                .map_err(google_error)?;
            Ok(convert::convert_google_calendars(response.items))
        }
        CalendarProviderType::Outlook => {
            let response = OutlookCalendarClient::new(http)
                .list_calendars()
                .await
                .map_err(outlook_error)?;
            Ok(convert::convert_outlook_calendars(response.value))
        }
        CalendarProviderType::Apple => unreachable!("base_url rejects Apple"),
    }
}

async fn list_events_at(
    provider: CalendarProviderType,
    access_token: &str,
    filter: EventFilter,
    override_url: Option<&str>,
) -> Result<Vec<CalendarEvent>, Error> {
    let http = BearerHttpClient::new(&base_url(provider, override_url)?, access_token)?;
    let calendar_id = filter.calendar_tracking_id.clone();
    match provider {
        CalendarProviderType::Google => {
            let client = GoogleCalendarClient::new(http);
            // The calendar id goes into the URL path.
            let encoded_id = urlencoding::encode(&calendar_id).into_owned();
            let mut request = anlg_google_calendar::ListEventsRequest {
                calendar_id: encoded_id,
                time_min: Some(filter.from),
                time_max: Some(filter.to),
                single_events: Some(true),
                order_by: Some(EventOrderBy::StartTime),
                event_types: Some(vec![EventType::Default]),
                ..Default::default()
            };
            let mut events = Vec::new();
            let mut seen = std::collections::HashSet::new();
            for _ in 0..MAX_GOOGLE_PAGES {
                let page = client
                    .list_events(request.clone())
                    .await
                    .map_err(google_error)?;
                events.extend(page.items);
                let Some(token) = page.next_page_token.filter(|t| !t.is_empty()) else {
                    return Ok(convert::convert_google_events(events, &calendar_id));
                };
                if !seen.insert(token.clone()) {
                    return Err(Error::Api(
                        "google events.list returned a repeated page token".into(),
                    ));
                }
                request.page_token = Some(token);
            }
            Err(Error::Api(
                "google events.list returned too many pages".into(),
            ))
        }
        CalendarProviderType::Outlook => {
            // Raw, as the server sends it: Graph's next-page links repeat the
            // id unencoded and the client checks they stay on this calendar.
            let request = anlg_outlook_calendar::ListEventsRequest {
                calendar_id: calendar_id.clone(),
                start_date_time: Some(filter.from),
                end_date_time: Some(filter.to),
                order_by: Some("start/dateTime".to_string()),
                ..Default::default()
            };
            let response = OutlookCalendarClient::new(http)
                .list_events(request)
                .await
                .map_err(outlook_error)?;
            Ok(convert::convert_outlook_events(
                response.value,
                &calendar_id,
            ))
        }
        CalendarProviderType::Apple => unreachable!("base_url rejects Apple"),
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;
    use wiremock::matchers::{header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;

    fn filter(calendar: &str) -> EventFilter {
        EventFilter {
            from: chrono::Utc.with_ymd_and_hms(2026, 11, 16, 0, 0, 0).unwrap(),
            to: chrono::Utc.with_ymd_and_hms(2026, 11, 17, 0, 0, 0).unwrap(),
            calendar_tracking_id: calendar.to_string(),
        }
    }

    #[test]
    fn recognises_local_connection_ids() {
        assert_eq!(local_account_id("local:google-123"), Some("google-123"));
        assert_eq!(local_account_id("local:"), None);
        assert_eq!(local_account_id("conn_abc"), None);
    }

    #[tokio::test]
    async fn reads_google_events_with_the_users_token_across_pages() {
        let server = MockServer::start().await;
        let event = |id: &str| {
            serde_json::json!({
                "kind": "calendar#event", "id": id, "status": "confirmed",
                "summary": "Weekly sync",
                "start": {"dateTime": "2026-11-16T15:00:00Z"},
                "end": {"dateTime": "2026-11-16T15:30:00Z"}
            })
        };
        Mock::given(method("GET"))
            .and(path("/calendar/v3/calendars/sam%40example.com/events"))
            .and(header("authorization", "Bearer user-token"))
            .and(query_param("pageToken", "p2"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"items": [event("b")]})),
            )
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/calendar/v3/calendars/sam%40example.com/events"))
            .and(header("authorization", "Bearer user-token"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(
                    serde_json::json!({"items": [event("a")], "nextPageToken": "p2"}),
                ),
            )
            .mount(&server)
            .await;

        let events = list_events_at(
            CalendarProviderType::Google,
            "user-token",
            filter("sam@example.com"),
            Some(&server.uri()),
        )
        .await
        .unwrap();
        assert_eq!(events.len(), 2);
        assert!(events.iter().all(|e| e.calendar_id == "sam@example.com"));
    }

    #[tokio::test]
    async fn an_expired_token_is_reported_as_unauthorized() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;
        let error = list_calendars_at(CalendarProviderType::Outlook, "old", Some(&server.uri()))
            .await
            .unwrap_err();
        assert!(matches!(error, Error::Unauthorized));
    }
}
