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

    #[error("authentication was cancelled")]
    AuthCancelled,

    #[error("the authentication question was not answered in time")]
    AuthPromptTimeout,

    #[error("no SSH agent is available (is one running and reachable?)")]
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

    #[error("the remote host is not accepting input (the input queue is full)")]
    InputBacklog,

    #[error("failed to load key “{path}”: {detail}")]
    KeyLoadFailed { path: String, detail: String },

    #[error("the key requires a passphrase and none was provided")]
    PassphraseRequired,

    #[error("the passphrase for “{path}” is incorrect")]
    WrongPassphrase { path: String },

    /// A jump host on the way failed. `source` is the failure itself; the
    /// wrapper only says which hop it was (`index` is 1-based).
    #[error("jump host {index} of {total} ({label}): {source}")]
    Hop {
        index: usize,
        total: usize,
        label: String,
        #[source]
        source: Box<SshError>,
    },

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("{0}")]
    Protocol(String),
}

impl SshError {
    /// Whether retrying the same operation could plausibly succeed without
    /// user intervention. This is **the** classification used to gate
    /// automatic reconnects: it is exactly `ErrorCategory::Network` or
    /// `ErrorCategory::Timeout` (dial, handshake, connection-interrupted
    /// failures), and never authentication, host-key, protocol, or local
    /// failures.
    pub fn is_retriable(&self) -> bool {
        use crate::session::ErrorCategory;
        matches!(
            ErrorCategory::of(self),
            ErrorCategory::Network | ErrorCategory::Timeout
        )
    }

    /// True when the failure is an authentication problem (never auto-retried,
    /// never keeps credentials cached).
    pub fn is_auth_failure(&self) -> bool {
        if let Self::Hop { source, .. } = self {
            return source.is_auth_failure();
        }
        matches!(
            self,
            Self::AuthFailed { .. }
                | Self::AuthTimeout
                | Self::AuthCancelled
                | Self::AuthPromptTimeout
                | Self::AgentUnavailable
                | Self::AgentNoKey
                | Self::KeyLoadFailed { .. }
                | Self::PassphraseRequired
                | Self::WrongPassphrase { .. }
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::ErrorCategory;

    fn hop(source: SshError) -> SshError {
        SshError::Hop {
            index: 2,
            total: 3,
            label: "bastion".into(),
            source: Box::new(source),
        }
    }

    /// A hop failure is classified by what went wrong on the hop, so a
    /// refused password on a jump host is still an authentication problem
    /// and a dropped link is still retriable.
    #[test]
    fn a_hop_failure_keeps_the_category_of_its_cause() {
        let auth = hop(SshError::AuthFailed {
            method: "password",
            detail: "denied".into(),
        });
        assert_eq!(ErrorCategory::of(&auth), ErrorCategory::Auth);
        assert!(auth.is_auth_failure());
        assert!(!auth.is_retriable());

        let network = hop(SshError::ConnectFailed {
            host: "bastion".into(),
            port: 22,
            detail: "refused".into(),
        });
        assert_eq!(ErrorCategory::of(&network), ErrorCategory::Network);
        assert!(network.is_retriable());

        assert_eq!(
            ErrorCategory::of(&hop(SshError::HostKeyRejected)),
            ErrorCategory::HostKey
        );
    }

    #[test]
    fn a_hop_failure_says_which_hop_it_was() {
        let message = hop(SshError::AuthTimeout).to_string();
        assert!(message.contains("jump host 2 of 3"), "{message}");
        assert!(message.contains("bastion"), "{message}");
        assert!(message.contains("authentication timed out"), "{message}");
    }
}
