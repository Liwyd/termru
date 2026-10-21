# Termru

A cross-platform SSH terminal workspace: connection manager, multiplexed
terminals, workspaces — desktop app (Tauri) with an optional self-hosted
sync server.

**Status: early development.** The SSH engine, the local store, the desktop
UI and the optional cloud sync API are implemented and tested; packaging and
release automation are not finished yet. See [TODO.md](TODO.md) for the
authoritative plan and the open items.

## Install

```bash
# Debian / Ubuntu
curl -fLO https://github.com/Liwyd/termru/releases/download/v0.1.0/termru_0.1.0_amd64.deb
sudo apt install ./termru_0.1.0_amd64.deb

# Fedora
curl -fLO https://github.com/Liwyd/termru/releases/download/v0.1.0/termru-0.1.0-1.x86_64.rpm
sudo dnf install ./termru-0.1.0-1.x86_64.rpm
```

Windows, the AppImage and the details are in
[the install guide](docs/install.md). The packages are not signed: the
`SHA256SUMS` file of the release is the trust anchor.

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
docs/architecture/     ADRs (accepted decisions) and the overview
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

* [Install guide](docs/install.md) — one command per operating system
* [Development guide](docs/development.md) — setup, tests, commands
* [Architecture overview](docs/architecture/overview.md) — how the pieces fit together
* [Architecture decisions](docs/architecture/README.md)
* [Cloud deployment and operations](docs/cloud/deployment.md)
* [Known limitations](docs/limitations.md)
* [Performance measurements](docs/performance.md)

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) and the
[code of conduct](CODE_OF_CONDUCT.md).

## License

Apache-2.0. See [LICENSE](LICENSE).
