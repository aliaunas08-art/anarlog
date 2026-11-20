use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tokio::sync::oneshot;

use crate::Error;
use crate::loopback::{Callback, Loopback};
use crate::pkce::{new_pkce, new_state};
use crate::provider::{
    ClientConfig, Endpoints, Identity, Provider, grants_required_scope, parse_identity, scopes,
};
use crate::store::{INDEX_KEY, SecretStore, last_connected_key, refresh_key};

const SIGN_IN_TIMEOUT: Duration = Duration::from_secs(5 * 60);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
/// Refresh this long before the provider's stated expiry.
const EXPIRY_MARGIN: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct Account {
    /// `google-<subject>` or `microsoft-<object id>`.
    pub id: String,
    pub provider: Provider,
    pub email: Option<String>,
    pub name: Option<String>,
    pub picture_url: Option<String>,
    /// RFC 3339.
    pub connected_at: String,
    /// The provider refused the stored refresh token (password change,
    /// revoked access): the user has to connect again.
    #[serde(default)]
    pub needs_reconnect: bool,
    /// Microsoft only: a personal Microsoft account or a work or school
    /// (Entra) account. One email can have both, and Microsoft's sign-in page
    /// can pick either, so the app shows which one is connected.
    #[serde(default)]
    pub kind: Option<AccountKind>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "snake_case")]
pub enum AccountKind {
    Personal,
    WorkOrSchool,
}

/// The tenant every personal Microsoft account signs in through.
const MICROSOFT_PERSONAL_TENANT: &str = "9188040d-6c67-4c5b-b112-36a304b66dad";

/// Reads the `tid` claim of the ID token Microsoft's token endpoint returned
/// over TLS (OpenID Connect Core 3.1.3.7 allows TLS in place of the signature
/// check for a token received directly from the token endpoint). Only used
/// to label the account, never to grant access.
fn microsoft_account_kind(id_token: Option<&str>) -> Option<AccountKind> {
    use base64::Engine as _;
    let payload = id_token?.split('.').nth(1)?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload.trim_end_matches('='))
        .ok()?;
    let claims: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    let tid = claims.get("tid")?.as_str()?;
    Some(if tid.eq_ignore_ascii_case(MICROSOFT_PERSONAL_TENANT) {
        AccountKind::Personal
    } else {
        AccountKind::WorkOrSchool
    })
}

/// The account a sign-in replaced, when it looks like the user meant to sign
/// in to the same account again but got a different one: the last account
/// connected for this provider has another id, and either it has the same
/// email or no other account of this provider was connected (a reconnect,
/// not "add another account").
pub fn replaced_account(
    previous: Option<&Account>,
    connected: &Account,
    connected_before: &[Account],
) -> Option<Account> {
    let previous = previous?;
    if previous.id == connected.id || previous.provider != connected.provider {
        return None;
    }
    let same_email = matches!(
        (&previous.email, &connected.email),
        (Some(a), Some(b)) if a.eq_ignore_ascii_case(b)
    );
    let reconnect = !connected_before
        .iter()
        .any(|account| account.provider == connected.provider && account.id != connected.id);
    (same_email || reconnect).then(|| previous.clone())
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    expires_in: Option<u64>,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    scope: Option<String>,
    #[serde(default)]
    id_token: Option<String>,
}

#[derive(Deserialize)]
struct TokenError {
    error: String,
    #[serde(default)]
    error_description: Option<String>,
}

struct CachedToken {
    value: String,
    expires_at: Instant,
}

type Overrides = HashMap<Provider, (ClientConfig, Endpoints)>;

pub struct AccountManager<S: SecretStore> {
    store: S,
    http: reqwest::Client,
    overrides: Option<Overrides>,
    tokens: Mutex<HashMap<String, CachedToken>>,
    /// One refresh at a time, so two callers don't both spend a rotating
    /// refresh token.
    refreshing: tokio::sync::Mutex<()>,
    /// The account list, read from the Keychain once and written through on
    /// every change (each Keychain read can cost a macOS prompt).
    index: Mutex<Option<Vec<Account>>>,
    /// The sign-in in progress, by attempt number; starting another cancels it.
    pending: Mutex<Option<(u64, oneshot::Sender<()>)>>,
    attempts: std::sync::atomic::AtomicU64,
}

impl<S: SecretStore> AccountManager<S> {
    pub fn new(store: S) -> Self {
        Self::build(store, None)
    }

    /// Fixed client settings and endpoints instead of the environment and the
    /// real providers. For tests.
    pub fn with_overrides(store: S, overrides: Overrides) -> Self {
        Self::build(store, Some(overrides))
    }

    fn build(store: S, overrides: Option<Overrides>) -> Self {
        let http = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .build()
            .expect("reqwest client");
        Self {
            store,
            http,
            overrides,
            tokens: Mutex::new(HashMap::new()),
            refreshing: tokio::sync::Mutex::new(()),
            index: Mutex::new(None),
            pending: Mutex::new(None),
            attempts: std::sync::atomic::AtomicU64::new(0),
        }
    }

    fn settings(&self, provider: Provider) -> Result<(ClientConfig, Endpoints), Error> {
        if let Some(overrides) = &self.overrides {
            return overrides
                .get(&provider)
                .cloned()
                .ok_or(Error::NotConfigured(provider.key()));
        }
        let config =
            ClientConfig::from_env(provider).ok_or(Error::NotConfigured(provider.key()))?;
        let endpoints = Endpoints::for_provider(provider, &config);
        Ok((config, endpoints))
    }

    /// Providers whose sign-in is set up in this build.
    pub fn configured_providers(&self) -> Vec<Provider> {
        Provider::ALL
            .into_iter()
            .filter(|provider| self.settings(*provider).is_ok())
            .collect()
    }

    pub fn list(&self) -> Result<Vec<Account>, Error> {
        let mut index = self.index.lock().unwrap();
        if index.is_none() {
            *index = Some(self.read_index()?);
        }
        Ok(index.clone().unwrap_or_default())
    }

    pub fn get(&self, account_id: &str) -> Result<Account, Error> {
        self.list()?
            .into_iter()
            .find(|account| account.id == account_id)
            .ok_or_else(|| Error::UnknownAccount(account_id.to_string()))
    }

    fn read_index(&self) -> Result<Vec<Account>, Error> {
        let Some(raw) = self.store.get(INDEX_KEY)? else {
            return Ok(Vec::new());
        };
        // A damaged list shouldn't lock the user out of connecting again.
        Ok(serde_json::from_str(&raw).unwrap_or_else(|error| {
            tracing::warn!(%error, "local_accounts_index_unreadable");
            Vec::new()
        }))
    }

    fn write_index(&self, accounts: &[Account]) -> Result<(), Error> {
        let raw = serde_json::to_string(accounts).map_err(|e| Error::Keychain(e.to_string()))?;
        self.store.set(INDEX_KEY, &raw)
    }

    fn update_index(&self, edit: impl FnOnce(&mut Vec<Account>)) -> Result<(), Error> {
        let mut index = self.index.lock().unwrap();
        let mut accounts = match index.take() {
            Some(accounts) => accounts,
            None => self.read_index()?,
        };
        edit(&mut accounts);
        let written = self.write_index(&accounts);
        // Keep memory in step with the Keychain: on a failed write, re-read next time.
        *index = written.is_ok().then_some(accounts);
        written
    }

    /// Cancels a sign-in that is waiting for the browser.
    pub fn cancel_connect(&self) {
        self.pending.lock().unwrap().take();
    }

    /// Runs the browser sign-in and stores the account. `open_browser` gets the
    /// consent page URL.
    pub async fn connect(
        &self,
        provider: Provider,
        open_browser: impl FnOnce(&str) -> Result<(), String>,
    ) -> Result<Account, Error> {
        let (config, endpoints) = self.settings(provider)?;
        let loopback = Loopback::bind().await?;
        let redirect_uri = format!("http://{}:{}", endpoints.redirect_host, loopback.port());
        let pkce = new_pkce();
        let state = new_state();

        let mut authorize =
            url::Url::parse(&endpoints.authorize).map_err(|e| Error::Provider(e.to_string()))?;
        {
            let mut query = authorize.query_pairs_mut();
            query
                .append_pair("client_id", &config.client_id)
                .append_pair("redirect_uri", &redirect_uri)
                .append_pair("response_type", "code")
                .append_pair("scope", scopes(provider))
                .append_pair("state", &state)
                .append_pair("code_challenge", &pkce.challenge)
                .append_pair("code_challenge_method", "S256");
            match provider {
                Provider::Google => {
                    // Offline access plus consent is what makes Google
                    // return a refresh token every time.
                    query
                        .append_pair("access_type", "offline")
                        .append_pair("prompt", "consent");
                }
                Provider::Microsoft => {
                    query.append_pair("prompt", "select_account");
                }
            }
        }

        let (cancel_tx, cancel_rx) = oneshot::channel();
        let attempt = self
            .attempts
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        // Replacing the sender drops the previous one, which cancels it.
        *self.pending.lock().unwrap() = Some((attempt, cancel_tx));

        open_browser(authorize.as_str()).map_err(Error::Browser)?;

        let callback = tokio::select! {
            callback = loopback.wait(&state) => callback?,
            _ = tokio::time::sleep(SIGN_IN_TIMEOUT) => return Err(Error::TimedOut),
            _ = cancel_rx => return Err(Error::Cancelled),
        };
        let code = match callback {
            Callback::Code(code) => code,
            Callback::Denied(reason) => return Err(Error::Denied(reason)),
        };

        let mut form = vec![
            ("grant_type", "authorization_code".to_string()),
            ("code", code),
            ("redirect_uri", redirect_uri),
            ("client_id", config.client_id.clone()),
            ("code_verifier", pkce.verifier),
        ];
        if let Some(secret) = &config.client_secret {
            form.push(("client_secret", secret.clone()));
        }
        if provider == Provider::Microsoft {
            form.push(("scope", scopes(provider).to_string()));
        }
        let tokens = self.token_request(&endpoints.token, &form).await?;
        if !grants_required_scope(provider, tokens.scope.as_deref()) {
            return Err(Error::MissingScope);
        }
        let refresh_token = tokens.refresh_token.clone().ok_or(Error::NoRefreshToken)?;
        let identity = self
            .fetch_identity(provider, &endpoints.userinfo, &tokens.access_token)
            .await?;

        let account = Account {
            id: format!("{}-{}", provider.key(), sanitize(&identity.subject)),
            provider,
            email: identity.email,
            name: identity.name,
            picture_url: identity.picture_url,
            connected_at: chrono::Utc::now().to_rfc3339(),
            needs_reconnect: false,
            kind: match provider {
                Provider::Microsoft => microsoft_account_kind(tokens.id_token.as_deref()),
                Provider::Google => None,
            },
        };
        self.store.set(&refresh_key(&account.id), &refresh_token)?;
        self.update_index(|accounts| {
            accounts.retain(|existing| existing.id != account.id);
            accounts.push(account.clone());
        })?;
        self.cache(&account.id, &tokens);
        {
            let mut pending = self.pending.lock().unwrap();
            // Only clear this attempt: a newer sign-in may be waiting.
            if pending.as_ref().is_some_and(|(id, _)| *id == attempt) {
                pending.take();
            }
        }
        // Remembered after a disconnect too, so a reconnect that lands on a
        // different account can be pointed out. Not a secret.
        if let Ok(raw) = serde_json::to_string(&account)
            && let Err(error) = self.store.set(&last_connected_key(provider), &raw)
        {
            tracing::warn!(%error, "local_account_last_connected_save_failed");
        }
        // The account id is the provider's opaque id, never the email.
        tracing::info!(
            provider = provider.key(),
            account_id = %account.id,
            kind = ?account.kind,
            "local_account_connected"
        );
        Ok(account)
    }

    /// The last account connected for this provider on this Mac, kept after
    /// it is disconnected.
    pub fn last_connected(&self, provider: Provider) -> Option<Account> {
        let raw = self.store.get(&last_connected_key(provider)).ok()??;
        serde_json::from_str(&raw).ok()
    }

    /// A current access token for the account, refreshing it when needed.
    pub async fn access_token(&self, account_id: &str) -> Result<String, Error> {
        if let Some(token) = self.cached(account_id) {
            return Ok(token);
        }
        let _refreshing = self.refreshing.lock().await;
        // Another caller may have refreshed while this one waited.
        if let Some(token) = self.cached(account_id) {
            return Ok(token);
        }

        let account = self.get(account_id)?;
        if account.needs_reconnect {
            return Err(Error::ReconnectRequired);
        }
        let (config, endpoints) = self.settings(account.provider)?;
        let refresh_token = self
            .store
            .get(&refresh_key(account_id))?
            .ok_or(Error::ReconnectRequired)?;

        let mut form = vec![
            ("grant_type", "refresh_token".to_string()),
            ("refresh_token", refresh_token),
            ("client_id", config.client_id.clone()),
        ];
        if let Some(secret) = &config.client_secret {
            form.push(("client_secret", secret.clone()));
        }
        if account.provider == Provider::Microsoft {
            form.push(("scope", scopes(account.provider).to_string()));
        }
        let tokens = match self.token_request(&endpoints.token, &form).await {
            Ok(tokens) => tokens,
            // `invalid_grant`: revoked or expired. Microsoft's
            // `interaction_required`: MFA or a sign-in policy needs the user.
            Err(Error::Provider(message))
                if message.starts_with("invalid_grant")
                    || message.starts_with("interaction_required") =>
            {
                self.update_index(|accounts| {
                    if let Some(account) = accounts.iter_mut().find(|a| a.id == account_id) {
                        account.needs_reconnect = true;
                    }
                })?;
                tracing::warn!(
                    provider = account.provider.key(),
                    "local_account_refresh_rejected"
                );
                return Err(Error::ReconnectRequired);
            }
            Err(error) => return Err(error),
        };
        // Microsoft rotates refresh tokens; keep the newest.
        if let Some(rotated) = &tokens.refresh_token {
            self.store.set(&refresh_key(account_id), rotated)?;
        }
        self.cache(account_id, &tokens);
        Ok(tokens.access_token)
    }

    /// Drops the token after the provider rejected it, so the next call refreshes.
    pub fn forget_access_token(&self, account_id: &str) {
        self.tokens.lock().unwrap().remove(account_id);
    }

    /// Revokes access where the provider supports it and removes the account
    /// and its token from the Keychain.
    pub async fn disconnect(&self, account_id: &str) -> Result<(), Error> {
        // A refresh in flight could write a rotated token back after the delete.
        let _refreshing = self.refreshing.lock().await;
        let account = self.get(account_id)?;
        if let Ok((_, endpoints)) = self.settings(account.provider)
            && let Some(revoke) = endpoints.revoke
            && let Ok(Some(token)) = self.store.get(&refresh_key(account_id))
        {
            // Best effort: the account is removed here either way.
            let result = self
                .http
                .post(&revoke)
                .form(&[("token", token.as_str())])
                .send()
                .await;
            if let Err(error) = result {
                tracing::warn!(error = %error.without_url(), "local_account_revoke_failed");
            }
        }
        self.store.delete(&refresh_key(account_id))?;
        self.update_index(|accounts| accounts.retain(|a| a.id != account_id))?;
        self.forget_access_token(account_id);
        tracing::info!(
            provider = account.provider.key(),
            "local_account_disconnected"
        );
        Ok(())
    }

    fn cached(&self, account_id: &str) -> Option<String> {
        let tokens = self.tokens.lock().unwrap();
        let token = tokens.get(account_id)?;
        (token.expires_at > Instant::now() + EXPIRY_MARGIN).then(|| token.value.clone())
    }

    fn cache(&self, account_id: &str, tokens: &TokenResponse) {
        let lifetime = Duration::from_secs(tokens.expires_in.unwrap_or(3_600));
        self.tokens.lock().unwrap().insert(
            account_id.to_string(),
            CachedToken {
                value: tokens.access_token.clone(),
                expires_at: Instant::now() + lifetime,
            },
        );
    }

    async fn token_request(
        &self,
        url: &str,
        form: &[(&str, String)],
    ) -> Result<TokenResponse, Error> {
        let response = self.http.post(url).form(form).send().await?;
        let status = response.status();
        let body = response.bytes().await?;
        if status.is_success() {
            return serde_json::from_slice(&body)
                .map_err(|_| Error::Provider("unexpected token response".into()));
        }
        // Only the provider's error code and description are kept: the body
        // of a failed token request never contains a token.
        Err(match serde_json::from_slice::<TokenError>(&body) {
            Ok(error) => Error::Provider(match error.error_description {
                Some(description) => format!("{}: {}", error.error, first_line(&description)),
                None => error.error,
            }),
            Err(_) => Error::Provider(format!("token request failed ({status})")),
        })
    }

    async fn fetch_identity(
        &self,
        provider: Provider,
        url: &str,
        access_token: &str,
    ) -> Result<Identity, Error> {
        let response = self.http.get(url).bearer_auth(access_token).send().await?;
        if !response.status().is_success() {
            return Err(Error::Provider(format!(
                "identity request failed ({})",
                response.status()
            )));
        }
        let body: serde_json::Value = response.json().await?;
        parse_identity(provider, &body).ok_or(Error::NoIdentity)
    }
}

fn first_line(text: &str) -> String {
    text.lines()
        .next()
        .unwrap_or_default()
        .chars()
        .take(200)
        .collect()
}

/// Account ids go into keychain item names and connection ids.
fn sanitize(subject: &str) -> String {
    subject
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .take(128)
        .collect()
}

#[cfg(test)]
mod tests;
