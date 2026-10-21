//! Audit log.
//!
//! Security relevant events (authentication, device lifecycle, token reuse,
//! sync conflicts) are written to the `audit_log` table. The log is append
//! only and never contains credentials.

use deadpool_postgres::Client;
use serde_json::Value;
use uuid::Uuid;

/// Event names used by the server. Kept as constants so the log can be
/// queried by a stable name.
pub mod events {
    pub const REGISTER: &str = "user.register";
    pub const LOGIN_SUCCESS: &str = "auth.login.success";
    pub const LOGIN_FAILURE: &str = "auth.login.failure";
    pub const LOGIN_LOCKED: &str = "auth.login.locked";
    pub const LOGOUT: &str = "auth.logout";
    pub const TOKEN_REFRESH: &str = "auth.token.refresh";
    pub const TOKEN_REFRESH_RACE: &str = "auth.token.refresh.race";
    pub const TOKEN_REUSE: &str = "auth.token.reuse";
    pub const DEVICE_ENROLLED: &str = "device.enrolled";
    pub const DEVICE_RENAMED: &str = "device.renamed";
    pub const DEVICE_REVOKED: &str = "device.revoked";
    pub const SYNC_CONFLICT: &str = "sync.conflict";
    pub const RATE_LIMITED: &str = "security.rate_limited";
}

/// Everything that is recorded about one event.
#[derive(Debug, Clone)]
pub struct AuditEvent<'a> {
    /// The user the event belongs to (if known).
    pub user_id: Option<Uuid>,
    /// The device the event belongs to (if known).
    pub device_id: Option<Uuid>,
    /// Event name, see [`events`].
    pub event: &'a str,
    /// Free-form detail. Must not contain secrets.
    pub detail: Option<Value>,
    /// The client address the event came from.
    pub ip: Option<String>,
}

/// Writes one audit record. Audit failures are logged but never fail the
/// request that triggered them: losing an audit row must not break the API.
pub async fn record(client: &Client, event: AuditEvent<'_>) {
    let result = client
        .execute(
            "INSERT INTO audit_log (user_id, device_id, event, detail, ip)
             VALUES ($1, $2, $3, $4, $5)",
            &[
                &event.user_id,
                &event.device_id,
                &event.event,
                &event.detail,
                &event.ip,
            ],
        )
        .await;

    match result {
        Ok(_) => {}
        Err(error) => tracing::error!(
            event = event.event,
            %error,
            "failed to write an audit record"
        ),
    }
}
