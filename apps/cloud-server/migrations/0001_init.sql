-- Termru cloud sync schema (revision 1).
--
-- Design notes (see docs/architecture/0005-sync-protocol.md):
-- * `users.next_rev` is the per-user monotonic revision counter. Every accepted
--   mutation gets the next value; revisions are unique per user.
-- * `records` stores the *latest* state of every record per user, including
--   tombstones (`deleted = true`, `payload = NULL`).
-- * `applied_mutations` provides idempotency for retried pushes.
-- * `refresh_tokens` keeps the hash of every issued refresh token so that a
--   replay (reuse) can be detected even after a rotation.

CREATE TABLE users (
    id            UUID PRIMARY KEY,
    email         TEXT NOT NULL,
    password_hash TEXT NOT NULL,
    next_rev      BIGINT NOT NULL DEFAULT 0,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT users_email_length CHECK (length(email) BETWEEN 3 AND 254)
);

CREATE UNIQUE INDEX users_email_unique ON users (email);

CREATE TABLE devices (
    id           UUID PRIMARY KEY,
    user_id      UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    name         TEXT NOT NULL,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    revoked_at   TIMESTAMPTZ,
    last_seen_at TIMESTAMPTZ,
    CONSTRAINT devices_name_length CHECK (length(name) BETWEEN 1 AND 128)
);

CREATE INDEX devices_user_idx ON devices (user_id);

CREATE TABLE sessions (
    id                UUID PRIMARY KEY,
    user_id           UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    device_id         UUID NOT NULL REFERENCES devices (id) ON DELETE CASCADE,
    family_id         UUID NOT NULL,
    access_hash       BYTEA NOT NULL,
    access_expires_at TIMESTAMPTZ NOT NULL,
    issued_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    revoked_at        TIMESTAMPTZ,
    revoked_reason    TEXT
);

CREATE UNIQUE INDEX sessions_access_hash_unique ON sessions (access_hash);
CREATE INDEX sessions_family_idx ON sessions (family_id);
CREATE INDEX sessions_user_idx ON sessions (user_id);
CREATE INDEX sessions_device_idx ON sessions (device_id);

CREATE TABLE refresh_tokens (
    hash       BYTEA PRIMARY KEY,
    session_id UUID NOT NULL REFERENCES sessions (id) ON DELETE CASCADE,
    family_id  UUID NOT NULL,
    user_id    UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    issued_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at TIMESTAMPTZ NOT NULL,
    used_at    TIMESTAMPTZ
);

CREATE INDEX refresh_tokens_family_idx ON refresh_tokens (family_id);
CREATE INDEX refresh_tokens_session_idx ON refresh_tokens (session_id);

CREATE TABLE records (
    user_id    UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    kind       TEXT NOT NULL,
    id         TEXT NOT NULL,
    rev        BIGINT NOT NULL,
    deleted    BOOLEAN NOT NULL DEFAULT FALSE,
    payload    JSONB,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, kind, id),
    CONSTRAINT records_payload_present CHECK (deleted OR payload IS NOT NULL)
);

CREATE INDEX records_user_rev_idx ON records (user_id, rev);

CREATE TABLE applied_mutations (
    user_id     UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    mutation_id UUID NOT NULL,
    rev         BIGINT NOT NULL,
    applied_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, mutation_id)
);

CREATE TABLE audit_log (
    id        BIGSERIAL PRIMARY KEY,
    user_id   UUID,
    device_id UUID,
    event     TEXT NOT NULL,
    detail    JSONB,
    ip        TEXT,
    at        TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX audit_log_user_idx ON audit_log (user_id, at DESC);
CREATE INDEX audit_log_event_idx ON audit_log (event, at DESC);

CREATE TABLE login_attempts (
    key          TEXT PRIMARY KEY,
    failures     INTEGER NOT NULL DEFAULT 0,
    window_start TIMESTAMPTZ NOT NULL DEFAULT now(),
    locked_until TIMESTAMPTZ
);
