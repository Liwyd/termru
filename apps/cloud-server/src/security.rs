//! Transport hardening: client address resolution and security headers.

use std::net::SocketAddr;

use axum::extract::{ConnectInfo, FromRequestParts};
use axum::http::header::{AUTHORIZATION, HeaderMap, HeaderName, HeaderValue};
use axum::http::request::Parts;

use crate::error::ApiError;
use crate::state::AppState;

/// The address a request came from.
///
/// `X-Forwarded-For` is only honoured when the operator explicitly enabled
/// `TERMRU_TRUST_FORWARDED_FOR`; otherwise a client could spoof the header
/// and slip past the rate limiter.
#[derive(Debug, Clone)]
pub struct ClientIp(pub String);

impl ClientIp {
    /// The address as a string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromRequestParts<AppState> for ClientIp {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        if state.config.trust_forwarded_for
            && let Some(value) = parts.headers.get("x-forwarded-for")
            && let Ok(value) = value.to_str()
        {
            // The left-most entry is the original client.
            if let Some(first) = value.split(',').next() {
                let trimmed = first.trim();
                if !trimmed.is_empty() {
                    return Ok(Self(trimmed.to_string()));
                }
            }
        }

        let address = parts
            .extensions
            .get::<ConnectInfo<SocketAddr>>()
            .map(|info| info.0.ip().to_string());
        Ok(Self(address.unwrap_or_else(|| "unknown".to_string())))
    }
}

/// Extracts the bearer token from the `Authorization` header.
pub fn bearer_token(headers: &HeaderMap) -> Option<&str> {
    let value = headers.get(AUTHORIZATION)?.to_str().ok()?;
    let token = value.strip_prefix("Bearer ")?;
    let token = token.trim();
    if token.is_empty() { None } else { Some(token) }
}

/// Headers added to every response.
///
/// The API serves JSON only, so the policy can be strict: no scripts, no
/// framing, no cross-origin embedding.
pub const SECURITY_HEADERS: &[(&str, &str)] = &[
    ("x-content-type-options", "nosniff"),
    ("x-frame-options", "DENY"),
    ("referrer-policy", "no-referrer"),
    (
        "content-security-policy",
        "default-src 'none'; frame-ancestors 'none'; base-uri 'none'",
    ),
    ("cross-origin-opener-policy", "same-origin"),
    ("cross-origin-resource-policy", "same-origin"),
    (
        "permissions-policy",
        "geolocation=(), microphone=(), camera=()",
    ),
    ("cache-control", "no-store"),
];

/// Adds the [`SECURITY_HEADERS`] to a response.
pub fn apply_security_headers(headers: &mut HeaderMap) {
    for &(name, value) in SECURITY_HEADERS {
        headers.insert(
            HeaderName::from_static(name),
            HeaderValue::from_static(value),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bearer_token_is_extracted() {
        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, HeaderValue::from_static("Bearer abc123"));
        assert_eq!(bearer_token(&headers), Some("abc123"));
    }

    #[test]
    fn missing_or_malformed_authorization_is_rejected() {
        let headers = HeaderMap::new();
        assert_eq!(bearer_token(&headers), None);

        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, HeaderValue::from_static("Basic abc"));
        assert_eq!(bearer_token(&headers), None);

        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, HeaderValue::from_static("Bearer   "));
        assert_eq!(bearer_token(&headers), None);
    }

    #[test]
    fn security_headers_are_valid() {
        let mut headers = HeaderMap::new();
        apply_security_headers(&mut headers);
        assert_eq!(headers.get("x-content-type-options").unwrap(), "nosniff");
        assert_eq!(headers.get("x-frame-options").unwrap(), "DENY");
        assert!(headers.contains_key("content-security-policy"));
    }
}
