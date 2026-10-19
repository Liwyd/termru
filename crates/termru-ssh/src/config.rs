//! Connection parameters and credential types.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;

use crate::known_hosts::HostKeyEntry;

/// Wrapper that zeroizes its contents on drop and never prints them.
#[derive(Clone, zeroize::ZeroizeOnDrop)]
pub struct SecretValue(String);

impl SecretValue {
    pub fn new(value: String) -> Self {
        Self(value)
    }

    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for SecretValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SecretValue([redacted])")
    }
}

/// How the session authenticates. Secrets are supplied per connection by the
/// caller (desktop resolves them from the OS vault) and never stored here.
#[derive(Debug, Clone)]
pub enum AuthChoice {
    Password {
        password: SecretValue,
    },
    PrivateKey {
        key_path: PathBuf,
        passphrase: Option<SecretValue>,
    },
    /// Authenticate via an SSH agent.
    Agent {
        /// Explicit agent socket path. `None` = read `SSH_AUTH_SOCK` from the
        /// environment (the desktop resolves and passes the path explicitly).
        socket: Option<PathBuf>,
    },
}

impl AuthChoice {
    pub fn method_name(&self) -> &'static str {
        match self {
            Self::Password { .. } => "password",
            Self::PrivateKey { .. } => "publickey",
            Self::Agent { .. } => "publickey (agent)",
        }
    }
}

/// What to do when the known-hosts evaluation is Unknown or Changed.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum HostKeyPromptRequest {
    Unknown {
        host: String,
        port: u16,
        key: HostKeyEntry,
    },
    Changed {
        host: String,
        port: u16,
        old: HostKeyEntry,
        new: HostKeyEntry,
    },
}

/// Interactive host-key trust decision. Implemented by the desktop app (UI
/// prompt) and by automated test policies.
#[async_trait]
pub trait HostKeyPrompt: Send + Sync {
    /// Returns `true` to trust the key. For `Changed` keys, a `true` answer
    /// replaces the stored entry (the UI must have shown the warning).
    async fn decide(&self, request: HostKeyPromptRequest) -> bool;

    /// Whether this prompt may block for a human (extends the connect
    /// deadline accordingly).
    fn is_interactive(&self) -> bool {
        false
    }
}

/// Trusts any key. **Only for automated tests** — never wire this to a UI.
pub struct AcceptAllHostKeys;

#[async_trait]
impl HostKeyPrompt for AcceptAllHostKeys {
    async fn decide(&self, _request: HostKeyPromptRequest) -> bool {
        true
    }
}

/// Rejects every unproven key (used when a caller wants strict TOFU-free
/// verification, e.g. connections to pre-pinned hosts).
pub struct RejectUnknownHostKeys;

#[async_trait]
impl HostKeyPrompt for RejectUnknownHostKeys {
    async fn decide(&self, _request: HostKeyPromptRequest) -> bool {
        false
    }
}

#[derive(Clone)]
pub struct ConnectParams {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub auth: AuthChoice,
    /// Terminal type for pty-req (e.g. `xterm-256color`).
    pub term: String,
    /// Server keepalive in seconds; 0 disables keepalives.
    pub keepalive_seconds: u32,
    pub timeouts: ConnectTimeouts,
    pub known_hosts_path: PathBuf,
    pub host_key_prompt: Arc<dyn HostKeyPrompt>,
}

impl std::fmt::Debug for ConnectParams {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // `host_key_prompt` is a trait object; `auth` is Debug-safe because
        // SecretValue redacts itself.
        f.debug_struct("ConnectParams")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("username", &self.username)
            .field("auth", &self.auth)
            .field("term", &self.term)
            .field("keepalive_seconds", &self.keepalive_seconds)
            .field("timeouts", &self.timeouts)
            .field("known_hosts_path", &self.known_hosts_path)
            .finish_non_exhaustive()
    }
}

impl ConnectParams {
    pub fn target(&self) -> String {
        if self.port == 22 {
            self.host.clone()
        } else {
            format!("{}:{}", self.host, self.port)
        }
    }
}

#[derive(Debug, Clone)]
pub struct ConnectTimeouts {
    /// TCP connect per address + key exchange.
    pub connect: Duration,
    /// Authentication phase.
    pub auth: Duration,
    /// Human host-key decision budget (interactive prompts only).
    pub host_key_decision: Duration,
}

impl Default for ConnectTimeouts {
    fn default() -> Self {
        Self {
            connect: Duration::from_secs(15),
            auth: Duration::from_secs(30),
            host_key_decision: Duration::from_secs(120),
        }
    }
}
