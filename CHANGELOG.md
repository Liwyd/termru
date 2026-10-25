# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.2.0] — 2026-10-25

The first release of the UI/UX redesign (Phase 19, R1–R3 of the plan in
`TODO.md`): a new visual language, a routed shell and a rebuilt hosts page.
Everything is covered by the test suites; the remaining redesign steps
(sessions view, editors as pages, dialogs, accessibility pass) follow in
later releases.

### Added

* Design-token layer per the redesign specification: a retargeted palette,
  a spacing/geometry scale, a type scale with self-hosted Inter and IBM
  Plex Mono fonts (no remote font URLs — the CSP stays `font-src 'self'`),
  and motion tokens with a reduced-motion collapse.
* A dark theme re-derived from the same semantic tokens, every text pair
  verified at ≥ 4.5:1 contrast, and the terminal color scheme from the
  specification.
* A routed application shell: a 36 px titlebar, a 64 px navigation
  (Hosts · Sessions · Folders · Identities · Sync · Preferences), a status
  bar with a per-route line, a hash-synced route store and `route.*`
  palette commands. Per-platform window controls (macOS overlay, Windows
  custom titlebar, Linux keeps the native decorations — documented).
* The hosts page (frame 1): a windowed host table (STAR · HOST/ADDRESS ·
  FOLDER · AUTHENTICATION · LAST SESSION · ACTIONS) that renders ~16 DOM
  rows at 10,000 hosts, filter tabs with live counts, a search over name,
  address, username, description and tag *names* (`Ctrl F`), a folder
  dropdown with nested paths, a spelled count line ("Five saved
  connections. Two sessions running."), a "your sessions are still
  running" banner that jumps back to a live session, and the full state
  matrix (loading, empty, filtered-empty, error, offline note).
* In-app confirmation dialogs replace every `window.confirm`.

### Changed

* `recent_list` now returns `last_used_at` next to each host, so the
  table can show `Connected` / `Today, 09:18` / `Yesterday` / `18 Oct` /
  `—` (internal IPC; both sides ship in one artifact).
* The host list renders a window of rows and filters inside a frame at
  10,000 hosts (measured: 4.7–7.7 ms of store work per keystroke; the
  same interaction cost 2.2 s before — see `docs/performance.md`).
* Preferences gained additive fields with serde defaults
  (`reduced_motion`, `focus_visibility`, `routine_notices`,
  `terminal.line_height`); old local and synced records load unchanged.

## [0.1.0-rc.1] — 2026-10-22

The pre-release that exercised the release pipeline: the packages, the
cloud image and the GitHub release were built and published by the pipeline
alone. It is marked as a pre-release and its image carries no `latest` tag.

## [0.1.1] — 2026-10-23

The fixed build, re-packaged: `v0.1.0` was re-published on the day of
the release to fix the startup crash listed below under its *Fixed*,
but the re-uploaded assets kept their name, which made the broken and
the fixed download indistinguishable. `v0.1.1` carries the same code
as the fixed `v0.1.0`, plus the correction of the install commands, so
the artifacts to download are unmistakable.

### Documentation

* The install commands in `docs/install.md` and the README used the
  lower-case file names (`termru_…`), while the published assets are
  `Termru_…` — the documented commands returned HTTP 404. The file
  names and the version now match the release they document.

## [0.1.0] — 2026-10-22

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
* The packaged apps exited immediately on launch: the background sync
  scheduler called `tokio::spawn` from Tauri's `setup` hook, which runs
  on the main thread before a Tokio runtime exists ("there is no reactor
  running"). It now uses `tauri::async_runtime::spawn`, which runs its
  own standalone runtime; a regression test starts the scheduler from
  a plain test with no runtime at all. The `v0.1.0` upload was re-cut
  with this fix within the same day.

### Changed

* The cloud server uses `tokio-postgres`/`deadpool-postgres` instead of
  `sqlx`: the `sqlx` facade can not share a workspace with `rusqlite`
  because both would link the native library `sqlite3` (see ADR 0003).
* The workspace `dev` profile keeps line tables only (`debug = 1`), so the
  build artifacts stay small enough for constrained development machines.
* The executable is called `termru` (not `termru-desktop`), so the packages
  install it as `/usr/bin/termru`.
