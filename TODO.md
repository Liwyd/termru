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
- [x] Decide cloud persistence: **PostgreSQL + sqlx** for the cloud manager.
  - Rationale: standard self-hosted deployment shape (compose with db + app, healthcheck-gated startup), mature backup/restore (`pg_dump`), safe concurrent multi-device writes. Data volume is small but Postgres adds only one compose service; documented in ADR.
- [x] Decide cloud API framework: **Axum** (candidate; stable ecosystem, first-class tower middleware for rate limiting).
- [x] Decide secret policy v1: **no secret synchronization**. Secrets stay in OS keyring (keyring crate) only. Documented limitation; config sync only. (Avoids inventing a recovery mechanism that breaks E2E guarantees.)
- [x] Decide sync model: **transactional, revision-based, per-user monotonic `rev`, tombstones, cursor pagination, explicit conflict surfacing** (no silent LWW, no CRDTs).
- [ ] ADRs written in `docs/architecture/` for: SSH transport, desktop stack, persistence split, sync protocol, secret policy, multi-window session ownership, licensing.
  - Verification: files exist, decisions match code.

## Phase 2 — Repository structure and development tooling

- [x] Cargo workspace with the three core crates (`termru-core`, `termru-store`, `termru-ssh`) + shared lints/profiles.
- [ ] Add `apps/desktop/src-tauri` + `apps/cloud-server` to the workspace (they do not exist yet).
- [ ] Frontend package layout (`apps/desktop`, Svelte 5 + Vite 8 + TS 5) + editor/format configs for TS/Svelte.
- [x] Rust tooling: rustfmt config, clippy workspace lints (incl. unwrap/expect warns + allow-in-tests), `.editorconfig`, `.gitignore`, `cargo-deny` config (licenses + advisories).
- [ ] Root scripts (`scripts/` + npm scripts) for: dev, test, lint, format, typecheck.
- [ ] Base project files beyond LICENSE: README stub, CONTRIBUTING/SECURITY/CODE_OF_CONDUCT/CHANGELOG, issue/PR templates, Dependabot config.
- [ ] CI workflow for PRs: fmt, clippy, tests, tsc/svelte-check, vitest, cargo-deny. → Phase 14.
- Verification (Rust scope): `cargo fmt --check`, `cargo clippy --workspace --all-targets` (0 warnings), `cargo test --workspace` (63 tests) all pass locally. Frontend checks come with the frontend scaffold.

## Phase 3 — Core domain and local persistence

- [x] `crates/termru-core`: typed IDs, Host/Identity/Group/Tag/Snippet/Preference models, validation, serialization (serde), sync payload types + envelope versioning.
- [x] `crates/termru-store`: SQLite schema + versioned migrations (up migrations with recorded schema version; tested), repositories (hosts, groups, tags, favorites, recents, preferences, workspaces, sync queue/meta), soft delete + tombstone bookkeeping for sync.
- [x] Store safety: WAL mode, busy timeout, foreign keys ON, transactions per operation, corruption detection on open (quick_check → actionable error), file permissions 0600.
- [x] Export/import/backup (VACUUM INTO → atomic rename; restore verifies, keeps pre-restore aside) with round-trip test + corrupt-backup rejection test.
- [ ] Concurrent-access test (two connections, busy-timeout behavior).
- Verification: unit tests incl. migration-from-empty AND future-schema rejection, backup/restore round-trip, 24 store tests green.

## Phase 4 — SSH engine and terminal reliability (russh)

- [x] `crates/termru-ssh`: connection state machine (idle→connecting→authenticating→ready→reconnecting→closed/failed), timeouts (connect/auth), keepalive + broken-connection detection, cancellable connect.
- [x] Auth: password, private key (+ passphrase), SSH agent (OpenSSH agent socket) with graceful degradation when unavailable.
- [x] Known-hosts: OpenSSH format read/write incl. hashed entries, TOFU first-use confirmation flow, **changed host key = hard warning with old/new fingerprint**, never auto-accept on change.
- [x] PTY shell: pty-req with TERM/size, window-change resize propagation, shell selection, output streaming with bounded buffering (backpressure), input writes, exit/close propagation, bounded scrollback replay buffer for re-attach.
- [ ] Reconnect policy: bounded backoff with cap, no uncontrolled loops; explicit user action vs automatic. (Error classification primitives — `is_retriable`, categories — exist; the reconnect loop itself is not implemented.)
- [ ] Port forwarding (local/remote/dynamic), jump hosts: implement if time-justified for v1 → tracked under Phase 5 scope decision (ADR).
- [ ] Integration tests against real OpenSSH (local `sshd` as unprivileged user on high port; CI with root sshd): success, wrong password, wrong key, passphrase-protected key, host-key-change detection, resize, large output flood, interrupted connection, malformed/binary output.
- Verification: integration suite green locally; each negative case asserts actionable error classification.

## Phase 5 — Desktop MVP (main UI + SSH end-to-end)

- [ ] Tauri app scaffold: windows, capabilities (least privilege), app state (`SessionManager` owns all SSH sessions in Rust — single owner across windows), structured logging (file + stderr, no secrets), panic hygiene.
- [ ] Commands: host CRUD (validation in-app), session open/close/write/resize/duplicate, known-host confirm/decline, preferences.
- [ ] SessionManager: per-session tokio task (actor exists in `termru-ssh`), Tauri command layer, bounded input/output channels, status events, graceful shutdown on app exit, orphan prevention.
- [ ] Frontend foundation: design tokens, theme (light/dark), layout shell (sidebar + main), components: HostList/HostCard, host editor form, connection dialog (password/passphrase prompts), status badges, empty states, toasts/errors.
- [ ] Terminal view: @xterm/xterm + fit/webgl(addon fallback to DOM renderer), attach/detach to sessions via Tauri `Channel` streaming, resize debounce → `window-change`, copy/paste, scrollback, selection.
- [ ] Tabs: create/close/rename/reorder, keyboard navigation, session indicators (activity/error), unsaved-layout persistence.
- Verification: manual E2E against local sshd + automated Rust tests for SessionManager; frontend unit tests for state logic; `svelte-check`+vitest green.

## Phase 6 — Splits, multiple windows, workspaces

- [ ] Split panes: horizontal/vertical split, adjustable ratios, pane focus model, keyboard navigation between panes.
- [ ] Multi-window: create/close native windows, per-window tab sets, **tab move between windows without dropping SSH** (re-attach + replay buffer), no duplicate session ownership.
- [ ] Workspaces: save/restore layout (tabs+splits+window geometry), crash recovery of layout metadata (never claim remote shell recovery), stale session cleanup on startup.
- [ ] Race-condition review: rapid open/close/move/quit interleavings (test-cased).
- Verification: tests for session ownership invariants (single owner, no leaks, no double-free of channels) + manual multi-window passes.

## Phase 7 — Keyboard, palette, search, theming, preferences

- [ ] Global + terminal-aware keybinding system (platform defaults, conflict detection, remapping UI, works when terminal focused without stealing input).
- [ ] Command palette (fuzzy, discoverable, shows bindings).
- [ ] Global search across hosts/tags/snippets.
- [ ] Appearance settings: fonts, sizes, terminal theme overrides, cursor style, reduced motion respected.
- [ ] Context menus, tooltips, drag-and-drop (host → tab), recent hosts, favorites, tags, folders.
- Verification: unit tests for keybinding conflict detection and command registry; `svelte-check` clean; manual keyboard-only walkthrough of core journeys.

## Phase 8 — Local security (desktop)

- [ ] Secrets in OS keyring via `keyring` crate (Secret Service / Windows Credential Manager / macOS Keychain); no plaintext secrets in SQLite; secret values zeroized where feasible; never logged.
- [ ] Host editor: key file picker (path validation, no copy of private key into DB), passphrase never persisted unless stored in keyring (user choice, explicit).
- [ ] Webview hardening: CSP, no remote content loading in webview, single-instance plugin, secure IPC (input validation on every command: sizes, ids, path sanity).
- [ ] Logging audit: grep-based test asserting no secret material patterns in log files.
- Verification: unit tests for keyring abstraction (mock backend), CSP in prod config, command input validation tests.

## Phase 9 — Cloud API (self-hosted manager)

- [ ] `apps/cloud-server` scaffold: Axum 0.8/0.9, config via env (validated at startup, fail-fast), structured logs, graceful shutdown, healthz/readyz (readyz checks DB).
- [ ] Auth: register/login (argon2id), opaque access+refresh tokens (hashed at rest, rotation with reuse detection → family revocation), session/device revocation.
- [ ] Devices: enrollment, list, rename, revoke (revoked device loses API access immediately).
- [ ] Sync endpoints: `GET /v1/sync/changes?since=<rev>` (bounded cursor pagination, tombstones included) + `POST /v1/sync/changes` (batch, per-record base_rev conflict detection → 409 payload with server records), idempotency-safe retries (client mutation ids), payload size limits, schema version field.
- [ ] Per-user isolation enforced at query level (every query scoped by user_id from token; IDOR tests).
- [ ] Rate limiting (IP + token keyed), request body limits, timeouts, security headers, strict CORS (none needed for desktop client → deny by default; documented).
- [ ] Audit log for auth/device/security events; login throttling + account-enumeration-resistant responses.
- [ ] DB migrations (forward + tested on empty and existing DB), seed-free, pg_dump backup/restore script + test.
- [ ] Integration tests: full auth/device/sync/conflict/revocation/isolation/rate-limit matrix against real Postgres (CI service; local cluster).
- Verification: `cargo test` for server incl. testcontainer-free Postgres (CI service container), `cargo deny`, manual curl walkthrough.

## Phase 10 — Desktop sync client

- [ ] Sync settings UI (server URL, sign-in, device enrollment, status, last-sync, offline indicator).
- [ ] Offline-first: local edits queued in SQLite (ordered, idempotent op ids), push/pull with cursor, deferred sync on reconnect, bounded retry with backoff, no data loss on restart.
- [ ] Conflict surfacing UI: list of conflicts with local/remote comparison + explicit resolution (keep local / take remote); deletions vs edits surfaced, never silently resurrect or drop.
- [ ] Cloud outage does not block SSH (enforced by architecture; test).
- [ ] Secrets NOT synced (enforced: sync payload builder has no access to keyring).
- Verification: integration tests simulating two devices (two clients + one server): initial sync, offline edit, conflict, delete-propagation, device revocation.

## Phase 11 — Security hardening

- [ ] Threat model document (`docs/security/threat-model.md`) covering client, SSH, local storage, cloud API, sync, update, build pipeline, artifacts.
- [ ] `cargo deny` (licenses, advisories, duplicates-bans policy), `npm audit` gate in CI, secret scanning (gitleaks) in CI.
- [ ] SQL injection: parameterized queries only (enforced by code review + tests), authz matrix tests on every endpoint.
- [ ] Webview XSS surface: no `dangerouslySetInnerHTML`-equivalent, sanitization tests for any rendered remote-influenced strings (host names etc.).
- [ ] Container hardening: non-root user, read-only FS where possible, no secrets in image, resource limits, minimal base.
- [ ] Dependency review for significant deps (maintenance, license, security history) → `docs/security/dependency-review.md`.
- [ ] SECURITY.md with reporting process; residual risks documented.
- Verification: audit checklist complete; CI gates green; no open FATAL/SEVERE findings.

## Phase 12 — Desktop packaging and installation

- [ ] Debian `.deb` for Ubuntu (built on ubuntu runner; webkit2gtk-4.1 deps declared), RPM for Fedora (built in Fedora container), AppImage portable, Windows NSIS installer (x86_64).
- [ ] Package install smoke tests (dpkg/rpm query + launch where runner allows); WebKit/keyring/font deps validated on targets.
- [ ] Checksums (SHA256SUMS) for all artifacts; document signing limitations (no code-signing credentials → documented trust limitation).
- [ ] Install docs: one recommended command per OS (apt repo/direct .deb download, dnf/rpm, AppImage, winget/NSIS) — no manual compilation.
- Verification: release workflow dry-run on a pre-release tag; artifacts inspected.

## Phase 13 — Cloud containerization and deployment

- [ ] Multi-stage Dockerfile (distroless or debian-slim, non-root, tini/dumb-init graceful shutdown), healthcheck.
- [ ] `compose.yaml`: db + app, pinned image tags, volume for Postgres, healthcheck gating, resource limits, restart policy, secrets via env file (not in image), DB not exposed publicly by default.
- [ ] One-copy-paste README install command: versioned compose fetch with SHA256 verification → `docker compose up -d` (no `curl | sh` as root).
- [ ] Migrations run at container start (or documented `migrate` step) with lock safety (single-runner), backup/restore docs + tested procedure, upgrade/rollback docs, first-run setup + secret generation/rotation docs.
- Verification: CI builds image, runs it with compose, hits healthz + runs migration + backup/restore smoke test (docker available in CI).

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
- [ ] `docs/`: development guide (setup, tests), architecture overview + ADRs, cloud deployment/ops, backup/restore, performance notes, known limitations.
- [ ] Repo hygiene: LICENSE, CONTRIBUTING, SECURITY, CODE_OF_CONDUCT, CHANGELOG (Keep a Changelog), .editorconfig, issue+PR templates, Dependabot (cargo/npm/github-actions).
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

Phases 0–1 done (environment baseline + decisions recorded here). Phase 2–4 Rust scope largely done: workspace + three crates compile clean, `cargo fmt`/`cargo clippy` (0 warnings)/`cargo test --workspace` (63 tests) green. Known-hosts engine verified against OpenSSH format incl. hashed entries, wildcards, negation, `@revoked`, changed-key detection. Next milestone: ADRs in `docs/architecture/`, then Tauri app scaffold (Phase 5) and SSH integration tests against a local sshd.
