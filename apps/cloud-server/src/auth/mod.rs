//! Authentication: sessions, tokens and the bearer-token extractor.

pub mod password;
pub mod tokens;

use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use chrono::{DateTime, Utc};
use deadpool_postgres::Client;
use uuid::Uuid;

use crate::config::Config;
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

/// The tokens handed to a client after a successful authentication.
#[derive(Debug, Clone)]
pub struct IssuedTokens {
    /// The bearer token for API calls.
    pub access_token: String,
    /// The token used to obtain a new pair.
    pub refresh_token: String,
    /// When the access token stops working.
    pub access_expires_at: DateTime<Utc>,
    /// When the refresh token stops working.
    pub refresh_expires_at: DateTime<Utc>,
}

/// The identity behind an authenticated request.
#[derive(Debug, Clone, Copy)]
pub struct AuthContext {
    /// The authenticated user.
    pub user_id: Uuid,
    /// The device the request came from.
    pub device_id: Uuid,
    /// The session the request came from.
    pub session_id: Uuid,
}

impl FromRequestParts<AppState> for AuthContext {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let token = crate::security::bearer_token(&parts.headers)
            .ok_or_else(|| ApiError::Unauthorized("missing bearer token".to_string()))?;
        tokens::validate_token_shape(token)?;

        let hash = tokens::hash_token(token);
        let now = state.clock.now();
        let client = state.pool.get().await?;

        let row = client
            .query_opt(
                "SELECT s.id, s.user_id, s.device_id
                   FROM sessions s
                   JOIN devices d ON d.id = s.device_id
                  WHERE s.access_hash = $1
                    AND s.revoked_at IS NULL
                    AND s.access_expires_at > $2
                    AND d.revoked_at IS NULL",
                &[&hash, &now],
            )
            .await?;

        let Some(row) = row else {
            return Err(ApiError::Unauthorized(
                "invalid or expired access token".to_string(),
            ));
        };

        let context = AuthContext {
            session_id: row.get("id"),
            user_id: row.get("user_id"),
            device_id: row.get("device_id"),
        };

        // Keep the device's "last seen" marker fresh without writing on
        // every single request.
        let _ = client
            .execute(
                "UPDATE devices
                    SET last_seen_at = $2
                  WHERE id = $1
                    AND (last_seen_at IS NULL OR last_seen_at < $2 - INTERVAL '60 seconds')",
                &[&context.device_id, &now],
            )
            .await;

        Ok(context)
    }
}

/// Creates a session and issues the first token pair.
pub async fn issue_session(
    client: &Client,
    user_id: Uuid,
    device_id: Uuid,
    config: &Config,
    now: DateTime<Utc>,
) -> ApiResult<IssuedTokens> {
    let session_id = Uuid::new_v4();
    let family_id = Uuid::new_v4();
    let access_token = tokens::generate_token();
    let refresh_token = tokens::generate_token();
    let access_expires_at =
        now + chrono::Duration::from_std(config.access_token_ttl).unwrap_or_default();
    let refresh_expires_at =
        now + chrono::Duration::from_std(config.refresh_token_ttl).unwrap_or_default();

    client
        .execute(
            "INSERT INTO sessions (id, user_id, device_id, family_id, access_hash, access_expires_at)
             VALUES ($1, $2, $3, $4, $5, $6)",
            &[
                &session_id,
                &user_id,
                &device_id,
                &family_id,
                &tokens::hash_token(&access_token),
                &access_expires_at,
            ],
        )
        .await?;

    client
        .execute(
            "INSERT INTO refresh_tokens (hash, session_id, family_id, user_id, expires_at)
             VALUES ($1, $2, $3, $4, $5)",
            &[
                &tokens::hash_token(&refresh_token),
                &session_id,
                &family_id,
                &user_id,
                &refresh_expires_at,
            ],
        )
        .await?;

    Ok(IssuedTokens {
        access_token,
        refresh_token,
        access_expires_at,
        refresh_expires_at,
    })
}

/// Exchanges a refresh token for a new token pair.
///
/// * A refresh token that was never used rotates normally.
/// * A refresh token that is replayed shortly after its use is treated as a
///   benign race (two tabs refreshing at the same moment) and rotates again.
/// * A refresh token that is replayed later is treated as a theft: the whole
///   token family is revoked.
pub async fn rotate_session(
    client: &Client,
    presented_refresh: &str,
    config: &Config,
    now: DateTime<Utc>,
) -> ApiResult<IssuedTokens> {
    tokens::validate_token_shape(presented_refresh)?;
    let hash = tokens::hash_token(presented_refresh);

    let row = client
        .query_opt(
            "SELECT hash, session_id, family_id, user_id, expires_at, used_at
               FROM refresh_tokens
              WHERE hash = $1",
            &[&hash],
        )
        .await?
        .ok_or_else(|| ApiError::Unauthorized("invalid refresh token".to_string()))?;

    let session_id: Uuid = row.get("session_id");
    let family_id: Uuid = row.get("family_id");
    let user_id: Uuid = row.get("user_id");
    let expires_at: DateTime<Utc> = row.get("expires_at");
    let used_at: Option<DateTime<Utc>> = row.get("used_at");

    if expires_at <= now {
        return Err(ApiError::Unauthorized("refresh token expired".to_string()));
    }

    if let Some(used_at) = used_at {
        let age = now.signed_duration_since(used_at);
        let grace = chrono::Duration::from_std(config.refresh_reuse_grace).unwrap_or_default();
        if age <= grace {
            // Benign race: rotate again for the same session.
            return rotate_for_session(client, session_id, user_id, config, now).await;
        }

        // Replay of an already used token: assume the token was stolen and
        // revoke every session of the family.
        revoke_family(client, family_id, "token_reuse_detected").await?;
        return Err(ApiError::Unauthorized(
            "refresh token reuse detected; the session family was revoked".to_string(),
        ));
    }

    client
        .execute(
            "UPDATE refresh_tokens SET used_at = $2 WHERE hash = $1",
            &[&hash, &now],
        )
        .await?;

    rotate_for_session(client, session_id, user_id, config, now).await
}

/// Issues a new token pair for an existing session.
async fn rotate_for_session(
    client: &Client,
    session_id: Uuid,
    user_id: Uuid,
    config: &Config,
    now: DateTime<Utc>,
) -> ApiResult<IssuedTokens> {
    let access_token = tokens::generate_token();
    let refresh_token = tokens::generate_token();
    let access_expires_at =
        now + chrono::Duration::from_std(config.access_token_ttl).unwrap_or_default();
    let refresh_expires_at =
        now + chrono::Duration::from_std(config.refresh_token_ttl).unwrap_or_default();

    client
        .execute(
            "UPDATE sessions
                SET access_hash = $2, access_expires_at = $3
              WHERE id = $1 AND revoked_at IS NULL",
            &[
                &session_id,
                &tokens::hash_token(&access_token),
                &access_expires_at,
            ],
        )
        .await?;

    let family_id: Uuid = client
        .query_one(
            "SELECT family_id FROM sessions WHERE id = $1",
            &[&session_id],
        )
        .await?
        .get("family_id");

    client
        .execute(
            "INSERT INTO refresh_tokens (hash, session_id, family_id, user_id, expires_at)
             VALUES ($1, $2, $3, $4, $5)",
            &[
                &tokens::hash_token(&refresh_token),
                &session_id,
                &family_id,
                &user_id,
                &refresh_expires_at,
            ],
        )
        .await?;

    Ok(IssuedTokens {
        access_token,
        refresh_token,
        access_expires_at,
        refresh_expires_at,
    })
}

/// Revokes every session of a token family.
pub async fn revoke_family(client: &Client, family_id: Uuid, reason: &str) -> ApiResult<()> {
    client
        .execute(
            "UPDATE sessions
                SET revoked_at = now(), revoked_reason = $2
              WHERE family_id = $1 AND revoked_at IS NULL",
            &[&family_id, &reason],
        )
        .await?;
    Ok(())
}

/// Revokes the session behind a refresh token (logout).
pub async fn revoke_session_by_refresh(
    client: &Client,
    presented_refresh: &str,
) -> ApiResult<Uuid> {
    tokens::validate_token_shape(presented_refresh)?;
    let hash = tokens::hash_token(presented_refresh);

    let row = client
        .query_opt(
            "SELECT session_id, family_id FROM refresh_tokens WHERE hash = $1",
            &[&hash],
        )
        .await?
        .ok_or_else(|| ApiError::Unauthorized("invalid refresh token".to_string()))?;

    let session_id: Uuid = row.get("session_id");
    let family_id: Uuid = row.get("family_id");
    revoke_family(client, family_id, "logout").await?;
    Ok(session_id)
}

/// Looks up the user id for an email address.
pub async fn find_user_by_email(client: &Client, email: &str) -> ApiResult<Option<(Uuid, String)>> {
    let row = client
        .query_opt(
            "SELECT id, password_hash FROM users WHERE email = $1",
            &[&email],
        )
        .await?;
    Ok(row.map(|row| (row.get("id"), row.get("password_hash"))))
}

/// Rejects a login while the identifier is locked out.
pub async fn check_login_lock(client: &Client, key: &str, now: DateTime<Utc>) -> ApiResult<()> {
    let row = client
        .query_opt(
            "SELECT locked_until FROM login_attempts WHERE key = $1",
            &[&key],
        )
        .await?;
    if let Some(row) = row
        && let Some(locked_until) = row.get::<_, Option<DateTime<Utc>>>("locked_until")
        && locked_until > now
    {
        return Err(ApiError::TooManyRequests(
            "too many failed attempts; try again later".to_string(),
        ));
    }
    Ok(())
}

/// Counts a failed login and locks the identifier when the budget is spent.
pub async fn record_login_failure(
    client: &Client,
    key: &str,
    config: &Config,
    now: DateTime<Utc>,
) -> ApiResult<()> {
    let lockout = chrono::Duration::from_std(config.login_lockout).unwrap_or_default();
    let locked_until = now + lockout;
    client
        .execute(
            "INSERT INTO login_attempts (key, failures, window_start, locked_until)
             VALUES ($1, 1, $2, NULL)
             ON CONFLICT (key) DO UPDATE
                SET failures = CASE
                      WHEN login_attempts.window_start < $2::timestamptz - INTERVAL '15 minutes' THEN 1
                      ELSE login_attempts.failures + 1
                    END,
                    window_start = CASE
                      WHEN login_attempts.window_start < $2::timestamptz - INTERVAL '15 minutes' THEN $2
                      ELSE login_attempts.window_start
                    END,
                    locked_until = CASE
                      WHEN (CASE
                              WHEN login_attempts.window_start < $2::timestamptz - INTERVAL '15 minutes' THEN 1
                              ELSE login_attempts.failures + 1
                            END) >= $3
                      THEN $4
                      ELSE login_attempts.locked_until
                    END",
            &[
                &key,
                &now,
                &(config.login_max_failures as i32),
                &locked_until,
            ],
        )
        .await?;
    Ok(())
}

/// Clears the failure counter of an identifier after a successful login.
pub async fn clear_login_failures(client: &Client, key: &str) -> ApiResult<()> {
    client
        .execute("DELETE FROM login_attempts WHERE key = $1", &[&key])
        .await?;
    Ok(())
}

/// Normalises an email address for storage and lookup.
pub fn normalize_email(email: &str) -> String {
    email.trim().to_lowercase()
}

/// Rejects obviously invalid email addresses.
pub fn validate_email(email: &str) -> ApiResult<()> {
    let trimmed = email.trim();
    if trimmed.len() < 3 || trimmed.len() > 254 {
        return Err(ApiError::BadRequest(
            "the email address has an invalid length".to_string(),
        ));
    }
    let Some((local, domain)) = trimmed.split_once('@') else {
        return Err(ApiError::BadRequest(
            "the email address is not valid".to_string(),
        ));
    };
    if local.is_empty() || domain.is_empty() || !domain.contains('.') {
        return Err(ApiError::BadRequest(
            "the email address is not valid".to_string(),
        ));
    }
    if trimmed.chars().any(char::is_whitespace) {
        return Err(ApiError::BadRequest(
            "the email address must not contain whitespace".to_string(),
        ));
    }
    Ok(())
}
