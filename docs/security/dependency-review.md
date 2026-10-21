# Dependency review

* Date: 2026-10-21
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
| `russh` | 0.64 | Apache-2.0 | SSH transport (we do not shell out to OpenSSH) | Pure Rust; gives us full control of the host-key UX |
| `ssh-key` / `rsa` | 0.7.0-rc.11 / 0.10.0-rc.18 | Apache-2.0 / MIT | RSA host-key and user-key support inside `russh` | **RUSTSEC-2023-0071** (Marvin timing attack) — see below |
| `rusqlite` | 0.3x | MIT | Local persistence (bundled SQLite) | WAL mode, `0600` file permissions |
| `keyring` | 4.2 | MIT | OS credential store (Secret Service / Credential Manager / Keychain) | Its Linux backend pulls `zbus`/`zvariant`, which are currently *unmaintained* (RUSTSEC-2026-037x) — transitive only, tracked here |
| `tokio` | 1.x | MIT | Async runtime for the session actors and the Tauri runtime | |
| `serde` / `serde_json` | 1.x | MIT / Apache-2.0 | Wire format for IPC, the database and the sync payload | `deny_unknown_fields` is **not** used for the IPC types, so a newer frontend can talk to an older backend |
| `tracing` (+ `tracing-subscriber`, `tracing-appender`) | 0.1 / 0.3 / 0.2 | MIT | Structured logging (stderr + rotating file) | The logging audit test proves no secret reaches a log file |
| `zeroize` | 1.x | Apache-2.0 / MIT | Wipes secret buffers on drop | Used by `SecretValue` |
| `async-trait` | 0.1 | MIT / Apache-2.0 | Object-safe async traits (`HostKeyPrompt`) | |
| `base64` | 0.22 | MIT / Apache-2.0 | Terminal payload encoding over IPC | |
| `uuid` | 1.x | MIT / Apache-2.0 | Typed identifiers | |

### Known advisories

**RUSTSEC-2023-0071 — Marvin attack (timing side channel) in `rsa` 0.10.0-rc.18.**
`rsa` enters the tree through `russh` → `ssh-key`, which uses it for RSA host
keys and RSA user authentication. No fixed release exists: `russh` 0.64.1
(the latest) still depends on `ssh-key` 0.7.0-rc.11.
*Impact:* the attack recovers RSA **private** keys through timing
measurements of the local RSA implementation. Termru does not use RSA user
authentication by default (Ed25519 keys and the SSH agent are the norm), and
the attacker must already be local and able to time our own operations.
*Action:* ignored in `deny.toml` with a review date (2027-01-01). Revisit
when `russh` updates `ssh-key`.

**RUSTSEC-2026-0370/0371/0372 — `zvariant`, `zbus_names`, `zvariant_derive`
are unmaintained.** They arrive transitively through `keyring`'s Linux
Secret Service backend (`zbus`). They are build/runtime plumbing of a
credential-store client, not part of our own logic, and no vulnerability is
reported for them. `keyring` 4 does not expose a per-backend feature that
would let us drop `zbus` without downgrading the crate, so the crates stay
and the advisories are tracked here. Revisit when `keyring` restructures its
backends.

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

`npm audit` reports **0 vulnerabilities** at the time of writing.

## Update strategy

* Dependabot opens grouped PRs (weekly). Security updates are prioritised.
* The SSH engine is the most sensitive dependency: `russh` updates can
  change wire behaviour, so the integration suite against a real OpenSSH
  server must pass before merging.
* Version bumps that change the lockfile substantially should be reviewed
  with `cargo deny check` and `npm audit` before merging.
