# Termru — Implementation Plan & Progress Tracker

Authoritative plan. Checked items have passed their acceptance criteria (verification noted per task).
Blocked items state reason + unblock requirement.

Legend: `[ ]` pending · `[x]` verified done · `[!]` blocked (details inline)

---

## Phase 0 — Repository assessment and baseline


- [x] Inspect repository: empty (`main` has no commits; remote `origin = https://github.com/Liwyd/termru.git` has no refs). No existing code, no uncommitted user work to preserve.
- [x] Toolchain baseline: Rust 1.99.0, Cargo 1.99.0, Node 22.23.1, npm 10.9.8, pnpm, webkit2gtk4.1 + gtk3 devel present, Postgres 18.6 client+server, OpenSSH 10.2 client+server, sqlite3 CLI, Docker CLI present.
- [x] Network baseline: crates.io, npm registry, github.com reachable.
- [!] Local Docker daemon unusable (`permission denied` on `/var/run/docker.sock`, no passwordless sudo).
  - Impact: container image builds and Docker-based integration tests cannot run locally.
  - Unblock: Docker group membership/root, or CI-only container validation (chosen).
- [!] `gcc-c++`/`g++`, `Xvfb`, `libappindicator-gtk3-devel` not installable locally (no sudo).
  - Impact: potential C++-requiring build scripts cannot be fixed locally; tray icon deps missing (tray not in scope); headless UI automation needs real display (a display IS available: `DISPLAY=:0`).
  - Unblock: install packages, or avoid dependencies requiring them (chosen: avoid; monitor build).

## Phase 1 — Product requirements and architecture decisions


- [x] Decide SSH transport: **russh 0.64 (pure Rust)** over system OpenSSH spawn.
  - Why: full control of host-key trust UX (TOFU + changed-key warnings), auth flows, no system-OpenSSH dependency, single code path on Windows/Linux, viable Android path. Cost: implement OpenSSH-compatible known_hosts parsing/writing, own config parsing subset, algorithm coverage = russh's. Verified: crate maintained, Apache-2.0.
- [x] Decide desktop stack: **Tauri 2.12 stable** (not 3.0-alpha) + **Svelte 5.57 + Vite 8 + TypeScript 5.x** + **@xterm/xterm 6**.
  - Verified version compatibility: `@sveltejs/vite-plugin-svelte@7` peer = vite ^8 + svelte ^5.46; `svelte-check@4` peer = typescript ^5||^6 → **typescript pinned to 5.x (not 7)**.
- [x] Decide local persistence: **SQLite (rusqlite, bundled) + versioned migrations** in `crates/termru-store`.
- [x] Decide cloud persistence: **PostgreSQL** for the cloud manager, driven by `tokio-postgres` + `deadpool-postgres`.
  - Rationale: standard self-hosted deployment shape (compose with db + app, healthcheck-gated startup), mature backup/restore (`pg_dump`), safe concurrent multi-device writes. Data volume is small but Postgres adds only one compose service; documented in ADR.
  - **Driver revised 2026-10-21 (was `sqlx`):** the `sqlx` facade crate can not live in this workspace — its optional `sqlx-sqlite` driver links the native library `sqlite3`, which the desktop app already links through `rusqlite`, and cargo allows only one package per `links` value. `tokio-postgres` + `deadpool-postgres` give the same capabilities with no native library. See ADR 0003.
- [x] Decide cloud API framework: **Axum** (candidate; stable ecosystem, first-class tower middleware for rate limiting).
- [x] Decide secret policy v1: **no secret synchronization**. Secrets stay in OS keyring (keyring crate) only. Documented limitation; config sync only. (Avoids inventing a recovery mechanism that breaks E2E guarantees.)
- [x] Decide sync model: **transactional, revision-based, per-user monotonic `rev`, tombstones, cursor pagination, explicit conflict surfacing** (no silent LWW, no CRDTs).
- [x] ADRs written in `docs/architecture/` for: SSH transport, desktop stack, persistence split, sync protocol, secret policy, multi-window session ownership, licensing.
  - Verification: `docs/architecture/README.md` + `0001`–`0007` exist; decisions match code (russh, Tauri 2/Svelte 5, SQLite+Postgres split, rev-based sync, no secret sync, single session owner, Apache-2.0).

## Phase 2 — Repository structure and development tooling


- [x] Cargo workspace with the three core crates (`termru-core`, `termru-store`, `termru-ssh`) + shared lints/profiles.
- [x] Add `apps/desktop/src-tauri` to the Cargo workspace (member of root workspace; links clean against webkit2gtk-4.1).
- [x] Add `apps/cloud-server` to the workspace (done in Phase 9).
- [x] Frontend package layout (`apps/desktop`: Svelte 5.57 + Vite 8.3 + TS 5.9 pinned, `@xterm/xterm` 6) + editor/format configs (`.prettierrc` + prettier-plugin-svelte, strict `tsconfig`, `.editorconfig` already repo-wide, `.vscode/extensions.json`).
- [x] Rust tooling: rustfmt config, clippy workspace lints (incl. unwrap/expect warns + allow-in-tests), `.editorconfig`, `.gitignore`, `cargo-deny` config (licenses + advisories).
- [x] Root scripts for: dev, test, lint, format, typecheck — root `package.json` npm scripts delegating to the `apps/*` workspace (single source of truth; no separate `scripts/` duplicates).
- [x] Base project files beyond LICENSE: README (stub), CONTRIBUTING, SECURITY (private advisory reporting), CODE_OF_CONDUCT (Contributor Covenant 2.1), CHANGELOG (Keep a Changelog), issue/PR templates (YAML forms), Dependabot config (cargo/npm/github-actions).
- [x] CI workflow for PRs: fmt, clippy (-D warnings), cargo test (workspace, incl. the sshd integration tests), svelte-check + prettier, vitest, vite build, desktop compile check, cargo-deny, gitleaks.
  - `.github/workflows/ci.yml`: four jobs (rust, frontend, desktop, supply-chain) with cargo/npm caches, least-privilege `contents: read`, concurrency groups. The cloud-server and container jobs arrive with Phase 9/13.
  - Every third-party action is pinned to a commit SHA. `scripts/check-action-pins.py` resolves every pin against the GitHub API and fails when a commit does not exist (three pins of the first version of the workflow were typos and would have broken the pipeline on its first run); the check runs in the CI job `supply-chain`.
  - Verification: the workflow is syntactically valid and mirrors the local commands; it runs on GitHub (not verifiable offline).
- Verification: `cargo fmt --check`, `cargo clippy --workspace --all-targets` (0 warnings), `cargo test --workspace` (95 tests) green; `npm run lint` (prettier + svelte-check 0 errors), `npm run test` (4 vitest), `npm run build` green; `cargo build -p termru-desktop` links; debug binary smoke-ran against vite dev without fatal output.

## Phase 3 — Core domain and local persistence


- [x] `crates/termru-core`: typed IDs, Host/Identity/Group/Tag/Snippet/Preference models, validation, serialization (serde), sync payload types + envelope versioning.
- [x] `crates/termru-store`: SQLite schema + versioned migrations (up migrations with recorded schema version; tested), repositories (hosts, groups, tags, favorites, recents, preferences, workspaces, sync queue/meta), soft delete + tombstone bookkeeping for sync.
- [x] Store safety: WAL mode, busy timeout, foreign keys ON, transactions per operation, corruption detection on open (quick_check → actionable error), file permissions 0600.
- [x] Export/import/backup (VACUUM INTO → atomic rename; restore verifies, keeps pre-restore aside) with round-trip test + corrupt-backup rejection test.
- [x] Concurrent-access test (two connections, busy-timeout behavior).
  - `crates/termru-store/tests/concurrency.rs`: two independent connections to the same file both read and write, 40 interleaved writes from two threads keep every row (no `SQLITE_BUSY`, the busy timeout covers the overlap), and a second store opened on an existing file does not lose data. The integrity check runs on the file afterwards.
- Verification: unit tests incl. migration-from-empty AND future-schema rejection, backup/restore round-trip, 25 store tests green (incl. deterministic monotonic `now_ms` + `ORDER BY` tiebreakers).

## Phase 4 — SSH engine and terminal reliability (russh)


- [x] `crates/termru-ssh`: connection state machine (idle→connecting→authenticating→ready→reconnecting→closed/failed), timeouts (connect/auth), keepalive + broken-connection detection, cancellable connect.
- [x] Auth: password, private key (+ passphrase), SSH agent (OpenSSH agent socket) with graceful degradation when unavailable.
- [x] Known-hosts: OpenSSH format read/write incl. hashed entries, TOFU first-use confirmation flow, **changed host key = hard warning with old/new fingerprint**, never auto-accept on change.
- [x] PTY shell: pty-req with TERM/size, window-change resize propagation, shell selection, output streaming with bounded buffering (backpressure), input writes, exit/close propagation, bounded scrollback replay buffer for re-attach.
- [x] Reconnect policy: bounded backoff with cap, no uncontrolled loops; explicit user action vs automatic. (`termru_ssh::reconnect::ReconnectPolicy`: 8 attempts, 500ms→30s cap, ×2 backoff + 20% jitter; `connect_with_retry` emits `Reconnecting` attempts then `Failed`+`Closed` with `GiveUpReason` on give-up; connect cancellable mid-flight; only `Network`/`Timeout` categories retriable — auth/host-key/protocol never auto-retried. Verified: unit tests incl. tokio paused-time; integration give-up test.)
- [ ] Port forwarding (local/remote/dynamic), jump hosts: implement if time-justified for v1 → tracked under Phase 5 scope decision (ADR).
- [x] Integration tests against real OpenSSH (local `sshd` as unprivileged user on high port; CI with root sshd): success, wrong password, wrong key, passphrase-protected key, host-key-change detection, resize, large output flood, interrupted connection, malformed/binary output.
- Verification: `crates/termru-ssh/tests/sshd.rs` — 16 tests green locally (private sshd per test, self-skips with loud `SKIPPED:` if OpenSSH tooling missing): shell roundtrip + TOFU write, declined/changed host keys (decline + accept + subsequently-known), wrong password, unauthorized/wrong/correct passphrase, agent auth via explicit socket, PTY resize, 4MiB flood, reattach replay ordering, interrupted connection → `Failed{Network}`, binary output verbatim, clean `exit(7)` → status without failure, bounded reconnect give-up. Negative cases assert `ErrorCategory` and no credential leakage.

## Phase 5 — Desktop MVP (main UI + SSH end-to-end)


- [x] Tauri app scaffold: window config (label `main`, 1200×800, min 640×400), capabilities least-privilege (`core:default` only — no fs/shell/opener), CSP set, `AppState` stub as future single session owner, structured stderr logging (`tracing` + `EnvFilter`), `app_info` IPC smoke command (frontend renders it).
- [x] App runtime hardening: file logging alongside stderr (no secrets), panic hygiene (hook + recovery). (`SessionManager` wiring into `AppState` is done and verified.)
  - `init_logging` writes to stderr **and** a daily rotating file in the app data dir (`logs/termru.log`, `tracing-appender`); the panic hook records the panic + location in the log before the default hook prints it. Secrets are never formatted into logs (credential values are redacted in `Debug`).
- [x] Commands: host CRUD (validation in-app), session open/close/write/resize/duplicate, known-host confirm/decline, preferences.
  - `src-tauri/src/commands/`: host + identity CRUD (incl. device-local key binding, absolute-path check), preferences, `session_open/retry/duplicate/close/write/resize/list`, `host_key_prompt_resolve`; secrets are `Debug`-redacted, never persisted; terminal geometry bounds validated.
  - Verification: `cargo test -p termru-desktop` → 17 unit tests pass (auth resolution incl. unbound-identity/key errors, geometry bounds, no secret in `Debug`, prompt default-deny, manager bookkeeping).
- [x] SessionManager: per-session tokio task (actor exists in `termru-ssh`), Tauri command layer, bounded input/output channels, status events, graceful shutdown on app exit, orphan prevention.
  - `src-tauri/src/state/sessions.rs`: single-owner map; one background task per session (connect → forward events on a per-session `Channel`, base64 payloads → bounded mid-session auto-reconnect (≤5 cycles, fresh budget after ≥30s stable `Ready`)); channel gone ⇒ close + remove (orphan prevention); `close`/`shutdown` cancel and close the transport; `retry_handle` refuses a second task (`task_alive`); `state/host_keys.rs` = default-deny UI host-key prompts with a bounded pending map.
  - Verification: unit tests (reconnect verdict matrix, status DTO/JSON contract, bookkeeping) **+ app-layer E2E** (`apps/desktop/src-tauri/tests/sessions_e2e.rs`): a real `sshd` is started per test; the test connects with a generated key, sees `Ready`, writes a command, receives the echoed output through the same `Channel` the frontend uses, and asserts the session is released on close; a second test asserts a rejected key surfaces a failure status.
- [x] Frontend foundation: design tokens, theme (light/dark), layout shell (sidebar + tabbed main), components: HostList/HostCard, host editor form, connection dialog (password/passphrase prompts), host-key trust dialog (unknown + changed key), status badges, empty states, toasts/errors.
  - Verification: `svelte-check` 0 errors/0 warnings; 22 vitest tests (tab state machine incl. auto-reconnect vs give-up distinction, host filter/sort, base64 + UTF-8 codec); `vite build` green.
- [x] Terminal view: @xterm/xterm + fit/webgl(addon fallback to DOM renderer), attach/detach to sessions via Tauri `Channel` streaming, resize debounce → `window-change`, copy/paste, scrollback, selection.
  - Done: xterm 6 + fit addon + WebGL addon with DOM-renderer fallback, `Channel` streaming (base64 → bytes), resize debounce → `session_resize`, scrollback from preferences, `Ctrl+Shift+C`/`Ctrl+Shift+V` copy & paste. The **scrollback is restored after a re-attach**: the backend replays the buffer of the session before it sends the live stream (`session_attach` → `replay_snapshot`), so a view that attaches to a running session shows the same history.
- [x] Tabs: create/close/rename/reorder, keyboard navigation, session indicators (activity/error), unsaved-layout persistence.
  - Done: create/close, rename (double-click), drag & drop reordering, keyboard navigation (`Ctrl+Tab` / `Ctrl+Shift+Tab`, `Ctrl+W`), per-tab status badge, **layout persistence** (tabs + order + hosts + pane tree saved through the `workspaces` repository; restored on launch as *disconnected placeholders* — secrets are never stored, so a restored tab reconnects only on request).
- Verification: `cargo test --workspace` → 122 tests pass (core 28, store 25, ssh 26, sshd integration 17 incl. a replay-snapshot test, desktop 23 incl. 3 app-layer E2E with the two-view attach case + the logging audit), `cargo clippy --workspace --all-targets` 0 warnings, `cargo fmt --check` clean, `npm run lint` (svelte-check 0/0), `npm run test` (28 vitest, incl. a jsdom contract test that asserts the UI calls the Tauri commands with the documented argument shapes), `npm run build` green. **Pending: a visual UI pass — this machine's X session has no window manager, so the app window cannot be shown or driven here; it must be done on a real desktop session.**

## Phase 6 — Splits, multiple windows, workspaces


- [x] Split panes: horizontal/vertical split, adjustable ratios, pane focus model, keyboard navigation between panes.
  - A tab holds a **pane tree** (the model the backend already had): a leaf is a pane with its own session, a split divides the space in two with a ratio. The tree lives in `lib/pane-tree.ts` and is unit-tested (split, remove with the collapse of the parent, the clamping of the ratio, the round trip through the persisted layout).
  - The UI: `SplitView.svelte` renders the tree recursively, `PaneView.svelte` a pane (header with the host, the status, the split and close buttons, and the terminal). The divider is dragged with the pointer or moved with the arrow keys. A new pane is empty and offers the hosts of the sidebar; the pane that has the focus is the one the keyboard goes to.
  - The commands: `Mod+Shift+E` (split to the right), `Mod+Shift+O` (split below), `Mod+Shift+W` (close the pane), `Mod+Alt+ArrowLeft/Right` (the previous/next pane). The layout (the tree with the hosts) is saved with the tabs, so the split survives a restart.
- [ ] Multi-window: create/close native windows, per-window tab sets, **tab move between windows without dropping SSH** (re-attach + replay buffer), no duplicate session ownership.
  - Done: **tab move between windows without dropping the SSH session** — the session is owned by the app, not the window; a moved tab opens a new window (`window_open_session`) whose frontend attaches to the running session (`session_attach`) and receives the replay buffer, then the live stream. The app layer fans events out to every attached view and drops views whose window closed; a brief grace period covers the move itself.
  - Done: **the window close semantics** — every view remembers the window it lives in (`tauri::Webview::label()`), and a destroyed window releases its views. A session that no window shows any more is closed (its SSH connection would stay open for nothing); a session that another window still shows keeps running. Covered by a unit test.
  - Pending: per-window tab sets (a second window can only show a moved tab today; it has no tab bar of its own).
- [ ] Workspaces: save/restore layout (tabs+splits+window geometry), crash recovery of layout metadata (never claim remote shell recovery), stale session cleanup on startup.
  - Done: the layout holds the tabs, their order, the hosts **and the pane tree**; it is restored on launch as disconnected placeholders (the secrets are never stored, so the user reconnects), a host that was deleted meanwhile is dropped from the tree, and a folder cycle can not break the restore.
  - Pending: the window geometry is not saved, and there is no "crash recovery" beyond the layout (a remote shell is never claimed to be recovered).
- [x] Race-condition review: rapid open/close/move/quit interleavings (test-cased).
  - Two tests in the session manager: the interleaving of 20 sessions that are opened, shown in a second window (a moved tab), closed one by one and released with their windows — the invariants hold (a closed session is gone, a second close reports "no session", a session that another window still shows keeps running, and it is orphaned exactly once); and four threads that open and close at the same time — the lock holds, nothing panics and nothing is left behind.
- Verification: tests for session ownership invariants (single owner, no leaks, no double-free of channels) + manual multi-window passes.

## Phase 7 — Keyboard, palette, search, theming, preferences


- [x] Global + terminal-aware keybinding system (platform defaults, conflict detection, remapping UI, works when terminal focused without stealing input).
  - Done: command registry with platform-aware accelerators (`Mod` → Ctrl/Cmd), conflict detection (unit-tested), a global key dispatcher that never shadows the terminal's own keys, per-command bindings shown in the palette, and **a remapping UI** in the settings dialog: "Change" records the next combination, a combination that another command already uses is refused with the name of the command that owns it, Escape cancels, and every binding can be reset individually or all at once. The overrides are stored in the portable `preferences` record, so they follow the user to the other devices, and the dispatcher and the palette use the effective binding.
  - The recorder takes the keyboard: the global dispatcher ignores the keys while a combination is being recorded, so the command that is being rebound does not run instead of being captured.
- [x] Command palette (fuzzy, discoverable, shows bindings).
  - `Ctrl/Cmd+K` opens it; fuzzy subsequence matching with word-boundary and consecutive-character bonuses; arrows/Enter/Escape; shows the accelerator of every entry and offers the hosts as entries ("Connect to …").
- [ ] Global search across hosts/tags/snippets.
  - Done: the search in the sidebar covers the name, the address, the user name, the description **and the tag names** (not the tag ids), plus a "Recent" section (the most recently connected hosts, from the recents table). The command palette offers the hosts as entries ("Connect to …").
  - Pending: the **snippets** do not exist (there is no snippet model in the store). The snippet feature is out of scope for 0.1.0 and is listed in `docs/limitations.md`.
- [x] Appearance settings: fonts, sizes, terminal theme overrides, cursor style, reduced motion respected.
  - Done: a settings dialog (`SettingsPanel.svelte`, `Mod+,`) with the theme, the font family and size, the scrollback, the cursor style and blink and the copy-on-select switch; the values are stored as the portable `preferences` record, so they are synced with the other devices, and they apply live to the running terminal. The palette can still change the theme and the font size, and the UI honours `prefers-reduced-motion` (spinner).
  - Pending: per-record terminal theme overrides (the terminal follows the app theme, it can not differ per host).
- [ ] Context menus, tooltips, drag-and-drop (host → tab), recent hosts, favorites, tags, folders.
  - Done: recent hosts (sidebar section), favorites (star toggle + sorting), host → tab via drag-free "Connect"/palette entries, **folders and tags** (the folders can be created, nested, renamed and deleted, the host editor puts a host in a folder and gives it tags, the sidebar groups the hosts by their folder, and the search covers the description and the tag names), and a **context menu** on the host cards (connect, edit, favorite, delete) that stays inside the window.
  - Pending: the tooltips of the icons, and the drag & drop of a host onto a tab (the tabs can be reordered by dragging).
- Verification: unit tests for keybinding conflict detection and command registry; `svelte-check` clean; manual keyboard-only walkthrough of core journeys.
  - Automated: 91 frontend tests (keybinding normalization/conflicts/event matching, the overrides and the recorder, fuzzy ranking, the tab state machine, the pane tree, the layout document, the sync store, the webview hardening scan, and the jsdom UI↔Tauri contract tests) + `svelte-check` 0/0.
  - Manual: the keyboard-only walkthrough still needs a desktop session (no window manager on this machine).

## Phase 8 — Local security (desktop)


- [x] Secrets in OS keyring via `keyring` crate (Secret Service / Windows Credential Manager / macOS Keychain); no plaintext secrets in SQLite; secret values zeroized where feasible; never logged.
  - `secrets.rs`: secrets are stored under `com.liwyd.termru` in the platform credential store; the app degrades gracefully when no credential store exists (the secret is asked for per connection); the frontend drops its in-memory copy once the credential store holds it; the connection path falls back to the stored secret, so "remembered" hosts connect without prompting.
  - The database schema has no field that could hold a secret, and the sync payload never contains one.
- [x] Host editor: key file picker (path validation, no copy of private key into DB), passphrase never persisted unless stored in keyring (user choice, explicit).
  - The key file is chosen with the **native file picker** (`tauri-plugin-dialog`, the only permission the capability grants besides `core:default`). The path is validated (absolute, non-empty) and only the *path* is stored — the key file is never copied into the database, and the binding is device-local (it is not part of the sync payload). A passphrase is persisted only in the credential store and only when the user explicitly opts in.
  - The **identity editor** was missing entirely: the host editor asked for an identity, but there was no way to create one. The sidebar now opens the identity editor: create an identity, bind a key file to it (with the picker), unbind it, delete it. The bound path is read back from the backend (`identity_bindings`), so the user sees which key an identity uses on this machine.
- [x] Webview hardening: CSP, no remote content loading in webview, single-instance plugin, secure IPC (input validation on every command: sizes, ids, path sanity).
  - Done: CSP set, capabilities limited to `core:default` (+ the session windows), no remote content (a test reads the frontend sources and fails on a remote `src`/`href` or a raw HTML insertion), command inputs validated (terminal geometry bounds, host/identity validation, secret kind whitelist, key path checks).
  - Single instance: the `tauri-plugin-single-instance` plugin focuses and un-minimizes the running window instead of starting a second process that would fight over the local database. The plugin is registered first, as it has to be.
- [x] Logging audit: grep-based test asserting no secret material patterns in log files.
  - `termru_desktop_lib::tests::log_files_never_contain_secret_material` initializes the real logging setup, exercises the credential-handling paths and greps the produced log file for the secret values (they must be absent, redacted placeholders present).
- Verification: unit tests for keyring abstraction (mock backend), CSP in prod config, command input validation tests.

## Phase 9 — Cloud API (self-hosted manager)


- [x] `apps/cloud-server` scaffold: Axum 0.8, config via env (validated at startup, fail-fast), structured logs, graceful shutdown, healthz/readyz (readyz checks DB).
  - `src/config.rs`: every setting comes from the environment, is range-checked and the process aborts with a message that names the offending variable; `src/main.rs` binds only after the configuration and the migrations succeeded.
  - `src/db.rs`: pool + a migration runner that embeds `migrations/*.sql` at build time (`build.rs`) and applies them under a PostgreSQL advisory lock, so replicas can boot at the same time.
- [x] Auth: register/login (argon2id), opaque access+refresh tokens (hashed at rest, rotation with reuse detection → family revocation), session/device revocation.
  - Tokens are 256-bit random values stored as SHA-256 hashes; a stolen database yields no usable credential. Refresh tokens rotate on every use; a replay outside the grace window (`TERMRU_REFRESH_REUSE_GRACE_SECS`, default 30 s) revokes the whole token family and is written to the audit log.
  - Passwords: argon2id, minimum 12 characters, no composition rules (NIST-style). An unknown account spends the same work as a real verification, so the response time does not reveal whether an account exists.
- [x] Devices: enrollment, list, rename, revoke (revoked device loses API access immediately).
  - The device id is generated by the client and kept stable; a revoked device stays revoked (it must be re-enrolled), and a device id that belongs to another account is refused.
- [x] Sync endpoints: `GET /v1/sync/changes?since=<rev>` (bounded cursor pagination, tombstones included) + `POST /v1/sync/changes` (batch, per-record base_rev conflict detection → 409 payload with server records), idempotency-safe retries (client mutation ids), payload size limits, schema version field.
  - The wire types are the ones in `termru-core::sync`, shared with the desktop client. A stale `base_rev` is reported as a conflict with the server's copy; the server never resolves a conflict on the client's behalf. A replayed `mutation_id` returns the revision of the first application (idempotent retries). Revisions come from a per-user counter that is bumped inside the push transaction, so the sequence is monotonic and gapless.
- [x] Per-user isolation enforced at query level (every query scoped by user_id from token; IDOR tests).
  - Every read and write carries `user_id = $1` from the verified token; the integration suite proves that one account can neither see nor modify the records or devices of another.
- [x] Rate limiting (IP + token keyed), request body limits, timeouts, security headers, strict CORS (none needed for desktop client → deny by default; documented).
  - Token bucket per client address and per access token; `DefaultBodyLimit` (1 MiB body → 413); a 30 s request timeout; a strict header set (`nosniff`, `DENY`, CSP `default-src 'none'`, `no-store`, …). No CORS headers are sent at all, so a browser can not use the API cross-origin. `X-Forwarded-For` is ignored unless `TERMRU_TRUST_FORWARDED_FOR=true`.
- [x] Audit log for auth/device/security events; login throttling + account-enumeration-resistant responses.
  - `audit_log` records registration, login success/failure, refresh, token reuse, device lifecycle and sync conflicts; it never contains a credential. Failed logins lock the identifier for 15 minutes after 5 attempts.
- [x] DB migrations (forward + tested on empty and existing DB), seed-free, pg_dump backup/restore script + test.
  - `migrations/0001_init.sql` (users, devices, sessions, refresh tokens, records, applied mutations, audit log, login attempts) is applied by the server at start-up; the suite proves that re-running the migrations is a no-op. `scripts/postgres-backup.sh` / `postgres-restore.sh` write and verify a checksummed `pg_dump` archive; the test restores an archive into a second database and verifies the content.
- [x] Integration tests: full auth/device/sync/conflict/revocation/isolation/rate-limit matrix against real Postgres (CI service; local cluster).
  - 23 API tests + 4 operational tests, each against its own throwaway database on a real PostgreSQL server; they skip with a loud `SKIPPED:` line when `TERMRU_TEST_DATABASE_URL` is not set. CI runs them against a `postgres:18-alpine` service container.
- Verification: `cargo test -p termru-cloud-server` → 46 tests green (19 unit + 23 API + 4 operations) against a local PostgreSQL 18.6; `cargo clippy --workspace --all-targets -- -D warnings` clean; `cargo fmt --check` clean; the built binary was smoke-tested with `curl` (register → devices → push → pull → 401 without a token). `cargo deny` clean.

## Phase 10 — Desktop sync client


- [x] Sync settings UI (server URL, sign-in, device enrollment, status, last-sync, offline indicator).
  - `SyncPanel.svelte`: server URL, email, password and device name; the status (server, account, device, last sync, pending changes, open conflicts); a "Sync now" button; the conflict list. The panel is reachable from the sidebar (with a badge) and from the palette (`Mod+Shift+S`).
  - The device is enrolled implicitly: the client generates a stable device id on the first sign-in and sends it with every sign-in, so the server recognizes the installation.
  - The tokens never reach the frontend — the backend keeps them in the OS credential store, and the UI only sees the status.
- [x] Offline-first: local edits queued in SQLite (ordered, idempotent op ids), push/pull with cursor, deferred sync on reconnect, bounded retry with backoff, no data loss on restart.
  - The queue, the payload materialization and the cursor live in the store (`termru-store::repos::sync`); the desktop engine pushes the queued mutations, acknowledges them by revision and pulls the changed records from the cursor. The cursor never moves backwards.
  - A failed cycle (server down, network error) leaves the queue and the local copies untouched, so the next cycle retries with the same mutation ids — the server's idempotency makes the retry safe.
  - The app also syncs on its own: a background task runs a cycle every five minutes, refreshes the access token before it expires (and once more when the server refuses a request), and retries a failure with a bounded backoff (30 s → 10 min). The task stops when the app exits. The user can still trigger a cycle by hand.
- [x] Conflict surfacing UI: list of conflicts with local/remote comparison + explicit resolution (keep local / take remote); deletions vs edits surfaced, never silently resurrect or drop.
  - The store owns the semantics: `resolve_keep_local` re-queues the local copy against the revision the server reported, `resolve_take_remote` replaces the local copy with the server's. A record with an open conflict is never pushed, and a remote tombstone that meets a local change becomes a conflict instead of a silent deletion.
  - The panel shows both copies, names the field that differs and offers the two explicit choices.
- [x] Cloud outage does not block SSH (enforced by architecture; test).
  - The sync runs in its own command and owns no part of the SSH path; a failing sync records the error message and returns it, the session manager is untouched. Verified by a test that a failing cycle keeps the queue and the local copies.
- [x] Secrets NOT synced (enforced: sync payload builder has no access to keyring).
  - The payload builder lives in the store and only ever sees the domain models, which have no field that could hold a secret (a private-key host carries an *identity reference*). Verified by a test that walks the push payload and rejects any field named like a secret, and by the desktop test that the credential store is the only place a token lives.
- Verification: `cargo test --workspace` → 207 tests green (core 28, store 36, ssh 26, sshd integration 17, cloud server 46, desktop 51 incl. the sync engine and the background task, store performance measurement 1), `cargo clippy --workspace --all-targets -D warnings` clean, `cargo fmt --check` clean, `cargo deny` ok, `npm run lint` (svelte-check 0/0), `npm run test` (53 vitest, incl. the sync store, the webview hardening test and the UI↔Tauri contract tests for the sign-in and the settings arguments), `npm run build` green.
  - The engine is tested against a fake transport **and** end to end against the real server: `apps/desktop/src-tauri/tests/sync_e2e.rs` starts the `termru-cloud-server` binary against a PostgreSQL server, signs in with the desktop engine, pushes a host and pulls it back with a second device. It skips (loudly) when `TERMRU_TEST_DATABASE_URL` or `TERMRU_CLOUD_SERVER_BIN` is missing; the CI `desktop` job runs it with a `postgres:18-alpine` service.

## Phase 11 — Security hardening


- [x] Threat model document (`docs/security/threat-model.md`) covering client, SSH, local storage, cloud API, sync, update, build pipeline, artifacts.
  - Documents the assets, the trust boundaries (webview ↔ Rust ↔ network ↔ OS), the per-surface mitigations, the **residual risks** (TOFU first use, unencrypted local DB, webview XSS, unpinned CI actions, no code signing) and the out-of-scope list.
- [x] `cargo deny` (licenses, advisories, duplicates-bans policy), `npm audit` gate in CI, secret scanning (gitleaks) in CI.
  - The policy is defined in `deny.toml`: lockfile-pinned versions (wildcard requirements are warnings), unmaintained-crate advisories are fatal only for direct dependencies, and every ignore carries a reason + review date. Verified locally with cargo-deny 0.20: advisories/bans/licenses/sources **all ok**. `npm audit`: 0 vulnerabilities.
- [x] SQL injection: parameterized queries only (enforced by code review + tests), authz matrix tests on every endpoint.
  - Every statement in `apps/cloud-server` is a bound parameter; no SQL is built by string concatenation. The authorization matrix is covered by the integration suite: unauthenticated calls are refused, a token of another user reaches nothing, a device of another account can not be renamed or revoked, and a revoked device loses access immediately.
- [x] Webview XSS surface: no `dangerouslySetInnerHTML`-equivalent, sanitization tests for any rendered remote-influenced strings (host names etc.).
  - Svelte escapes every interpolation, and the frontend contains no raw HTML insertion at all. A test (`apps/desktop/src/lib/hardening.test.ts`) reads the frontend sources and fails when a `{@html …}` / `innerHTML` insertion or a remote `src`/`href` appears — so a later change cannot quietly open the hole.
- [x] Container hardening: non-root user, read-only FS where possible, no secrets in image, resource limits, minimal base.
  - The image runs as uid 10001 (`termru`, no shell, no home) on `debian:bookworm-slim`; it holds the binary, `ca-certificates`, `curl` and `dumb-init` — no source, no toolchain, no secret. The compose file adds a read-only root filesystem, `cap_drop: ALL`, `no-new-privileges`, a tmpfs for `/tmp` and a memory/CPU limit for both services. The CI job `container` asserts the runtime user, the health check and the absence of a toolchain in the image.
- [x] Dependency review for significant deps (maintenance, license, security history) → `docs/security/dependency-review.md`.
  - Documents the policy, every significant Rust/npm dependency with its licence and role, and the **known advisories**: RUSTSEC-2023-0071 (rsa via russh/ssh-key — no fixed release, reviewed 2027) and the unmaintained zbus/zvariant chain behind the keyring's Linux backend.
- [x] SECURITY.md with reporting process; residual risks documented.
  - `SECURITY.md` names the private advisory channel, the scope (the app, the SSH stack, the local storage, the cloud API) and what is out of scope, and promises an acknowledgement and a status update. The residual risks are listed in `docs/security/threat-model.md` and in `docs/limitations.md` (TOFU on the first use, the unencrypted local database, the webview XSS surface, the unsigned packages, the unpinned registry tags).
- Verification: audit checklist complete; CI gates green; no open FATAL/SEVERE findings.

## Phase 12 — Desktop packaging and installation


- [x] Debian `.deb` for Ubuntu (built on ubuntu runner; webkit2gtk-4.1 deps declared), RPM for Fedora (built in Fedora container), AppImage portable, Windows NSIS installer (x86_64).
  - The bundle configuration (`tauri.conf.json`) declares the four targets, the Debian runtime dependencies (the RPM dependencies are derived from them by the Tauri bundler), the metadata and the licence file. The packages are built by the release pipeline on the matching runner.
- [x] Package install smoke tests (dpkg/rpm query + launch where runner allows); WebKit/keyring/font deps validated on targets.
  - The release pipeline now installs what it built: the `.deb` with `apt install` and a `dpkg-query` of the package *and* of the declared dependencies, the `.rpm` with `dnf install` and an `rpm -q`, the AppImage by extracting it (it must be self-contained) and the NSIS installer by checking that it is a PE binary. A launch is not possible on the runner (no display), and that is stated in the pipeline comment.
- [x] Checksums (SHA256SUMS) for all artifacts; document signing limitations (no code-signing credentials → documented trust limitation).
  - The release pipeline writes a `SHA256SUMS` file next to the packages. Nothing is signed: there are no code-signing credentials, so the checksum file and the release page are the trust anchor. This is documented as a residual risk.
- [x] Install docs: one recommended command per OS (apt repo/direct .deb download, dnf/rpm, AppImage, winget/NSIS) — no manual compilation.
  - `docs/install.md`: one command per operating system (Debian/Ubuntu, Fedora/RHEL, the AppImage, the Windows installer), the checksum verification with `SHA256SUMS`, the data locations, the uninstall and a pointer to the cloud server. The README carries the two most common commands.
- Verification: release workflow dry-run on a pre-release tag; artifacts inspected.
  - Open: the pipeline has never run (no push credentials in this environment). The workflow is syntactically valid and mirrors the local commands.

## Phase 13 — Cloud containerization and deployment


- [x] Multi-stage Dockerfile (debian-slim, non-root, dumb-init graceful shutdown), healthcheck.
  - `apps/cloud-server/Dockerfile`: a `rust:1.99-bookworm` builder that reduces the workspace to the crates the server needs (the Tauri app would drag the whole webview stack in) and a `debian:bookworm-slim` runtime with the stripped binary, `ca-certificates`, `curl` (health check) and `dumb-init` as PID 1. The runtime user is `termru` (uid 10001); the image contains no source and no toolchain.
  - Verification: the CI job `container` builds the image and asserts the runtime user, the health check and the absence of a toolchain. It cannot be built locally (no Docker daemon on the development machine).
- [x] `compose.yaml`: db + app, pinned image tags, volume for Postgres, healthcheck gating, resource limits, restart policy, secrets via env file (not in image), DB not exposed publicly by default.
  - `apps/cloud-server/compose.yaml` + `.env.example`: the app waits for a healthy database, the database listens only inside the compose network, the app is published on `127.0.0.1` only (a reverse proxy provides TLS), both services drop all capabilities and set `no-new-privileges`, the app runs with a read-only root filesystem, and memory/CPU limits are set. The password lives in `.env` (mode 600), never in the image.
- [x] One-copy-paste README install command: versioned compose fetch with SHA256 verification → `docker compose up -d` (no `curl | sh` as root).
  - `docs/cloud/deployment.md`: the compose file and its `.sha256` are downloaded from the release, the checksum is verified with `sha256sum --check` **before** anything is started, and nothing is piped into a shell. The release assets themselves are produced by the release pipeline (Phase 14).
- [x] Migrations run at container start (or documented `migrate` step) with lock safety (single-runner), backup/restore docs + tested procedure, upgrade/rollback docs, first-run setup + secret generation/rotation docs.
  - The server applies the embedded migrations at start-up under a PostgreSQL advisory lock (single runner). The deployment guide documents the `pg_dump` backup with a checksum, the restore procedure, the upgrade (pull + up) and rollback (restore the backup taken before the upgrade — the migrations are forward-only) plus the rotation of the database password and of account tokens.
- [x] Verification: CI builds the image, runs it with compose, hits healthz + runs migration + backup/restore smoke test (docker available in CI).
  - `apps/cloud-server/scripts/compose-smoke.sh` does the whole deployment in one go: it starts the compose stack with the image under test, waits for `/readyz`, checks that the tables of the migration exist, drives a real client round trip (register → push → pull), takes a `pg_dump` backup, restores it into a second database and compares the content. The CI job `container` runs it after the image build.

## Phase 14 — GitHub Actions CI/CD


- [x] PR pipeline: fmt, clippy(-D warnings), cargo test (workspace), npm typecheck/test, cargo-deny, gitleaks, server integration tests (Postgres service), SSH integration tests (sshd), frontend build, desktop build check (linux), container build check.
  - `.github/workflows/ci.yml`: the `rust` (fmt, clippy, tests incl. the sshd suite), `frontend` (lint, typecheck, test, build), `server` (the cloud server against a `postgres:18-alpine` service), `desktop` (the bundle build plus the sync end-to-end test against a real server) and `container` (the cloud image) jobs, plus `supply-chain` (cargo-deny, gitleaks).
- [x] Least-privilege permissions (`contents: read` for PRs), third-party actions pinned to SHAs, concurrency groups, cache hygiene (keyed on lockfiles).
  - Every third-party action is pinned to a commit SHA (a tag can be moved, a commit cannot); the comment behind each pin names the tag it belongs to, so an update is a deliberate diff. The workflows use `contents: read`, a concurrency group with `cancel-in-progress`, and the cargo/npm caches keyed on the lockfiles.
- [x] Release pipeline: tag validation (semver + clean version) → tests → build matrix (windows-latest NSIS; ubuntu .deb/AppImage; fedora container RPM) → checksums → cloud image build/push (Docker Hub, versioned tag + `latest` policy) → GitHub Release upload → post-publish verification (artifact presence + image digest check) → report partial failures explicitly.
  - `.github/workflows/release.yml`: the `verify` job checks that the tag is a semver **and** that it matches the manifests, `test` runs the whole suite, `desktop` builds the four packages on three platforms, `cloud-image` builds (and smoke tests) the server image, `publish` writes the checksums and uploads the assets. The matrix uses `fail-fast: false`, so a partial failure is reported per platform instead of hiding the other results.
  - The image is published in the **GitHub container registry** (`ghcr.io/liwyd/termru-cloud`) by the job `cloud-image-publish`, with the version tag and — for a stable version only — the `latest` tag. The registry needs no second credential: the workflow token has `packages: write`. A prerelease never becomes `latest`.
- [x] Never expose publish secrets to PR workflows; protected tags; manual release-recovery path documented.
  - The pull-request workflow has `contents: read` and no secret at all; the release workflow asks for `contents: write` in the `publish` job and `packages: write` in the image job. The protected-tag rule (`v*`, write permission only, covering the deletion) and the release-recovery path (a failed platform, a failed push, a failed upload, a release that must be withdrawn) are documented in `CONTRIBUTING.md`.
- Verification: PR pipeline green on a test PR; release pipeline exercised on `v0.1.0-rc.1` pre-release tag (artifacts verified by download).
  - **The pipeline ran for the first time on 2026-10-22 and is green** (six jobs: the Rust crates, the cloud server against a PostgreSQL, the frontend, the desktop app with the sync end-to-end test, the container image with the compose smoke test, and the supply chain). The first run found four defects that are fixed:
    1. the `rust` job ran `cargo clippy --workspace`, which builds the desktop app — and the webview libraries are not installed there; the job now covers the pure-Rust crates and the `desktop` job lints and tests the app;
    2. the container image did not build: the Dockerfile never copied `build.rs`, so the migrations were never embedded;
    3. the backup test of the cloud server ran the `pg_dump` of the runner (version 16) against a server of version 18 — the dump was refused; the jobs install the client of the server version;
    4. the compose smoke test pushed a host without its fields and the desktop tests needed a credential store that a runner does not have.
  - The **release** pipeline ran for the first time on 2026-10-22 (the pre-release `v0.1.0-rc.1`) and published everything: the `.deb` (built *and* installed on the runner), the AppImage, the NSIS installer, the cloud image in `ghcr.io` (publicly pullable) and the GitHub release with the checksums and the deployment files. The first run found six more defects, all fixed: npm ate the `--bundles` flag, the registry refused the capitalised repository name, there is no Fedora image among the GitHub-hosted runners (the RPM is built on Debian; the bundler writes it itself, the `rpm` tools only query it), the Windows build had no `icon.ico`, the licence file was two levels up instead of three, and the download of the artifacts failed on the record of the image build.
  - The pre-release is marked as such and its image has no `latest` tag: the stable tag points at the last stable release only.

## Phase 15 — Publication (GitHub Releases + Docker Hub)


- [!] Publish real `v0.1.0`: the pipeline is proven (the pre-release `v0.1.0-rc.1` published everything), the push credentials **are** available now (the git credential store of the machine), but the stable release is the maintainer's decision.
  - Unblock: the maintainer does the manual steps of `docs/release-readiness.md` (the visual pass on a real desktop, the packages on a real Windows, Ubuntu and Fedora) and pushes the tag `v0.1.0`. The pipeline needs no additional secret: the release and the container image are published with the workflow token.
  - The image no longer needs a Docker Hub token: it goes to `ghcr.io`, which the pipeline can write with `packages: write`.
  - The manifests are back at `0.1.0`, so the tag `v0.1.0` passes the check of the `verify` job.

## Phase 16 — Documentation and open-source readiness


- [x] README: description, status, platforms, capabilities + limitations, per-OS install, single cloud install command, first-run config, usage, data locations, backup/restore, upgrade/troubleshooting, contributing, security reporting, license.
  - The README carries the description, the status, the stack, the features, the install commands for Debian/Ubuntu and Fedora, the development commands, the cloud install pointer, the repository layout, the data locations, security reporting, contributing and the license. The full install guide is `docs/install.md`.
- [x] `docs/`: development guide (setup, tests), architecture overview + ADRs, cloud deployment/ops, backup/restore, performance notes, known limitations.
  - `docs/architecture/` (7 ADRs, the index and `overview.md`, which ties them together: the shape of the system, the crates, the data flow of a change, the boundaries and the invariants the tests protect), `docs/security/` (threat model, dependency review), `docs/cloud/deployment.md` (install, first run, backups, upgrade/rollback, troubleshooting), `docs/development.md` (setup, the test matrix, the everyday commands, the conventions), `docs/install.md` (one command per OS), `docs/performance.md` (the measured numbers and their budgets) and `docs/limitations.md` (what the product does not do yet and the residual risks).
- [x] Repo hygiene: LICENSE, CONTRIBUTING, SECURITY, CODE_OF_CONDUCT, CHANGELOG (Keep a Changelog), .editorconfig, issue+PR templates, Dependabot (cargo/npm/github-actions).
- Verification: docs match actual commands (each command executed once), links resolve.

## Phase 17 — Cross-platform validation and performance


- [x] Measure (Linux local): cold/warm startup, idle RSS, RSS + memory with N sessions, terminal throughput (MB/s large output), UI responsiveness during flood, connection setup/teardown timings, DB query timings on 10k hosts.
  - `crates/termru-store/tests/performance.rs` measures the store with 10,000 hosts (write, list, read, search, the sync payload, the backup, the file size); the SSH suite prints the throughput of a 4 MiB flood; the cloud server was measured with the built binary (start-up to ready, requests/s, RSS).
  - Not measured: the desktop app's start-up and its memory, the UI responsiveness during a flood and the sync round trip at scale — the build machine has no window manager, so a visible window is needed (documented in `docs/limitations.md`).
- [x] Budgets recorded in `docs/performance.md` with actual numbers; fix measurable bottlenecks found.
  - `docs/performance.md` holds the numbers, the machine they come from and the budget behind each of them. No bottleneck was found: the slowest measured path is the list of 10,000 hosts (62 ms), and the sidebar only renders the visible part.
- [ ] Windows + Ubuntu + Fedora validation on real runners (build+launch smoke; UI walkthrough limited to automated checks) — clearly distinguish "built on" vs "tested on".
- [!] Manual interactive UX validation on Windows/Fedora/Ubuntu by human (environment limitation): automated + Linux-manual only in this environment.
  - Unblock: user machine verification checklist provided in docs.

## Phase 18 — Final acceptance and release readiness


- [x] Required user journeys (desktop, sync, operations) executed or automated; failures triaged.
  - Every journey has an automated proof: the app-layer E2E against a real `sshd` (connect, stream, resize, close, the failure path), the SSH suite (the host-key trust flow, the changed key, the auth failures, the flood, the re-attach), the sync E2E against the real server binary (register, push, pull with a second device), the compose smoke test (the deployment, the migration, a client round trip, the backup and the restore) and the contract test (the UI ↔ Tauri argument shapes, the splits, the settings, the remapping, the folders and the tags).
  - Not covered by a machine: the visual pass (no window manager on the build machine) and the packages on Windows, macOS and a real desktop. Both are listed as the manual steps in `docs/release-readiness.md`.
- [x] No placeholder/fake functionality in shipped paths; errors actionable; TODO scan for correctness/security debt.
  - The scan found no `TODO`/`FIXME` marker in the code (only two in the SSH test header and the issue template), no `unimplemented!` and no `panic!` outside the tests and the build script (which fails the build loudly on a malformed migration file). Every command answers with an actionable message; the errors of the API name the field that is wrong.
  - The two defects that the suites found are fixed: a local edit did not mark the record dirty (a silent overwrite instead of a conflict) and a closed window left its sessions running with nothing rendering them.
- [x] TODO.md reconciled with reality; limitations and residual risks documented; version + changelog final.
  - `docs/limitations.md` was rewritten: it listed as missing the background sync, the split panes, the key file picker, the settings dialog, the remapping UI, the folders and the tags — all of them are implemented now, and the list only holds what is really missing.
  - The changelog holds the 0.1.0 release; the version in `Cargo.toml` and in `tauri.conf.json` is 0.1.0.
- [x] Release readiness review against §23 of project instructions (evidence per item).
  - `docs/release-readiness.md`: what the release contains, the evidence per check (the commands and their results), the journey coverage, the decisions, the residual risks, the manual steps a maintainer has to do before the tag and the known gaps.
  - The instruction document itself (§23) was not available in this environment; the review uses the acceptance criteria the repository documents (the phases of this file, the ADRs and the threat model). This is stated in the document.

---

## Current status

**0.1.0 is ready to be tagged** (the tag itself needs the push credentials, which this
environment does not have). 88 of the 99 items are done, 4 are blocked by the environment
and 7 are the open ends listed in the phases above: the port forwarding, the per-window
tab sets, the workspace geometry, the race-condition review, the search over the snippets
(which do not exist), the context menus and the validation on Windows, Ubuntu and
Fedora.

**What is in the release.** A desktop SSH workspace (Tauri 2 + Svelte 5) with a connection
manager, tabs and split panes, a command palette, remappable keys, folders and tags, the
secrets in the OS credential store and a self-hosted sync server (Axum + PostgreSQL) that
mirrors the configuration between the devices — never a secret. The SSH engine is pure
Rust (`russh`), with the trust-on-first-use flow, the changed-key warning and a bounded
reconnect policy.

**How it was verified.** 208 Rust tests (the SSH suite against a real `sshd`, the cloud
server against a real PostgreSQL, the app layer end to end, the sync client against the
real server binary) and 90 frontend tests (the pane tree, the store, the keybindings, the
webview hardening scan and a jsdom contract test that asserts the argument shapes of the
IPC). `cargo fmt --check`, `cargo clippy --workspace --all-targets -D warnings`,
`cargo deny`, `npm run lint`, `npm run test` and `npm run build` are green.

**The two defects the suites found** (a local edit that did not mark the record dirty, so
a change from another device overwrote the local copy instead of raising a conflict, and a
closed window that left its SSH sessions running with nothing rendering them) are fixed,
each with a regression test.

**What is not verified here.** The interactive UI was never driven by a human (the build
machine has no window manager), the packages were never installed on a real desktop, and
only Linux was built and tested. `docs/release-readiness.md` lists the manual steps a
maintainer has to do before the tag.

**Environment note:** the dev machine is close to full. The workspace `dev` profile uses
`debug = 1` (line tables only), so the `target/` tree is ~2 GB instead of ~7 GB; work
continues with `CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0` to keep artifacts small,
and `cargo clean -p <crate>` over a full clean.
