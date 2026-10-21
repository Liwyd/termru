//! Custom extractors.

use axum::body::Bytes;
use axum::extract::{FromRequest, Request};
use serde::de::DeserializeOwned;

use crate::error::ApiError;

/// A JSON body that reports parse failures through the API's own error
/// format instead of the framework's plain-text rejection.
pub struct JsonBody<T>(pub T);

impl<T, S> FromRequest<S> for JsonBody<T>
where
    T: DeserializeOwned,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        let bytes = match Bytes::from_request(req, state).await {
            Ok(bytes) => bytes,
            Err(rejection) => {
                // The body limit must be reported as such, not as a generic
                // "bad request".
                return Err(
                    if rejection.status() == axum::http::StatusCode::PAYLOAD_TOO_LARGE {
                        ApiError::PayloadTooLarge(
                            "the request body exceeds the configured limit".to_string(),
                        )
                    } else {
                        ApiError::BadRequest("the request body could not be read".to_string())
                    },
                );
            }
        };
        let value = serde_json::from_slice::<T>(&bytes).map_err(|e| {
            ApiError::BadRequest(format!("the request body is not valid JSON: {e}"))
        })?;
        Ok(Self(value))
    }
}

/// A query parameter that is present but malformed.
pub fn invalid_query(name: &str) -> ApiError {
    ApiError::BadRequest(format!("the query parameter '{name}' is not valid"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Deserialize)]
    struct Sample {
        value: i32,
    }

    #[test]
    fn the_error_mentions_the_parameter() {
        let error = invalid_query("since");
        assert!(error.to_string().contains("since"));
    }

    #[test]
    fn a_sample_type_deserializes() {
        let value: Sample = serde_json::from_str(r#"{"value": 3}"#).unwrap();
        assert_eq!(value.value, 3);
    }
}
