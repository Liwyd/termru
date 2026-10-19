//! Error taxonomy with user-actionable messages.
//!
//! Every variant maps to a clear UI string; authentication failures never
//! embed credentials, and host-key failures always carry fingerprints.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum SshError {
    #[error("could not resolve host “{host}”: {detail}")]
    DnsFailure { host: String, detail: String },

    #[error("connection to {host}:{port} timed out after {seconds}s")]
    ConnectTimeout {
        host: String,
        port: u16,
        seconds: u64,
    },

    #[error("connection to {host}:{port} failed: {detail}")]
    ConnectFailed {
        host: String,
        port: u16,
        detail: String,
    },

    #[error("connection closed by the remote host")]
    ConnectionClosed,

    #[error("the network connection was interrupted")]
    Interrupted,

    #[error("authentication failed ({method}): {detail}")]
    AuthFailed {
        method: &'static str,
        detail: String,
    },

    #[error("authentication timed out")]
    AuthTimeout,

    #[error("no SSH agent is available (SSH_AUTH_SOCK is not set)")]
    AgentUnavailable,

    #[error("the SSH agent does not contain a usable key for this connection")]
    AgentNoKey,

    #[error(
        "the host key for {host}:{port} is unknown \
         (fingerprint {fingerprint}); explicit trust is required"
    )]
    HostKeyUnknown {
        host: String,
        port: u16,
        fingerprint: String,
    },

    #[error(
        "WARNING: the host key for {host}:{port} has CHANGED \
         (known {old_fingerprint}, presented {new_fingerprint}); \
         possible man-in-the-middle attack"
    )]
    HostKeyChanged {
        host: String,
        port: u16,
        old_fingerprint: String,
        new_fingerprint: String,
    },

    #[error("the host key for {host}:{port} is marked REVOKED in known_hosts")]
    HostKeyRevoked { host: String, port: u16 },

    #[error("the host key was rejected")]
    HostKeyRejected,

    #[error("the host key trust decision was not answered in time")]
    HostKeyPromptTimeout,

    #[error("no terminal was requested for this session")]
    NoPty,

    #[error("the remote shell exited with status {code}")]
    ShellExited { code: i32 },

    #[error("session is closing")]
    SessionClosing,

    #[error("failed to load key “{path}”: {detail}")]
    KeyLoadFailed { path: String, detail: String },

    #[error("the key requires a passphrase and none was provided")]
    PassphraseRequired,

    #[error("the passphrase for “{path}” is incorrect")]
    WrongPassphrase { path: String },

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("{0}")]
    Protocol(String),
}

impl SshError {
    /// Whether retrying the same operation could plausibly succeed without
    /// user intervention (used to gate automatic reconnects).
    pub fn is_retriable(&self) -> bool {
        matches!(
            self,
            Self::ConnectionClosed | Self::Interrupted | Self::ConnectFailed { .. }
        )
    }

    /// True when the failure is an authentication problem (never auto-retried,
    /// never keeps credentials cached).
    pub fn is_auth_failure(&self) -> bool {
        matches!(
            self,
            Self::AuthFailed { .. }
                | Self::AuthTimeout
                | Self::AgentUnavailable
                | Self::AgentNoKey
                | Self::KeyLoadFailed { .. }
                | Self::PassphraseRequired
                | Self::WrongPassphrase { .. }
        )
    }
}
