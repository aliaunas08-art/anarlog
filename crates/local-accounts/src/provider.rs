//! Google and Microsoft endpoints, scopes and client configuration.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    Google,
    Microsoft,
}

impl Provider {
    pub const ALL: [Provider; 2] = [Provider::Google, Provider::Microsoft];

    pub fn key(self) -> &'static str {
        match self {
            Provider::Google => "google",
            Provider::Microsoft => "microsoft",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        match key {
            "google" => Some(Provider::Google),
            "microsoft" => Some(Provider::Microsoft),
            _ => None,
        }
    }

    /// Base URL the calendar clients prepend to their API paths.
    pub fn api_base_url(self) -> &'static str {
        match self {
            Provider::Google => "https://www.googleapis.com",
            Provider::Microsoft => "https://graph.microsoft.com/v1.0",
        }
    }
}

/// Read-only access: who the user is, and their calendars.
pub(crate) fn scopes(provider: Provider) -> &'static str {
    match provider {
        Provider::Google => {
            "openid email profile https://www.googleapis.com/auth/calendar.readonly"
        }
        Provider::Microsoft => "openid profile email offline_access User.Read Calendars.Read",
    }
}

/// OAuth client registered by the app's publisher with Google or Microsoft.
///
/// Desktop OAuth clients are public: the ID is not a secret, and Google's
/// "secret" for a desktop client is not confidential either (Google says so
/// in its installed-app guide). Neither is compiled into the source: they come
/// from the environment at run time, or at build time for a release build.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientConfig {
    pub client_id: String,
    pub client_secret: Option<String>,
    /// Microsoft tenant: `common` (work, school and personal accounts) by default.
    pub tenant: String,
}

const GOOGLE_CLIENT_ID: &str = "NOTES_GOOGLE_CLIENT_ID";
const GOOGLE_CLIENT_SECRET: &str = "NOTES_GOOGLE_CLIENT_SECRET";
const MICROSOFT_CLIENT_ID: &str = "NOTES_MICROSOFT_CLIENT_ID";
const MICROSOFT_TENANT: &str = "NOTES_MICROSOFT_TENANT";

fn env_value(name: &str, build_time: Option<&'static str>) -> Option<String> {
    std::env::var(name)
        .ok()
        .or_else(|| build_time.map(str::to_string))
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

impl ClientConfig {
    pub fn from_env(provider: Provider) -> Option<Self> {
        match provider {
            Provider::Google => Some(Self {
                client_id: env_value(GOOGLE_CLIENT_ID, option_env!("NOTES_GOOGLE_CLIENT_ID"))?,
                // Google's token endpoint requires the secret for desktop
                // clients even with PKCE, so without it Google isn't offered.
                client_secret: Some(env_value(
                    GOOGLE_CLIENT_SECRET,
                    option_env!("NOTES_GOOGLE_CLIENT_SECRET"),
                )?),
                tenant: String::new(),
            }),
            Provider::Microsoft => Some(Self {
                client_id: env_value(
                    MICROSOFT_CLIENT_ID,
                    option_env!("NOTES_MICROSOFT_CLIENT_ID"),
                )?,
                client_secret: None,
                tenant: env_value(MICROSOFT_TENANT, option_env!("NOTES_MICROSOFT_TENANT"))
                    .filter(|tenant| {
                        tenant
                            .chars()
                            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.')
                    })
                    .unwrap_or_else(|| "common".to_string()),
            }),
        }
    }
}

/// Where the OAuth and identity requests go. Tests point these at a mock server.
#[derive(Debug, Clone)]
pub struct Endpoints {
    pub authorize: String,
    pub token: String,
    pub userinfo: String,
    pub revoke: Option<String>,
    /// Host the browser is sent back to; Microsoft only accepts `localhost`
    /// for a desktop app's loopback redirect, Google prefers `127.0.0.1`.
    pub redirect_host: &'static str,
}

impl Endpoints {
    pub fn for_provider(provider: Provider, config: &ClientConfig) -> Self {
        match provider {
            Provider::Google => Self {
                authorize: "https://accounts.google.com/o/oauth2/v2/auth".into(),
                token: "https://oauth2.googleapis.com/token".into(),
                userinfo: "https://openidconnect.googleapis.com/v1/userinfo".into(),
                revoke: Some("https://oauth2.googleapis.com/revoke".into()),
                redirect_host: "127.0.0.1",
            },
            Provider::Microsoft => {
                let tenant = &config.tenant;
                Self {
                    authorize: format!(
                        "https://login.microsoftonline.com/{tenant}/oauth2/v2.0/authorize"
                    ),
                    token: format!("https://login.microsoftonline.com/{tenant}/oauth2/v2.0/token"),
                    userinfo: "https://graph.microsoft.com/v1.0/me".into(),
                    revoke: None,
                    redirect_host: "localhost",
                }
            }
        }
    }
}

/// The scope without which the account is of no use here. Google lets the
/// user untick it on the consent page.
pub(crate) fn required_scope(provider: Provider) -> &'static str {
    match provider {
        Provider::Google => "https://www.googleapis.com/auth/calendar.readonly",
        Provider::Microsoft => "calendars.read",
    }
}

/// Whether a token response's `scope` grants `required_scope`. Microsoft may
/// return `https://graph.microsoft.com/Calendars.Read`; casing varies. A
/// response without `scope` means the requested scopes were granted.
pub(crate) fn grants_required_scope(provider: Provider, granted: Option<&str>) -> bool {
    let Some(granted) = granted else {
        return true;
    };
    let required = required_scope(provider).to_ascii_lowercase();
    granted.split_whitespace().any(|scope| {
        let scope = scope.to_ascii_lowercase();
        scope == required || scope.ends_with(&format!("/{required}"))
    })
}

/// The identity fields read from Google's userinfo or Microsoft Graph `/me`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Identity {
    pub subject: String,
    pub email: Option<String>,
    pub name: Option<String>,
    pub picture_url: Option<String>,
}

pub(crate) fn parse_identity(provider: Provider, body: &serde_json::Value) -> Option<Identity> {
    let text = |key: &str| {
        body.get(key)
            .and_then(|value| value.as_str())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    };
    match provider {
        Provider::Google => Some(Identity {
            subject: text("sub")?,
            email: text("email"),
            name: text("name"),
            picture_url: text("picture").filter(|url| url.starts_with("https://")),
        }),
        Provider::Microsoft => Some(Identity {
            subject: text("id")?,
            email: text("mail").or_else(|| text("userPrincipalName")),
            name: text("displayName"),
            picture_url: None,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_google_and_microsoft_identities() {
        let google = serde_json::json!({
            "sub": "123", "email": "sam@example.com", "name": "Sam Lee",
            "picture": "https://lh3.googleusercontent.com/a/x"
        });
        let identity = parse_identity(Provider::Google, &google).unwrap();
        assert_eq!(identity.subject, "123");
        assert_eq!(identity.email.as_deref(), Some("sam@example.com"));
        assert_eq!(identity.name.as_deref(), Some("Sam Lee"));

        let microsoft = serde_json::json!({
            "id": "abc", "mail": null, "userPrincipalName": "sam@contoso.com",
            "displayName": "Sam Lee"
        });
        let identity = parse_identity(Provider::Microsoft, &microsoft).unwrap();
        assert_eq!(identity.subject, "abc");
        assert_eq!(identity.email.as_deref(), Some("sam@contoso.com"));

        assert!(parse_identity(Provider::Google, &serde_json::json!({})).is_none());
    }

    #[test]
    fn checks_the_granted_calendar_scope() {
        assert!(grants_required_scope(Provider::Google, None));
        assert!(grants_required_scope(
            Provider::Google,
            Some("openid https://www.googleapis.com/auth/calendar.readonly email")
        ));
        assert!(!grants_required_scope(
            Provider::Google,
            Some("openid email profile")
        ));
        assert!(grants_required_scope(
            Provider::Microsoft,
            Some(
                "https://graph.microsoft.com/Calendars.Read https://graph.microsoft.com/User.Read"
            )
        ));
        assert!(grants_required_scope(
            Provider::Microsoft,
            Some("Calendars.Read User.Read")
        ));
        assert!(!grants_required_scope(
            Provider::Microsoft,
            Some("User.Read openid")
        ));
    }

    #[test]
    fn microsoft_tenant_defaults_to_common_and_rejects_odd_values() {
        let config = ClientConfig {
            client_id: "id".into(),
            client_secret: None,
            tenant: "common".into(),
        };
        let endpoints = Endpoints::for_provider(Provider::Microsoft, &config);
        assert!(
            endpoints
                .authorize
                .contains("/common/oauth2/v2.0/authorize")
        );
        assert_eq!(endpoints.redirect_host, "localhost");
    }
}
