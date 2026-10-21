# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

* Core domain models, validation, and sync payload types (`termru-core`).
* SQLite persistence with migrations, backup/restore, soft-delete tombstones
  (`termru-store`).
* SSH engine on russh: auth (password/key/agent), known-hosts with TOFU and
  changed-key detection, PTY sessions with reconnect policy (`termru-ssh`).
* SSH integration test suite against a real OpenSSH server.
* Architecture decision records under `docs/architecture/`.
* Desktop app scaffold: Tauri 2 + Svelte 5 + Vite 8 + TypeScript 5 + xterm 6.
* Desktop app: host list and editor, connection and host-key dialogs, tabbed
  terminal workspace over Tauri channels, command palette, keybinding
  registry, OS keyring for secrets, file logging and panic hygiene.
* Self-hosted cloud sync API (`apps/cloud-server`): accounts with argon2id
  and rotating opaque tokens, device enrollment and revocation, and the
  revision-based sync protocol (pull/push, tombstones, explicit conflicts,
  idempotent retries) shared with the desktop client.
* Container image and `compose.yaml` for the cloud server, with a
  checksum-verified install command and a backup/restore procedure.
* Desktop sync client: sign in to a sync server, push and pull the host
  configuration, and resolve conflicts explicitly (keep the local or take the
  server copy). The tokens stay in the OS credential store; a secret is never
  part of a sync payload.

### Fixed

* A local edit did not mark the record dirty, so a change from another device
  silently overwrote the local copy instead of raising a conflict. Local
  edits now mark the record dirty in every repository, and the conflict
  resolution ("keep local" / "take remote") is implemented in the store.

### Changed

* The cloud server uses `tokio-postgres`/`deadpool-postgres` instead of
  `sqlx`: the `sqlx` facade can not share a workspace with `rusqlite`
  because both would link the native library `sqlite3` (see ADR 0003).
* The workspace `dev` profile keeps line tables only (`debug = 1`), so the
  build artifacts stay small enough for constrained development machines.
