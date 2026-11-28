//! Forward-only schema migrations with version tracking.

use rusqlite::Connection;

use crate::error::StoreError;

/// A single forward migration. Migrations must be append-only: never edit an
/// applied migration, add a new one instead.
struct Migration {
    version: u32,
    name: &'static str,
    sql: &'static str,
}

const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        name: "initial_schema",
        sql: r#"
CREATE TABLE groups (
    id             TEXT PRIMARY KEY,
    name           TEXT NOT NULL,
    parent_id      TEXT REFERENCES groups(id) ON DELETE SET NULL,
    sort_order     INTEGER NOT NULL DEFAULT 0,
    created_at     INTEGER NOT NULL,
    updated_at     INTEGER NOT NULL,
    deleted        INTEGER NOT NULL DEFAULT 0 CHECK (deleted IN (0, 1)),
    server_rev     INTEGER,
    dirty          INTEGER NOT NULL DEFAULT 0 CHECK (dirty IN (0, 1))
);

CREATE TABLE tags (
    id             TEXT PRIMARY KEY,
    name           TEXT NOT NULL,
    color          TEXT,
    created_at     INTEGER NOT NULL,
    updated_at     INTEGER NOT NULL,
    deleted        INTEGER NOT NULL DEFAULT 0 CHECK (deleted IN (0, 1)),
    server_rev     INTEGER,
    dirty          INTEGER NOT NULL DEFAULT 0 CHECK (dirty IN (0, 1))
);

CREATE TABLE identities (
    id             TEXT PRIMARY KEY,
    name           TEXT NOT NULL,
    comment        TEXT,
    fingerprint    TEXT,
    created_at     INTEGER NOT NULL,
    updated_at     INTEGER NOT NULL,
    deleted        INTEGER NOT NULL DEFAULT 0 CHECK (deleted IN (0, 1)),
    server_rev     INTEGER,
    dirty          INTEGER NOT NULL DEFAULT 0 CHECK (dirty IN (0, 1))
);

-- Device-local binding of a logical identity to a private key path.
-- Deliberately NOT synced: paths are machine-specific and the private key
-- itself never leaves the machine.
CREATE TABLE identity_bindings (
    identity_id    TEXT PRIMARY KEY REFERENCES identities(id) ON DELETE CASCADE,
    key_path       TEXT NOT NULL,
    updated_at     INTEGER NOT NULL
);

CREATE TABLE hosts (
    id                TEXT PRIMARY KEY,
    name              TEXT NOT NULL,
    address           TEXT NOT NULL,
    port              INTEGER NOT NULL CHECK (port BETWEEN 1 AND 65535),
    username          TEXT NOT NULL,
    auth              TEXT NOT NULL,
    group_id          TEXT REFERENCES groups(id) ON DELETE SET NULL,
    favorite          INTEGER NOT NULL DEFAULT 0 CHECK (favorite IN (0, 1)),
    description       TEXT,
    term              TEXT NOT NULL,
    keepalive_seconds INTEGER NOT NULL DEFAULT 0,
    sort_order        INTEGER NOT NULL DEFAULT 0,
    created_at        INTEGER NOT NULL,
    updated_at        INTEGER NOT NULL,
    deleted           INTEGER NOT NULL DEFAULT 0 CHECK (deleted IN (0, 1)),
    server_rev        INTEGER,
    dirty             INTEGER NOT NULL DEFAULT 0 CHECK (dirty IN (0, 1))
);

CREATE INDEX idx_hosts_visibility ON hosts(deleted, group_id, sort_order, name);

CREATE TABLE host_tags (
    host_id     TEXT NOT NULL REFERENCES hosts(id) ON DELETE CASCADE,
    tag_id      TEXT NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
    PRIMARY KEY (host_id, tag_id)
);

-- Portable (synced) preferences are stored as one JSON document under the
-- fixed key "portable". Device-local settings use keys prefixed "device:".
CREATE TABLE preferences (
    key         TEXT PRIMARY KEY,
    value       TEXT NOT NULL,
    updated_at  INTEGER NOT NULL,
    server_rev  INTEGER,
    dirty       INTEGER NOT NULL DEFAULT 0 CHECK (dirty IN (0, 1))
);

CREATE TABLE workspaces (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL,
    doc         TEXT NOT NULL,
    updated_at  INTEGER NOT NULL
);

CREATE TABLE recents (
    host_id      TEXT PRIMARY KEY REFERENCES hosts(id) ON DELETE CASCADE,
    last_used_at INTEGER NOT NULL,
    use_count    INTEGER NOT NULL DEFAULT 0
);

-- One pending mutation per record (coalesced). The push payload is
-- materialized from the live row at push time so retries always carry the
-- latest local state; mutation_id is stable across retries of this entry so
-- the server can deduplicate.
CREATE TABLE sync_queue (
    mutation_id TEXT PRIMARY KEY,
    record_id   TEXT NOT NULL,
    kind        TEXT NOT NULL,
    created_at  INTEGER NOT NULL
);

CREATE INDEX idx_sync_queue_record ON sync_queue(record_id);
CREATE INDEX idx_sync_queue_order ON sync_queue(created_at, mutation_id);

CREATE TABLE sync_meta (
    id            INTEGER PRIMARY KEY CHECK (id = 1),
    cursor        INTEGER NOT NULL DEFAULT 0,
    last_sync_at  INTEGER,
    last_error    TEXT
);

-- Server state observed for a locally-modified record (i.e. a conflict).
-- The presence of a row for a record blocks pushing that record until the
-- user resolves it — conflicts are never silently overwritten.
CREATE TABLE sync_conflicts (
    record_id     TEXT PRIMARY KEY,
    kind          TEXT NOT NULL,
    server_record TEXT NOT NULL,
    server_rev    INTEGER NOT NULL,
    observed_at   INTEGER NOT NULL
);

INSERT INTO sync_meta (id, cursor) VALUES (1, 0);
"#,
    },
    Migration {
        version: 2,
        name: "host_data_model_v2",
        // Data model v2 (docs/architecture/0010-data-model-v2.md). The list
        // fields are JSON text like `auth` is; the defaults make every
        // existing row a plain host with none of the new features.
        sql: r#"
ALTER TABLE hosts ADD COLUMN jump_chain TEXT NOT NULL DEFAULT '[]';
ALTER TABLE hosts ADD COLUMN forwards TEXT NOT NULL DEFAULT '[]';
ALTER TABLE hosts ADD COLUMN env TEXT NOT NULL DEFAULT '[]';
ALTER TABLE hosts ADD COLUMN startup_command TEXT;
ALTER TABLE hosts ADD COLUMN agent_forward INTEGER NOT NULL DEFAULT 0
    CHECK (agent_forward IN (0, 1));
ALTER TABLE hosts ADD COLUMN auth_order TEXT;
"#,
    },
];

/// Current schema version (must equal the highest migration version).
pub const SCHEMA_VERSION: u32 = 2;

fn ensure_tracking_table(conn: &Connection) -> Result<(), StoreError> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_migrations (
            version    INTEGER PRIMARY KEY,
            name       TEXT NOT NULL,
            applied_at INTEGER NOT NULL
        );",
    )
    .map_err(StoreError::Sqlite)?;
    Ok(())
}

fn applied_version(conn: &Connection) -> Result<u32, StoreError> {
    ensure_tracking_table(conn)?;
    let version: Option<u32> = conn
        .query_row("SELECT MAX(version) FROM schema_migrations", [], |row| {
            row.get(0)
        })
        .map_err(StoreError::Sqlite)?;
    Ok(version.unwrap_or(0))
}

/// Fails when the database was created/migrated by a newer Termru.
pub(crate) fn verify_not_from_future(conn: &Connection) -> Result<(), StoreError> {
    // A brand-new file has no tracking table yet; that is not "from the future".
    let exists: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='schema_migrations'",
            [],
            |row| row.get(0),
        )
        .map_err(StoreError::Sqlite)?;
    if exists == 0 {
        return Ok(());
    }
    let found = applied_version(conn)?;
    if found > SCHEMA_VERSION {
        return Err(StoreError::SchemaFromFuture {
            found,
            supported: SCHEMA_VERSION,
        });
    }
    Ok(())
}

/// Applies all pending migrations inside a transaction. Safe to call on every
/// open: an already-current database is a no-op.
pub(crate) fn run(conn: &mut Connection) -> Result<(), StoreError> {
    let current = applied_version(conn)?;
    if current > SCHEMA_VERSION {
        return Err(StoreError::SchemaFromFuture {
            found: current,
            supported: SCHEMA_VERSION,
        });
    }
    let pending: Vec<&Migration> = MIGRATIONS.iter().filter(|m| m.version > current).collect();
    if pending.is_empty() {
        return Ok(());
    }
    let tx = conn
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(StoreError::Sqlite)?;
    for migration in pending {
        tx.execute_batch(migration.sql).map_err(|e| {
            StoreError::migration(format!("{} (v{}): {e}", migration.name, migration.version))
        })?;
        tx.execute(
            "INSERT INTO schema_migrations (version, name, applied_at) VALUES (?1, ?2, ?3)",
            rusqlite::params![migration.version, migration.name, crate::util::now_ms()],
        )
        .map_err(|e| StoreError::migration(format!("recording {}: {e}", migration.name)))?;
    }
    tx.commit().map_err(StoreError::Sqlite)?;
    Ok(())
}

/// Test helper: current applied version of a connection.
#[cfg(test)]
pub(crate) fn current_version(conn: &Connection) -> Result<u32, StoreError> {
    applied_version(conn)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_connection_is_migrated_and_idempotent() {
        let mut conn = Connection::open_in_memory().unwrap();
        run(&mut conn).unwrap();
        assert_eq!(current_version(&conn).unwrap(), SCHEMA_VERSION);
        // Re-running applies nothing and must not fail.
        run(&mut conn).unwrap();
        assert_eq!(current_version(&conn).unwrap(), SCHEMA_VERSION);
    }

    #[test]
    fn future_schema_is_rejected() {
        let mut conn = Connection::open_in_memory().unwrap();
        run(&mut conn).unwrap();
        conn.execute(
            "INSERT INTO schema_migrations (version, name, applied_at) VALUES (?1, 'future', 0)",
            [SCHEMA_VERSION + 1],
        )
        .unwrap();
        assert!(matches!(
            verify_not_from_future(&conn),
            Err(StoreError::SchemaFromFuture { .. })
        ));
        assert!(matches!(
            run(&mut conn),
            Err(StoreError::SchemaFromFuture { .. })
        ));
    }

    /// The v1 migration on its own, recorded the way an older Termru did.
    fn at_version_1() -> Connection {
        let mut conn = Connection::open_in_memory().unwrap();
        ensure_tracking_table(&conn).unwrap();
        let v1 = &MIGRATIONS[0];
        let tx = conn.transaction().unwrap();
        tx.execute_batch(v1.sql).unwrap();
        tx.execute(
            "INSERT INTO schema_migrations (version, name, applied_at) VALUES (?1, ?2, 0)",
            rusqlite::params![v1.version, v1.name],
        )
        .unwrap();
        tx.commit().unwrap();
        conn
    }

    #[test]
    fn a_v1_database_migrates_to_v2_and_keeps_its_hosts() {
        let mut conn = at_version_1();
        assert_eq!(current_version(&conn).unwrap(), 1);
        conn.execute(
            "INSERT INTO hosts (id, name, address, port, username, auth, favorite, term, \
             keepalive_seconds, sort_order, created_at, updated_at, deleted, dirty) \
             VALUES ('11111111-1111-4111-8111-111111111111', 'old', 'old.example.com', 2222, \
             'root', '{\"type\":\"agent\"}', 1, 'xterm', 45, 3, 1000, 2000, 0, 0)",
            [],
        )
        .unwrap();

        run(&mut conn).unwrap();
        assert_eq!(current_version(&conn).unwrap(), 2);

        let host = crate::repos::hosts::get_row(&conn, "11111111-1111-4111-8111-111111111111")
            .unwrap()
            .expect("the host survives the migration");
        assert_eq!(host.name, "old");
        assert_eq!(host.port, 2222);
        assert!(host.favorite);
        assert_eq!(host.keepalive_seconds, 45);
        assert_eq!(host.auth, termru_core::AuthMethod::Agent);
        // The new fields read as "not used".
        assert!(host.jump_chain.is_empty() && host.forwards.is_empty() && host.env.is_empty());
        assert!(host.startup_command.is_none() && host.auth_order.is_none());
        assert!(!host.agent_forward);

        // Idempotent: running again changes nothing.
        run(&mut conn).unwrap();
        assert_eq!(current_version(&conn).unwrap(), 2);
    }

    #[test]
    fn a_failed_v2_migration_leaves_the_v1_database_untouched() {
        let mut conn = at_version_1();
        // A column of the same name makes the migration fail half-way.
        conn.execute_batch("ALTER TABLE hosts ADD COLUMN forwards TEXT")
            .unwrap();
        assert!(run(&mut conn).is_err());
        assert_eq!(current_version(&conn).unwrap(), 1);
        let columns: Vec<String> = conn
            .prepare("SELECT name FROM pragma_table_info('hosts')")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert!(
            !columns.contains(&"jump_chain".to_string()),
            "the first ALTER of the failed migration was rolled back: {columns:?}"
        );
    }
}
