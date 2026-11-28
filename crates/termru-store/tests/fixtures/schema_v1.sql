-- A frozen copy of schema version 1 (what Termru 0.2.0 created before data
-- model v2). The migration tests build a database from it, put rows in it
-- the way that version did, and open it with the current code. Never edit it
-- to follow the schema: the point is that it stays old.
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

CREATE TABLE IF NOT EXISTS schema_migrations (
    version    INTEGER PRIMARY KEY,
    name       TEXT NOT NULL,
    applied_at INTEGER NOT NULL
);
INSERT INTO schema_migrations (version, name, applied_at) VALUES (1, 'initial_schema', 0);
