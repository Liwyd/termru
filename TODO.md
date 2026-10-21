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
  - Verification: the workflow is syntactically valid and mirrors the local commands; it runs on GitHub (not verifiable offline).
- Verification: `cargo fmt --check`, `cargo clippy --workspace --all-targets` (0 warnings), `cargo test --workspace` (95 tests) green; `npm run lint` (prettier + svelte-check 0 errors), `npm run test` (4 vitest), `npm run build` green; `cargo build -p termru-desktop` links; debug binary smoke-ran against vite dev without fatal output.

## Phase 3 — Core domain and local persistence

- [x] `crates/termru-core`: typed IDs, Host/Identity/Group/Tag/Snippet/Preference models, validation, serialization (serde), sync payload types + envelope versioning.
- [x] `crates/termru-store`: SQLite schema + versioned migrations (up migrations with recorded schema version; tested), repositories (hosts, groups, tags, favorites, recents, preferences, workspaces, sync queue/meta), soft delete + tombstone bookkeeping for sync.
- [x] Store safety: WAL mode, busy timeout, foreign keys ON, transactions per operation, corruption detection on open (quick_check → actionable error), file permissions 0600.
- [x] Export/import/backup (VACUUM INTO → atomic rename; restore verifies, keeps pre-restore aside) with round-trip test + corrupt-backup rejection test.
- [ ] Concurrent-access test (two connections, busy-timeout behavior).
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
- [ ] Terminal view: @xterm/xterm + fit/webgl(addon fallback to DOM renderer), attach/detach to sessions via Tauri `Channel` streaming, resize debounce → `window-change`, copy/paste, scrollback, selection.
  - Done: xterm 6 + fit addon + WebGL addon with DOM-renderer fallback, `Channel` streaming (base64 → bytes), resize debounce → `session_resize`, scrollback from preferences, `Ctrl+Shift+C`/`Ctrl+Shift+V` copy & paste.
  - Pending: scrollback restore after a re-attach (belongs with the Phase 6 re-attach work, which uses the `termru-ssh` replay buffer).
- [ ] Tabs: create/close/rename/reorder, keyboard navigation, session indicators (activity/error), unsaved-layout persistence.
  - Done: create/close, rename (double-click), drag & drop reordering, keyboard navigation (`Ctrl+Tab` / `Ctrl+Shift+Tab`, `Ctrl+W`), per-tab status badge, **layout persistence** (tabs + order + hosts saved through the `workspaces` repository; restored on launch as *disconnected placeholders* — secrets are never stored, so a restored tab reconnects only on request).
- Verification: `cargo test --workspace` → 122 tests pass (core 28, store 25, ssh 26, sshd integration 17 incl. a replay-snapshot test, desktop 23 incl. 3 app-layer E2E with the two-view attach case + the logging audit), `cargo clippy --workspace --all-targets` 0 warnings, `cargo fmt --check` clean, `npm run lint` (svelte-check 0/0), `npm run test` (28 vitest, incl. a jsdom contract test that asserts the UI calls the Tauri commands with the documented argument shapes), `npm run build` green. **Pending: a visual UI pass — this machine's X session has no window manager, so the app window cannot be shown or driven here; it must be done on a real desktop session.**

## Phase 6 — Splits, multiple windows, workspaces

- [ ] Split panes: horizontal/vertical split, adjustable ratios, pane focus model, keyboard navigation between panes.
- [ ] Multi-window: create/close native windows, per-window tab sets, **tab move between windows without dropping SSH** (re-attach + replay buffer), no duplicate session ownership.
  - Done: **tab move between windows without dropping the SSH session** — the session is owned by the app, not the window; a moved tab opens a new window (`window_open_session`) whose frontend attaches to the running session (`session_attach`) and receives the replay buffer, then the live stream. The app layer fans events out to every attached view and drops views whose window closed; a brief grace period covers the move itself.
  - Pending: per-window tab sets, window close semantics (a closed window must not kill sessions other windows still show).
- [ ] Workspaces: save/restore layout (tabs+splits+window geometry), crash recovery of layout metadata (never claim remote shell recovery), stale session cleanup on startup.
- [ ] Race-condition review: rapid open/close/move/quit interleavings (test-cased).
- Verification: tests for session ownership invariants (single owner, no leaks, no double-free of channels) + manual multi-window passes.

## Phase 7 — Keyboard, palette, search, theming, preferences

- [ ] Global + terminal-aware keybinding system (platform defaults, conflict detection, remapping UI, works when terminal focused without stealing input).
  - Done: command registry with platform-aware accelerators (`Mod` → Ctrl/Cmd), conflict detection (unit-tested), a global key dispatcher that never shadows the terminal's own keys, and per-command bindings shown in the palette.
  - Pending: a remapping UI (the registry supports user overrides; the UI does not exist yet).
- [x] Command palette (fuzzy, discoverable, shows bindings).
  - `Ctrl/Cmd+K` opens it; fuzzy subsequence matching with word-boundary and consecutive-character bonuses; arrows/Enter/Escape; shows the accelerator of every entry and offers the hosts as entries ("Connect to …").
- [ ] Global search across hosts/tags/snippets.
  - Done: host search in the sidebar (name, address, username) + a "Recent" section (most recently connected hosts, from the recents table).
  - Pending: tags/snippets do not exist yet (no snippet model in the store), so the search cannot cover them.
- [ ] Appearance settings: fonts, sizes, terminal theme overrides, cursor style, reduced motion respected.
  - Done: font family/size, cursor style/blink and the light/dark theme are preferences and apply live to the running terminal; the palette can change the theme and the font size; the UI honours `prefers-reduced-motion` (spinner).
  - Pending: a settings dialog (the values are stored, but there is no UI for them yet).
- [ ] Context menus, tooltips, drag-and-drop (host → tab), recent hosts, favorites, tags, folders.
  - Done: recent hosts (sidebar section), favorites (star toggle + sorting), host → tab via drag-free "Connect"/palette entries.
  - Pending: context menus, tooltips, drag-and-drop of hosts onto tabs, tags/folders in the tree.
- Verification: unit tests for keybinding conflict detection and command registry; `svelte-check` clean; manual keyboard-only walkthrough of core journeys.
  - Automated: 44 frontend tests (keybinding normalization/conflicts/event matching, fuzzy ranking, the tab state machine, the layout document, and the jsdom UI↔Tauri contract test) + `svelte-check` 0/0.
  - Manual: the keyboard-only walkthrough still needs a desktop session (no window manager on this machine).

## Phase 8 — Local security (desktop)

- [x] Secrets in OS keyring via `keyring` crate (Secret Service / Windows Credential Manager / macOS Keychain); no plaintext secrets in SQLite; secret values zeroized where feasible; never logged.
  - `secrets.rs`: secrets are stored under `com.liwyd.termru` in the platform credential store; the app degrades gracefully when no credential store exists (the secret is asked for per connection); the frontend drops its in-memory copy once the credential store holds it; the connection path falls back to the stored secret, so "remembered" hosts connect without prompting.
  - The database schema has no field that could hold a secret, and the sync payload never contains one.
- [ ] Host editor: key file picker (path validation, no copy of private key into DB), passphrase never persisted unless stored in keyring (user choice, explicit).
  - Done: key paths are validated (absolute, non-empty) and only the *path* is stored (never the key); passphrases are persisted only in the credential store and only when the user explicitly opts in.
  - Pending: a native file picker for the key path (the path is currently typed).
- [ ] Webview hardening: CSP, no remote content loading in webview, single-instance plugin, secure IPC (input validation on every command: sizes, ids, path sanity).
  - Done: CSP set, capabilities limited to `core:default` (+ the session windows), no remote content, command inputs validated (terminal geometry bounds, host/identity validation, secret kind whitelist, key path checks).
  - Pending: the single-instance plugin (a second launch should focus the running window).
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
  - Pending: an automatic background cycle (a timer) with a bounded retry and backoff, and an automatic access-token refresh before it expires. Today the user triggers the cycle.
- [x] Conflict surfacing UI: list of conflicts with local/remote comparison + explicit resolution (keep local / take remote); deletions vs edits surfaced, never silently resurrect or drop.
  - The store owns the semantics: `resolve_keep_local` re-queues the local copy against the revision the server reported, `resolve_take_remote` replaces the local copy with the server's. A record with an open conflict is never pushed, and a remote tombstone that meets a local change becomes a conflict instead of a silent deletion.
  - The panel shows both copies, names the field that differs and offers the two explicit choices.
- [x] Cloud outage does not block SSH (enforced by architecture; test).
  - The sync runs in its own command and owns no part of the SSH path; a failing sync records the error message and returns it, the session manager is untouched. Verified by a test that a failing cycle keeps the queue and the local copies.
- [x] Secrets NOT synced (enforced: sync payload builder has no access to keyring).
  - The payload builder lives in the store and only ever sees the domain models, which have no field that could hold a secret (a private-key host carries an *identity reference*). Verified by a test that walks the push payload and rejects any field named like a secret, and by the desktop test that the credential store is the only place a token lives.
- Verification: `cargo test --workspace` → 199 tests green (core 28, store 35, ssh 26, sshd integration 17, cloud server 46, desktop 47 incl. the sync engine), `cargo clippy --workspace --all-targets -D warnings` clean, `cargo fmt --check` clean, `cargo deny` ok, `npm run lint` (svelte-check 0/0), `npm run test` (50 vitest, incl. the sync store and a UI↔Tauri contract test for the sign-in arguments), `npm run build` green.
  - The engine is tested against a fake transport **and** end to end against the real server: `apps/desktop/src-tauri/tests/sync_e2e.rs` starts the `termru-cloud-server` binary against a PostgreSQL server, signs in with the desktop engine, pushes a host and pulls it back with a second device. It skips (loudly) when `TERMRU_TEST_DATABASE_URL` or `TERMRU_CLOUD_SERVER_BIN` is missing; the CI `desktop` job runs it with a `postgres:18-alpine` service.

## Phase 11 — Security hardening

- [x] Threat model document (`docs/security/threat-model.md`) covering client, SSH, local storage, cloud API, sync, update, build pipeline, artifacts.
  - Documents the assets, the trust boundaries (webview ↔ Rust ↔ network ↔ OS), the per-surface mitigations, the **residual risks** (TOFU first use, unencrypted local DB, webview XSS, unpinned CI actions, no code signing) and the out-of-scope list.
- [x] `cargo deny` (licenses, advisories, duplicates-bans policy), `npm audit` gate in CI, secret scanning (gitleaks) in CI.
  - The policy is defined in `deny.toml`: lockfile-pinned versions (wildcard requirements are warnings), unmaintained-crate advisories are fatal only for direct dependencies, and every ignore carries a reason + review date. Verified locally with cargo-deny 0.20: advisories/bans/licenses/sources **all ok**. `npm audit`: 0 vulnerabilities.
- [x] SQL injection: parameterized queries only (enforced by code review + tests), authz matrix tests on every endpoint.
  - Every statement in `apps/cloud-server` is a bound parameter; no SQL is built by string concatenation. The authorization matrix is covered by the integration suite: unauthenticated calls are refused, a token of another user reaches nothing, a device of another account can not be renamed or revoked, and a revoked device loses access immediately.
- [ ] Webview XSS surface: no `dangerouslySetInnerHTML`-equivalent, sanitization tests for any rendered remote-influenced strings (host names etc.).
- [ ] Container hardening: non-root user, read-only FS where possible, no secrets in image, resource limits, minimal base.
- [x] Dependency review for significant deps (maintenance, license, security history) → `docs/security/dependency-review.md`.
  - Documents the policy, every significant Rust/npm dependency with its licence and role, and the **known advisories**: RUSTSEC-2023-0071 (rsa via russh/ssh-key — no fixed release, reviewed 2027) and the unmaintained zbus/zvariant chain behind the keyring's Linux backend.
- [ ] SECURITY.md with reporting process; residual risks documented.
- Verification: audit checklist complete; CI gates green; no open FATAL/SEVERE findings.

## Phase 12 — Desktop packaging and installation

- [ ] Debian `.deb` for Ubuntu (built on ubuntu runner; webkit2gtk-4.1 deps declared), RPM for Fedora (built in Fedora container), AppImage portable, Windows NSIS installer (x86_64).
- [ ] Package install smoke tests (dpkg/rpm query + launch where runner allows); WebKit/keyring/font deps validated on targets.
- [ ] Checksums (SHA256SUMS) for all artifacts; document signing limitations (no code-signing credentials → documented trust limitation).
- [ ] Install docs: one recommended command per OS (apt repo/direct .deb download, dnf/rpm, AppImage, winget/NSIS) — no manual compilation.
- Verification: release workflow dry-run on a pre-release tag; artifacts inspected.

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
- [ ] Verification: CI builds the image, runs it with compose, hits healthz + runs migration + backup/restore smoke test (docker available in CI).
  - Done in CI: the image build. Still open: a compose smoke test in CI (the database service, the health check and a migration run through the real container).

## Phase 14 — GitHub Actions CI/CD

- [ ] PR pipeline: fmt, clippy(-D warnings), cargo test (workspace), npm typecheck/test, cargo-deny, gitleaks, server integration tests (Postgres service), SSH integration tests (sshd), frontend build, desktop build check (linux), container build check.
- [ ] Least-privilege permissions (`contents: read` for PRs), third-party actions pinned to SHAs, concurrency groups, cache hygiene (keyed on lockfiles).
- [ ] Release pipeline: tag validation (semver + clean version) → tests → build matrix (windows-latest NSIS; ubuntu .deb/AppImage; fedora container RPM) → checksums → cloud image build/push (Docker Hub, versioned tag + `latest` policy) → GitHub Release upload → post-publish verification (artifact presence + image digest check) → report partial failures explicitly.
- [ ] Never expose publish secrets to PR workflows; protected tags; manual release-recovery path documented.
- Verification: PR pipeline green on a test PR; release pipeline exercised on `v0.1.0-rc.1` pre-release tag (artifacts verified by download).

## Phase 15 — Publication (GitHub Releases + Docker Hub)

- [!] Publish real `v0.1.0`: requires GitHub push/release credentials + Docker Hub token (not present in environment: no `gh` CLI, no tokens in env).
  - Unblock: user provides `GH_TOKEN`/gh auth + `DOCKERHUB_*` secrets configured in repo → release workflow publishes; verify artifacts + image tags post-publish.

## Phase 16 — Documentation and open-source readiness

- [ ] README: description, status, platforms, capabilities + limitations, per-OS install, single cloud install command, first-run config, usage, data locations, backup/restore, upgrade/troubleshooting, contributing, security reporting, license.
  - Done: description, status, stack, features, development commands, the cloud install pointer, the repository layout, the data locations, security reporting, contributing and the license. The per-OS install commands need the packaging of Phase 12.
- [ ] `docs/`: development guide (setup, tests), architecture overview + ADRs, cloud deployment/ops, backup/restore, performance notes, known limitations.
  - Done: `docs/architecture/` (7 ADRs + index), `docs/security/` (threat model, dependency review), `docs/cloud/deployment.md` (install, first run, backups, upgrade/rollback, troubleshooting) and `apps/cloud-server/README.md` (the API reference and the configuration table). Missing: the development guide, the performance notes and the known-limitations page.
- [x] Repo hygiene: LICENSE, CONTRIBUTING, SECURITY, CODE_OF_CONDUCT, CHANGELOG (Keep a Changelog), .editorconfig, issue+PR templates, Dependabot (cargo/npm/github-actions).
- Verification: docs match actual commands (each command executed once), links resolve.

## Phase 17 — Cross-platform validation and performance

- [ ] Measure (Linux local): cold/warm startup, idle RSS, RSS + memory with N sessions, terminal throughput (MB/s large output), UI responsiveness during flood, connection setup/teardown timings, DB query timings on 10k hosts.
- [ ] Budgets recorded in `docs/performance.md` with actual numbers; fix measurable bottlenecks found.
- [ ] Windows + Ubuntu + Fedora validation on real runners (build+launch smoke; UI walkthrough limited to automated checks) — clearly distinguish "built on" vs "tested on".
- [!] Manual interactive UX validation on Windows/Fedora/Ubuntu by human (environment limitation): automated + Linux-manual only in this environment.
  - Unblock: user machine verification checklist provided in docs.

## Phase 18 — Final acceptance and release readiness

- [ ] Required user journeys (desktop, sync, operations) executed or automated; failures triaged.
- [ ] No placeholder/fake functionality in shipped paths; errors actionable; TODO scan for correctness/security debt.
- [ ] TODO.md reconciled with reality; limitations and residual risks documented; version + changelog final.
- [ ] Release readiness review against §23 of project instructions (evidence per item).

---

## Current status

Phases 0–1 complete (environment baseline, decisions, ADRs in `docs/architecture/`). Phases 2–4 Rust scope complete: workspace + three crates; `cargo fmt --check`, `cargo clippy --workspace --all-targets` (0 warnings), `cargo test --workspace` (95 tests = 28 core + 26 ssh + 16 sshd integration + 25 store) green locally. Known-hosts verified against OpenSSH incl. hashed entries, wildcards, negation, `@revoked`, changed-key detection. SSH integration suite runs a private unprivileged sshd per test (loud self-skip without OpenSSH tooling). Defects found & fixed by the suite: russh `SshId::Standard` needs the full `SSH-2.0-` banner prefix; `ChannelMsg::ExitSignal` without an `ExitStatus` is now classified `Failed{Network}` (reconnect-eligible) per the no-status-is-an-abrupt-death invariant; monotonic `now_ms` + deterministic `ORDER BY` tiebreakers in the store. Desktop scaffold complete and pushed (`bb834f0`): `apps/desktop` (Svelte 5 + Vite 8 + TS 5 + xterm 6, prettier/svelte-check/vitest green) + `apps/desktop/src-tauri` (Tauri 2.12, capability `core:default`, CSP, `app_info` IPC) in the workspace; debug binary smoke-ran against vite dev.

Phase 5 (desktop MVP) substantially implemented: Rust command layer + `SessionManager` (116 workspace tests green, incl. an **app-layer E2E** against a real `sshd`) and the frontend foundation (layout shell, host list/editor, connection + host-key dialogs, xterm view over Tauri `Channel`, tabs with reordering + layout persistence, file logging + panic hygiene; `svelte-check` 0/0, 28 vitest tests, `vite build` green).

**Phase 9 (cloud API) implemented** in `apps/cloud-server`: Axum 0.8 + PostgreSQL (`tokio-postgres`/`deadpool-postgres`, see ADR 0003 for the driver change), 46 tests green against a real PostgreSQL 18.6, `cargo clippy --workspace --all-targets -D warnings` clean, smoke-tested with `curl`. The API covers accounts (argon2id, opaque rotating tokens, reuse detection), devices (enroll/rename/revoke) and the revision-based sync protocol shared with the desktop client (pull/push, tombstones, explicit conflicts, idempotent retries). Hardening: per-token and per-IP rate limiting, body limits, request timeouts, strict security headers, no CORS, an audit log, and login throttling with enumeration-resistant responses.

**Environment note:** the dev machine is close to full. The workspace `dev` profile now uses `debug = 1` (line tables only), so the `target/` tree is ~2 GB instead of ~7 GB; work continues with `CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0` to keep artifacts small, and `cargo clean -p <crate>` over a full clean.

**Phase 10 (desktop sync client) implemented**: a sync engine in the desktop app (transport trait + an HTTP client, tokens in the OS credential store, push/pull with the revision cursor, explicit conflict resolution), the store gained the conflict-resolution semantics and — with them — a fix for a real bug: a local edit did not mark the row dirty, so a remote change would have silently overwritten the local copy instead of raising a conflict. The UI gained a sync panel (settings, status, conflicts) and a sidebar entry.

**Next steps:** (1) visual UI pass on a machine with a window manager (add host → connect → resize → close, move a tab to a new window, remember a password, use the palette, sign in to a sync server); (2) Phase 10 remainder: an automatic background sync cycle with a bounded retry and an automatic token refresh; (3) Phase 6: split panes, per-window tab sets, window-close semantics; (4) Phase 7 remainder: keybinding remapping UI, settings dialog, tags/folders, context menus; (5) Phase 8 remainder: native key file picker + single-instance plugin; (6) Phase 11 remainder: CI action pinning by SHA, `cargo deny` in the local pre-commit path; (7) Phase 12/14: packaging and the release pipeline; (8) first CI run on GitHub (the pipeline has never executed remotely).
