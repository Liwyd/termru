# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0-rc.1] — 2026-10-22

The pre-release that exercised the release pipeline: the packages, the
cloud image and the GitHub release were built and published by the pipeline
alone. It is marked as a pre-release and its image carries no `latest` tag.

## [0.1.0] — unreleased

The first version. Everything below is implemented and covered by tests; the
open items are listed in [TODO.md](TODO.md) and in
[docs/limitations.md](docs/limitations.md).

### Added

* Core domain models, validation, and sync payload types (`termru-core`).
* SQLite persistence with migrations, backup/restore, soft-delete tombstones
  (`termru-store`).
* SSH engine on russh: auth (password/key/agent), known-hosts with TOFU and
  changed-key detection, PTY sessions with reconnect policy (`termru-ssh`).
* SSH integration test suite against a real OpenSSH server.
* Architecture decision records under `docs/architecture/`, and an overview
  that ties them together.
* Desktop app: host list and editor, connection and host-key dialogs, tabbed
  terminal workspace over Tauri channels, command palette, keybinding
  registry, OS keyring for secrets, file logging and panic hygiene.
* Split panes: a tab holds a pane tree, the divider is draggable, the panes
  are navigable with the keyboard, and the tree is saved with the layout.
* Folders and tags: the hosts are grouped in the sidebar, the search covers
  the description and the tag names.
* An identity editor with a native key file picker; the key file is never
  copied into the database and the binding stays device-local.
* A settings dialog (theme, font, scrollback, cursor, copy-on-select) and a
  keybinding remapping UI; both are stored in the portable preferences, so
  they are synced with the other devices.
* Self-hosted cloud sync API (`apps/cloud-server`): accounts with argon2id
  and rotating opaque tokens, device enrollment and revocation, and the
  revision-based sync protocol (pull/push, tombstones, explicit conflicts,
  idempotent retries) shared with the desktop client.
* Desktop sync client: sign in to a sync server, push and pull the host
  configuration, and resolve conflicts explicitly (keep the local or take the
  server copy). The tokens stay in the OS credential store; a secret is never
  part of a sync payload. An end-to-end test drives the client against the
  real cloud server.
* Automatic background sync: the app syncs every few minutes on its own,
  refreshes the access token before it expires and retries a failure with a
  bounded backoff.
* Container image and `compose.yaml` for the cloud server, with a
  checksum-verified install command and a backup/restore procedure.
* The release pipeline: the packages, their checksums, the cloud image in
  the GitHub container registry, and a smoke test that installs what it
  built.

### Fixed

* A local edit did not mark the record dirty, so a change from another device
  silently overwrote the local copy instead of raising a conflict. Local
  edits now mark the record dirty in every repository, and the conflict
  resolution ("keep local" / "take remote") is implemented in the store.
* A closed window left the sessions it showed running with nothing rendering
  them. A view now knows its window, and a destroyed window releases the
  sessions nobody shows any more.

### Changed

* The cloud server uses `tokio-postgres`/`deadpool-postgres` instead of
  `sqlx`: the `sqlx` facade can not share a workspace with `rusqlite`
  because both would link the native library `sqlite3` (see ADR 0003).
* The workspace `dev` profile keeps line tables only (`debug = 1`), so the
  build artifacts stay small enough for constrained development machines.
* The executable is called `termru` (not `termru-desktop`), so the packages
  install it as `/usr/bin/termru`.
