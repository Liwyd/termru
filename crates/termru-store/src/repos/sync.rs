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
        Self::apply_remote_with(conn, wire, false)
    }

    /// Applies the server's copy **over** the local one, ignoring the local
    /// modification. Used when the user resolves a conflict in favour of the
    /// server ("take remote").
    pub(crate) fn apply_remote_force_in_tx(
        conn: &rusqlite::Connection,
        wire: &WireRecord,
    ) -> Result<ApplyOutcome, StoreError> {
        Self::apply_remote_with(conn, wire, true)
    }

    fn apply_remote_with(
        conn: &rusqlite::Connection,
        wire: &WireRecord,
        force: bool,
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

        if local.dirty && !force {
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
            if !force && wire.rev <= local.server_rev.unwrap_or(-1) {
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

    /// The local copy of a record as a sync payload.
    ///
    /// Used by the conflict view, which shows the local and the remote copy
    /// side by side so the user can decide which one to keep.
    pub fn local_payload(
        &self,
        kind: RecordKind,
        record_id: &str,
    ) -> Result<Option<serde_json::Value>, StoreError> {
        self.store.with_conn(|conn| {
            let Some(record) =
                materialize(conn, kind, &uuid::Uuid::new_v4().to_string(), record_id)?
            else {
                return Ok(None);
            };
            Ok(record.payload)
        })
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

    /// Resolves a conflict in favour of the **local** copy.
    ///
    /// The server revision that was observed becomes the new base revision, so
    /// the next push overwrites the server's copy with the local state. The
    /// record stays dirty and is queued again.
    pub fn resolve_keep_local(&self, record_id: &str) -> Result<(), StoreError> {
        let id = record_id.to_string();
        self.store.with_tx(move |conn| {
            let Some(conflict) = Conflicts::get_in_conn(conn, &id)? else {
                return Err(StoreError::NotFound("the record has no open conflict"));
            };
            set_row_meta(conn, conflict.kind, &id, Some(conflict.server_rev), true)?;
            Conflicts::remove(conn, &id)?;
            crate::repos::enqueue_mutation(conn, &id, conflict.kind)?;
            Ok(())
        })
    }

    /// Resolves a conflict in favour of the **server** copy.
    ///
    /// The server's record replaces the local one, the local modification is
    /// dropped and the queue entry is removed.
    pub fn resolve_take_remote(&self, record_id: &str) -> Result<ApplyOutcome, StoreError> {
        let id = record_id.to_string();
        self.store.with_tx(move |conn| {
            let Some(conflict) = Conflicts::get_in_conn(conn, &id)? else {
                return Err(StoreError::NotFound("the record has no open conflict"));
            };
            let outcome = Self::apply_remote_force_in_tx(conn, &conflict.server_record)?;
            Conflicts::remove(conn, &id)?;
            remove_queued(conn, &id)?;
            Ok(outcome)
        })
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Store;
    use termru_core::Host;

    fn store() -> Store {
        Store::open_in_memory().expect("in-memory store")
    }

    fn host(name: &str) -> Host {
        Host::new(
            name.to_string(),
            format!("{name}.example.com"),
            "deploy".to_string(),
        )
    }

    /// A wire record that carries the given host.
    fn wire(host: &Host, rev: i64, deleted: bool) -> WireRecord {
        WireRecord {
            id: host.id.to_string(),
            kind: RecordKind::Host,
            rev,
            updated_at: chrono::Utc::now(),
            deleted,
            payload: if deleted {
                None
            } else {
                Some(serde_json::to_value(host).expect("serialize the host"))
            },
        }
    }

    #[test]
    fn a_local_edit_is_queued_and_materialized() {
        let store = store();
        let mut host = host("web-1");
        store
            .repos()
            .hosts()
            .save(&mut host)
            .expect("save the host");

        let pending = store.repos().sync().pending().expect("pending");
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].record_id, host.id.to_string());
        assert_eq!(pending[0].kind, RecordKind::Host);

        let push = store.repos().sync().build_push().expect("build the push");
        assert_eq!(push.len(), 1);
        assert_eq!(push[0].record_id, host.id.to_string());
        assert!(!push[0].deleted);
        assert!(
            push[0].base_rev.is_none(),
            "a new record has no base revision"
        );
        assert!(push[0].payload.is_some());
    }

    #[test]
    fn acknowledging_clears_the_queue_and_the_dirtiness() {
        let store = store();
        let mut host = host("web-1");
        store
            .repos()
            .hosts()
            .save(&mut host)
            .expect("save the host");

        store
            .repos()
            .sync()
            .acknowledge(&host.id.to_string(), RecordKind::Host, 7)
            .expect("acknowledge");

        assert!(store.repos().sync().pending().expect("pending").is_empty());
        let stored = store
            .repos()
            .hosts()
            .get(host.id)
            .expect("get")
            .expect("the host is still there");
        assert_eq!(stored.server_rev, Some(7));
        assert!(!stored.dirty);
    }

    #[test]
    fn a_remote_record_replaces_a_clean_local_copy() {
        let store = store();
        let mut local = host("web-1");
        store
            .repos()
            .hosts()
            .save(&mut local)
            .expect("save the host");
        store
            .repos()
            .sync()
            .acknowledge(&local.id.to_string(), RecordKind::Host, 3)
            .expect("acknowledge");

        let mut remote = local.clone();
        remote.name = "web-1-renamed".to_string();
        let outcome = store
            .repos()
            .sync()
            .apply_remote(&wire(&remote, 8, false))
            .expect("apply");
        assert_eq!(outcome, ApplyOutcome::Applied);

        let stored = store
            .repos()
            .hosts()
            .get(local.id)
            .expect("get")
            .expect("the host is still there");
        assert_eq!(stored.name, "web-1-renamed");
        assert_eq!(stored.server_rev, Some(8));
        assert!(!stored.dirty);
    }

    #[test]
    fn a_remote_tombstone_deletes_the_local_row() {
        let store = store();
        let mut local = host("web-1");
        store
            .repos()
            .hosts()
            .save(&mut local)
            .expect("save the host");
        store
            .repos()
            .sync()
            .acknowledge(&local.id.to_string(), RecordKind::Host, 3)
            .expect("acknowledge");

        let outcome = store
            .repos()
            .sync()
            .apply_remote(&wire(&local, 5, true))
            .expect("apply");
        assert_eq!(outcome, ApplyOutcome::RemoteDeleted);
        assert!(store.repos().hosts().get(local.id).expect("get").is_none());
    }

    #[test]
    fn a_remote_change_to_a_modified_record_becomes_a_conflict() {
        let store = store();
        let mut local = host("web-1");
        store
            .repos()
            .hosts()
            .save(&mut local)
            .expect("save the host");
        store
            .repos()
            .sync()
            .acknowledge(&local.id.to_string(), RecordKind::Host, 3)
            .expect("acknowledge");

        // The local copy changes again (dirty).
        local.name = "local-name".to_string();
        local.server_rev = Some(3);
        store
            .repos()
            .hosts()
            .save(&mut local)
            .expect("save the host");

        // The server changes the same record.
        let mut remote = local.clone();
        remote.name = "remote-name".to_string();
        let outcome = store
            .repos()
            .sync()
            .apply_remote(&wire(&remote, 4, false))
            .expect("apply");
        assert_eq!(outcome, ApplyOutcome::Conflict);

        // The local copy is untouched and the conflict is recorded.
        let stored = store
            .repos()
            .hosts()
            .get(local.id)
            .expect("get")
            .expect("the host is still there");
        assert_eq!(stored.name, "local-name");
        assert!(stored.dirty);

        let conflicts = store.repos().conflicts().list().expect("conflicts");
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].record_id, local.id.to_string());
        assert_eq!(conflicts[0].server_rev, 4);

        // A record with an open conflict is never pushed.
        let push = store.repos().sync().build_push().expect("build the push");
        assert!(push.is_empty(), "a conflicted record must not be pushed");
    }

    #[test]
    fn keeping_the_local_copy_queues_it_against_the_server_revision() {
        let store = store();
        let mut local = host("web-1");
        store
            .repos()
            .hosts()
            .save(&mut local)
            .expect("save the host");
        store
            .repos()
            .sync()
            .acknowledge(&local.id.to_string(), RecordKind::Host, 3)
            .expect("acknowledge");
        local.name = "local-name".to_string();
        local.server_rev = Some(3);
        store
            .repos()
            .hosts()
            .save(&mut local)
            .expect("save the host");

        let mut remote = local.clone();
        remote.name = "remote-name".to_string();
        store
            .repos()
            .sync()
            .apply_remote(&wire(&remote, 4, false))
            .expect("apply");

        store
            .repos()
            .sync()
            .resolve_keep_local(&local.id.to_string())
            .expect("resolve");

        // The conflict is gone, the record is queued again and the base
        // revision is the one the server reported.
        assert!(
            store
                .repos()
                .conflicts()
                .list()
                .expect("conflicts")
                .is_empty()
        );
        let push = store.repos().sync().build_push().expect("build the push");
        assert_eq!(push.len(), 1);
        assert_eq!(push[0].base_rev, Some(4));

        let stored = store
            .repos()
            .hosts()
            .get(local.id)
            .expect("get")
            .expect("the host is still there");
        assert_eq!(stored.name, "local-name");
        assert!(stored.dirty);
    }

    #[test]
    fn taking_the_remote_copy_replaces_the_local_one() {
        let store = store();
        let mut local = host("web-1");
        store
            .repos()
            .hosts()
            .save(&mut local)
            .expect("save the host");
        store
            .repos()
            .sync()
            .acknowledge(&local.id.to_string(), RecordKind::Host, 3)
            .expect("acknowledge");
        local.name = "local-name".to_string();
        local.server_rev = Some(3);
        store
            .repos()
            .hosts()
            .save(&mut local)
            .expect("save the host");

        let mut remote = local.clone();
        remote.name = "remote-name".to_string();
        store
            .repos()
            .sync()
            .apply_remote(&wire(&remote, 4, false))
            .expect("apply");

        store
            .repos()
            .sync()
            .resolve_take_remote(&local.id.to_string())
            .expect("resolve");

        assert!(
            store
                .repos()
                .conflicts()
                .list()
                .expect("conflicts")
                .is_empty()
        );
        assert!(store.repos().sync().pending().expect("pending").is_empty());

        let stored = store
            .repos()
            .hosts()
            .get(local.id)
            .expect("get")
            .expect("the host is still there");
        assert_eq!(stored.name, "remote-name");
        assert_eq!(stored.server_rev, Some(4));
        assert!(!stored.dirty);
    }

    #[test]
    fn resolving_an_unknown_conflict_is_an_error() {
        let store = store();
        let error = store
            .repos()
            .sync()
            .resolve_keep_local("no-such-record")
            .expect_err("must fail");
        assert!(matches!(error, StoreError::NotFound(_)));
    }

    #[test]
    fn an_invalid_remote_payload_is_rejected() {
        let store = store();
        let mut local = host("web-1");
        store
            .repos()
            .hosts()
            .save(&mut local)
            .expect("save the host");
        store
            .repos()
            .sync()
            .acknowledge(&local.id.to_string(), RecordKind::Host, 3)
            .expect("acknowledge");

        let mut broken = wire(&local, 4, false);
        broken.payload = Some(serde_json::json!({ "name": "" }));
        // A payload that does not validate is rejected; nothing is written.
        let error = store
            .repos()
            .sync()
            .apply_remote(&broken)
            .expect_err("the payload must be rejected");
        assert!(matches!(error, StoreError::Validation(_)));
        let stored = store
            .repos()
            .hosts()
            .get(local.id)
            .expect("get")
            .expect("the host is still there");
        assert_eq!(stored.name, "web-1", "the local copy must be untouched");
    }

    #[test]
    fn the_push_payload_never_contains_a_secret() {
        let store = store();
        let mut local = host("web-1");
        // The host references an identity; the key itself lives in the
        // credential store and must never reach the sync payload.
        local.auth = termru_core::AuthMethod::PrivateKey {
            identity_id: termru_core::ids::IdentityId::new(),
        };
        store
            .repos()
            .hosts()
            .save(&mut local)
            .expect("save the host");

        let push = store.repos().sync().build_push().expect("build the push");
        assert_eq!(push.len(), 1);
        let payload = push[0].payload.as_ref().expect("a payload");

        // No field of the payload may carry a secret.
        fn assert_no_secret_fields(value: &serde_json::Value, path: &str) {
            match value {
                serde_json::Value::Object(map) => {
                    for (key, child) in map {
                        let lower = key.to_lowercase();
                        assert!(
                            !["password", "passphrase", "secret", "privatekey", "key"]
                                .contains(&lower.as_str()),
                            "the sync payload must not contain the field '{path}{key}'"
                        );
                        assert_no_secret_fields(child, &format!("{path}{key}."));
                    }
                }
                serde_json::Value::Array(items) => {
                    for item in items {
                        assert_no_secret_fields(item, path);
                    }
                }
                _ => {}
            }
        }
        assert_no_secret_fields(payload, "");

        // The private key auth method carries a *reference* to an identity,
        // never key material.
        let auth = payload.get("auth").expect("the auth method");
        assert_eq!(
            auth.get("type").and_then(|v| v.as_str()),
            Some("private_key")
        );
        assert!(auth.get("identity_id").is_some());
        assert_eq!(auth.as_object().expect("an object").len(), 2);
    }
}
