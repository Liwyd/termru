//! Host (connection profile) model.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::CoreError;
use crate::ids::{GroupId, HostId, IdentityId, TagId};
use crate::validate;

pub const DEFAULT_SSH_PORT: u16 = 22;
pub const DEFAULT_TERM: &str = "xterm-256color";

/// Keepalive interval (seconds) a new host starts with, and the interval the
/// SSH engine uses when a host stores `0`.
///
/// There is no separate "disabled" state in the data model, so `0` can only
/// mean "use the default": a keepalive-less connection silently dies behind
/// NAT/firewall idle timeouts, which is the failure this default prevents.
/// A user who really wants (almost) none can store a large interval.
pub const DEFAULT_KEEPALIVE_SECONDS: u32 = 30;

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

/// One authentication method of an explicit [`Host::auth_order`]. The
/// credentials behind a kind come from the host's [`AuthMethod`] and the OS
/// credential vault; the order only says which to try, and in what sequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthKind {
    Password,
    /// The key file of the host's identity.
    PrivateKey,
    /// The running SSH agent.
    Agent,
    KeyboardInteractive,
}

impl AuthKind {
    /// Human-readable name (messages only; the wire name is the serde one).
    pub fn label(self) -> &'static str {
        match self {
            Self::Password => "password",
            Self::PrivateKey => "private key",
            Self::Agent => "agent",
            Self::KeyboardInteractive => "keyboard-interactive",
        }
    }
}

/// Direction of a port forward.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ForwardKind {
    /// `-L`: listen here, connect to the target from the server.
    Local,
    /// `-R`: the server listens, connections come back here to the target.
    Remote,
    /// `-D`: listen here as a SOCKS5 proxy; the client names the target.
    Dynamic,
}

impl ForwardKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Remote => "remote",
            Self::Dynamic => "dynamic",
        }
    }
}

fn default_true() -> bool {
    true
}

/// A port forward started with the session (see `docs/architecture/0002-ssh-transport.md`).
///
/// With no `bind_address` a forward listens on loopback only, whichever side
/// listens. A listener that is reachable from the network is an explicit
/// choice (`0.0.0.0`, `::`, a LAN address).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct Forward {
    pub kind: ForwardKind,
    /// Where the listener binds: on this machine for `local` and `dynamic`
    /// (an IP address or `localhost`), on the server for `remote`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bind_address: Option<String>,
    pub bind_port: u16,
    /// `local` / `remote`: the host the traffic is delivered to (reached from
    /// the server for `local`, from this machine for `remote`). `dynamic`: none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_host: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_port: Option<u16>,
    /// A disabled forward is kept but not started.
    #[serde(default = "default_true")]
    pub enabled: bool,
}

/// An environment variable the client asks the server to set (`SetEnv`).
/// Servers accept only the names their `AcceptEnv` allows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct EnvVar {
    pub name: String,
    pub value: String,
}

fn is_false(value: &bool) -> bool {
    !*value
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
    /// Server keepalive interval in seconds; `0` means "automatic"
    /// ([`DEFAULT_KEEPALIVE_SECONDS`]) — keepalives cannot be switched off
    /// entirely (see the constant for why).
    #[serde(default)]
    pub keepalive_seconds: u32,
    /// Hosts to pass through on the way, in order (`ProxyJump`). Each hop is
    /// another host of the library with its own address, user and
    /// authentication. Empty for a direct connection. See
    /// [`validate::resolve_jump_chain`] for how the full route is derived.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub jump_chain: Vec<HostId>,
    /// Port forwards started with the session.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub forwards: Vec<Forward>,
    /// Environment variables requested from the server before the shell
    /// starts. Synced in plain text: never put a secret here.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub env: Vec<EnvVar>,
    /// Typed into the shell as soon as it starts (as if the user had typed it
    /// and pressed Enter), on every (re)connect. The shell is interactive, so
    /// this is input, not `exec`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub startup_command: Option<String>,
    /// Lets the server use the local SSH agent while the session is open
    /// (`ForwardAgent`). **Off by default and risky**: an administrator of the
    /// server (or anyone who can read its agent socket) can have the agent
    /// sign requests as the user for as long as the session lives. Enable it
    /// only for servers that are trusted with that.
    #[serde(default, skip_serializing_if = "is_false")]
    pub agent_forward: bool,
    /// Explicit order of the authentication methods. `None` derives the
    /// default chain from [`Host::auth`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth_order: Option<Vec<AuthKind>>,
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
            keepalive_seconds: DEFAULT_KEEPALIVE_SECONDS,
            jump_chain: Vec::new(),
            forwards: Vec::new(),
            env: Vec::new(),
            startup_command: None,
            agent_forward: false,
            auth_order: None,
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
        validate::validate_jump_chain(self.id, &self.jump_chain)?;
        validate::validate_forwards(&self.forwards)?;
        validate::validate_env(&self.env)?;
        if let Some(command) = &self.startup_command {
            validate::validate_startup_command(command)?;
        }
        if let Some(order) = &self.auth_order {
            validate::validate_auth_order(order)?;
        }
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

    /// Regression test: new hosts used to start with keepalives off, so an
    /// idle connection silently died behind a NAT/firewall.
    #[test]
    fn new_hosts_keep_the_connection_alive_by_default() {
        assert_eq!(valid_host().keepalive_seconds, DEFAULT_KEEPALIVE_SECONDS);
        const { assert!(DEFAULT_KEEPALIVE_SECONDS > 0) };
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

    fn rich_host() -> Host {
        let mut host = valid_host();
        host.jump_chain = vec![HostId::new(), HostId::new()];
        host.forwards = vec![
            Forward {
                kind: ForwardKind::Local,
                bind_address: Some("127.0.0.1".into()),
                bind_port: 5433,
                target_host: Some("db.internal".into()),
                target_port: Some(5432),
                enabled: true,
            },
            Forward {
                kind: ForwardKind::Dynamic,
                bind_address: None,
                bind_port: 1080,
                target_host: None,
                target_port: None,
                enabled: false,
            },
        ];
        host.env = vec![EnvVar {
            name: "TERMRU_PROFILE".into(),
            value: "ops".into(),
        }];
        host.startup_command = Some("cd /srv && ls".into());
        host.agent_forward = true;
        host.auth_order = Some(vec![AuthKind::Agent, AuthKind::Password]);
        host
    }

    #[test]
    fn new_hosts_have_no_extras() {
        let host = valid_host();
        assert!(host.jump_chain.is_empty());
        assert!(host.forwards.is_empty());
        assert!(host.env.is_empty());
        assert!(host.startup_command.is_none());
        assert!(!host.agent_forward, "agent forwarding is opt-in");
        assert!(host.auth_order.is_none());
    }

    #[test]
    fn the_v2_fields_roundtrip() {
        let host = rich_host();
        host.validate().unwrap();
        let json = serde_json::to_string(&host).unwrap();
        let back: Host = serde_json::from_str(&json).unwrap();
        assert_eq!(back.jump_chain, host.jump_chain);
        assert_eq!(back.forwards, host.forwards);
        assert_eq!(back.env, host.env);
        assert_eq!(back.startup_command, host.startup_command);
        assert!(back.agent_forward);
        assert_eq!(back.auth_order, host.auth_order);
    }

    /// A host that uses none of the v2 features serializes exactly as it did
    /// before they existed, so syncing it does not look like an edit to an
    /// older device (and cannot raise a conflict).
    #[test]
    fn a_plain_host_serializes_without_the_v2_fields() {
        let value = serde_json::to_value(valid_host()).unwrap();
        for field in [
            "jump_chain",
            "forwards",
            "env",
            "startup_command",
            "agent_forward",
            "auth_order",
        ] {
            assert!(value.get(field).is_none(), "{field} must be omitted");
        }
    }

    #[test]
    fn the_wire_names_of_the_v2_fields_are_stable() {
        let value = serde_json::to_value(rich_host()).unwrap();
        assert_eq!(value["forwards"][0]["kind"], "local");
        assert_eq!(value["forwards"][1]["kind"], "dynamic");
        assert_eq!(value["forwards"][1]["enabled"], false);
        assert!(value["forwards"][1].get("target_host").is_none());
        assert_eq!(value["env"][0]["name"], "TERMRU_PROFILE");
        assert_eq!(
            value["auth_order"],
            serde_json::json!(["agent", "password"])
        );
        assert_eq!(value["agent_forward"], true);
        assert!(value["jump_chain"][0].is_string());
        let kinds: Vec<String> = [
            AuthKind::Password,
            AuthKind::PrivateKey,
            AuthKind::Agent,
            AuthKind::KeyboardInteractive,
        ]
        .iter()
        .map(|kind| {
            serde_json::to_value(kind)
                .unwrap()
                .as_str()
                .unwrap()
                .to_string()
        })
        .collect();
        assert_eq!(
            kinds,
            ["password", "private_key", "agent", "keyboard_interactive"]
        );
    }

    /// A record written by Termru before data model v2 (and the payload an
    /// older device syncs) has none of the new fields and must load as a
    /// plain host.
    #[test]
    fn an_old_record_loads_with_defaults() {
        let old = serde_json::json!({
            "id": HostId::new().to_string(),
            "name": "legacy",
            "address": "legacy.example.com",
            "port": 22,
            "username": "root",
            "auth": {"type": "password"},
            "tags": [],
            "favorite": false,
            "term": "xterm-256color",
            "keepalive_seconds": 0,
            "sort_order": 0,
            "created_at": "2025-01-01T00:00:00Z",
            "updated_at": "2025-01-01T00:00:00Z",
        });
        let host: Host = serde_json::from_value(old).unwrap();
        host.validate().unwrap();
        assert!(host.jump_chain.is_empty() && host.forwards.is_empty() && host.env.is_empty());
        assert!(host.startup_command.is_none() && host.auth_order.is_none());
        assert!(!host.agent_forward);
    }

    /// A forward written without `enabled` is on (the field was added with
    /// the forwards themselves, but a hand-edited or partial record is safe).
    #[test]
    fn a_forward_without_enabled_is_enabled() {
        let forward: Forward = serde_json::from_value(serde_json::json!({
            "kind": "local",
            "bind_port": 8080,
            "target_host": "db",
            "target_port": 5432,
        }))
        .unwrap();
        assert!(forward.enabled);
        assert!(forward.bind_address.is_none());
    }

    #[test]
    fn invalid_v2_fields_make_the_host_invalid() {
        let mut host = rich_host();
        host.jump_chain = vec![host.id];
        assert!(host.validate().is_err(), "self reference");

        let mut host = rich_host();
        host.forwards[1].target_host = Some("example.com".into());
        assert!(host.validate().is_err(), "dynamic forward with a target");

        let mut host = rich_host();
        host.env[0].name = "1BAD".into();
        assert!(host.validate().is_err(), "env name");

        let mut host = rich_host();
        host.startup_command = Some("ls\nreboot".into());
        assert!(host.validate().is_err(), "multi-line command");

        let mut host = rich_host();
        host.auth_order = Some(vec![]);
        assert!(host.validate().is_err(), "empty auth order");
    }

    #[test]
    fn ipv6_zone_addresses_are_valid_hosts() {
        let mut host = valid_host();
        host.address = "fe80::1%eth0".into();
        host.validate().unwrap();
        host.address = "[::1]".into();
        host.validate().unwrap();
    }
}
