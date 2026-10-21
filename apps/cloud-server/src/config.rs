//! Startup configuration.
//!
//! Every setting comes from the environment and is validated **before** the
//! server binds a socket: an invalid configuration aborts the process with an
//! actionable message instead of starting in a half-working state.

use std::net::SocketAddr;
use std::time::Duration;

/// Environment variable names, kept in one place so the docs and the code
/// cannot drift apart.
pub mod env_keys {
    pub const BIND_ADDR: &str = "TERMRU_BIND_ADDR";
    pub const DATABASE_URL: &str = "TERMRU_DATABASE_URL";
    pub const POOL_MAX_SIZE: &str = "TERMRU_POOL_MAX_SIZE";
    pub const ACCESS_TOKEN_TTL_SECS: &str = "TERMRU_ACCESS_TOKEN_TTL_SECS";
    pub const REFRESH_TOKEN_TTL_SECS: &str = "TERMRU_REFRESH_TOKEN_TTL_SECS";
    pub const REFRESH_REUSE_GRACE_SECS: &str = "TERMRU_REFRESH_REUSE_GRACE_SECS";
    pub const RATE_LIMIT_PER_MINUTE: &str = "TERMRU_RATE_LIMIT_PER_MINUTE";
    pub const LOGIN_MAX_FAILURES: &str = "TERMRU_LOGIN_MAX_FAILURES";
    pub const LOGIN_LOCKOUT_SECS: &str = "TERMRU_LOGIN_LOCKOUT_SECS";
    pub const REQUEST_TIMEOUT_SECS: &str = "TERMRU_REQUEST_TIMEOUT_SECS";
    pub const MAX_BODY_BYTES: &str = "TERMRU_MAX_BODY_BYTES";
    pub const TRUST_FORWARDED_FOR: &str = "TERMRU_TRUST_FORWARDED_FOR";
}

/// A configuration problem. The process refuses to start when it occurs.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("missing required environment variable {0}")]
    Missing(&'static str),

    #[error("invalid value for {name}: {reason}")]
    Invalid { name: &'static str, reason: String },
}

/// Validated server configuration.
#[derive(Debug, Clone)]
pub struct Config {
    /// Address the HTTP server binds to.
    pub bind_addr: SocketAddr,
    /// PostgreSQL connection URL.
    pub database_url: String,
    /// Maximum number of pooled database connections.
    pub pool_max_size: u32,
    /// Lifetime of an access token.
    pub access_token_ttl: Duration,
    /// Lifetime of a refresh token.
    pub refresh_token_ttl: Duration,
    /// Window in which a replayed refresh token is treated as a benign race
    /// instead of an attack. Outside the window a replay revokes the family.
    pub refresh_reuse_grace: Duration,
    /// Requests per minute allowed per client (IP or token).
    pub rate_limit_per_minute: u32,
    /// Failed logins before an account/identifier is locked out.
    pub login_max_failures: u32,
    /// How long a locked-out identifier stays locked.
    pub login_lockout: Duration,
    /// Server-side request timeout.
    pub request_timeout: Duration,
    /// Maximum accepted request body size.
    pub max_body_bytes: usize,
    /// Whether `X-Forwarded-For` may be used as the client address. Only
    /// enable this behind a reverse proxy you control, otherwise the header
    /// can be spoofed to bypass the rate limiter.
    pub trust_forwarded_for: bool,
}

impl Config {
    /// Reads and validates the configuration from the process environment.
    pub fn from_env() -> Result<Self, ConfigError> {
        let bind_addr = match optional(env_keys::BIND_ADDR) {
            Some(raw) => raw
                .parse::<SocketAddr>()
                .map_err(|e| ConfigError::Invalid {
                    name: env_keys::BIND_ADDR,
                    reason: format!("not a socket address ({e})"),
                })?,
            None => SocketAddr::from(([0, 0, 0, 0], 8787)),
        };

        let database_url = required(env_keys::DATABASE_URL)?;
        if !database_url.starts_with("postgres://") && !database_url.starts_with("postgresql://") {
            return Err(ConfigError::Invalid {
                name: env_keys::DATABASE_URL,
                reason: "must be a postgres:// or postgresql:// URL".to_string(),
            });
        }

        let pool_max_size = number(env_keys::POOL_MAX_SIZE, 16, 1, 512)?;
        let access_token_ttl = seconds(env_keys::ACCESS_TOKEN_TTL_SECS, 3600, 60, 86_400)?;
        let refresh_token_ttl = seconds(
            env_keys::REFRESH_TOKEN_TTL_SECS,
            30 * 86_400,
            300,
            365 * 86_400,
        )?;
        let refresh_reuse_grace = seconds(env_keys::REFRESH_REUSE_GRACE_SECS, 30, 0, 3600)?;
        let rate_limit_per_minute = number(env_keys::RATE_LIMIT_PER_MINUTE, 120, 1, 100_000)?;
        let login_max_failures = number(env_keys::LOGIN_MAX_FAILURES, 5, 1, 100)?;
        let login_lockout = seconds(env_keys::LOGIN_LOCKOUT_SECS, 900, 1, 86_400)?;
        let request_timeout = seconds(env_keys::REQUEST_TIMEOUT_SECS, 30, 1, 600)?;
        let max_body_bytes = number(
            env_keys::MAX_BODY_BYTES,
            4 * 1024 * 1024,
            1024,
            64 * 1024 * 1024,
        )?;
        let trust_forwarded_for = match optional(env_keys::TRUST_FORWARDED_FOR) {
            Some(raw) => parse_bool(&raw).ok_or(ConfigError::Invalid {
                name: env_keys::TRUST_FORWARDED_FOR,
                reason: "expected 'true' or 'false'".to_string(),
            })?,
            None => false,
        };

        if refresh_token_ttl <= access_token_ttl {
            return Err(ConfigError::Invalid {
                name: env_keys::REFRESH_TOKEN_TTL_SECS,
                reason: "must be longer than the access token lifetime".to_string(),
            });
        }

        Ok(Self {
            bind_addr,
            database_url,
            pool_max_size,
            access_token_ttl,
            refresh_token_ttl,
            refresh_reuse_grace,
            rate_limit_per_minute,
            login_max_failures,
            login_lockout,
            request_timeout,
            max_body_bytes: max_body_bytes as usize,
            trust_forwarded_for,
        })
    }
}

fn optional(key: &'static str) -> Option<String> {
    std::env::var(key)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn required(key: &'static str) -> Result<String, ConfigError> {
    optional(key).ok_or(ConfigError::Missing(key))
}

fn parse_bool(raw: &str) -> Option<bool> {
    match raw.to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

fn number(key: &'static str, default: u32, min: u32, max: u32) -> Result<u32, ConfigError> {
    let Some(raw) = optional(key) else {
        return Ok(default);
    };
    let value = raw.parse::<u64>().map_err(|e| ConfigError::Invalid {
        name: key,
        reason: format!("not a number ({e})"),
    })?;
    if value < u64::from(min) || value > u64::from(max) {
        return Err(ConfigError::Invalid {
            name: key,
            reason: format!("must be between {min} and {max}"),
        });
    }
    Ok(value as u32)
}

fn seconds(key: &'static str, default: u64, min: u64, max: u64) -> Result<Duration, ConfigError> {
    let Some(raw) = optional(key) else {
        return Ok(Duration::from_secs(default));
    };
    let value = raw.parse::<u64>().map_err(|e| ConfigError::Invalid {
        name: key,
        reason: format!("not a number of seconds ({e})"),
    })?;
    if value < min || value > max {
        return Err(ConfigError::Invalid {
            name: key,
            reason: format!("must be between {min} and {max} seconds"),
        });
    }
    Ok(Duration::from_secs(value))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_database_url_is_an_error() {
        // The test process must not inherit a valid configuration.
        if std::env::var(env_keys::DATABASE_URL).is_ok() {
            return;
        }
        let err = Config::from_env().expect_err("missing database url must fail");
        assert!(matches!(err, ConfigError::Missing(env_keys::DATABASE_URL)));
    }

    #[test]
    fn non_postgres_url_is_rejected() {
        let err = parse_bool("maybe");
        assert!(err.is_none());
        assert!(parse_bool("TRUE").is_some_and(|v| v));
        assert!(parse_bool("off").is_some_and(|v| !v));
    }
}
