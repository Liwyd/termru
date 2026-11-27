# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

* SSH authentication is an ordered chain driven by the server (ADR 0008):
  a `none` probe lists the allowed methods, methods the server does not allow
  are skipped, partial success is honoured (publickey, then
  keyboard-interactive for two-factor logins), and a failed login lists what
  was tried and what the server allows. Hosts get a sensible default chain
  (password, key or agent, each followed by keyboard-interactive; a stored
  password answers a lone "Password:" prompt).
* Keyboard-interactive authentication with a dialog for the server's
  questions (one-time codes, PAM prompts): queued, cancellable, answered with
  Enter, password fields hidden, closed again when the connection ends.
  The time a person takes is not charged to the connect or authentication
  deadline.
* An encrypted key whose passphrase is not stored asks for it ("Passphrase for
  *identity*") and offers to remember it; it is stored only after the
  connection worked. Key hosts no longer ask for a passphrase before
  connecting.
* RSA keys (file and agent) sign with the hash the server announces
  (`rsa-sha2-512`/`-256`) instead of a fixed one; a certificate next to the key
  (`<key>-cert.pub`) and agent certificates are used; keys the agent cannot sign
  with are skipped and at most 5 agent keys are offered, so `MaxAuthTries` is
  not exhausted; the OpenSSH named-pipe agent and Pageant on Windows (untested,
  see `docs/limitations.md`).
* The server's authentication banner is shown as plain text at the start of the
  terminal (control characters and escape sequences removed).
* The host-key question warns when the host is already known under other key
  types, and the handshake asks for the known key types first.
* Sessions page rebuilt to the design frames (R4): a tab strip with
  `+ New session`, a toolbar with the status, `user@host:port`, the folder,
  Split H / Split V, Duplicate, Detach and a More menu, and a bar with
  Reconnect and Close session. Each pane has a mono header
  (`host / shell / active`, or `ACTIVE PANE` and the grid when alone), a 2px
  accent rule when focused, a footer (`INPUT READY · UTF-8 · LF` and the grid),
  an Expand button and a divider you can drag, double-click to even out, or move
  with the arrow keys. A hint above a split tab explains the split and can be
  dismissed. The status bar says how long the session has been connected.
* Terminal context menu (copy, paste, select all, clear scrollback, find,
  split, and open/copy for a link under the pointer), reachable from the
  keyboard (Menu key, Shift+F10).
* Find in the terminal (`@xterm/addon-search`): Ctrl+Shift+F (Cmd+F on macOS)
  inside a terminal, with match highlighting, a match count, Enter and
  Shift+Enter, a match-case toggle and Escape to close.
* Links in terminal output (`@xterm/addon-web-links`): Ctrl or Cmd+click.
  Only `http` and `https` links are followed and they never navigate the app
  window; the address goes to the system opener when the app has one and is
  copied to the clipboard otherwise (the opener plugin is not part of this build
  yet).
* A paste of two or more lines asks first (line count and a preview), with
  "Don't ask again" for the rest of the session. Control characters are removed
  from pasted text.
* Closing a tab or a pane with a live session asks first (the close button,
  Close session, `Ctrl+Shift+W` and the pane shortcut).
* Host editor rebuilt to the design frame (R5): a routed page with numbered
  sections (`01 / CONNECTION`, `02 / AUTHENTICATION`, `03 / ORGANIZATION` and
  `04 / ADVANCED` for the terminal type and the keepalive), a right rail with
  the unsaved-changes indicator, the local credential (a stored password or
  key passphrase, with a Forget button that calls `secret_delete`) and the
  connection preview (`ssh user@host -p port`), and Cancel / Save host /
  Save & connect in a footer that stays in reach. Fields validate on blur with
  the backend's own rules (`validate.rs`) and the messages are linked with
  `aria-invalid` / `aria-describedby`; a refusal from the backend is put next
  to its field. A password can be typed and kept in the credential store from
  the editor; a keepalive of 0 reads "Automatic (30 s)". Leaving the page,
  opening another host or closing the window with unsaved changes asks in an
  in-app dialog first.
* Folders page rebuilt to the design frame (R6): a folder tree with host
  counts, the hosts of the selected folder with Move and "Choose hosts", and an
  edit rail. A folder cannot be moved under itself or one of its descendants
  (the parent picker shows those folders disabled with the reason; the store
  refuses them too, tests added) and deleting asks, saying what happens to the
  hosts and subfolders inside (they move up to the parent).
* Identities page: the list, the key file, the fingerprint and the public key
  with copy buttons, rename, the hosts that use the identity (each opens its
  editor) and a delete that lists them. New `identity_generate` command writes
  an ed25519 key (OpenSSH format, optional passphrase) into the app's `keys/`
  folder with mode 0600, never overwrites a file, and can keep the passphrase
  in the credential store under the identity; `identity_public_key` reads the
  public key of the bound file without its passphrase. `ssh-key` is now a
  direct dependency of the desktop crate (already in the tree through
  `russh`; no new crate).

### Security

* Dependency updates for RustSec advisories: `russh` 0.64 -> 0.65
  (RUSTSEC-2026-0495; also pulls `rsa` 0.10.0, which resolves
  RUSTSEC-2023-0071 so its `deny.toml` ignore is removed), `rusqlite`
  0.40 -> 0.41 / `libsqlite3-sys` 0.39 (RUSTSEC-2026-0459, bundled SQLite
  CVE), `url` 2.5.8 -> 2.5.9 (RUSTSEC-2026-0592) and `cookie` 0.18.2 ->
  0.18.3 (RUSTSEC-2026-0594).
* `chrono` (RUSTSEC-2026-0543, unmaintained, no patched release) is
  accepted with a dated `deny.toml` ignore (review by 2027-03-01);
  migration to `jiff`/`time` is tracked in `TODO.md`.

### Fixed

* Desktop UI: switching routes or splitting panes no longer kills SSH
  sessions (terminals live in a registry outside the route tree); app
  shortcuts work inside the terminal; "+ New host" and other dead buttons
  work again.
* SSH engine: a large paste can no longer deadlock a connection (separate
  reader and writer tasks); keystrokes keep their order (synchronous write
  command plus an ordered, batching write queue per session in the UI);
  `TCP_NODELAY` and TCP keepalive are set; new hosts default to a 30 s SSH
  keepalive and a stored `0` now means "automatic (30 s)".
* A killed remote shell or a server-side close ends the session instead of
  reconnecting in a loop; only transport loss is retried, with a lifetime
  cap of 20 reconnects.
* One overall connect deadline covers DNS, TCP, key exchange and shell
  setup; addresses are raced (Happy Eyeballs) and cancel works in every
  phase.
* Trusting a changed host key only replaces this host's key of the same
  type in `known_hosts`; wildcard, `@revoked`, `@cert-authority` and other
  hosts' lines are kept, as are the file's permissions.
* A view that attaches while output streams sees every byte exactly once;
  the terminal size set while connecting is applied on every connect.
* A tab moved to a new window keeps its session: a session without a view
  now lives for a 10 s grace period instead of being closed at once.
* Hosts and preferences reload after a sync pull (`sync-applied` event).

### Changed

* Key passphrases are stored per identity in the credential store (not per
  host); an entry stored per host by an earlier version is still read.
* Host-key and authentication questions carry the session they belong to and
  go to the window that shows it (before: every window); they are removed when
  the question times out or the session closes, and a full registry refuses the
  new question instead of clearing the old ones.
* known_hosts is read and written off the async workers.
* The default terminal line height is 1.3.
* Session output is merged into larger updates (up to 64 KiB / 8 ms);
  small chunks such as keystroke echo are still sent immediately.
* The size of a terminal has one owner per pane: a single `ResizeObserver` fits
  the grid on `requestAnimationFrame` and the backend is told once the size has
  settled (150 ms), so dragging a divider no longer resizes the remote shell on
  every pointer move.
* A pane that becomes the active pane takes the keyboard (unless a field, a
  dialog or a menu has it).
* The editor, the folders page and the identities page are routed pages with
  the shell's anatomy (breadcrumb, title, one line, primary action); the old
  modal editors for hosts, folders and identities are gone. The folders and
  identities pages remember their selection while another page is open.
* The dev mock backend (`?mock`) refuses hosts and folders in the words of the
  real backend, deletes a folder the way the store does (contents move up),
  implements the two new identity commands, and takes `&folder=Name` and
  `&identity=Name` to start those pages on an entry.

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
