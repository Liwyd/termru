//! API error type and its HTTP representation.
//!
//! Every failure that reaches a client is one of these variants. Internal
//! errors are logged with their cause but never expose internals to the
//! caller, so a database message can not leak schema details.

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Serialize;

/// Result alias used by every handler.
pub type ApiResult<T> = Result<T, ApiError>;

/// A failure that is safe to report to the client.
#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    /// The request was malformed or failed validation.
    #[error("{0}")]
    BadRequest(String),

    /// No usable credentials were presented.
    #[error("{0}")]
    Unauthorized(String),

    /// The caller is authenticated but not allowed to do this.
    #[error("{0}")]
    Forbidden(String),

    /// The addressed object does not exist (or is not visible to the caller).
    #[error("{0}")]
    NotFound(String),

    /// The request conflicts with the current state.
    #[error("{0}")]
    Conflict(String),

    /// The body exceeded the configured limit.
    #[error("{0}")]
    PayloadTooLarge(String),

    /// The caller exceeded a rate limit.
    #[error("{0}")]
    TooManyRequests(String),

    /// The server is temporarily unable to serve the request.
    #[error("{0}")]
    Unavailable(String),

    /// An unexpected internal failure. The cause is logged, not returned.
    #[error("internal error")]
    Internal(#[source] anyhow::Error),
}

impl ApiError {
    /// Stable, machine-readable error code.
    pub fn code(&self) -> &'static str {
        match self {
            Self::BadRequest(_) => "bad_request",
            Self::Unauthorized(_) => "unauthorized",
            Self::Forbidden(_) => "forbidden",
            Self::NotFound(_) => "not_found",
            Self::Conflict(_) => "conflict",
            Self::PayloadTooLarge(_) => "payload_too_large",
            Self::TooManyRequests(_) => "too_many_requests",
            Self::Unavailable(_) => "service_unavailable",
            Self::Internal(_) => "internal_error",
        }
    }

    /// HTTP status for this error.
    pub fn status(&self) -> StatusCode {
        match self {
            Self::BadRequest(_) => StatusCode::BAD_REQUEST,
            Self::Unauthorized(_) => StatusCode::UNAUTHORIZED,
            Self::Forbidden(_) => StatusCode::FORBIDDEN,
            Self::NotFound(_) => StatusCode::NOT_FOUND,
            Self::Conflict(_) => StatusCode::CONFLICT,
            Self::PayloadTooLarge(_) => StatusCode::PAYLOAD_TOO_LARGE,
            Self::TooManyRequests(_) => StatusCode::TOO_MANY_REQUESTS,
            Self::Unavailable(_) => StatusCode::SERVICE_UNAVAILABLE,
            Self::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    /// The message that is safe to send to the client.
    pub fn public_message(&self) -> String {
        match self {
            Self::Internal(_) => "internal server error".to_string(),
            other => other.to_string(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = self.status();
        if let Self::Internal(cause) = &self {
            // `Debug` on an `anyhow::Error` prints the whole cause chain.
            tracing::error!(error = ?cause, "request failed with an internal error");
        }
        let body = ErrorBody {
            error: ErrorDetail {
                code: self.code(),
                message: self.public_message(),
            },
        };
        (status, Json(body)).into_response()
    }
}

#[derive(Debug, Serialize)]
struct ErrorBody {
    error: ErrorDetail,
}

#[derive(Debug, Serialize)]
struct ErrorDetail {
    code: &'static str,
    message: String,
}

impl From<deadpool_postgres::PoolError> for ApiError {
    fn from(value: deadpool_postgres::PoolError) -> Self {
        Self::Internal(anyhow::Error::new(value))
    }
}

impl From<tokio_postgres::Error> for ApiError {
    fn from(value: tokio_postgres::Error) -> Self {
        Self::Internal(anyhow::Error::new(value))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn internal_errors_do_not_leak_details() {
        let err = ApiError::Internal(anyhow::anyhow!("connection to 10.0.0.5 refused"));
        assert_eq!(err.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(err.public_message(), "internal server error");
        assert!(!err.public_message().contains("10.0.0.5"));
    }

    #[test]
    fn client_errors_keep_their_message() {
        let err = ApiError::Conflict("stale base_rev".to_string());
        assert_eq!(err.status(), StatusCode::CONFLICT);
        assert_eq!(err.public_message(), "stale base_rev");
        assert_eq!(err.code(), "conflict");
    }
}
