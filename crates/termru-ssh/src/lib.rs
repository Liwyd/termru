//! SSH transport engine for Termru, built on `russh`.
//!
//! Responsibilities:
//! * connection lifecycle with timeouts, keepalives, and bounded reconnects
//! * authentication as an ordered chain (password, key file + passphrase and
//!   certificate, SSH agent, keyboard-interactive), driven by what the server
//!   allows; see the `auth` module
//! * OpenSSH-compatible known-hosts verification (TOFU + changed-key refusal)
//! * interactive PTY channel with resize and bounded replay buffering

mod auth;
pub mod config;
mod dial;
pub mod error;
pub mod forward;
mod jump;
pub mod known_hosts;
pub mod reconnect;
pub mod replay;
pub mod session;
pub mod ssh_config;

pub use config::{
    AgentForward, AuthChoice, AuthPrompt, AuthPromptField, AuthPromptKind, AuthPromptRequest,
    AuthPromptResponse, ConnectParams, ConnectTimeouts, ForwardSpec, HostKeyPrompt,
    HostKeyPromptRequest, JumpHop, KeyIdentity, MAX_AGENT_KEYS, NoAuthPrompts, SecretValue,
    SessionSetup,
};
pub use error::SshError;
pub use forward::{ForwardState, ForwardStatus};
pub use known_hosts::{Evaluation, HostKeyEntry, KnownHosts};
pub use reconnect::{GiveUpReason, ReconnectDecision, ReconnectPolicy};
pub use replay::ReplayRing;
pub use session::{ErrorCategory, Session, SessionEvent, SessionOptions, SessionStatus};
