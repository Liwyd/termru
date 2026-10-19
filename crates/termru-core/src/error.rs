//! Error types shared across the core crate.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum CoreError {
    #[error("validation failed: {0}")]
    Validation(String),

    #[error("serialization failed: {0}")]
    Serialization(String),

    #[error("not found: {0}")]
    NotFound(String),
}

impl CoreError {
    pub fn validation(msg: impl Into<String>) -> Self {
        Self::Validation(msg.into())
    }
}
