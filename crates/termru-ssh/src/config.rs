//! Connection parameters and credential types.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;

use crate::known_hosts::HostKeyEntry;

pub use termru_core::host::DEFAULT_KEEPALIVE_SECONDS;

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

/// One way of authenticating. A connection carries an ordered list of them
/// ([`ConnectParams::auth`]); secrets are supplied per connection by the
/// caller (desktop resolves them from the OS vault) and never stored here.
#[derive(Debug, Clone)]
pub enum AuthChoice {
    Password {
        password: SecretValue,
    },
    PrivateKey {
        key_path: PathBuf,
        passphrase: Option<SecretValue>,
        /// The logical identity the key belongs to. It names the key in the
        /// passphrase prompt and lets the UI offer to remember the
        /// passphrase for exactly this identity.
        identity: Option<KeyIdentity>,
    },
    /// Authenticate via an SSH agent.
    Agent {
        /// Explicit agent socket path. `None` = read `SSH_AUTH_SOCK` from the
        /// environment (the desktop resolves and passes the path explicitly).
        /// On Windows this is the named pipe of the OpenSSH agent
        /// (`\\.\pipe\openssh-ssh-agent` when `None`), with Pageant as the
        /// fallback.
        socket: Option<PathBuf>,
        /// `SHA256:...` fingerprint of the key to offer first, when the
        /// connection is tied to a known identity. The other agent keys
        /// follow (see [`MAX_AGENT_KEYS`]).
        prefer_fingerprint: Option<String>,
    },
    /// Keyboard-interactive (RFC 4256): the server asks the questions. The
    /// answers come from `password` when the server asks for nothing but a
    /// password, and from the [`AuthPrompt`] of the connection otherwise.
    KeyboardInteractive {
        /// Answers one lone, hidden "Password:" prompt — once per attempt, so
        /// a wrong stored password is not retried against the server's
        /// lock-out counter.
        password: Option<SecretValue>,
    },
}

/// Which identity a private key file belongs to (shown to the user, never a
/// secret).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct KeyIdentity {
    pub id: String,
    pub name: String,
}

/// Agent keys offered to the server at most. `sshd` drops the connection
/// after `MaxAuthTries` failed attempts (6 by default) and counts every
/// rejected key, so an agent with many keys, offered in full, would use up
/// the allowance before the later methods — and the key that would have
/// worked — are reached. Five leaves one attempt for the method after the
/// agent.
pub const MAX_AGENT_KEYS: usize = 5;

impl AuthChoice {
    pub fn method_name(&self) -> &'static str {
        match self {
            Self::Password { .. } => "password",
            Self::PrivateKey { .. } => "publickey",
            Self::Agent { .. } => "publickey (agent)",
            Self::KeyboardInteractive { .. } => "keyboard-interactive",
        }
    }
}

/// One question of a keyboard-interactive round (or a passphrase prompt).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct AuthPromptField {
    pub text: String,
    /// `true` when what the user types may be shown; secrets are `false`.
    pub echo: bool,
}

/// What an [`AuthPrompt`] is asked for.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AuthPromptKind {
    /// A server-driven keyboard-interactive round.
    KeyboardInteractive,
    /// The passphrase of an encrypted private key. A UI may offer to
    /// remember the answer for `identity`.
    Passphrase {
        identity: Option<KeyIdentity>,
        /// `true` when an earlier answer did not decrypt the key.
        retry: bool,
    },
}

/// A question for the user during authentication.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct AuthPromptRequest {
    pub kind: AuthPromptKind,
    /// Title the server gave the round (often empty).
    pub name: String,
    /// Text the server shows above the prompts (often empty).
    pub instructions: String,
    pub prompts: Vec<AuthPromptField>,
}

/// What the user did with an [`AuthPromptRequest`].
pub enum AuthPromptResponse {
    /// One answer per prompt, in order.
    Answers(Vec<SecretValue>),
    /// The user declined to answer: authentication stops.
    Cancelled,
    /// Nobody answered in time (the prompt gave up on its own).
    TimedOut,
}

impl std::fmt::Debug for AuthPromptResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Answers(answers) => write!(f, "Answers({} redacted)", answers.len()),
            Self::Cancelled => f.write_str("Cancelled"),
            Self::TimedOut => f.write_str("TimedOut"),
        }
    }
}

/// Interactive authentication questions (keyboard-interactive rounds and key
/// passphrases). Implemented by the desktop app (UI prompt) and by automated
/// test policies.
#[async_trait]
pub trait AuthPrompt: Send + Sync {
    /// Asks the user. The engine bounds the wait with
    /// [`ConnectTimeouts::auth_prompt`] and does not charge it to any
    /// connect or authentication deadline.
    async fn ask(&self, request: AuthPromptRequest) -> AuthPromptResponse;

    /// Whether a human can answer. When not, the engine does not even try
    /// (a key passphrase that is missing is then an error, not a question).
    fn is_interactive(&self) -> bool {
        false
    }
}

/// Answers nothing: for connections without a UI and for tests that must not
/// prompt.
pub struct NoAuthPrompts;

#[async_trait]
impl AuthPrompt for NoAuthPrompts {
    async fn ask(&self, _request: AuthPromptRequest) -> AuthPromptResponse {
        AuthPromptResponse::Cancelled
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
        /// Key types this host is already known under (a different type
        /// than `key`'s). Non-empty means the server now presents a type we
        /// have not seen: legitimate after a server upgrade, but also what
        /// an attacker who lacks the real key would present.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        other_key_types: Vec<String>,
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
    /// The authentication methods to try, in order. The engine probes the
    /// server first and skips the methods it does not allow, so a longer
    /// list is only ever a wider net, never a more insistent one.
    pub auth: Vec<AuthChoice>,
    /// Terminal type for pty-req (e.g. `xterm-256color`).
    pub term: String,
    /// Keepalive interval in seconds. `0` means "automatic"
    /// ([`DEFAULT_KEEPALIVE_SECONDS`]): the data model has no separate
    /// "disabled" state, and a connection without keepalives silently dies
    /// behind NAT/firewall idle timeouts. See [`ConnectParams::effective_keepalive`].
    pub keepalive_seconds: u32,
    pub timeouts: ConnectTimeouts,
    pub known_hosts_path: PathBuf,
    pub host_key_prompt: Arc<dyn HostKeyPrompt>,
    /// Asked for keyboard-interactive answers and missing key passphrases.
    pub auth_prompt: Arc<dyn AuthPrompt>,
}

impl std::fmt::Debug for ConnectParams {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The prompts are trait objects; `auth` is Debug-safe because
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
    /// The keepalive interval the engine actually uses: the configured value,
    /// or [`DEFAULT_KEEPALIVE_SECONDS`] when it is `0`.
    pub fn effective_keepalive(&self) -> Duration {
        let seconds = if self.keepalive_seconds == 0 {
            DEFAULT_KEEPALIVE_SECONDS
        } else {
            self.keepalive_seconds
        };
        Duration::from_secs(u64::from(seconds))
    }

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
    /// One overall budget for name resolution, TCP connect (all addresses
    /// together), key exchange and shell setup. Time a human spends on a
    /// host-key prompt and the authentication phase are not charged to it.
    pub connect: Duration,
    /// Authentication phase (all methods together). Time a human spends
    /// answering an [`AuthPrompt`] is not charged to it.
    pub auth: Duration,
    /// Human host-key decision budget (interactive prompts only).
    pub host_key_decision: Duration,
    /// How long one authentication question may wait for the user.
    pub auth_prompt: Duration,
}

impl Default for ConnectTimeouts {
    fn default() -> Self {
        Self {
            connect: Duration::from_secs(15),
            auth: Duration::from_secs(30),
            host_key_decision: Duration::from_secs(120),
            auth_prompt: Duration::from_secs(120),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(keepalive_seconds: u32) -> ConnectParams {
        ConnectParams {
            host: "h".into(),
            port: 22,
            username: "u".into(),
            auth: vec![AuthChoice::Agent {
                socket: None,
                prefer_fingerprint: None,
            }],
            term: "xterm".into(),
            keepalive_seconds,
            timeouts: ConnectTimeouts::default(),
            known_hosts_path: PathBuf::from("/dev/null"),
            host_key_prompt: Arc::new(RejectUnknownHostKeys),
            auth_prompt: Arc::new(NoAuthPrompts),
        }
    }

    /// Regression test: a stored `0` used to mean "no keepalive at all".
    #[test]
    fn zero_keepalive_means_the_default_not_off() {
        assert_eq!(
            params(0).effective_keepalive(),
            Duration::from_secs(u64::from(DEFAULT_KEEPALIVE_SECONDS))
        );
        assert_eq!(params(45).effective_keepalive(), Duration::from_secs(45));
    }
}
