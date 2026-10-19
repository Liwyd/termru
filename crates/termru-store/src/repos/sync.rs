//! Sync queue, cursor, remote application, and conflict bookkeeping.
//!
//! This module encodes the invariants described in
//! `docs/architecture/0005-sync-protocol.md`:
//!
//! * Local edits write the row **and** the queue entry in one transaction.
//! * The push payload is materialized from the live row at push time, so a
//!   retry always carries the latest state; `base_rev` comes from the row's
//!   last observed server revision.
//! * A record with an open conflict is never pushed until the user resolves
//!   it (silent overwrite is impossible by construction).
//! * Remote records are applied only when `wire.rev` is newer than the locally
//!   observed revision and the record is not locally modified; otherwise a
//!   conflict is recorded for explicit resolution.

use rusqlite::{OptionalExtension, Row};

use termru_core::sync::{PREFERENCES_RECORD_ID, RecordKind, WireRecord};
use termru_core::{Group, Host, Identity, Tag};

use crate::Store;
use crate::error::StoreError;
use crate::repos::conflicts::Conflicts;
use crate::repos::remove_queued;
use crate::util::{datetime_from_ms, now_ms};

pub struct Sync<'a> {
    pub(crate) store: &'a Store,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PendingMutation {
    pub mutation_id: uuid::Uuid,
    pub record_id: String,
    pub kind: RecordKind,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PushRecord {
    pub mutation_id: uuid::Uuid,
    pub record_id: String,
    pub kind: RecordKind,
    pub deleted: bool,
    /// Last server revision this device observed for the record.
    pub base_rev: Option<i64>,
    /// Full record payload (None for deletions).
    pub payload: Option<serde_json::Value>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SyncMeta {
    pub cursor: i64,
    pub last_sync_at: Option<chrono::DateTime<chrono::Utc>>,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApplyOutcome {
    /// Remote state stored locally.
    Applied,
    /// Remote tombstone applied (local row removed).
    RemoteDeleted,
    /// Local copy was modified; conflict recorded for explicit resolution.
    Conflict,
    /// Record already up to date locally (stale or duplicate delivery).
    Ignored,
    /// Payload failed validation; nothing was written.
    InvalidPayload(String),
    /// Record id exists under a different kind (protocol violation).
    KindMismatch,
}

struct LocalState {
    exists: bool,
    deleted: bool,
    server_rev: Option<i64>,
    dirty: bool,
}

fn local_state(
    conn: &rusqlite::Connection,
    kind: RecordKind,
    id: &str,
) -> Result<LocalState, StoreError> {
    let (table, extra_filter) = match kind {
        RecordKind::Host => ("hosts", ""),
        RecordKind::Group => ("groups", ""),
        RecordKind::Tag => ("tags", ""),
        RecordKind::Identity => ("identities", ""),
        RecordKind::Preferences => {
            if id != PREFERENCES_RECORD_ID {
                return Ok(LocalState {
                    exists: false,
                    deleted: false,
                    server_rev: None,
                    dirty: false,
                });
            }
            let row: Option<(Option<i64>, i64)> = conn
                .query_row(
                    "SELECT server_rev, dirty FROM preferences WHERE key = 'portable'",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()
                .map_err(StoreError::Sqlite)?;
            return Ok(match row {
                Some((rev, dirty)) => LocalState {
                    exists: true,
                    deleted: false,
                    server_rev: rev,
                    dirty: dirty != 0,
                },
                None => LocalState {
                    exists: false,
                    deleted: false,
                    server_rev: None,
                    dirty: false,
                },
            });
        }
    };
    let sql =
        format!("SELECT deleted, server_rev, dirty FROM {table} WHERE id = ?1 {extra_filter}");
    let row: Option<(i64, Option<i64>, i64)> = conn
        .query_row(&sql, [id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .optional()
        .map_err(StoreError::Sqlite)?;
    Ok(match row {
        Some((deleted, server_rev, dirty)) => LocalState {
            exists: true,
            deleted: deleted != 0,
            server_rev,
            dirty: dirty != 0,
        },
        None => LocalState {
            exists: false,
            deleted: false,
            server_rev: None,
            dirty: false,
        },
    })
}

/// Defense against a hostile server sending a record id that exists under a
/// different kind (which could confuse per-kind storage).
fn id_exists_in_other_kind(
    conn: &rusqlite::Connection,
    kind: RecordKind,
    id: &str,
) -> Result<bool, StoreError> {
    if kind == RecordKind::Preferences || id == PREFERENCES_RECORD_ID {
        return Ok(false);
    }
    for (other, table) in [
        (RecordKind::Host, "hosts"),
        (RecordKind::Group, "groups"),
        (RecordKind::Tag, "tags"),
        (RecordKind::Identity, "identities"),
    ] {
        if other == kind {
            continue;
        }
        let found: i64 = conn
            .query_row(&format!("SELECT 1 FROM {table} WHERE id = ?1"), [id], |r| {
                r.get(0)
            })
            .optional()
            .map_err(StoreError::Sqlite)?
            .unwrap_or(0);
        if found != 0 {
            return Ok(true);
        }
    }
    Ok(false)
}

impl Sync<'_> {
    pub fn pending(&self) -> Result<Vec<PendingMutation>, StoreError> {
        self.store.with_conn(|conn| {
            let mut stmt = conn
                .prepare(
                    "SELECT mutation_id, record_id, kind FROM sync_queue \
                     ORDER BY created_at, mutation_id",
                )
                .map_err(StoreError::Sqlite)?;
            let rows = stmt
                .query_map([], |row: &Row<'_>| {
                    let mid: String = row.get(0)?;
                    let record_id: String = row.get(1)?;
                    let kind: String = row.get(2)?;
                    Ok((mid, record_id, kind))
                })
                .map_err(StoreError::Sqlite)?;
            let mut out = Vec::new();
            for row in rows {
                let (mid, record_id, kind) = row.map_err(StoreError::Sqlite)?;
                if let (Ok(mutation_id), Some(kind)) = (mid.parse(), RecordKind::parse(&kind)) {
                    out.push(PendingMutation {
                        mutation_id,
                        record_id,
                        kind,
                    });
                }
            }
            Ok(out)
        })
    }

    /// Materializes pending queue entries into push payloads. Queue entries
    /// whose underlying record vanished (already purged) are dropped. Records
    /// with open conflicts are **excluded** until resolved.
    pub fn build_push(&self) -> Result<Vec<PushRecord>, StoreError> {
        self.store.with_tx(|conn| {
            let pending = {
                let mut stmt = conn
                    .prepare(
                        "SELECT mutation_id, record_id, kind FROM sync_queue \
                         ORDER BY created_at, mutation_id",
                    )
                    .map_err(StoreError::Sqlite)?;
                let rows = stmt
                    .query_map([], |row: &Row<'_>| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                        ))
                    })
                    .map_err(StoreError::Sqlite)?;
                rows.collect::<Result<Vec<_>, _>>()
                    .map_err(StoreError::Sqlite)?
            };
            let mut out = Vec::new();
            for (mutation_id, record_id, kind_str) in pending {
                let Some(kind) = RecordKind::parse(&kind_str) else {
                    remove_queued(conn, &record_id)?;
                    continue;
                };
                // Conflict gate: never push a record the user hasn't resolved.
                if crate::repos::conflicts::Conflicts::get_in_conn(conn, &record_id)?.is_some() {
                    continue;
                }
                match materialize(conn, kind, &mutation_id, &record_id)? {
                    Some(push) => out.push(push),
                    None => {
                        // Row gone (already acknowledged purge): drop entry.
                        remove_queued(conn, &record_id)?;
                    }
                }
            }
            Ok(out)
        })
    }

    pub fn meta(&self) -> Result<SyncMeta, StoreError> {
        self.store.with_conn(|conn| {
            conn.query_row(
                "SELECT cursor, last_sync_at, last_error FROM sync_meta WHERE id = 1",
                [],
                |row| {
                    Ok(SyncMeta {
                        cursor: row.get(0)?,
                        last_sync_at: row.get::<_, Option<i64>>(1)?.map(datetime_from_ms),
                        last_error: row.get(2)?,
                    })
                },
            )
            .map_err(StoreError::Sqlite)
        })
    }

    /// Records a successful sync cycle: advances the cursor (never rewinds)
    /// and stores the timestamp + optional error of the attempt.
    pub fn record_sync_outcome(
        &self,
        cursor: Option<i64>,
        error: Option<String>,
    ) -> Result<(), StoreError> {
        self.store.with_conn(move |conn| {
            if let Some(c) = cursor {
                conn.execute(
                    "UPDATE sync_meta SET cursor = MAX(cursor, ?1), last_sync_at = ?2, last_error = ?3 WHERE id = 1",
                    rusqlite::params![c, now_ms(), error],
                )
                .map_err(StoreError::Sqlite)?;
            } else {
                conn.execute(
                    "UPDATE sync_meta SET last_error = ?1 WHERE id = 1",
                    rusqlite::params![error],
                )
                .map_err(StoreError::Sqlite)?;
            }
            Ok(())
        })
    }

    /// Applies a server record (pull delivery **or** push-conflict echo) using
    /// the conflict rules in the module documentation.
    pub fn apply_remote(&self, wire: &WireRecord) -> Result<ApplyOutcome, StoreError> {
        self.store
            .with_tx(|conn| Self::apply_remote_in_tx(conn, wire))
    }

    pub(crate) fn apply_remote_in_tx(
        conn: &rusqlite::Connection,
        wire: &WireRecord,
    ) -> Result<ApplyOutcome, StoreError> {
        if id_exists_in_other_kind(conn, wire.kind, &wire.id)? {
            return Ok(ApplyOutcome::KindMismatch);
        }
        let local = local_state(conn, wire.kind, &wire.id)?;

        if !local.exists {
            if wire.deleted {
                return Ok(ApplyOutcome::Ignored);
            }
            let payload = wire.payload.as_ref().ok_or_else(|| {
                StoreError::Validation(termru_core::CoreError::validation(
                    "alive record without payload",
                ))
            })?;
            let decoded = termru_core::sync::RecordPayload::decode_and_validate(wire.kind, payload)
                .map_err(StoreError::Validation)?;
            return store_payload(conn, decoded, wire.rev).map(|_| ApplyOutcome::Applied);
        }

        if local.dirty {
            let base = local.server_rev.unwrap_or(-1);
            if wire.rev > base {
                // Both sides changed: record the conflict, remember the server
                // revision we observed, keep local changes pending resolution.
                let server_record = wire.clone();
                Conflicts::store_conflict(conn, &wire.id, wire.kind, &server_record)?;
                set_row_server_rev(conn, wire.kind, &wire.id, Some(wire.rev))?;
                Ok(ApplyOutcome::Conflict)
            } else {
                Ok(ApplyOutcome::Ignored)
            }
        } else {
            if wire.rev <= local.server_rev.unwrap_or(-1) {
                return Ok(ApplyOutcome::Ignored);
            }
            if wire.deleted {
                hard_delete(conn, wire.kind, &wire.id)?;
                Conflicts::remove(conn, &wire.id)?;
                remove_queued(conn, &wire.id)?;
                return Ok(ApplyOutcome::RemoteDeleted);
            }
            let payload = wire.payload.as_ref().ok_or_else(|| {
                StoreError::Validation(termru_core::CoreError::validation(
                    "alive record without payload",
                ))
            })?;
            let decoded = termru_core::sync::RecordPayload::decode_and_validate(wire.kind, payload)
                .map_err(StoreError::Validation)?;
            store_payload(conn, decoded, wire.rev)?;
            Conflicts::remove(conn, &wire.id)?;
            remove_queued(conn, &wire.id)?;
            Ok(ApplyOutcome::Applied)
        }
    }

    /// Marks a pushed record as accepted at `rev`: clears dirtiness, updates
    /// the observed revision, purges the row if it was a deletion, and removes
    /// queue/conflict entries.
    pub fn acknowledge(
        &self,
        record_id: &str,
        kind: RecordKind,
        rev: i64,
    ) -> Result<(), StoreError> {
        let record_id = record_id.to_string();
        self.store.with_tx(move |conn| {
            let state = local_state(conn, kind, &record_id)?;
            if state.exists {
                if state.deleted {
                    hard_delete(conn, kind, &record_id)?;
                } else {
                    set_row_meta(conn, kind, &record_id, Some(rev), false)?;
                }
            }
            remove_queued(conn, &record_id)?;
            Conflicts::remove(conn, &record_id)?;
            Ok(())
        })
    }

    /// Removes a queue entry that the server reported as a duplicate of an
    /// already-applied mutation (idempotent retry path) without touching rows.
    pub fn drop_queue_entry(&self, record_id: &str) -> Result<(), StoreError> {
        let id = record_id.to_string();
        self.store.with_conn(move |conn| remove_queued(conn, &id))
    }

    /// Stores a push-time conflict observation (server returned its copy).
    pub fn observe_push_conflict(&self, wire: &WireRecord) -> Result<ApplyOutcome, StoreError> {
        self.apply_remote(wire)
    }
}

fn materialize(
    conn: &rusqlite::Connection,
    kind: RecordKind,
    mutation_id: &str,
    record_id: &str,
) -> Result<Option<PushRecord>, StoreError> {
    let parse_ts = |ms: i64| datetime_from_ms(ms);
    let (deleted, server_rev, updated_at, payload) = match kind {
        RecordKind::Host => {
            let row: Option<(i64, Option<i64>, i64)> = conn
                .query_row(
                    "SELECT deleted, server_rev, updated_at FROM hosts WHERE id = ?1",
                    [record_id],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .optional()
                .map_err(StoreError::Sqlite)?;
            let Some((deleted, server_rev, updated_at)) = row else {
                return Ok(None);
            };
            if deleted != 0 {
                (true, server_rev, updated_at, None)
            } else {
                let host: Option<Host> = crate::repos::hosts::get_row(conn, record_id)?;
                let Some(host) = host else { return Ok(None) };
                let value = serde_json::to_value(&host)
                    .map_err(|e| StoreError::migration(format!("serialize host: {e}")))?;
                (false, server_rev, updated_at, Some(value))
            }
        }
        RecordKind::Group => {
            let row: Option<(i64, Option<i64>, i64)> = conn
                .query_row(
                    "SELECT deleted, server_rev, updated_at FROM groups WHERE id = ?1",
                    [record_id],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .optional()
                .map_err(StoreError::Sqlite)?;
            let Some((deleted, server_rev, updated_at)) = row else {
                return Ok(None);
            };
            if deleted != 0 {
                (true, server_rev, updated_at, None)
            } else {
                let group: Option<Group> = crate::repos::groups::get_row(conn, record_id)?;
                let Some(group) = group else { return Ok(None) };
                let value = serde_json::to_value(&group)
                    .map_err(|e| StoreError::migration(format!("serialize group: {e}")))?;
                (false, server_rev, updated_at, Some(value))
            }
        }
        RecordKind::Tag => {
            let row: Option<(i64, Option<i64>, i64)> = conn
                .query_row(
                    "SELECT deleted, server_rev, updated_at FROM tags WHERE id = ?1",
                    [record_id],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .optional()
                .map_err(StoreError::Sqlite)?;
            let Some((deleted, server_rev, updated_at)) = row else {
                return Ok(None);
            };
            if deleted != 0 {
                (true, server_rev, updated_at, None)
            } else {
                let tag: Option<Tag> = crate::repos::tags::get_row(conn, record_id)?;
                let Some(tag) = tag else { return Ok(None) };
                let value = serde_json::to_value(&tag)
                    .map_err(|e| StoreError::migration(format!("serialize tag: {e}")))?;
                (false, server_rev, updated_at, Some(value))
            }
        }
        RecordKind::Identity => {
            let row: Option<(i64, Option<i64>, i64)> = conn
                .query_row(
                    "SELECT deleted, server_rev, updated_at FROM identities WHERE id = ?1",
                    [record_id],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .optional()
                .map_err(StoreError::Sqlite)?;
            let Some((deleted, server_rev, updated_at)) = row else {
                return Ok(None);
            };
            if deleted != 0 {
                (true, server_rev, updated_at, None)
            } else {
                let identity: Option<Identity> =
                    crate::repos::identities::get_row(conn, record_id)?;
                let Some(identity) = identity else {
                    return Ok(None);
                };
                let value = serde_json::to_value(&identity)
                    .map_err(|e| StoreError::migration(format!("serialize identity: {e}")))?;
                (false, server_rev, updated_at, Some(value))
            }
        }
        RecordKind::Preferences => {
            if record_id != PREFERENCES_RECORD_ID {
                return Ok(None);
            }
            let Some((prefs, server_rev, _dirty)) =
                crate::repos::prefs::Prefs::portable_snapshot(conn)?
            else {
                return Ok(None);
            };
            let value = serde_json::to_value(&prefs)
                .map_err(|e| StoreError::migration(format!("serialize preferences: {e}")))?;
            // Preferences rows carry an updated_at in their own table row.
            let updated_at: i64 = conn
                .query_row(
                    "SELECT updated_at FROM preferences WHERE key = 'portable'",
                    [],
                    |r| r.get(0),
                )
                .optional()
                .map_err(StoreError::Sqlite)?
                .unwrap_or_else(now_ms);
            (false, server_rev, updated_at, Some(value))
        }
    };
    Ok(Some(PushRecord {
        mutation_id: mutation_id.parse().unwrap_or_default(),
        record_id: record_id.to_string(),
        kind,
        deleted,
        base_rev: server_rev,
        payload,
        updated_at: parse_ts(updated_at),
    }))
}

fn store_payload(
    conn: &rusqlite::Connection,
    decoded: termru_core::sync::RecordPayload,
    rev: i64,
) -> Result<(), StoreError> {
    use termru_core::sync::RecordPayload;
    let mut payload = decoded;
    match &mut payload {
        RecordPayload::Host(h) => {
            h.server_rev = Some(rev);
            h.dirty = false;
            h.deleted = false;
            crate::repos::hosts::apply_remote(conn, h)?;
        }
        RecordPayload::Group(g) => {
            g.server_rev = Some(rev);
            g.dirty = false;
            g.deleted = false;
            crate::repos::groups::upsert_group_row(conn, g)?;
        }
        RecordPayload::Tag(t) => {
            t.server_rev = Some(rev);
            t.dirty = false;
            t.deleted = false;
            crate::repos::tags::upsert_tag_row(conn, t)?;
        }
        RecordPayload::Identity(i) => {
            i.server_rev = Some(rev);
            i.dirty = false;
            i.deleted = false;
            crate::repos::identities::upsert_identity_row(conn, i)?;
        }
        RecordPayload::Preferences(p) => {
            crate::repos::prefs::Prefs::apply_remote_portable(conn, p, rev)?;
        }
    }
    Ok(())
}

fn set_row_server_rev(
    conn: &rusqlite::Connection,
    kind: RecordKind,
    id: &str,
    rev: Option<i64>,
) -> Result<(), StoreError> {
    set_row_meta(conn, kind, id, rev, true)
}

/// Updates sync bookkeeping for a row without touching its payload.
/// `keep_dirty` preserves dirtiness (conflict observation) or clears it.
fn set_row_meta(
    conn: &rusqlite::Connection,
    kind: RecordKind,
    id: &str,
    rev: Option<i64>,
    keep_dirty: bool,
) -> Result<(), StoreError> {
    let table = match kind {
        RecordKind::Host => "hosts",
        RecordKind::Group => "groups",
        RecordKind::Tag => "tags",
        RecordKind::Identity => "identities",
        RecordKind::Preferences => {
            if !keep_dirty {
                crate::repos::prefs::Prefs::set_server_rev(conn, rev, false)?;
            } else {
                crate::repos::prefs::Prefs::set_server_rev(conn, rev, true)?;
            }
            return Ok(());
        }
    };
    if keep_dirty {
        conn.execute(
            &format!("UPDATE {table} SET server_rev = ?2, dirty = 1 WHERE id = ?1"),
            rusqlite::params![id, rev],
        )
        .map_err(StoreError::Sqlite)?;
    } else {
        conn.execute(
            &format!("UPDATE {table} SET server_rev = ?2, dirty = 0 WHERE id = ?1"),
            rusqlite::params![id, rev],
        )
        .map_err(StoreError::Sqlite)?;
    }
    Ok(())
}

/// Removes a local record entirely (remote tombstone applied or local
/// deletion acknowledged by the server).
fn hard_delete(conn: &rusqlite::Connection, kind: RecordKind, id: &str) -> Result<(), StoreError> {
    let table = match kind {
        RecordKind::Host => "hosts",
        RecordKind::Group => "groups",
        RecordKind::Tag => "tags",
        RecordKind::Identity => "identities",
        RecordKind::Preferences => {
            // The portable preferences document is never deleted — a remote
            // tombstone for it would wipe the singleton; treat as ignore.
            return Ok(());
        }
    };
    conn.execute(&format!("DELETE FROM {table} WHERE id = ?1"), [id])
        .map_err(StoreError::Sqlite)?;
    Ok(())
}
