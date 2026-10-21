//! Shared application state.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use deadpool_postgres::Pool;

use crate::config::Config;
use crate::ratelimit::RateLimiter;

/// Clock abstraction so that time-dependent behaviour (token expiry, login
/// lockouts) can be tested deterministically.
pub trait Clock: Send + Sync + std::fmt::Debug {
    /// The current time.
    fn now(&self) -> DateTime<Utc>;
}

/// The real clock.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}

/// State shared by every handler.
#[derive(Clone)]
pub struct AppState {
    /// Database connection pool.
    pub pool: Pool,
    /// Validated configuration.
    pub config: Arc<Config>,
    /// Request rate limiter.
    pub limiter: Arc<RateLimiter>,
    /// Clock used for every time-dependent decision.
    pub clock: Arc<dyn Clock>,
}

impl std::fmt::Debug for AppState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppState")
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}
