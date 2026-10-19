# 0005. Sync protocol: monotonic revisions, tombstones, explicit conflicts

* Status: Accepted
* Date: 2026-10-19

## Context

Multiple devices of one user must converge on the same configuration
(hosts, identities, groups, tags, portable preferences). Constraints:

* offline-first: local edits must never block on the server;
* deletions must propagate (no zombie records);
* the server must not be trusted to validate semantics of records it stores
  opaquely — a compromised or malicious *peer device* must not be able to
  inject invalid/hostile data into another device;
* conflicts must be *visible*, never silently resolved in a way the user
  did not choose;
* retries must be safe (clients retry aggressively after network failure).

## Decision

Revision-based, per-user, transactional sync (see `crates/termru-core/src/sync.rs`
for wire types):

* Every accepted mutation assigns the user a **monotonically increasing
  integer `rev`** on the server.
* **Pull**: `GET /v1/sync/changes?since=<rev>` returns records with
  `rev > since`, **cursor-paginated with a bounded page size**
  (`MAX_PULL_RECORDS`), including **tombstones** for deletions.
* **Push**: `POST /v1/sync/changes` submits an ordered batch. Each mutation
  carries:
  * `mutation_id` — client-generated UUID, **idempotency key** (server
    applies each at most once; safe to retry);
  * `base_rev` — the server revision the client last observed for that
    record.
* **Conflict detection**: if `base_rev` is stale (record mutated by someone
  else since), the server **does not apply** that mutation and returns it in
  a `409` payload *alongside the current server record*. The client surfaces
  the conflict in the UI with local/remote comparison and an explicit
  resolution (keep local / take remote). Deletions vs edits are surfaced the
  same way — records are never silently resurrected or dropped.
* **Payload handling**: server enforces size/kind/id limits
  (`MAX_PAYLOAD_BYTES`, `MAX_BATCH_MUTATIONS`) but treats record *content*
  as opaque; the **client re-validates every pulled record** before applying
  it (`serde` + domain validation), so hostile content is rejected at the
  boundary that understands it.
* **Schema version** field on payloads (`CLIENT_SCHEMA_VERSION`); incompatible
  changes bump it and require a migration path before shipping.
* Push and pull are applied **transactionally in the local store**: a local
  mutation is queued in the same SQLite transaction as the row write.
* Record kinds v1: `host`, `identity`, `group`, `tag`, `preferences`
  (singleton). Secrets are excluded — see ADR 0004.

## Consequences

* No CRDT/LWW complexity; convergence requires the client to resolve
  conflicts explicitly, which is the intended UX.
* Cursor pagination (`since=<rev>`) is O(changed records) — no `OFFSET`
  scans; safe at any record count for one user.
* Server logic stays simple and small (auth, scoping, batch apply,
  conflict compare), which keeps the authz surface auditable.
* Bounded batches mean a huge backlog syncs over multiple round trips;
  clients loop until no more changes.
* Each user's `rev` counter is a single hot row per user during bursts of
  writes from many devices — acceptable at the expected scale (a handful of
  devices per user); revisit only with evidence.

## Alternatives considered

* **Last-write-wins timestamps**: silently loses user edits; requires trusting
  client clocks; rejected.
* **CRDTs**: unjustified complexity for a low-write config dataset, and still
  needs a UI story for semantic conflicts (deleting a host another device
  edited); rejected.
* **Per-record vector clocks**: more complex cursoring and payload shape for
  no gain over per-user revisions with explicit conflicts; rejected.
* **Websocket push**: adds long-lived connection state for a low-frequency
  data set; polling on foreground/reconnect is sufficient for v1.
