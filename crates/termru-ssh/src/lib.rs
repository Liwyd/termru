//! SSH transport engine for Termru, built on `russh`.
//!
//! Responsibilities:
//! * connection lifecycle with timeouts, keepalives, and bounded reconnects
//! * authentication (password, key file + passphrase, SSH agent)
//! * OpenSSH-compatible known-hosts verification (TOFU + changed-key refusal)
//! * interactive PTY channel with resize and bounded replay buffering

pub mod config;
pub mod error;
pub mod known_hosts;
pub mod reconnect;
pub mod session;

pub use config::{
    AuthChoice, ConnectParams, ConnectTimeouts, HostKeyPrompt, HostKeyPromptRequest, SecretValue,
};
pub use error::SshError;
pub use known_hosts::{Evaluation, HostKeyEntry, KnownHosts};
pub use reconnect::{GiveUpReason, ReconnectDecision, ReconnectPolicy};
pub use session::{ErrorCategory, Session, SessionEvent, SessionOptions, SessionStatus};
