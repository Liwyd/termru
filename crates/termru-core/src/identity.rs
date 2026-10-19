//! Logical SSH identities.
//!
//! An identity is the *logical* reference a host uses for key authentication.
//! The device-local binding (private key path) lives in the local store and is
//! intentionally not synced: key paths differ between machines, and syncing
//! them would break other devices.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::CoreError;
use crate::ids::IdentityId;
use crate::validate;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct Identity {
    pub id: IdentityId,
    pub name: String,
    /// Free-form comment (e.g. "work laptop key"). Never contains key material.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    /// SHA256 fingerprint of the public key, when known. Public keys are not
    /// secret; syncing the fingerprint lets another device show which key the
    /// identity refers to even before a local path is bound.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fingerprint: Option<String>,
    #[serde(default)]
    pub created_at: DateTime<Utc>,
    #[serde(default)]
    pub updated_at: DateTime<Utc>,
    #[serde(default)]
    pub deleted: bool,
    #[serde(skip)]
    pub server_rev: Option<i64>,
    #[serde(skip)]
    pub dirty: bool,
}

impl Identity {
    pub fn new(name: String) -> Self {
        let now = Utc::now();
        Self {
            id: IdentityId::new(),
            name,
            comment: None,
            fingerprint: None,
            created_at: now,
            updated_at: now,
            deleted: false,
            server_rev: None,
            dirty: false,
        }
    }

    pub fn validate(&self) -> Result<(), CoreError> {
        validate::validate_display_name("identity name", &self.name, validate::MAX_NAME_LEN)?;
        if let Some(comment) = &self.comment {
            if comment.chars().count() > validate::MAX_DESCRIPTION_LEN {
                return Err(CoreError::validation(format!(
                    "identity comment must be at most {} characters",
                    validate::MAX_DESCRIPTION_LEN
                )));
            }
            if comment
                .chars()
                .any(|c| c == '\0' || (c.is_control() && c != '\n'))
            {
                return Err(CoreError::validation(
                    "identity comment must not contain control characters",
                ));
            }
        }
        if let Some(fp) = &self.fingerprint {
            if !fp.starts_with("SHA256:") || fp.len() > 64 || fp.chars().any(char::is_control) {
                return Err(CoreError::validation("identity fingerprint is malformed"));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_validates() {
        let mut id = Identity::new("default key".to_string());
        id.validate().unwrap();
        id.fingerprint = Some("SHA256:abc123".to_string());
        id.validate().unwrap();
        id.fingerprint = Some("not-a-fingerprint".to_string());
        assert!(id.validate().is_err());
    }
}
