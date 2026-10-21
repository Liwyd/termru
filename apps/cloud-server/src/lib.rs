//! Termru cloud sync server.
//!
//! The server stores the configuration of a user's hosts (and their groups,
//! tags, identities and preferences) and hands it to the user's other
//! devices. It never sees a secret: passwords, passphrases and private keys
//! stay in the credential store of the device they belong to.
//!
//! The protocol is documented in `docs/architecture/0005-sync-protocol.md`.
//! The wire types live in the `termru-core` crate and are shared with the
//! desktop client, so both sides agree on the format by construction.

pub mod audit;
pub mod auth;
pub mod config;
pub mod db;
pub mod error;
pub mod extract;
pub mod ratelimit;
pub mod routes;
pub mod security;
pub mod state;
pub mod sync;

pub use config::{Config, ConfigError};
pub use error::{ApiError, ApiResult};
pub use ratelimit::RateLimiter;
pub use state::{AppState, Clock, SystemClock};

use std::sync::Arc;

/// Builds the application state from a validated configuration.
pub fn build_state(config: Config, pool: deadpool_postgres::Pool) -> AppState {
    let limiter = RateLimiter::per_minute(config.rate_limit_per_minute);
    AppState {
        pool,
        config: Arc::new(config),
        limiter: Arc::new(limiter),
        clock: Arc::new(SystemClock),
    }
}
