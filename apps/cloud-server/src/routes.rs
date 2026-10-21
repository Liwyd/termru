//! HTTP routes.
//!
//! The API is intentionally small:
//!
//! | Method | Path                  | Purpose                                   |
//! |--------|-----------------------|-------------------------------------------|
//! | GET    | `/healthz`            | liveness                                  |
//! | GET    | `/readyz`             | readiness (checks the database)           |
//! | POST   | `/v1/auth/register`   | create an account and a first device      |
//! | POST   | `/v1/auth/login`      | sign in                                   |
//! | POST   | `/v1/auth/refresh`    | rotate a token pair                       |
//! | POST   | `/v1/auth/logout`     | revoke the session of the caller          |
//! | GET    | `/v1/devices`         | list the devices of the caller            |
//! | POST   | `/v1/devices`         | enroll an additional device               |
//! | PATCH  | `/v1/devices/{id}`    | rename a device                           |
//! | DELETE | `/v1/devices/{id}`    | revoke a device                           |
//! | GET    | `/v1/sync/changes`    | pull the changed records                  |
//! | POST   | `/v1/sync/changes`    | push a batch of mutations                 |

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, patch, post};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio_postgres::error::SqlState;
use uuid::Uuid;

use crate::audit::{self, AuditEvent, events};
use crate::auth::{self, AuthContext, IssuedTokens};
use crate::db;
use crate::error::{ApiError, ApiResult};
use crate::extract::{JsonBody, invalid_query};
use crate::security::ClientIp;
use crate::state::AppState;
use crate::sync;

/// Builds the application router.
pub fn router(state: AppState) -> Router {
    let public = Router::new()
        .route("/healthz", get(healthz))
        .route("/readyz", get(readyz))
        .route("/v1/auth/register", post(register))
        .route("/v1/auth/login", post(login))
        .route("/v1/auth/refresh", post(refresh));

    let private = Router::new()
        .route("/v1/auth/logout", post(logout))
        .route("/v1/devices", get(list_devices).post(enroll_device))
        .route(
            "/v1/devices/{id}",
            patch(rename_device).delete(revoke_device),
        )
        .route("/v1/sync/changes", get(pull_changes).post(push_changes));

    Router::new()
        .merge(public)
        .merge(private)
        .layer(axum::extract::DefaultBodyLimit::max(
            state.config.max_body_bytes,
        ))
        .layer(tower_http::timeout::TimeoutLayer::with_status_code(
            axum::http::StatusCode::GATEWAY_TIMEOUT,
            state.config.request_timeout,
        ))
        .layer(middleware::from_fn_with_state(state.clone(), rate_limit))
        .layer(middleware::from_fn(security_headers))
        .with_state(state)
}

// ---------------------------------------------------------------------------
// Middleware
// ---------------------------------------------------------------------------

/// Adds the security headers to every response.
async fn security_headers(request: axum::extract::Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    crate::security::apply_security_headers(response.headers_mut());
    response
}

/// Limits the number of requests per client.
async fn rate_limit(
    State(state): State<AppState>,
    request: axum::extract::Request,
    next: Next,
) -> Response {
    let ip = client_ip(&request);
    let key = rate_limit_key(&request, &ip);

    if let Some(retry_after) = state.limiter.check(&key) {
        tracing::warn!(%ip, key = %key, "request rejected by the rate limiter");
        let body = json!({
            "error": {
                "code": "too_many_requests",
                "message": "too many requests; slow down",
            }
        });
        return (
            StatusCode::TOO_MANY_REQUESTS,
            [(header::RETRY_AFTER, retry_after.to_string())],
            Json(body),
        )
            .into_response();
    }

    next.run(request).await
}

/// The address a request came from.
fn client_ip(request: &axum::extract::Request) -> String {
    request
        .extensions()
        .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
        .map(|info| info.0.ip().to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

/// The key the rate limiter counts against. Authenticated calls are counted
/// per token, anonymous calls per address.
fn rate_limit_key(request: &axum::extract::Request, ip: &str) -> String {
    match request.headers().get(header::AUTHORIZATION) {
        Some(value) if value.len() <= 512 => {
            let digest = crate::auth::tokens::hash_bytes(value.as_bytes());
            format!("token:{}", hex::encode(&digest[..8]))
        }
        _ => format!("ip:{ip}"),
    }
}

// ---------------------------------------------------------------------------
// Health
// ---------------------------------------------------------------------------

/// Liveness probe: the process is up.
async fn healthz() -> impl IntoResponse {
    (StatusCode::OK, "ok")
}

/// Readiness probe: the database is reachable.
async fn readyz(State(state): State<AppState>) -> Response {
    match db::check_ready(&state.pool).await {
        Ok(()) => (StatusCode::OK, "ready").into_response(),
        Err(error) => {
            tracing::warn!(%error, "readiness check failed");
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({"error": {"code": "service_unavailable", "message": "the database is not reachable"}})),
            )
                .into_response()
        }
    }
}

// ---------------------------------------------------------------------------
// Authentication
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct Credentials {
    email: String,
    password: String,
    /// Stable, client generated id of the installation.
    device_id: Uuid,
    /// Human readable name of the installation.
    device_name: String,
}

#[derive(Debug, Deserialize)]
struct RegisterRequest {
    email: String,
    password: String,
    device_id: Uuid,
    device_name: String,
}

#[derive(Debug, Deserialize)]
struct RefreshRequest {
    refresh_token: String,
}

#[derive(Debug, Serialize)]
struct AuthResponse {
    user_id: Uuid,
    device_id: Uuid,
    access_token: String,
    refresh_token: String,
    access_expires_at: DateTime<Utc>,
    refresh_expires_at: DateTime<Utc>,
}

impl AuthResponse {
    fn new(user_id: Uuid, device_id: Uuid, tokens: IssuedTokens) -> Self {
        Self {
            user_id,
            device_id,
            access_token: tokens.access_token,
            refresh_token: tokens.refresh_token,
            access_expires_at: tokens.access_expires_at,
            refresh_expires_at: tokens.refresh_expires_at,
        }
    }
}

/// Creates an account and signs in the first device.
async fn register(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    JsonBody(request): JsonBody<RegisterRequest>,
) -> ApiResult<Json<AuthResponse>> {
    auth::validate_email(&request.email)?;
    auth::password::validate_password(&request.password)?;
    validate_device_name(&request.device_name)?;

    let email = auth::normalize_email(&request.email);
    let now = state.clock.now();
    let user_id = Uuid::new_v4();
    let client = state.pool.get().await?;

    let inserted = client
        .execute(
            "INSERT INTO users (id, email, password_hash) VALUES ($1, $2, $3)",
            &[
                &user_id,
                &email,
                &auth::password::hash_password(&request.password)?,
            ],
        )
        .await;

    if let Err(error) = inserted {
        if error.code() == Some(&SqlState::UNIQUE_VIOLATION) {
            return Err(ApiError::Conflict(
                "an account with this email address already exists".to_string(),
            ));
        }
        return Err(ApiError::Internal(anyhow::Error::new(error)));
    }

    upsert_device(&client, user_id, request.device_id, &request.device_name).await?;
    let tokens =
        auth::issue_session(&client, user_id, request.device_id, &state.config, now).await?;

    audit::record(
        &client,
        AuditEvent {
            user_id: Some(user_id),
            device_id: Some(request.device_id),
            event: events::REGISTER,
            detail: Some(json!({ "email": email })),
            ip: Some(ip),
        },
    )
    .await;

    Ok(Json(AuthResponse::new(user_id, request.device_id, tokens)))
}

/// Signs in and issues a token pair.
async fn login(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    JsonBody(request): JsonBody<Credentials>,
) -> ApiResult<Json<AuthResponse>> {
    auth::validate_email(&request.email)?;
    validate_device_name(&request.device_name)?;

    let email = auth::normalize_email(&request.email);
    let now = state.clock.now();
    let lock_key = format!("login:{email}");

    let client = state.pool.get().await?;
    auth::check_login_lock(&client, &lock_key, now).await?;

    let stored = auth::find_user_by_email(&client, &email).await?;
    let Some((user_id, password_hash)) = stored else {
        // Unknown account: spend the same work as a real verification so the
        // response time does not reveal whether the account exists.
        auth::password::verify_against_dummy(&request.password);
        auth::record_login_failure(&client, &lock_key, &state.config, now).await?;
        audit::record(
            &client,
            AuditEvent {
                user_id: None,
                device_id: Some(request.device_id),
                event: events::LOGIN_FAILURE,
                detail: Some(json!({ "email": email, "reason": "unknown_account" })),
                ip: Some(ip),
            },
        )
        .await;
        return Err(ApiError::Unauthorized(
            "invalid email address or password".to_string(),
        ));
    };

    if !auth::password::verify_password(&request.password, &password_hash) {
        auth::record_login_failure(&client, &lock_key, &state.config, now).await?;
        audit::record(
            &client,
            AuditEvent {
                user_id: Some(user_id),
                device_id: Some(request.device_id),
                event: events::LOGIN_FAILURE,
                detail: Some(json!({ "email": email, "reason": "bad_password" })),
                ip: Some(ip),
            },
        )
        .await;
        return Err(ApiError::Unauthorized(
            "invalid email address or password".to_string(),
        ));
    }

    auth::clear_login_failures(&client, &lock_key).await?;
    upsert_device(&client, user_id, request.device_id, &request.device_name).await?;
    let tokens =
        auth::issue_session(&client, user_id, request.device_id, &state.config, now).await?;

    // A successful login clears the request budget of this address.
    state.limiter.forget(&format!("ip:{ip}"));

    audit::record(
        &client,
        AuditEvent {
            user_id: Some(user_id),
            device_id: Some(request.device_id),
            event: events::LOGIN_SUCCESS,
            detail: Some(json!({ "email": email })),
            ip: Some(ip),
        },
    )
    .await;

    Ok(Json(AuthResponse::new(user_id, request.device_id, tokens)))
}

/// Exchanges a refresh token for a new token pair.
async fn refresh(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    JsonBody(request): JsonBody<RefreshRequest>,
) -> ApiResult<Json<AuthResponse>> {
    let now = state.clock.now();
    let client = state.pool.get().await?;

    let tokens = auth::rotate_session(&client, &request.refresh_token, &state.config, now).await?;

    let user_id: Uuid = client
        .query_one(
            "SELECT user_id FROM sessions WHERE access_hash = $1",
            &[&auth::tokens::hash_token(&tokens.access_token)],
        )
        .await
        .map(|row| row.get("user_id"))
        .unwrap_or(Uuid::nil());

    audit::record(
        &client,
        AuditEvent {
            user_id: Some(user_id),
            device_id: None,
            event: events::TOKEN_REFRESH,
            detail: None,
            ip: Some(ip),
        },
    )
    .await;

    Ok(Json(AuthResponse {
        user_id,
        device_id: Uuid::nil(),
        access_token: tokens.access_token,
        refresh_token: tokens.refresh_token,
        access_expires_at: tokens.access_expires_at,
        refresh_expires_at: tokens.refresh_expires_at,
    }))
}

/// Revokes the session family of the caller.
async fn logout(
    State(state): State<AppState>,
    context: AuthContext,
    ClientIp(ip): ClientIp,
) -> ApiResult<StatusCode> {
    let client = state.pool.get().await?;
    let family_id: Uuid = client
        .query_one(
            "SELECT family_id FROM sessions WHERE id = $1",
            &[&context.session_id],
        )
        .await?
        .get("family_id");

    auth::revoke_family(&client, family_id, "logout").await?;

    audit::record(
        &client,
        AuditEvent {
            user_id: Some(context.user_id),
            device_id: Some(context.device_id),
            event: events::LOGOUT,
            detail: None,
            ip: Some(ip),
        },
    )
    .await;

    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// Devices
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
struct DeviceView {
    id: Uuid,
    name: String,
    created_at: DateTime<Utc>,
    revoked_at: Option<DateTime<Utc>>,
    last_seen_at: Option<DateTime<Utc>>,
    /// True for the device the request came from.
    current: bool,
}

#[derive(Debug, Deserialize)]
struct EnrollRequest {
    name: String,
}

#[derive(Debug, Deserialize)]
struct RenameRequest {
    name: String,
}

/// Lists the devices of the caller.
async fn list_devices(
    State(state): State<AppState>,
    context: AuthContext,
) -> ApiResult<Json<Vec<DeviceView>>> {
    let client = state.pool.get().await?;
    let rows = client
        .query(
            "SELECT id, name, created_at, revoked_at, last_seen_at
               FROM devices
              WHERE user_id = $1
              ORDER BY created_at ASC",
            &[&context.user_id],
        )
        .await?;

    let devices = rows
        .iter()
        .map(|row| DeviceView {
            id: row.get("id"),
            name: row.get("name"),
            created_at: row.get("created_at"),
            revoked_at: row.get("revoked_at"),
            last_seen_at: row.get("last_seen_at"),
            current: row.get::<_, Uuid>("id") == context.device_id,
        })
        .collect();

    Ok(Json(devices))
}

/// Enrolls an additional device and returns a token pair for it.
async fn enroll_device(
    State(state): State<AppState>,
    context: AuthContext,
    ClientIp(ip): ClientIp,
    JsonBody(request): JsonBody<EnrollRequest>,
) -> ApiResult<Json<AuthResponse>> {
    validate_device_name(&request.name)?;
    let now = state.clock.now();
    let device_id = Uuid::new_v4();
    let client = state.pool.get().await?;

    client
        .execute(
            "INSERT INTO devices (id, user_id, name) VALUES ($1, $2, $3)",
            &[&device_id, &context.user_id, &request.name],
        )
        .await?;

    let tokens =
        auth::issue_session(&client, context.user_id, device_id, &state.config, now).await?;

    audit::record(
        &client,
        AuditEvent {
            user_id: Some(context.user_id),
            device_id: Some(device_id),
            event: events::DEVICE_ENROLLED,
            detail: Some(json!({ "name": request.name })),
            ip: Some(ip),
        },
    )
    .await;

    Ok(Json(AuthResponse::new(context.user_id, device_id, tokens)))
}

/// Renames a device.
async fn rename_device(
    State(state): State<AppState>,
    context: AuthContext,
    Path(device_id): Path<Uuid>,
    JsonBody(request): JsonBody<RenameRequest>,
) -> ApiResult<StatusCode> {
    validate_device_name(&request.name)?;
    let client = state.pool.get().await?;

    let updated = client
        .execute(
            "UPDATE devices SET name = $3 WHERE id = $1 AND user_id = $2",
            &[&device_id, &context.user_id, &request.name],
        )
        .await?;

    if updated == 0 {
        return Err(ApiError::NotFound("no such device".to_string()));
    }
    Ok(StatusCode::NO_CONTENT)
}

/// Revokes a device and every session that belongs to it.
async fn revoke_device(
    State(state): State<AppState>,
    context: AuthContext,
    Path(device_id): Path<Uuid>,
    ClientIp(ip): ClientIp,
) -> ApiResult<StatusCode> {
    let client = state.pool.get().await?;

    let updated = client
        .execute(
            "UPDATE devices SET revoked_at = now() WHERE id = $1 AND user_id = $2 AND revoked_at IS NULL",
            &[&device_id, &context.user_id],
        )
        .await?;

    if updated == 0 {
        return Err(ApiError::NotFound("no such device".to_string()));
    }

    client
        .execute(
            "UPDATE sessions SET revoked_at = now(), revoked_reason = 'device_revoked'
              WHERE device_id = $1 AND revoked_at IS NULL",
            &[&device_id],
        )
        .await?;

    audit::record(
        &client,
        AuditEvent {
            user_id: Some(context.user_id),
            device_id: Some(device_id),
            event: events::DEVICE_REVOKED,
            detail: None,
            ip: Some(ip),
        },
    )
    .await;

    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// Sync
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct PullQuery {
    since: Option<i64>,
    limit: Option<i64>,
}

/// Returns the records that changed since the given revision.
async fn pull_changes(
    State(state): State<AppState>,
    context: AuthContext,
    Query(query): Query<PullQuery>,
) -> ApiResult<Json<termru_core::sync::PullResponse>> {
    let since = query.since.unwrap_or(0);
    if since < 0 {
        return Err(invalid_query("since"));
    }
    let limit = query.limit.unwrap_or(sync::DEFAULT_PULL_LIMIT);
    if !(1..=sync::MAX_PULL_LIMIT).contains(&limit) {
        return Err(invalid_query("limit"));
    }

    let client = state.pool.get().await?;
    let response = sync::pull(&client, context.user_id, since, limit).await?;
    Ok(Json(response))
}

/// Applies a batch of mutations.
async fn push_changes(
    State(state): State<AppState>,
    context: AuthContext,
    ClientIp(ip): ClientIp,
    JsonBody(request): JsonBody<termru_core::sync::PushRequest>,
) -> ApiResult<Json<termru_core::sync::PushResponse>> {
    let mut client = state.pool.get().await?;
    let response = sync::push(&mut client, context.user_id, request).await?;

    if !response.conflicts.is_empty() {
        audit::record(
            &client,
            AuditEvent {
                user_id: Some(context.user_id),
                device_id: Some(context.device_id),
                event: events::SYNC_CONFLICT,
                detail: Some(json!({
                    "conflicts": response
                        .conflicts
                        .iter()
                        .map(|c| json!({ "id": c.id, "kind": c.kind.as_str() }))
                        .collect::<Vec<_>>(),
                })),
                ip: Some(ip),
            },
        )
        .await;
    }

    Ok(Json(response))
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Creates or updates the device of a login.
///
/// A device id that belongs to another account is rejected; a revoked device
/// stays revoked, so a stolen device id can not be used to regain access.
async fn upsert_device(
    client: &deadpool_postgres::Client,
    user_id: Uuid,
    device_id: Uuid,
    name: &str,
) -> ApiResult<()> {
    let existing = client
        .query_opt(
            "SELECT user_id, revoked_at FROM devices WHERE id = $1",
            &[&device_id],
        )
        .await?;

    match existing {
        Some(row) => {
            let owner: Uuid = row.get("user_id");
            if owner != user_id {
                return Err(ApiError::Forbidden(
                    "this device id is already in use".to_string(),
                ));
            }
            if row.get::<_, Option<DateTime<Utc>>>("revoked_at").is_some() {
                return Err(ApiError::Forbidden(
                    "this device was revoked; enroll a new device to continue".to_string(),
                ));
            }
            client
                .execute(
                    "UPDATE devices SET name = $2 WHERE id = $1",
                    &[&device_id, &name],
                )
                .await?;
        }
        None => {
            client
                .execute(
                    "INSERT INTO devices (id, user_id, name) VALUES ($1, $2, $3)",
                    &[&device_id, &user_id, &name],
                )
                .await?;
        }
    }
    Ok(())
}

/// Checks the device name.
fn validate_device_name(name: &str) -> ApiResult<()> {
    let trimmed = name.trim();
    if trimmed.is_empty() || trimmed.chars().count() > 128 {
        return Err(ApiError::BadRequest(
            "the device name must have between 1 and 128 characters".to_string(),
        ));
    }
    if trimmed.chars().any(char::is_control) {
        return Err(ApiError::BadRequest(
            "the device name must not contain control characters".to_string(),
        ));
    }
    Ok(())
}

/// Reads a header for tests.
#[allow(dead_code)]
fn header_value(headers: &HeaderMap, name: &str) -> Option<String> {
    headers.get(name)?.to_str().ok().map(str::to_string)
}
