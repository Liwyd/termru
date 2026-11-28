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

/// A port forward the session starts once it is ready. Built from a host's
/// `termru_core::Forward` (only the enabled ones).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForwardSpec {
    /// The caller's name for this forward, handed back in every
    /// [`ForwardStatus`](crate::forward::ForwardStatus). The desktop uses the
    /// forward's index in the host, so a disabled forward keeps its slot.
    pub id: u32,
    pub kind: termru_core::ForwardKind,
    /// `None` means loopback (see `termru_core::Forward`).
    pub bind_address: Option<String>,
    pub bind_port: u16,
    pub target_host: Option<String>,
    pub target_port: Option<u16>,
}

impl ForwardSpec {
    pub fn from_forward(id: u32, forward: &termru_core::Forward) -> Self {
        Self {
            id,
            kind: forward.kind,
            bind_address: forward.bind_address.clone(),
            bind_port: forward.bind_port,
            target_host: forward.target_host.clone(),
            target_port: forward.target_port,
        }
    }
}

/// A host the connection passes through on its way to the target. It is
/// connected to and authenticated like any other host (its own host-key
/// check, known_hosts and authentication chain); only its `setup` is ignored.
#[derive(Clone)]
pub struct JumpHop {
    /// How the hop is named in errors (the host's display name).
    pub label: String,
    pub params: ConnectParams,
}

impl std::fmt::Debug for JumpHop {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JumpHop")
            .field("label", &self.label)
            .field("params", &self.params)
            .finish()
    }
}

/// Lets the server use the local SSH agent while the session is open.
///
/// **Risky by nature**: anyone who controls the server (or can read the
/// socket it creates) can have the agent sign as the user for as long as the
/// session lives. It is only ever requested on the final hop, never on a jump
/// host.
#[derive(Debug, Clone, Default)]
pub struct AgentForward {
    /// Explicit agent socket. `None` = `SSH_AUTH_SOCK` (on Windows the
    /// OpenSSH agent's named pipe).
    pub socket: Option<PathBuf>,
}

/// What a session does besides connecting: the route to the host, forwards,
/// environment, startup input. All off by default.
#[derive(Clone, Default)]
pub struct SessionSetup {
    /// Hosts to pass through, in order, before the target (see
    /// [`ConnectParams::setup`]). The caller has already resolved the full
    /// route (`termru_core::validate::resolve_jump_chain`).
    pub jump: Vec<JumpHop>,
    /// Forwards started after the session is ready, torn down when it ends
    /// and started again by a reconnect.
    pub forwards: Vec<ForwardSpec>,
    /// Environment variables requested before the shell starts. A server
    /// accepts only what its `AcceptEnv` allows; a refusal is not an error.
    pub env: Vec<(String, String)>,
    /// Typed into the shell once it is up, followed by Enter. It is input to
    /// an interactive shell, not an `exec` request: the session stays a
    /// normal terminal.
    pub startup_command: Option<String>,
    pub agent_forward: Option<AgentForward>,
}

impl std::fmt::Debug for SessionSetup {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Environment values and the startup command are the user's text and
        // may hold something private: names and presence only.
        f.debug_struct("SessionSetup")
            .field("jump", &self.jump)
            .field("forwards", &self.forwards)
            .field(
                "env",
                &self.env.iter().map(|(name, _)| name).collect::<Vec<_>>(),
            )
            .field("startup_command", &self.startup_command.is_some())
            .field("agent_forward", &self.agent_forward)
            .finish()
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
    /// More known_hosts files, **read only**: a key found in one of them (and
    /// nowhere in `known_hosts_path`) is trusted without a prompt; a key that
    /// contradicts one is a changed key, and `@revoked` is honored. They are
    /// never written (typically the user's `~/.ssh/known_hosts`).
    pub extra_known_hosts: Vec<PathBuf>,
    pub host_key_prompt: Arc<dyn HostKeyPrompt>,
    /// Asked for keyboard-interactive answers and missing key passphrases.
    pub auth_prompt: Arc<dyn AuthPrompt>,
    /// Route, forwards, environment and startup input of the session. Only
    /// the connection's final hop uses it: a [`JumpHop`]'s own `setup` is
    /// ignored.
    pub setup: SessionSetup,
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
            .field("extra_known_hosts", &self.extra_known_hosts)
            .field("setup", &self.setup)
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
            extra_known_hosts: Vec::new(),
            host_key_prompt: Arc::new(RejectUnknownHostKeys),
            auth_prompt: Arc::new(NoAuthPrompts),
            setup: SessionSetup::default(),
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

    /// Environment values and the startup command are the user's text: they
    /// never reach a log line through `Debug`.
    #[test]
    fn debug_output_names_environment_variables_but_not_values() {
        let mut p = params(0);
        p.setup.env = vec![("TERMRU_PROFILE".into(), "hunter2-value".into())];
        p.setup.startup_command = Some("echo supersecret-command".into());
        let debug = format!("{p:?}");
        assert!(debug.contains("TERMRU_PROFILE"), "{debug}");
        assert!(!debug.contains("hunter2-value"), "{debug}");
        assert!(!debug.contains("supersecret-command"), "{debug}");
    }
}
