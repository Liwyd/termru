# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

* Host editor sections for data model v2 (ADR 0010): **Jump hosts** (an ordered
  picker of the other hosts, reorder and remove, the route `you → gateway →
  bastion → app` with the first hop's own jump hosts, and the cycle, missing
  hop and more-than-8-hops errors next to the picker), **Port forwards** (local,
  remote and dynamic rows with a bind address, a bind port, a target, an
  enabled switch and a warning for an address other machines can reach;
  validated the way `validate.rs` does) and, under Advanced, **environment
  variables**, the **startup command**, **agent forwarding** with a one-line
  risk note and the **authentication order** (Automatic or a custom order).
  The command preview shows `-J`, `-L`/`-R`/`-D` and `-A`.
* The session toolbar shows a **forwards chip** (`2 forwards`, `1 of 2 failed`)
  with a menu: `L 127.0.0.1:8080 → db:5432 · listening` or `failed: reason`
  per forward. The `forward` session updates and `SessionInfo.forwards` feed it.
* The connect dialog asks for the **password of a jump host** that signs in
  with one and has none stored (a field per hop, labelled with its name, and a
  box per hop to keep it in the credential store). They go to the backend as
  `hop_secrets`.
* **Import from SSH config** on the Hosts page: lists the hosts of
  `~/.ssh/config` with a checkbox each (new, already in the library, cannot be
  imported and why), the jump routes, the key files that will be referenced
  (never copied) and the file's warnings; imports in one step and shows a
  summary and a notice. Empty file, missing file and read-error states.
* **Local approval of synced fields that act** (security): the startup command,
  agent forwarding, environment variables and forwards that listen on a
  non-loopback address arrive through sync, so every device keeps its own
  approval (a SHA-256 of those fields, in a device-local table that is in no
  sync payload; database migration 3). A host whose fields this device has not
  approved connects without them, loopback forwards still running, and the
  session says so with a notice, a toolbar chip and a review dialog that lists
  exactly the fields. Saving the host in this device's editor approves what it
  shows; `host_exec_approve(id, hash)` is refused when the stored fields no
  longer hash to what the user was shown.

* Data model v2 (ADR 0010): a host can name **jump hosts** (`jump_chain`),
  **port forwards** (local `-L`, remote `-R`, dynamic `-D` as a SOCKS5 proxy),
  **environment variables**, a **startup command**, **agent forwarding** (off
  by default) and an explicit **authentication order**. Old records and old
  sync payloads load unchanged, and a host that uses none of it serializes as
  before. Addresses accept IPv6 literals with zone ids (`fe80::1%eth0`,
  bracketed or bare). Database migration 2 adds the columns when the database
  is opened.
* Connections through jump hosts: every hop has its own host-key check,
  known_hosts decision and authentication; one cancel and the reconnect policy
  cover the chain; an error says which hop failed. The agent is never offered
  to a jump host.
* Port forwards start after the session is ready, report `listening` or
  `failed` (with the reason) per forward as a new `forward` session update,
  close their sockets before the session reports its end and start again on a
  reconnect. Listeners are loopback unless a bind address says otherwise.
* `SetEnv` requests (a refusal is ignored), a startup command typed into the
  interactive shell first, and agent forwarding proxied to the local agent.
* The user's `~/.ssh/known_hosts` is a read-only extra source of trusted host
  keys: a key found only there is trusted without a prompt, a contradicting key
  or `@revoked` still blocks.
* Import from `~/.ssh/config` (`ssh_config_preview`, `ssh_config_import`):
  `Host` patterns, `HostName`, `User`, `Port`, `IdentityFile`, `ProxyJump`,
  `Include`; hosts that exist are recognised, an identity is created per key
  file (bound by path, the key is never read), jump chains follow `ProxyJump`,
  all in one transaction. `Match` blocks are skipped with a warning.

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
  window; the address opens in the system browser through the Tauri opener
  plugin and is copied to the clipboard when the opener fails.
* A paste of two or more lines asks first (line count and a preview), with
  "Don't ask again", which turns the question off in Preferences
  (`terminal.confirm_multiline_paste`). Control characters are removed from
  pasted text.
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

* Sync page (a routed page, no longer a wrapper around a modal): the status in
  words and a glyph (up to date, changes waiting, last sync failed, conflicts
  to decide, syncing, "Sync offline" with the last known state and a note that
  SSH sessions keep running), server, account and this device, the open
  conflicts with the fields that differ and Take the server's copy / Keep this
  device's copy, Sync now, sign in with validation on blur (https required
  except for localhost), change server or account, and Sign out. It reads the
  status again on `sync-applied`. The app cannot list the account's other
  devices yet (no command for it); the page says so.
* Preferences page rebuilt to frame 8: font family (a list, or a custom
  family), size, line height, theme, a live preview of the terminal with those
  values, cursor shape and blink, scrollback, copy on select, confirm
  multi-line paste, reduce motion, high-visibility focus and routine notices.
  Every change applies and saves at once; a typed value is checked when the
  field is left and the reason is shown under it. The shortcut table shows the
  macOS and Windows / Linux keys of every command, with Record, Unbind and
  Reset per row and the fixed terminal keys; Restore defaults asks first. An
  About section shows the version from `app_info`.
* New preference `terminal.confirm_multiline_paste` (default on, serde default,
  so old local records and sync payloads load unchanged). An empty keybinding
  override now means "unbound" and passes validation.
* Connect, host-key and sign-in dialogs restyled on the session dialog: Connect
  (password only), "Verify this host" with the fingerprint to compare and how to
  check it, "Host key changed" that blocks the connection and shows the stored
  and the presented fingerprint side by side, the "known under another key
  type" warning, and `Verify & connect` / `Cancel` with Cancel focused. Text
  from the server stays plain text. The unreachable private-key branch of the
  connect dialog is removed (the backend asks for passphrases).
* Notices go through one facade (ADR 0009): keyed copy, cleaned values,
  deduplication, at most two visible and six queued (a failure never waits
  behind a routine notice), `role="alert"` for failures and `role="status"` for
  the rest, top right under the navigation, and the "Routine notices"
  preference. A failed sync offers Retry.
* Links in terminal output open in the system browser: `tauri-plugin-opener` and
  `@tauri-apps/plugin-opener`, with a capability that allows only `http://*`,
  `https://*` and `mailto:*` URLs (no file paths, no other schemes).
* Hosts page body rebuilt to frame 1 (R7): the shared page header (breadcrumb,
  title, count line, `New host` with an icon), the filter row (search with its
  shortcut, a folder select with a folder icon and caret, the sort line), plain
  filter tabs with counts, a table with a band header and 70 px rows (star,
  host and `user@address:port`, folder, authentication, last session, actions,
  all on fixed columns so every row lines up), the current row tinted, and the
  "Your sessions are still running" banner under the rows instead of over the
  filters. Narrow windows (900 px and below) drop the folder and authentication
  columns.
* Accessibility checks: `axe-core` 4.14.1 (MPL-2.0, dev dependency, never
  bundled) runs over every route and dialog in `src/lib/a11y.test.ts`, with the
  rules jsdom cannot evaluate (colour contrast, target size) switched off and
  named in `src/lib/test-support/axe.ts`; a second set of tests proves that
  every form error is wired with `aria-invalid` and `aria-describedby`.
  `docs/accessibility.md` has the keyboard checklist of every page, the focus
  order, screen reader notes and the known gaps.
* Motion: every button gives under the press (scale 0.97, 100 ms, transform
  only; menu entries excepted) and the global hover is behind
  `@media (hover: hover) and (pointer: fine)`. A block that arrives while the
  user is looking (the running-sessions banner, the form of the Folders and
  Identities rails) fades in and rises 4 px in 160 ms (`actions/appear.ts`,
  CSS transitions); it does not move in the first paint of a page, after a key
  press, or under reduced motion (no shift, 80 ms fade).
* Dev mock backend flags `&many[=N]` (N generated hosts, default 10 000) and
  `&long` (names at the 128 character limit, a folder five levels deep with long
  names, long identity and sync values, a 2 000 character comment, twelve tags,
  a one-letter host, a non-Latin name, two hosts with the same name).
  `scripts/scroll-perf.mjs` scrolls the windowed table in headless Chrome and
  reports frame times (numbers in `docs/performance.md`).

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
* Accessibility: the page has a `main` landmark (page headers were counted as
  extra banners); the status bar is a `status` region on a `div`, not a role on
  `footer`; the Sessions tab strip is a list of name and close buttons with
  `aria-current` (a `tab` may not contain buttons, and the strip never had the
  arrow-key behaviour of a tab list); the Notices region no longer shares its
  name with the Preferences section; the Scrollback and New tag errors are tied
  to their fields.
* The hosts table keeps its columns in line (the actions column was `auto`, so
  `Open` and `Connect` rows shifted the others), its star and More buttons are
  drawn the same on every platform, and it scrolls 10 000 rows at about 54 fps
  instead of 44 in a headless software-rendered Chrome (rows in a window moved
  with a transform, rows contained). Keyring lookups for the rows on screen wait
  until a scroll has been still for 150 ms.
* Counts of 1 000 and more have a thousands separator everywhere (the Hosts
  header, tabs, sort line, status bar, Folders tree). The banner names at most
  three sessions ("a, b and 4 more"), its button and the breadcrumbs are one
  line with the full text as a tooltip, a folder five levels deep shows its last
  two folders in the table and its leaf in the editor's breadcrumb, the Folders
  tree indents deep levels by 10 px and scrolls the selected folder into view in
  a narrow window, and the nav no longer wraps "3 sessions connected" at 760 px
  (it is hidden under 900 px; the count is on the Sessions item).
* The nav, the Hosts filter tabs, switches and the session tab names no longer
  fade when their state changes (a key press changes them as often as a click);
  only the hover fades, under the hover gate.

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
  `&identity=Name` to start those pages on an entry, `&sync[=conflicts|error]`
  to start signed in to a sync server and `&connect[=host]` to open the connect
  dialog.
* The shell draws the page on the white surface (as frames 1, 4, 5 and 8 do),
  the title bar on the canvas and the status bar on the secondary surface; the
  Hosts page no longer sits on the grey canvas.
* The row menu of the Hosts table is the `PopMenu` of the Sessions page (scales
  in from the button or the pointer, 160 ms in, 100 ms out, no motion when the
  keyboard opens it, named "Actions for <host>"); the old unstyled
  `ContextMenu` is removed.
* Copy: the empty Hosts state reads "No hosts yet. Add your first host."; the
  Hosts footer mentions Enter; the Favorites tab and the editor's Favorite
  button use the same star icon as the rows instead of a text glyph; the Folders
  rail no longer ends with "No dialog interrupts your workspace".
* **Obsidian redesign, part 1 of 4: foundation, shell and the Hosts page.**
  * **Look.** Dark is the default theme ("System" still follows the operating
    system, Light forces the light set): a near-black base with raised steps
    and a champagne accent, in the tokens of `app.css` (`base`, `side`,
    `raised`, `raised-2`, `text`/`text-2`/`text-3`, `line`/`border`, `accent`,
    `live`/`warn`/`err`). Geist and Geist Mono are bundled (SIL OFL, files in
    `src/assets/fonts/`, CSP `font-src 'self'` unchanged) and replace Inter;
    IBM Plex Mono stays as a font to choose in Preferences. Radii 7-14,
    spacing 4/6/8/10/12/16/20/24/28, UI text 13/20, labels 11 in sentence
    case, motion 100-140 ms ease-out with a 2 % press. The terminal palettes
    (dark and light, all 16 ANSI colours) are built on the same colours. The
    new defaults for new installs are the dark theme, Geist Mono and 13 px
    (`preference.rs`); a stored preference is never changed.
  * **Contrast.** Every text colour holds 4.5:1 on the base, side and raised
    surfaces in both themes (`design-tokens.test.ts`). To get there the
    tertiary text and the accent, live, warn and error colours *as text* are
    a step darker in the light theme (`text-3`, `accent-text`, `live-text`,
    `warn-text`, `err-text`); fills keep the approved colours.
  * **Shell.** A 236 px left sidebar replaces the top navigation: the brand,
    the "Search or run" pill that opens the command palette, the **Live**
    sessions (a dot and the pane count; a click focuses it), the **Library**
    (All hosts, Favorites, Recent) and the **Folders** tree with counts and a
    New folder button, then Identities, Sync (with a status dot) and
    Preferences. It collapses to an icon rail (`Ctrl+Shift+B`, a button, and by
    itself under 900 px); the choice is remembered on the device. The 28 px
    status bar shows the live count, the sync state, a line for the page and
    the keys that work on it. The window title bar remains only where the
    system needs it (Windows controls, macOS traffic lights).
  * **Command palette.** The one place that reaches everything: the running
    sessions, every host (Enter connects or opens, Shift+Enter edits), every
    page, new host, import `~/.ssh/config`, new folder, new identity (generate
    a key), split right and down, duplicate, move to a new window, find in the
    terminal, sync now, the theme, the library views and folders, the sections
    of Preferences and the inspector. Sections while the field is empty, a
    ranked list when it is not, shortcuts and the key hints on screen, no
    animation.
  * **One-line page header** on every page (title and count, chips, actions,
    one champagne primary action): no breadcrumbs, no description sentences.
    The host editor has a back button instead of its breadcrumb.
  * **Hosts.** Borderless 52 px rows (star, name and address, folder, sign-in,
    live or last session, Open/Connect with Enter and a menu with every row
    action), the selected row raised, the arrows and Enter drive the selection,
    the sort chips (Recent activity, Name, Folder, remembered), a search field
    in the header, Import `~/.ssh/config` as the secondary action, the
    running-sessions strip, and the empty, error, loading, 10 000-host and
    long-name states. The sidebar chooses the view (Library entries, folders,
    Unfiled).
  * **Inspector** (340 px, collapsible, remembered) for the selected host: the
    route `you → hops → host`, sign-in and its second step, the authentication
    order, folder, keepalive, the forwards with the live status of a running
    session, tags, what happens on connect (environment, startup command,
    agent forwarding) and whether this device has approved the synced
    settings that act, with Review. Its content fades in for 120 ms when a
    click chooses a host and never for a key. There is no Files action (no
    SFTP exists yet) and no host-key row (no command reads `known_hosts` for
    one host).
  * The other pages (Sessions, Folders, Identities, Sync, Preferences, the
    editor) keep their layouts on the new tokens and shell; their own redesign
    follows in parts 2 and 3. Their section labels are sentence case now.
  * Dev mock: `&palette[=text]` opens the command palette on start.

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
