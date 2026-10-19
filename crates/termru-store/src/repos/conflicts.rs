//! Sync conflict storage (records where local and remote both changed).

use rusqlite::{OptionalExtension, Row};

use termru_core::sync::{RecordKind, WireRecord};

use crate::Store;
use crate::error::StoreError;
use crate::repos::to_json;
use crate::util::{datetime_from_ms, now_ms};

pub struct Conflicts<'a> {
    pub(crate) store: &'a Store,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ConflictRecord {
    pub record_id: String,
    pub kind: RecordKind,
    /// The server's copy at the observed revision.
    pub server_record: WireRecord,
    pub server_rev: i64,
    pub observed_at: chrono::DateTime<chrono::Utc>,
}

fn row_to_conflict(row: &Row<'_>) -> rusqlite::Result<ConflictRecord> {
    let record_id: String = row.get(0)?;
    let kind: String = row.get(1)?;
    let server_json: String = row.get(2)?;
    let server_rev: i64 = row.get(3)?;
    let observed_ms: i64 = row.get(4)?;
    let server_record: WireRecord = serde_json::from_str(&server_json).map_err(|_| {
        rusqlite::Error::FromSqlConversionFailure(
            2,
            rusqlite::types::Type::Text,
            Box::new(std::io::Error::other("corrupt conflict record")),
        )
    })?;
    Ok(ConflictRecord {
        record_id,
        kind: RecordKind::parse(&kind).unwrap_or(RecordKind::Host),
        server_record,
        server_rev,
        observed_at: datetime_from_ms(observed_ms),
    })
}

impl Conflicts<'_> {
    pub fn list(&self) -> Result<Vec<ConflictRecord>, StoreError> {
        self.store.with_conn(|conn| {
            let mut stmt = conn
                .prepare(
                    "SELECT record_id, kind, server_record, server_rev, observed_at \
                     FROM sync_conflicts ORDER BY observed_at DESC, record_id",
                )
                .map_err(StoreError::Sqlite)?;
            let rows = stmt
                .query_map([], row_to_conflict)
                .map_err(StoreError::Sqlite)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(StoreError::Sqlite)?;
            Ok(rows)
        })
    }

    pub fn get(&self, record_id: &str) -> Result<Option<ConflictRecord>, StoreError> {
        self.store.with_conn(move |conn| {
            conn.query_row(
                "SELECT record_id, kind, server_record, server_rev, observed_at \
                 FROM sync_conflicts WHERE record_id = ?1",
                [record_id],
                row_to_conflict,
            )
            .optional()
            .map_err(StoreError::Sqlite)
        })
    }

    pub fn has_conflict(&self, record_id: &str) -> Result<bool, StoreError> {
        Ok(self.get(record_id)?.is_some())
    }

    pub(crate) fn get_in_conn(
        conn: &rusqlite::Connection,
        record_id: &str,
    ) -> Result<Option<ConflictRecord>, StoreError> {
        conn.query_row(
            "SELECT record_id, kind, server_record, server_rev, observed_at \
             FROM sync_conflicts WHERE record_id = ?1",
            [record_id],
            row_to_conflict,
        )
        .optional()
        .map_err(StoreError::Sqlite)
    }

    /// Stores (or replaces with a newer server observation of) a conflict.
    pub(crate) fn store_conflict(
        conn: &rusqlite::Connection,
        record_id: &str,
        kind: RecordKind,
        server_record: &WireRecord,
    ) -> Result<(), StoreError> {
        let existing_rev: Option<i64> = conn
            .query_row(
                "SELECT server_rev FROM sync_conflicts WHERE record_id = ?1",
                [record_id],
                |r| r.get(0),
            )
            .optional()
            .map_err(StoreError::Sqlite)?;
        if existing_rev.is_some_and(|rev| rev >= server_record.rev) {
            // Keep the newest observed server state.
            return Ok(());
        }
        let json = to_json(server_record, "conflict server record")?;
        conn.execute(
            "INSERT INTO sync_conflicts (record_id, kind, server_record, server_rev, observed_at) \
             VALUES (?1, ?2, ?3, ?4, ?5) \
             ON CONFLICT(record_id) DO UPDATE SET kind = excluded.kind, \
             server_record = excluded.server_record, server_rev = excluded.server_rev, \
             observed_at = excluded.observed_at",
            rusqlite::params![record_id, kind.as_str(), json, server_record.rev, now_ms()],
        )
        .map_err(StoreError::Sqlite)?;
        Ok(())
    }

    pub(crate) fn remove(conn: &rusqlite::Connection, record_id: &str) -> Result<(), StoreError> {
        conn.execute(
            "DELETE FROM sync_conflicts WHERE record_id = ?1",
            [record_id],
        )
        .map_err(StoreError::Sqlite)?;
        Ok(())
    }
}
