//! Shared domain models, validation, and sync protocol types for Termru.
//!
//! This crate is used by the desktop client and the cloud sync server. It must
//! stay free of platform and transport dependencies (no tokio, no russh, no
//! database access).

pub mod error;
pub mod host;
pub mod identity;
pub mod ids;
pub mod preference;
pub mod sync;
pub mod taxonomy;
pub mod validate;
pub mod workspace;

pub use error::CoreError;
pub use host::{AuthMethod, Host};
pub use identity::Identity;
pub use preference::Preferences;
pub use sync::{
    CLIENT_SCHEMA_VERSION, PullResponse, PushMutation, PushRequest, PushResponse, RecordKind,
    RecordPayload, WireRecord,
};
pub use taxonomy::{Group, Tag};
