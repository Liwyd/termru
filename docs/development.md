# Development guide

How to build, test and change Termru.

## Prerequisites

| Tool | Version | Needed for |
|---|---|---|
| Rust | 1.85 or newer (1.99 tested) | the workspace |
| Node.js | 22.12 or newer | the frontend |
| npm | 10 or newer | the frontend |
| OpenSSH (`sshd`, `ssh-keygen`) | any recent version | the SSH integration tests |
| PostgreSQL | 14 or newer | the cloud server and its tests |
| `cargo-deny` | 0.20 | the supply-chain check |
| Docker + Compose | any recent version | the cloud server image |

On Debian/Ubuntu the desktop app additionally needs the Tauri system
libraries:

```bash
sudo apt-get install -y libwebkit2gtk-4.1-dev libappindicator3-dev \
  librsvg2-dev patchelf libssl-dev
```

## Layout

```
crates/termru-core/     Domain models, validation, the sync wire types.
                        No I/O, no platform dependencies — shared by the
                        desktop app and the cloud server.
crates/termru-store/    SQLite persistence: schema, migrations, repositories.
crates/termru-ssh/      The SSH engine (russh): sessions, known-hosts,
                        reconnect policy.
apps/desktop/           The Tauri app: the Svelte frontend and the Rust
                        backend (src-tauri).
apps/cloud-server/      The self-hosted sync API (Axum + PostgreSQL).
docs/                   Architecture decisions, security, operations.
```

The dependency direction is strictly downwards: `termru-core` knows nothing,
`termru-store` and `termru-ssh` know `termru-core`, the apps know all three.
The cloud server must never depend on the desktop app and vice versa — the
protocol lives in `termru-core` so that both sides agree on it by
construction.

## Everyday commands

Everything is driven from the repository root.

```bash
# Rust
cargo fmt --check                                  # formatting
cargo clippy --workspace --all-targets -- -D warnings   # lints
cargo test --workspace                             # every test

# Frontend
npm install
npm run lint          # prettier + svelte-check
npm run test          # vitest
npm run build         # production bundle
npm run typecheck     # svelte-check only

# The desktop app
npm run tauri dev     # the app against the vite dev server
```

`npm run dev`, `npm run test`, `npm run lint`, `npm run format` and
`npm run typecheck` at the root delegate to the `apps/*` workspaces — there
is no second copy of a script.

## Tests

| Suite | Needs | Without it |
|---|---|---|
| `cargo test -p termru-core` | nothing | — |
| `cargo test -p termru-store` | nothing | — |
| `cargo test -p termru-ssh` | nothing | — |
| `cargo test -p termru-ssh --test sshd` | OpenSSH tooling | prints `SKIPPED:` and passes |
| `cargo test -p termru-cloud-server` | `TERMRU_TEST_DATABASE_URL` | prints `SKIPPED:` and passes |
| `cargo test -p termru-desktop` | nothing | — |
| `cargo test -p termru-desktop --test sync_e2e` | `TERMRU_TEST_DATABASE_URL` **and** `TERMRU_CLOUD_SERVER_BIN` | prints `SKIPPED:` and passes |
| `npm run test` | nothing | — |

A suite whose prerequisite is missing must print a loud `SKIPPED:` line — it
must never pass silently as if the scenario had been exercised.

The SSH integration suite starts a private `sshd` per test on a high port
with its own host key; no root and no system configuration are needed. The
cloud server tests create a throwaway database per test and drop it
afterwards.

To run the cloud server tests locally:

```bash
createdb termru_test
TERMRU_TEST_DATABASE_URL=postgres://user@127.0.0.1:5432/termru_test \
  cargo test -p termru-cloud-server
```

To run the end-to-end test of the desktop client against a real server:

```bash
cargo build -p termru-cloud-server --bin termru-cloud-server
TERMRU_TEST_DATABASE_URL=postgres://user@127.0.0.1:5432/termru_test \
TERMRU_CLOUD_SERVER_BIN=$PWD/target/debug/termru-cloud-server \
  cargo test -p termru-desktop --test sync_e2e
```

## Running the cloud server

```bash
export TERMRU_DATABASE_URL=postgres://user@127.0.0.1:5432/termru
cargo run -p termru-cloud-server
```

The server applies the pending migrations at start-up and serves
`http://127.0.0.1:8787` by default. The configuration is documented in
`apps/cloud-server/README.md`; every setting comes from the environment and
an invalid value aborts the process.

## Conventions

* **Errors are actionable.** A message shown in the UI says what went wrong
  and what the user can do about it. Internal errors are logged with their
  cause and answered with a generic message.
* **Secrets never reach a database, a log or a sync payload.** They live in
  the OS credential store. A `Debug` implementation of a type that holds a
  secret redacts it.
* **Tests live next to the code** (`#[cfg(test)] mod tests`) or in
  `tests/` when they need a real server.
* **Comments explain the *why*.** The code says what it does.
* **Every commit is one topic** and its message says what changed and why.

## The disk

The desktop app pulls in the whole Tauri stack, so a full build is large.
Two settings keep it manageable:

* the `dev` profile uses `debug = 1` (line tables only);
* `CARGO_INCREMENTAL=0` and `CARGO_PROFILE_DEV_DEBUG=0` shrink the
  artifacts further.

If the disk runs low, `cargo clean -p <crate>` removes the artifacts of a
single crate — much cheaper than a full `cargo clean`.
