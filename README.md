# Termru

A cross-platform SSH terminal workspace: connection manager, multiplexed
terminals, workspaces — desktop app (Tauri) with an optional self-hosted
sync server.

**Status: early development.** The SSH engine and local store are implemented
and tested; the desktop UI scaffold exists but the product UI is not usable
yet. See [TODO.md](TODO.md) for the full plan.

## Stack

* **Desktop shell**: Tauri 2 (Rust) — see [ADR 0001](docs/architecture/0001-desktop-stack.md)
* **SSH**: [russh](https://crates.io/crates/russh) (pure Rust), no system OpenSSH dependency
* **UI**: Svelte 5 + Vite 8 + TypeScript 5 + `@xterm/xterm` 6
* **Local storage**: SQLite (WAL) with versioned migrations
* **Cloud sync** (optional, later): PostgreSQL + self-hosted Axum API

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
```

SSH integration tests (`cargo test -p termru-ssh --test sshd`) need OpenSSH
tooling (`sshd`, `ssh-keygen`); they self-skip with a loud message when
missing.

## Repository layout

```
apps/desktop/          Tauri app (frontend + src-tauri)
crates/termru-core/    Domain models, validation, sync payload types
crates/termru-store/   SQLite persistence
crates/termru-ssh/     SSH engine (russh): sessions, known-hosts, reconnect
docs/architecture/     ADRs (accepted decisions)
```

## License

Apache-2.0. See [LICENSE](LICENSE).
