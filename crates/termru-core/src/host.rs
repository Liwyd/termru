//! Host (connection profile) model.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::CoreError;
use crate::ids::{GroupId, HostId, IdentityId, TagId};
use crate::validate;

pub const DEFAULT_SSH_PORT: u16 = 22;
pub const DEFAULT_TERM: &str = "xterm-256color";

/// How a host authenticates. Secrets referenced by these variants are stored
/// exclusively in the OS credential vault and are never part of the synced
/// payload or the local database.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AuthMethod {
    /// Password authentication; password lives in the OS keyring under the
    /// host's credential key (prompted on connect when absent).
    Password,
    /// Public key authentication using a logical identity. The identity's
    /// private key path is device-local state; the passphrase (if any) lives
    /// in the OS keyring.
    PrivateKey { identity_id: IdentityId },
    /// Authenticate through the running SSH agent (SSH_AUTH_SOCK / Pageant).
    Agent,
}

/// A connection profile. This is the primary synced entity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct Host {
    pub id: HostId,
    pub name: String,
    pub address: String,
    pub port: u16,
    pub username: String,
    pub auth: AuthMethod,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group_id: Option<GroupId>,
    #[serde(default)]
    pub tags: Vec<TagId>,
    #[serde(default)]
    pub favorite: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Terminal type advertised in the SSH pty-req.
    #[serde(default = "default_term")]
    pub term: String,
    /// Server keepalive interval in seconds; 0 disables keepalives.
    #[serde(default)]
    pub keepalive_seconds: u32,
    /// Sort position within its group (lower first).
    #[serde(default)]
    pub sort_order: i32,
    #[serde(default)]
    pub created_at: DateTime<Utc>,
    #[serde(default)]
    pub updated_at: DateTime<Utc>,
    /// Tombstone flag for sync. Deleted hosts stay as rows until the server
    /// acknowledges the deletion, then the row is removed (the server keeps
    /// its own tombstone for propagation to other devices).
    #[serde(default)]
    pub deleted: bool,
    /// Local sync bookkeeping. Never serialized: these are device-local
    /// states, not part of the record payload.
    #[serde(skip)]
    pub server_rev: Option<i64>,
    #[serde(skip)]
    pub dirty: bool,
}

fn default_term() -> String {
    DEFAULT_TERM.to_string()
}

impl Host {
    pub fn new(name: String, address: String, username: String) -> Self {
        let now = Utc::now();
        Self {
            id: HostId::new(),
            name,
            address,
            port: DEFAULT_SSH_PORT,
            username,
            auth: AuthMethod::Password,
            group_id: None,
            tags: Vec::new(),
            favorite: false,
            description: None,
            term: default_term(),
            keepalive_seconds: 0,
            sort_order: 0,
            created_at: now,
            updated_at: now,
            deleted: false,
            server_rev: None,
            dirty: false,
        }
    }

    /// Validates every field. Enforced before any local write and before any
    /// sync push so that a malicious peer device cannot poison other devices
    /// through synced records.
    pub fn validate(&self) -> Result<(), CoreError> {
        validate::validate_display_name("name", &self.name, validate::MAX_NAME_LEN)?;
        validate::validate_address(&self.address)?;
        validate::validate_port(self.port)?;
        validate::validate_username(&self.username)?;
        if let Some(desc) = &self.description {
            validate::validate_description(desc)?;
        }
        validate::validate_term(&self.term)?;
        validate::validate_keepalive(self.keepalive_seconds)?;
        if self.tags.len() > validate::MAX_TAGS_PER_HOST {
            return Err(CoreError::validation(format!(
                "a host may have at most {} tags",
                validate::MAX_TAGS_PER_HOST
            )));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_host() -> Host {
        Host::new(
            "web-1".to_string(),
            "web-1.example.com".to_string(),
            "deploy".to_string(),
        )
    }

    #[test]
    fn new_host_is_valid_with_defaults() {
        let host = valid_host();
        assert_eq!(host.port, DEFAULT_SSH_PORT);
        assert_eq!(host.term, DEFAULT_TERM);
        assert!(!host.deleted);
        host.validate().unwrap();
    }

    #[test]
    fn invalid_fields_are_rejected() {
        let mut host = valid_host();
        host.name = "".to_string();
        assert!(host.validate().is_err());

        let mut host = valid_host();
        host.address = "host name".to_string();
        assert!(host.validate().is_err());

        let mut host = valid_host();
        host.port = 0;
        assert!(host.validate().is_err());

        let mut host = valid_host();
        host.username = "user;id".to_string();
        assert!(host.validate().is_err());

        let mut host = valid_host();
        host.term = "xterm;drop".to_string();
        assert!(host.validate().is_err());
    }

    #[test]
    fn auth_method_serializes_with_tag() {
        let host = valid_host();
        let json = serde_json::to_string(&host).unwrap();
        assert!(json.contains("\"auth\":{\"type\":\"password\"}"));
        let _: Host = serde_json::from_str(&json).unwrap();
    }

    #[test]
    fn private_key_auth_roundtrips() {
        let mut host = valid_host();
        host.auth = AuthMethod::PrivateKey {
            identity_id: crate::ids::IdentityId::new(),
        };
        let json = serde_json::to_string(&host).unwrap();
        let back: Host = serde_json::from_str(&json).unwrap();
        assert_eq!(back.auth, host.auth);
    }
}
