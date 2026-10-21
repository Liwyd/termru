//! The revision based sync protocol (see `docs/architecture/0005-sync-protocol.md`).
//!
//! * **Pull** returns every record of the user with `rev > since`, ordered by
//!   revision, including tombstones.
//! * **Push** applies a batch of mutations. Each mutation carries the
//!   revision the client based it on (`base_rev`); a stale `base_rev` is a
//!   conflict and is reported back — the server never resolves a conflict on
//!   the client's behalf.
//! * Mutations carry a client generated `mutation_id`; applying the same
//!   mutation twice is a no-op (idempotent retries).

use chrono::{DateTime, Utc};
use deadpool_postgres::Client;
use termru_core::sync::{
    AcceptedMutation, ConflictEntry, PullResponse, PushRequest, PushResponse, RecordKind,
    WireRecord,
};
use tokio_postgres::Transaction;
use uuid::Uuid;

use crate::error::{ApiError, ApiResult};

/// Maximum number of records a single pull may return.
pub const MAX_PULL_LIMIT: i64 = termru_core::sync::MAX_PULL_RECORDS as i64;

/// Default number of records a pull returns.
pub const DEFAULT_PULL_LIMIT: i64 = 100;

/// Returns the records that changed after `since`.
pub async fn pull(
    client: &Client,
    user_id: Uuid,
    since: i64,
    limit: i64,
) -> ApiResult<PullResponse> {
    if since < 0 {
        return Err(ApiError::BadRequest(
            "since must not be negative".to_string(),
        ));
    }
    let limit = limit.clamp(1, MAX_PULL_LIMIT);

    // One extra row tells us whether another page follows.
    let rows = client
        .query(
            "SELECT kind, id, rev, deleted, payload, updated_at
               FROM records
              WHERE user_id = $1 AND rev > $2
              ORDER BY rev ASC
              LIMIT $3",
            &[&user_id, &since, &(limit + 1)],
        )
        .await?;

    let has_more = rows.len() > limit as usize;
    let mut records = Vec::new();
    let mut next_rev = since;

    for row in rows.iter().take(limit as usize) {
        let kind_text: String = row.get("kind");
        let Some(kind) = RecordKind::parse(&kind_text) else {
            // A record of an unknown kind was written by a newer client.
            // Ignoring it keeps this server forward compatible.
            tracing::warn!(kind = %kind_text, "ignoring a record of an unknown kind");
            continue;
        };
        let rev: i64 = row.get("rev");
        let deleted: bool = row.get("deleted");
        let payload: Option<serde_json::Value> = row.get("payload");
        let updated_at: DateTime<Utc> = row.get("updated_at");
        let id: String = row.get("id");

        next_rev = next_rev.max(rev);
        records.push(WireRecord {
            id,
            kind,
            rev,
            updated_at,
            deleted,
            payload,
        });
    }

    Ok(PullResponse {
        records,
        next_rev,
        has_more,
        server_time: Utc::now(),
    })
}

/// Applies a batch of mutations.
pub async fn push(
    client: &mut Client,
    user_id: Uuid,
    request: PushRequest,
) -> ApiResult<PushResponse> {
    if request.mutations.len() > termru_core::sync::MAX_BATCH_MUTATIONS {
        return Err(ApiError::BadRequest(format!(
            "a batch may contain at most {} mutations",
            termru_core::sync::MAX_BATCH_MUTATIONS
        )));
    }

    let tx = client.transaction().await?;
    let mut accepted = Vec::new();
    let mut conflicts = Vec::new();

    for mutation in &request.mutations {
        mutation
            .validate_structure()
            .map_err(|e| ApiError::BadRequest(e.to_string()))?;

        // Idempotency: a mutation that was applied before is acknowledged
        // with the revision it received the first time.
        if let Some(rev) = applied_revision(&tx, user_id, mutation.mutation_id).await? {
            accepted.push(AcceptedMutation {
                mutation_id: mutation.mutation_id,
                id: mutation.id.clone(),
                rev,
            });
            continue;
        }

        let current = load_record(&tx, user_id, mutation.kind, &mutation.id).await?;

        let conflict = match (&current, mutation.base_rev) {
            // The record exists but the client believed it did not.
            (Some(_), None) => true,
            // The record exists with a different revision than the client saw.
            (Some(record), Some(base_rev)) => record.rev != base_rev,
            // The client knew the record but the server does not have it
            // any more: the record was deleted in the meantime.
            (None, Some(_)) => true,
            (None, None) => false,
        };

        if conflict {
            conflicts.push(ConflictEntry {
                mutation_id: mutation.mutation_id,
                id: mutation.id.clone(),
                kind: mutation.kind,
                server_record: current.map(|record| record.to_wire()),
            });
            continue;
        }

        let rev = next_rev(&tx, user_id).await?;
        let payload = if mutation.deleted {
            None
        } else {
            mutation.payload.clone()
        };

        tx.execute(
            "INSERT INTO records (user_id, kind, id, rev, deleted, payload, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7)
             ON CONFLICT (user_id, kind, id) DO UPDATE
                SET rev = EXCLUDED.rev,
                    deleted = EXCLUDED.deleted,
                    payload = EXCLUDED.payload,
                    updated_at = EXCLUDED.updated_at",
            &[
                &user_id,
                &mutation.kind.as_str(),
                &mutation.id,
                &rev,
                &mutation.deleted,
                &payload,
                &mutation.updated_at,
            ],
        )
        .await?;

        tx.execute(
            "INSERT INTO applied_mutations (user_id, mutation_id, rev) VALUES ($1, $2, $3)",
            &[&user_id, &mutation.mutation_id, &rev],
        )
        .await?;

        accepted.push(AcceptedMutation {
            mutation_id: mutation.mutation_id,
            id: mutation.id.clone(),
            rev,
        });
    }

    tx.commit().await?;

    Ok(PushResponse {
        accepted,
        conflicts,
        server_time: Utc::now(),
    })
}

/// The record as the server stores it.
#[derive(Debug, Clone)]
struct StoredRecord {
    id: String,
    kind: RecordKind,
    rev: i64,
    deleted: bool,
    payload: Option<serde_json::Value>,
    updated_at: DateTime<Utc>,
}

impl StoredRecord {
    fn to_wire(&self) -> WireRecord {
        WireRecord {
            id: self.id.clone(),
            kind: self.kind,
            rev: self.rev,
            updated_at: self.updated_at,
            deleted: self.deleted,
            payload: self.payload.clone(),
        }
    }
}

async fn load_record(
    tx: &Transaction<'_>,
    user_id: Uuid,
    kind: RecordKind,
    id: &str,
) -> ApiResult<Option<StoredRecord>> {
    let row = tx
        .query_opt(
            "SELECT rev, deleted, payload, updated_at
               FROM records
              WHERE user_id = $1 AND kind = $2 AND id = $3",
            &[&user_id, &kind.as_str(), &id],
        )
        .await?;

    Ok(row.map(|row| StoredRecord {
        id: id.to_string(),
        kind,
        rev: row.get("rev"),
        deleted: row.get("deleted"),
        payload: row.get("payload"),
        updated_at: row.get("updated_at"),
    }))
}

async fn applied_revision(
    tx: &Transaction<'_>,
    user_id: Uuid,
    mutation_id: Uuid,
) -> ApiResult<Option<i64>> {
    let row = tx
        .query_opt(
            "SELECT rev FROM applied_mutations WHERE user_id = $1 AND mutation_id = $2",
            &[&user_id, &mutation_id],
        )
        .await?;
    Ok(row.map(|row| row.get("rev")))
}

/// Allocates the next revision of the user.
///
/// The update locks the user row until the transaction ends, which serialises
/// concurrent pushes of the same user and keeps the revision sequence gapless
/// and monotonic.
async fn next_rev(tx: &Transaction<'_>, user_id: Uuid) -> ApiResult<i64> {
    let row = tx
        .query_one(
            "UPDATE users SET next_rev = next_rev + 1 WHERE id = $1 RETURNING next_rev",
            &[&user_id],
        )
        .await?;
    Ok(row.get("next_rev"))
}
