# Termru cloud server

The optional, self-hosted sync service of Termru. It stores the configuration
of a user's hosts (and their groups, tags, identities and preferences) and
hands it to the user's other devices.

**It never stores a secret.** Passwords, passphrases and private keys stay in
the credential store of the device they belong to. Only the sync payload
described in `docs/architecture/0005-sync-protocol.md` crosses the wire.

## Running it

```bash
export TERMRU_DATABASE_URL=postgres://termru:termru@127.0.0.1:5432/termru
export TERMRU_BIND_ADDR=127.0.0.1:8787
cargo run -p termru-cloud-server
```

The server applies the pending migrations at start-up (guarded by a
PostgreSQL advisory lock, so several replicas may boot at the same time) and
then serves HTTP until it receives `SIGINT` or `SIGTERM`.

## Configuration

Every setting comes from the environment. An invalid value aborts the process
with a message that names the variable — the server never starts in a
half-configured state.

| Variable | Default | Meaning |
|---|---|---|
| `TERMRU_DATABASE_URL` | *(required)* | PostgreSQL URL (`postgres://…`) |
| `TERMRU_BIND_ADDR` | `0.0.0.0:8787` | Address the HTTP server binds to |
| `TERMRU_POOL_MAX_SIZE` | `16` | Pooled database connections |
| `TERMRU_ACCESS_TOKEN_TTL_SECS` | `3600` | Lifetime of an access token |
| `TERMRU_REFRESH_TOKEN_TTL_SECS` | `2592000` | Lifetime of a refresh token |
| `TERMRU_REFRESH_REUSE_GRACE_SECS` | `30` | Window in which a replayed refresh token counts as a race instead of a theft |
| `TERMRU_RATE_LIMIT_PER_MINUTE` | `120` | Requests per minute per client |
| `TERMRU_LOGIN_MAX_FAILURES` | `5` | Failed logins before a lockout |
| `TERMRU_LOGIN_LOCKOUT_SECS` | `900` | Length of the lockout |
| `TERMRU_REQUEST_TIMEOUT_SECS` | `30` | Server-side request timeout |
| `TERMRU_MAX_BODY_BYTES` | `4194304` | Maximum request body size |
| `TERMRU_TRUST_FORWARDED_FOR` | `false` | Use `X-Forwarded-For` as the client address (only behind a proxy you control) |

`RUST_LOG` selects the log level (for example `RUST_LOG=info,sqlx=warn`).

## API

| Method | Path | Purpose |
|---|---|---|
| `GET` | `/healthz` | liveness |
| `GET` | `/readyz` | readiness (checks the database) |
| `POST` | `/v1/auth/register` | create an account and sign in the first device |
| `POST` | `/v1/auth/login` | sign in |
| `POST` | `/v1/auth/refresh` | rotate a token pair |
| `POST` | `/v1/auth/logout` | revoke the session of the caller |
| `GET` | `/v1/devices` | list the devices of the caller |
| `POST` | `/v1/devices` | enroll an additional device |
| `PATCH` | `/v1/devices/{id}` | rename a device |
| `DELETE` | `/v1/devices/{id}` | revoke a device (immediately invalidates its tokens) |
| `GET` | `/v1/sync/changes?since=<rev>&limit=<n>` | pull the changed records |
| `POST` | `/v1/sync/changes` | push a batch of mutations |

Authenticated calls use `Authorization: Bearer <access token>`. Tokens are
opaque random values that are stored hashed; a stolen database does not hand
out usable credentials.

Error responses always look like this:

```json
{ "error": { "code": "conflict", "message": "…" } }
```

## Security notes

* Passwords are hashed with argon2id; a login for an unknown account spends
  the same work as a real verification, so the response time does not reveal
  whether an account exists.
* Refresh tokens rotate on every use. A replayed refresh token revokes the
  whole token family (with a short grace window for honest races).
* Every query is scoped to the user of the token; one account can never read
  or modify the records of another.
* The rate limiter counts per access token and per client address. The
  limiter is in-process: with several replicas each enforces its own budget.
* CORS is denied by default (no CORS headers are sent at all), so a browser
  can not use the API from another origin. The desktop client is not a
  browser, and a deployment that wants a web client should put an
  authentication-aware reverse proxy in front.
* The database connection is **not** encrypted (`NoTls`). Run the database on
  a private network next to the server, or terminate TLS in a proxy.

## Backup and restore

```bash
export TERMRU_DATABASE_URL=postgres://termru:termru@127.0.0.1:5432/termru
./scripts/postgres-backup.sh ./backups      # writes a .dump and a .sha256
./scripts/postgres-restore.sh ./backups/termru-<timestamp>.dump
```

The restore script verifies the checksum before it touches the database and
refuses to run on a mismatch.

## Tests

The integration tests need a real PostgreSQL server. Without
`TERMRU_TEST_DATABASE_URL` they print a loud `SKIPPED:` line and pass
without asserting.

```bash
TERMRU_TEST_DATABASE_URL=postgres://termru@127.0.0.1:5432/postgres \
  cargo test -p termru-cloud-server
```

Each test creates its own throwaway database and drops it afterwards.
