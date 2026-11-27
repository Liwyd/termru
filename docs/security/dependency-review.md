# Dependency review

* Date: 2026-10-21 (advisory sections updated 2026-11-27)
* Scope: the crates and npm packages that carry the most risk or weight in
  Termru. The complete machine-readable inventory is `Cargo.lock` /
  `package-lock.json`; `cargo deny` gates licenses, advisories, sources and
  duplicate versions in CI.

## Policy

| Rule | Reason |
|---|---|
| Versions are pinned by the committed lockfiles | Reproducible builds; `cargo deny` reports wildcard *requirements* as a warning, not an error |
| Unmaintained-crate advisories are fatal only for **direct** dependencies | A transitive unmaintained crate is informational; it is tracked here instead of silently accepted |
| Every advisory ignore has a reason and a review date | Ignores must be revisited, not inherited silently |
| Dependabot watches cargo, npm and GitHub Actions | Patches arrive as PRs, not as surprises |
| `cargo deny` and `npm audit` run in CI | A new advisory fails the build |

## Significant Rust dependencies

| Crate | Version | Licence | Why we depend on it | Notes |
|---|---|---|---|---|
| `tauri` | 2.12 | Apache-2.0 / MIT | Desktop shell (window management, IPC, webview) | Stable 2.x on purpose; 3.0 is alpha |
| `russh` | 0.65 | Apache-2.0 | SSH transport (we do not shell out to OpenSSH) | Pure Rust; gives us full control of the host-key UX |
| `ssh-key` / `rsa` | 0.7.0-rc.12 / 0.10.0 | Apache-2.0 / MIT | RSA host-key and user-key support inside `russh` | `rsa` 0.10.0 carries the constant-time fix for RUSTSEC-2023-0071 (resolved, see below) |
| `rusqlite` | 0.41 | MIT | Local persistence (bundled SQLite via `libsqlite3-sys` 0.39) | WAL mode, `0600` file permissions |
| `chrono` | 0.4 | MIT / Apache-2.0 | Timestamps in the core types, the store, the cloud server (`tokio-postgres`) and the desktop app | **RUSTSEC-2026-0543** (unmaintained) — accepted, see below |
| `keyring` | 4.2 | MIT | OS credential store (Secret Service / Credential Manager / Keychain) | Its Linux backend pulls `zbus`/`zvariant`, which are currently *unmaintained* (RUSTSEC-2026-037x) — transitive only, tracked here |
| `tauri-plugin-opener` | 2.7 | Apache-2.0 / MIT | Opens http, https and mailto links from terminal output in the system browser | Scoped by `capabilities/default.json`; pulls `open`, `is-wsl` and `is-docker` (MIT); `zbus` and the platform crates were already in the tree |
| `tokio` | 1.x | MIT | Async runtime for the session actors and the Tauri runtime | |
| `serde` / `serde_json` | 1.x | MIT / Apache-2.0 | Wire format for IPC, the database and the sync payload | `deny_unknown_fields` is **not** used for the IPC types, so a newer frontend can talk to an older backend |
| `tracing` (+ `tracing-subscriber`, `tracing-appender`) | 0.1 / 0.3 / 0.2 | MIT | Structured logging (stderr + rotating file) | The logging audit test proves no secret reaches a log file |
| `zeroize` | 1.x | Apache-2.0 / MIT | Wipes secret buffers on drop | Used by `SecretValue` |
| `async-trait` | 0.1 | MIT / Apache-2.0 | Object-safe async traits (`HostKeyPrompt`) | |
| `base64` | 0.22 | MIT / Apache-2.0 | Terminal payload encoding over IPC | |
| `uuid` | 1.x | MIT / Apache-2.0 | Typed identifiers | |

### Known advisories

**RUSTSEC-2026-0543 — `chrono` is unmaintained.**
`chrono` is a *direct* workspace dependency (`termru-core`, `termru-store`,
`termru-cloud-server`, `termru-desktop`), so the `unmaintained = "workspace"`
policy makes the advisory fatal. The advisory is informational: no
vulnerability is reported and no patched release exists ("all versions are
affected"). The recommended replacements are `jiff` and `time`.
*Why accepted for now:* the crate is stable and widely used, we only use it
for UTC timestamps (epoch milliseconds in SQLite, RFC 3339 through `serde` in
sync payloads and IPC), and `tokio-postgres` (cloud server) maps its
timestamp columns onto `chrono` types, so removing it is a cross-cutting
change that does not belong in a security patch.
*Mitigation / plan:* ignored in `deny.toml` with a review date (2027-03-01).
Before that date evaluate migrating to `jiff` (or `time`), keeping the
existing wire formats stable (epoch milliseconds in the local database,
RFC 3339 in sync payloads, so old databases and peers keep working) and
replacing the `tokio-postgres` `with-chrono-0_4` feature.
Tracked in `TODO.md`. If `chrono` is revived or a vulnerability is reported,
revisit earlier.

**RUSTSEC-2026-0370/0371/0372 — `zvariant`, `zbus_names`, `zvariant_derive`
are unmaintained.** They arrive transitively through `keyring`'s Linux
Secret Service backend (`zbus`). They are build/runtime plumbing of a
credential-store client, not part of our own logic, and no vulnerability is
reported for them. `keyring` 4 does not expose a per-backend feature that
would let us drop `zbus` without downgrading the crate, so the crates stay
and the advisories are tracked here. Revisit when `keyring` restructures its
backends.

### Resolved advisories

| Advisory | Crate | Fixed by |
|---|---|---|
| RUSTSEC-2023-0071 (Marvin timing attack) | `rsa` | `russh` 0.65 pulls `rsa` 0.10.0 (patched `>= 0.10.0`); the `deny.toml` ignore was removed |
| RUSTSEC-2026-0495 (agent server signs with confirm-constrained keys) | `russh` | `russh` 0.65 (`>= 0.65.0`); Termru only uses the agent *client* |
| RUSTSEC-2026-0459 (C SQLite CVE-2026-119375) | `libsqlite3-sys` | `rusqlite` 0.41 (`libsqlite3-sys` 0.39) |
| RUSTSEC-2026-0592 (stack overflow in `Url::origin()`) | `url` | `url` 2.5.9 |
| RUSTSEC-2026-0594 (private cookie nonces can repeat after fork) | `cookie` | `cookie` 0.18.3 |

### Licences

`cargo deny` allows MIT, Apache-2.0 (with and without the LLVM exception),
BSD-2/3-Clause, ISC, Unicode-3.0, Zlib, MPL-2.0, CC0-1.0, 0BSD and
Unlicense. GPL-family licences are **not** allowed: the application must be
distributable under Apache-2.0 without copyleft obligations.

## Significant npm dependencies

| Package | Version | Licence | Why |
|---|---|---|---|
| `svelte` | 5.57 | MIT | UI framework (runes) |
| `vite` | 8.3 | MIT | Build/dev server |
| `typescript` | 5.9 | Apache-2.0 | Type checking (`svelte-check`) |
| `@xterm/xterm` (+ `addon-fit`, `addon-webgl`) | 6.0 | MIT | Terminal emulator (a proven implementation, not a reimplementation) |
| `@tauri-apps/api` / `@tauri-apps/cli` | 2.12 | MIT / Apache-2.0 | IPC client and dev tooling |
| `vitest` | 5.0 | MIT | Frontend tests |
| `jsdom` | latest | MIT | DOM environment for the UI contract test |
| `prettier` (+ `prettier-plugin-svelte`) | 3.9 | MIT | Formatting |
| `@tauri-apps/plugin-dialog` | 2.8 | MIT / Apache-2.0 | Native file picker for key files |
| `@tauri-apps/plugin-opener` | 2.7 | MIT / Apache-2.0 | Opens a link from terminal output (scoped to http, https and mailto by the capability) |

`npm audit` reports **0 vulnerabilities** at the time of writing.

### Fonts (runtime assets)

| Package | Version | Licence | Why |
|---|---|---|---|
| `@fontsource-variable/inter` | 5.3.0 | SIL OFL 1.1 | The UI typeface (Doc A.03), self-hosted |
| `@fontsource/ibm-plex-mono` | 5.3.0 | SIL OFL 1.1 | The terminal typeface (Doc A.03), self-hosted |

Both are **bundled into the app**, not fetched at run time: the webview CSP
is `font-src 'self'`, and the hardening test fails the build when any
`src`/`href` points at a remote origin. The fontsource packages ship the
`@font-face` declarations plus the `.woff2` files and nothing else (no
JavaScript); they are MIT/OFL build inputs reviewed like any other
dependency. The SIL OFL licence allows embedding and redistribution
without modification of the font itself.

## Update strategy

* Dependabot opens grouped PRs (weekly). Security updates are prioritised.
* The SSH engine is the most sensitive dependency: `russh` updates can
  change wire behaviour, so the integration suite against a real OpenSSH
  server must pass before merging.
* Version bumps that change the lockfile substantially should be reviewed
  with `cargo deny check` and `npm audit` before merging.
