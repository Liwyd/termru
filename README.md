# Termru

A cross-platform SSH terminal workspace: connection manager, multiplexed
terminals, workspaces — desktop app (Tauri) with an optional self-hosted
sync server.

**Status: early development.** The SSH engine, the local store, the desktop
UI and the optional cloud sync API are implemented and tested; packaging and
release automation are not finished yet. See [TODO.md](TODO.md) for the
authoritative plan and the open items.

## Stack

* **Desktop shell**: Tauri 2 (Rust) — see [ADR 0001](docs/architecture/0001-desktop-stack.md)
* **SSH**: [russh](https://crates.io/crates/russh) (pure Rust), no system OpenSSH dependency
* **UI**: Svelte 5 + Vite 8 + TypeScript 5 + `@xterm/xterm` 6
* **Local storage**: SQLite (WAL) with versioned migrations
* **Cloud sync** (optional): PostgreSQL + a self-hosted Axum API — see
  [ADR 0003](docs/architecture/0003-persistence-split.md) and
  [the deployment guide](docs/cloud/deployment.md)

## Features

* Host profiles with password, private key or SSH-agent authentication,
  groups, tags, favorites and recents.
* A tabbed terminal workspace with split panes, multiple windows, a command
  palette, global search and remappable keybindings.
* Host-key trust on first use, with a hard warning when a key changes.
* Optional sync of the configuration (never of secrets) across your devices.

## Development

Prerequisites: Rust 1.85+, Node 22.12+, npm 10+.

```bash
# Rust workspace
cargo test --workspace
cargo clippy --workspace --all-targets
cargo fmt --check

# Frontend
npm install
npm run typecheck
npm run test

# Desktop app (dev)
npm run tauri dev

# Cloud server (needs a PostgreSQL server)
TERMRU_DATABASE_URL=postgres://user@127.0.0.1:5432/termru \
  cargo run -p termru-cloud-server
```

The SSH integration tests (`cargo test -p termru-ssh --test sshd`) need
OpenSSH tooling (`sshd`, `ssh-keygen`); they self-skip with a loud message
when it is missing. The cloud server tests need a PostgreSQL server; they
self-skip with a loud message when `TERMRU_TEST_DATABASE_URL` is not set.

## Running the cloud server

See [docs/cloud/deployment.md](docs/cloud/deployment.md) for the install
command, the first-run setup, backups, upgrades and troubleshooting.

## Repository layout

```
apps/desktop/          Tauri app (frontend + src-tauri)
apps/cloud-server/     Self-hosted sync API (Axum + PostgreSQL)
crates/termru-core/    Domain models, validation, sync payload types
crates/termru-store/   SQLite persistence
crates/termru-ssh/     SSH engine (russh): sessions, known-hosts, reconnect
docs/architecture/     ADRs (accepted decisions)
docs/cloud/            Cloud deployment and operations
docs/security/         Threat model and dependency review
```

## Data locations

| What | Where |
|---|---|
| Hosts, preferences, workspaces | the app's data directory (SQLite, `0600`) |
| Passwords, passphrases, private key paths | the OS credential store (Secret Service / Credential Manager / Keychain) |
| Logs | the app's data directory (`logs/termru.log`) |
| Cloud server state | the PostgreSQL database of your deployment |

The desktop app never blocks on the cloud service: everything works offline
and the sync queue catches up later.

## Security

Report a vulnerability privately, see [SECURITY.md](SECURITY.md). The threat
model and the known residual risks are documented in
[docs/security/threat-model.md](docs/security/threat-model.md) and
[docs/limitations.md](docs/limitations.md).

## Documentation

* [Development guide](docs/development.md) — setup, tests, commands
* [Architecture decisions](docs/architecture/README.md)
* [Cloud deployment and operations](docs/cloud/deployment.md)
* [Known limitations](docs/limitations.md)
* [Performance measurements](docs/performance.md)

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) and the
[code of conduct](CODE_OF_CONDUCT.md).

## License

Apache-2.0. See [LICENSE](LICENSE).
