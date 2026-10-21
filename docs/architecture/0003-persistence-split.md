# 0003. Persistence split: local SQLite, cloud PostgreSQL

* Status: Accepted
* Date: 2026-10-19

## Context

Termru has two persistence surfaces:

1. **Local** — the desktop app's primary store: hosts, identities (metadata),
   groups, tags, favorites, recents, preferences, workspaces, sync queue.
   Must work fully offline, be fast on modest machines, be trivially
   backup-able, and be safe with concurrent access from the app and from
   external tools.
2. **Cloud** — an optional self-hosted sync manager serving multiple devices
   of the same user, written in `apps/cloud-server`.

Data volume for both is small (thousands of hosts per user, not millions),
but the cloud service must tolerate concurrent writes from several devices.

## Decision

* **Local: SQLite** via `rusqlite` with the bundled (amalgamation) library,
  in `crates/termru-store`, with **forward-only, versioned migrations**
  recorded in `user_version`.
  * WAL mode, `busy_timeout`, `foreign_keys=ON`, one connection behind a
    mutex (single-process app; serializing writes avoids `SQLITE_BUSY`
    complexity while WAL still lets external tools read).
  * File permissions `0600`; `quick_check` on open with an actionable
    corruption error.
  * Every user mutation and its sync-queue entry are written in **one
    transaction** — a crash can never produce a queued-but-missing edit.
  * Backup = `VACUUM INTO` + atomic rename; restore verifies and keeps the
    pre-restore file aside.
  * Opening a database created by a *newer* schema fails loudly instead of
    corrupting it.
* **Cloud: PostgreSQL + `tokio-postgres`/`deadpool-postgres`**, one `compose`
  service alongside the app.
  * Rationale: standard self-hosted shape (healthcheck-gated startup,
    `pg_dump` backup/restore, safe concurrent multi-device writes) at the
    cost of one extra container.
  * Migrations run at container start with single-runner lock safety.
  * **Driver choice (revised 2026-10-21):** the original decision named
    `sqlx`. The `sqlx` facade crate can not be used in this workspace: it
    depends on an *optional* `sqlx-sqlite` driver, and cargo allows only one
    package per `links` value in a dependency graph. `sqlx-sqlite` links the
    native library `sqlite3`, which the desktop app already links through
    `rusqlite` — the two collide even though the sqlite driver is never
    enabled (cargo resolves optional dependencies into the lock file and
    rejects the conflicting `links` value). Depending on `sqlx-core` +
    `sqlx-postgres` directly would avoid the collision, but only by giving
    up the `sqlx::` facade for the raw `sqlx_core::` paths.
    `tokio-postgres` + `deadpool-postgres` give the same capabilities
    (async Postgres, a pool, transactions) with no native library involved,
    a smaller dependency tree, and no compile-time database requirement.
    The wire types and the protocol are unaffected: they live in
    `termru-core` and are shared with the desktop client.

Secrets are in **neither** database — see ADR 0004.

## Consequences

* Local app has zero external service dependencies for its core function
  (SSH sessions never require the cloud store or network beyond the SSH
  target itself).
* Two migration systems (SQLite `PRAGMA user_version`, Postgres) must both
  be tested forward-on-empty and forward-on-existing data.
* SQLite single-connection mutex caps local write concurrency — acceptable:
  the app's write rate is user-driven and low. Revisit only if profiling
  shows contention (would manifest as UI stalls during bulk import).
* Cloud schema changes require Postgres-version-aware migration testing in
  CI (service container).
* The cloud server talks to PostgreSQL without TLS (`NoTls`). The database is
  expected to live on a private network next to the server (the `compose`
  deployment of ADR 0013). A deployment that needs an encrypted database
  connection has to add a TLS-terminating proxy; this is recorded as a
  residual risk in `docs/security/threat-model.md`.
* Because the driver has no compile-time query checking, every query is
  reviewed by hand and the integration tests run against a real PostgreSQL
  server (not a mock).

## Alternatives considered

* **Same engine locally (rusqlite) and in cloud**: Postgres on the desktop
  is an operational non-starter; SQLite in the cloud would make multi-device
  concurrent writes and hosted backup/restore needlessly risky.
* **Local JSON files**: no transactions, no crash safety, no query
  discipline for the sync queue; rejected.
* **Postgres locally**: heavyweight service dependency for an offline-first
  desktop app; rejected.
* **Embedded alternatives (sled, rocksdb)**: less mature ecosystem, weaker
  tooling (`sqlite3` CLI, `VACUUM INTO` backup story), no need for their
  strengths; rejected.
