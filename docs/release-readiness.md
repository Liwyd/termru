# Release readiness — 0.1.0

A review of what is in the 0.1.0 release, with the evidence for every
claim. The open work is in [TODO.md](../TODO.md) and the known gaps are in
[limitations.md](limitations.md).

> `v0.1.0` was published on 2026-10-22: the tag passed the pipeline
> (`verify`, `test`, `desktop`, `cloud-image`, `publish`) and the release
> page holds the `.deb`, the AppImage, the NSIS installer, the `.rpm`, the
> checksums and the deployment files; the image carries the tags `0.1.0`
> and `latest`. The manual items below (visual pass on a real desktop, the
> packages launched on real Windows, Ubuntu and Fedora) remain outstanding
> — nothing in the repository can do them for you.

> The project instructions that define the release criteria (§23) were not
> available in the environment this release was prepared in. The review
> below therefore uses the acceptance criteria the repository documents
> itself: the phases of `TODO.md`, the decisions in the ADRs, and the
> surfaces in the threat model.

## What the release contains

| Part | Where | State |
|---|---|---|
| The desktop app | `apps/desktop` | Implemented, tested; the packages are built by the release pipeline |
| The local store | `crates/termru-store` | Implemented, tested, backed up and restored |
| The SSH engine | `crates/termru-ssh` | Implemented, tested against a real OpenSSH server |
| The cloud server | `apps/cloud-server` | Implemented, tested against a real PostgreSQL, shipped as a container image |

## The evidence

Everything below was run on the release machine. The commands are the same
ones the CI runs.

| Check | Command | Result |
|---|---|---|
| Format | `cargo fmt --check` | clean |
| Lints | `cargo clippy --workspace --all-targets -- -D warnings` | clean |
| Tests | `cargo test --workspace` | 210 passed, 0 failed |
| Frontend lint | `npm run lint` (prettier + svelte-check) | 0 errors, 0 warnings |
| Frontend tests | `npm run test` | 91 passed |
| Frontend build | `npm run build` | green |
| Supply chain | `cargo deny check` | advisories, bans, licences, sources: ok |
| Action pins | `scripts/check-action-pins.py` | every pin resolves to a real commit |
| Secrets | gitleaks (CI) | no finding |

The tests that need something the machine does not have skip **loudly**
(a `SKIPPED:` line) and pass; a prerequisite that is present but broken is
a failure.

## The user journeys

| Journey | How it is covered |
|---|---|
| Add a host, connect with a password or a key | The app-layer E2E (`sessions_e2e.rs`) drives the real session manager against a real `sshd`: connect, stream, write, resize, close |
| The host key is unknown or changed | The SSH suite covers the trust flow, the refusal and the changed-key warning; the app defaults to *deny* |
| The connection dies | The reconnect policy is unit-tested (the budget, the backoff, the give-up), and the E2E covers the failure path |
| Tabs, splits, the layout | The pane tree and the store are unit-tested; the contract test drives the split through the real components |
| The settings and the remapped keys | The contract test opens the settings, changes a value and checks the argument shape of `prefs_set` |
| Sign in to a sync server, push, pull, resolve a conflict | The end-to-end test (`sync_e2e.rs`) drives the desktop engine against the **real** server binary: register, push, pull with a second device |
| The cloud server is deployed | `compose-smoke.sh` starts the compose stack with the image under test, waits for the readiness, checks that the migration ran, drives a client round trip, backs the database up and restores it |
| The packages are installable | The release pipeline installs the `.deb` with apt and checks the declared dependencies, extracts the AppImage and checks the PE header of the NSIS installer; the `.rpm` is queried with the `rpm` tools (the runner has no `dnf`, so it is not installed there) |

## The decisions that are still open

| Area | Decision | Where it is recorded |
|---|---|---|
| SSH transport | Pure Rust (`russh`), no system OpenSSH | ADR 0002 |
| Desktop stack | Tauri 2 + Svelte 5 | ADR 0001 |
| Persistence | SQLite locally, PostgreSQL for the cloud | ADR 0003 |
| Secrets | OS keyring only, never synced | ADR 0004 |
| Sync | Per-user revisions, explicit conflicts, no silent LWW | ADR 0005 |
| Session ownership | One owner in Rust, across the windows | ADR 0006 |
| Licence | Apache-2.0, permissive dependencies | ADR 0007 |

## The residual risks

The threat model (`docs/security/threat-model.md`) lists them per surface;
the short version:

* Trust on first use: the first connection to an unknown host is a decision
  the user makes without a second source of truth.
* The local database is not encrypted (file permissions only).
* The webview is a browser: the content security policy is strict and the
  frontend inserts no raw markup, but the engine is a large surface.
* The packages are not signed: `SHA256SUMS` is the trust anchor.
* The cloud database connection is expected to live on a private network.

## Before the tag

A maintainer has to do these by hand; nothing in the repository can do them
for you:

1. **The visual pass.** Drive the app on a real desktop session: add a
   host, connect, resize, split a pane, move a tab to a new window, store a
   password, use the palette, rebind a key, sign in to a sync server. The
   build machine has no window manager, so this has never been done.
2. **The platforms.** Build and launch the packages on Windows, Ubuntu and
   Fedora. Only Linux was built and tested here.
3. **The push.** Tag `v0.1.0` and push it: the pipeline validates the tag,
   runs the tests, builds the packages, installs them on the runners,
   publishes the image to `ghcr.io` and uploads the assets.
4. **The verification.** Check the release page (the assets and the
   `SHA256SUMS`) and the image tag, then update the version references in
   `docs/install.md` and `docs/cloud/deployment.md` if they differ from the
   tag.

## The known gaps of this release

They are listed in full in [limitations.md](limitations.md). The ones a user
is most likely to notice:

* the second window can only show a moved tab (no per-window tab bar);
* the restored tabs are placeholders (the user reconnects);
* no port forwarding, no jump hosts, no snippets;
* the packages are not signed.
