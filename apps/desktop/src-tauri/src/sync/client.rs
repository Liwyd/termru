//! Transport to the Termru cloud server.
//!
//! The transport is a trait so that the sync engine can be tested without a
//! network. The production implementation speaks HTTP(S) to the server of
//! `docs/architecture/0005-sync-protocol.md`.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use termru_core::sync::{PullResponse, PushRequest, PushResponse};
use uuid::Uuid;

/// How long a request may take before it is aborted.
const REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// The response of the authentication endpoints.
#[derive(Clone, Serialize, Deserialize)]
pub struct AuthResponse {
    /// The authenticated account.
    pub user_id: Uuid,
    /// The device the tokens belong to.
    pub device_id: Uuid,
    /// The bearer token for API calls.
    pub access_token: String,
    /// The token used to obtain a new pair.
    pub refresh_token: String,
    /// When the access token expires.
    pub access_expires_at: DateTime<Utc>,
    /// When the refresh token expires.
    pub refresh_expires_at: DateTime<Utc>,
}

/// The tokens are secrets: never print them.
impl std::fmt::Debug for AuthResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuthResponse")
            .field("user_id", &self.user_id)
            .field("device_id", &self.device_id)
            .field("access_token", &"[redacted]")
            .field("refresh_token", &"[redacted]")
            .field("access_expires_at", &self.access_expires_at)
            .field("refresh_expires_at", &self.refresh_expires_at)
            .finish()
    }
}

/// Everything that can go wrong while talking to the sync server.
#[derive(Debug, thiserror::Error)]
pub enum SyncError {
    /// The server could not be reached.
    #[error("the sync server could not be reached: {0}")]
    Network(String),

    /// The server answered with an error.
    #[error("the sync server refused the request: {0}")]
    Server(String),

    /// The credentials were rejected.
    #[error("the sync server rejected the credentials: {0}")]
    Unauthorized(String),

    /// The account already exists (the client then signs in instead).
    #[error("{0}")]
    Conflict(String),

    /// The answer could not be understood.
    #[error("the sync server sent an unexpected answer: {0}")]
    Protocol(String),

    /// The configured server URL is not usable.
    #[error("the sync server URL is not valid: {0}")]
    InvalidUrl(String),

    /// The app is not signed in.
    #[error("not signed in to the sync server")]
    NotSignedIn,

    /// The local store refused an operation.
    #[error("{0}")]
    Store(String),
}

impl From<termru_store::StoreError> for SyncError {
    fn from(value: termru_store::StoreError) -> Self {
        Self::Store(value.to_string())
    }
}

/// The operations the sync engine needs from the server.
#[async_trait]
pub trait SyncTransport: Send + Sync {
    /// Creates an account and returns the token pair.
    async fn register(
        &self,
        email: &str,
        password: &str,
        device_id: &str,
        device_name: &str,
    ) -> Result<AuthResponse, SyncError>;

    /// Signs in and returns the token pair.
    async fn sign_in(
        &self,
        email: &str,
        password: &str,
        device_id: &str,
        device_name: &str,
    ) -> Result<AuthResponse, SyncError>;

    /// Exchanges a refresh token for a new token pair.
    async fn refresh(&self, refresh_token: &str) -> Result<AuthResponse, SyncError>;

    /// Fetches the records that changed after `since`.
    async fn pull(
        &self,
        access_token: &str,
        since: i64,
        limit: i64,
    ) -> Result<PullResponse, SyncError>;

    /// Sends a batch of mutations.
    async fn push(
        &self,
        access_token: &str,
        request: PushRequest,
    ) -> Result<PushResponse, SyncError>;
}

/// Checks that a server URL is usable.
///
/// Plain HTTP is only allowed for the loopback interface: a self-hosted
/// server on the internet must be reached over TLS, because the access token
/// travels with every request.
pub fn validate_server_url(url: &str) -> Result<String, SyncError> {
    let trimmed = url.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return Err(SyncError::InvalidUrl("the URL is empty".to_string()));
    }
    let parsed = reqwest::Url::parse(trimmed)
        .map_err(|error| SyncError::InvalidUrl(format!("not a valid URL ({error})")))?;
    match parsed.scheme() {
        "https" => {}
        "http" => {
            let host = parsed.host_str().unwrap_or_default();
            if !matches!(host, "localhost" | "127.0.0.1" | "::1") {
                return Err(SyncError::InvalidUrl(
                    "https is required (plain http is only allowed for localhost)".to_string(),
                ));
            }
        }
        other => {
            return Err(SyncError::InvalidUrl(format!(
                "unsupported URL scheme '{other}'"
            )));
        }
    }
    if parsed.host_str().is_none_or(str::is_empty) {
        return Err(SyncError::InvalidUrl("the URL has no host".to_string()));
    }
    Ok(trimmed.to_string())
}

/// The HTTP implementation of [`SyncTransport`].
pub struct HttpTransport {
    client: reqwest::Client,
    base_url: String,
}

impl HttpTransport {
    /// Creates a transport for the given server URL.
    pub fn new(base_url: &str) -> Result<Self, SyncError> {
        let base_url = validate_server_url(base_url)?;
        let client = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .user_agent(concat!("termru-desktop/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|error| SyncError::Network(error.to_string()))?;
        Ok(Self { client, base_url })
    }

    /// The server URL this transport talks to.
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    async fn send<T: DeserializeOwned>(
        &self,
        method: reqwest::Method,
        path: &str,
        token: Option<&str>,
        body: Option<&impl Serialize>,
    ) -> Result<T, SyncError> {
        let url = format!("{}{path}", self.base_url);
        let mut request = self.client.request(method, &url);
        if let Some(token) = token {
            request = request.bearer_auth(token);
        }
        if let Some(body) = body {
            request = request.json(body);
        }

        let response = request
            .send()
            .await
            .map_err(|error| SyncError::Network(error.to_string()))?;

        let status = response.status();
        if status.is_success() {
            return response
                .json::<T>()
                .await
                .map_err(|error| SyncError::Protocol(error.to_string()));
        }

        let body = response.text().await.unwrap_or_default();
        let message = error_message(&body).unwrap_or_else(|| {
            if body.trim().is_empty() {
                format!("HTTP {}", status.as_u16())
            } else {
                body.trim().to_string()
            }
        });

        match status {
            reqwest::StatusCode::UNAUTHORIZED => Err(SyncError::Unauthorized(message)),
            reqwest::StatusCode::CONFLICT => Err(SyncError::Conflict(message)),
            _ => Err(SyncError::Server(message)),
        }
    }
}

/// Extracts the message of the server's error body.
fn error_message(body: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(body).ok()?;
    value
        .get("error")?
        .get("message")?
        .as_str()
        .map(str::to_string)
}

/// The body of the sign-in request.
#[derive(Serialize)]
struct Credentials<'a> {
    email: &'a str,
    password: &'a str,
    device_id: &'a str,
    device_name: &'a str,
}

/// The body of the refresh request.
#[derive(Serialize)]
struct RefreshBody<'a> {
    refresh_token: &'a str,
}

#[async_trait]
impl SyncTransport for HttpTransport {
    async fn register(
        &self,
        email: &str,
        password: &str,
        device_id: &str,
        device_name: &str,
    ) -> Result<AuthResponse, SyncError> {
        let body = Credentials {
            email,
            password,
            device_id,
            device_name,
        };
        self.send(
            reqwest::Method::POST,
            "/v1/auth/register",
            None,
            Some(&body),
        )
        .await
    }

    async fn sign_in(
        &self,
        email: &str,
        password: &str,
        device_id: &str,
        device_name: &str,
    ) -> Result<AuthResponse, SyncError> {
        let body = Credentials {
            email,
            password,
            device_id,
            device_name,
        };
        self.send(reqwest::Method::POST, "/v1/auth/login", None, Some(&body))
            .await
    }

    async fn refresh(&self, refresh_token: &str) -> Result<AuthResponse, SyncError> {
        let body = RefreshBody { refresh_token };
        self.send(reqwest::Method::POST, "/v1/auth/refresh", None, Some(&body))
            .await
    }

    async fn pull(
        &self,
        access_token: &str,
        since: i64,
        limit: i64,
    ) -> Result<PullResponse, SyncError> {
        let path = format!("/v1/sync/changes?since={since}&limit={limit}");
        self.send(reqwest::Method::GET, &path, Some(access_token), None::<&()>)
            .await
    }

    async fn push(
        &self,
        access_token: &str,
        request: PushRequest,
    ) -> Result<PushResponse, SyncError> {
        self.send(
            reqwest::Method::POST,
            "/v1/sync/changes",
            Some(access_token),
            Some(&request),
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn https_urls_are_accepted() {
        assert_eq!(
            validate_server_url("https://sync.example.com/").expect("valid"),
            "https://sync.example.com"
        );
        assert_eq!(
            validate_server_url("  https://sync.example.com  ").expect("valid"),
            "https://sync.example.com"
        );
    }

    #[test]
    fn plain_http_is_only_allowed_for_the_loopback_interface() {
        assert!(validate_server_url("http://localhost:8787").is_ok());
        assert!(validate_server_url("http://127.0.0.1:8787").is_ok());
        let error = validate_server_url("http://sync.example.com").expect_err("must be refused");
        assert!(error.to_string().contains("https"));
    }

    #[test]
    fn broken_urls_are_refused() {
        assert!(validate_server_url("").is_err());
        assert!(validate_server_url("not a url").is_err());
        assert!(validate_server_url("ftp://sync.example.com").is_err());
        assert!(validate_server_url("https://").is_err());
    }

    #[test]
    fn the_error_message_of_the_server_is_extracted() {
        let body = r#"{"error":{"code":"conflict","message":"stale base_rev"}}"#;
        assert_eq!(error_message(body), Some("stale base_rev".to_string()));
        assert_eq!(error_message("not json"), None);
        assert_eq!(error_message(r#"{"error":{}}"#), None);
    }

    #[test]
    fn the_token_debug_output_is_redacted() {
        let response = AuthResponse {
            user_id: Uuid::nil(),
            device_id: Uuid::nil(),
            access_token: "access-secret".to_string(),
            refresh_token: "refresh-secret".to_string(),
            access_expires_at: Utc::now(),
            refresh_expires_at: Utc::now(),
        };
        let debug = format!("{response:?}");
        assert!(!debug.contains("access-secret"));
        assert!(!debug.contains("refresh-secret"));
        assert!(debug.contains("[redacted]"));
    }
}
