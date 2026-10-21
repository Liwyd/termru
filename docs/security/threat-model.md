# Termru threat model

* Status: living document (updated with every architectural change)
* Date: 2026-10-21
* Scope: the desktop application (`apps/desktop`), the Rust crates it uses
  (`termru-core`, `termru-store`, `termru-ssh`) and — when it exists — the
  optional cloud sync server.

## 1. Assets

| Asset | Where it lives | Why it matters |
|---|---|---|
| SSH private keys | User's filesystem, referenced by path | Grants access to the remote hosts |
| Passwords / key passphrases | OS credential store (`keyring`) | Grants access to the remote hosts |
| Host profiles (address, user, port) | Local SQLite database | Reveals the infrastructure of the user |
| Known-hosts entries | `known_hosts` file in the app data dir | The trust anchor for host identity |
| Terminal output | Memory, scrollback buffer, screen | Can contain secrets printed by remote tools |
| Sync payload (future) | Local DB queue + server database | Infrastructure metadata, not secrets |

**Never an asset in our storage:** secrets. The database schema has no field
that can hold a password or passphrase, and the sync payload never contains
one (see ADR 0004).

## 2. Trust boundaries

```
┌──────────────────────── user account (host OS) ────────────────────────┐
│                                                                       │
│  ┌──────────── webview (untrusted-ish content) ────────────────┐      │
│  │  Svelte UI + xterm  ← renders remote output (attacker data) │      │
│  └───────────────┬────────────────────────────────────────────┘      │
│                  │ Tauri IPC (capabilities: core:default)            │
│  ┌───────────────▼────────────────────────────────────────────┐      │
│  │  Rust app process: commands → SessionManager → termru-ssh   │      │
│  └───────┬──────────────────────────────┬─────────────────────┘      │
│          │                              │                            │
│   ┌──────▼───────┐            ┌─────────▼──────────┐                 │
│   │ SQLite (0600)│            │ OS credential store│                 │
│   └──────────────┘            └────────────────────┘                 │
│                                                                       │
│  ══════════════════ network (untrusted) ══════════════════            │
│          │                                                             │
│   ┌──────▼──────────────────────────────────────────────┐              │
│   │ remote SSH server (hostile or impersonated possible)│              │
│   └─────────────────────────────────────────────────────┘              │
└───────────────────────────────────────────────────────────────────────┘
```

Boundary notes:

* **Webview ↔ Rust**: the webview runs our own bundle, but it *displays
  remote output*. Remote output is untrusted input that reaches the DOM
  (as text) and the terminal emulator.
* **Rust ↔ network**: the SSH server is untrusted. It supplies the host key,
  the key-exchange parameters and the terminal stream.
* **Rust ↔ OS credential store**: the store is a system service; we trust it
  as much as the OS.
* **Rust ↔ filesystem**: the local database and the `known_hosts` file are
  user-writable; an attacker with the user's account can modify them (that
  is the same trust level as the SSH keys themselves).

## 3. Attack surfaces and mitigations

### 3.1 Host identity (the highest-value target)

An attacker who can intercept the first connection (ARP/DNS spoofing, hostile
Wi-Fi, a compromised router) could impersonate a server and capture the
password or the private-key authentication attempt.

Mitigations implemented:

* The host key is always verified against `known_hosts` before
  authentication; there is no "accept anything" path in the product code
  (`AcceptAllHostKeys` exists only for automated tests).
* First use (TOFU) is explicit: the user is shown the key type and the
  `SHA256:` fingerprint and must confirm.
* A **changed** key is a hard warning with the old and the new fingerprint;
  the user must explicitly accept the change, and the acceptance replaces the
  stored entry.
* Host-key prompts are **default-deny**: a timeout, a dropped prompt or an
  unknown prompt id results in rejection, never in trust.
* `known_hosts` parsing follows the OpenSSH format, including hashed
  entries, wildcards, negations and `@revoked`.

Residual risk: TOFU protects the *second* connection onwards. The first
connection to a host relies on the user verifying the fingerprint out of
band. Distributing pre-verified host keys (or a CA-based model) is out of
scope for v1 and is the main residual risk in this area.

### 3.2 Authentication material

* Passwords and passphrases are only ever in memory or in the OS credential
  store. `SecretValue` zeroizes on drop, and its `Debug` implementation is
  redacted.
* Secrets are never written to the database, never included in the sync
  payload and never logged. A test (`log_files_never_contain_secret_material`)
  greps the log files for secret values.
* A secret supplied for a connection is used for that connection and for its
  reconnects; the frontend drops its copy as soon as the credential store
  holds it.
* If no credential store is available, the app degrades to asking per
  connection instead of storing anything weaker.

Residual risk: the process memory of a running app contains the credentials
of its live sessions. A local attacker with the user's privileges can read
them; this is accepted (the same is true for every SSH client).

### 3.3 Local storage

* The SQLite database is created with `0600` permissions on POSIX.
* WAL mode, foreign keys on, `busy_timeout` for concurrent access.
* Migrations are forward-only and refuse to open a database written by a
  newer version.
* Backups are written to a user-chosen path with the same permission
  discipline; a restore validates the file before replacing anything.

Residual risk: the database is **not encrypted at rest**. Host profiles are
metadata (not secrets), but they describe the user's infrastructure. A
full-disk-encryption recommendation is part of the install documentation.

### 3.4 IPC and the webview

* The Tauri capability set is minimal (`core:default`): no filesystem, shell,
  HTTP or opener permissions. The application registers no plugin
  permissions.
* A Content-Security-Policy is set for the production bundle.
* Every command validates its input: terminal geometry bounds, host and
  identity validation (shared with the sync path), secret-kind whitelist,
  absolute key paths, unknown command arguments are rejected.
* The webview never loads remote content; all assets are bundled.

Residual risk: a cross-site-scripting bug in the frontend would execute in
the webview. The IPC surface is limited by the capabilities, but a
compromised webview could still read what the UI shows. The frontend renders
remote output only as text (xterm canvas/DOM text nodes), never as HTML.

### 3.5 Terminal output handling

Remote output is untrusted. It is passed to the terminal emulator as bytes
and never interpolated into HTML by the app. The scrollback is bounded
(512 KiB in the engine, configurable in the UI).

### 3.6 Session ownership and multi-window

* The application process owns all sessions; windows are views. A tab moved
  to another window attaches to the same session (no duplicate connection,
  no double-free of the transport).
* Views that disappear are detected and removed; a session without any view
  is closed (no orphan connections).
* Cancellation is bounded: the reconnect policy caps attempts and the backoff
  is capped, so a flapping server cannot cause a reconnect storm.

### 3.7 Build and supply chain

* `cargo deny` checks licenses, advisories and duplicate/banned crates.
* Dependabot watches cargo, npm and GitHub Actions.
* gitleaks scans the repository for secrets.
* CI actions are referenced by major version today; they should be pinned to
  commit SHAs (see the residual risks).

### 3.8 Cloud sync (not implemented yet)

When the sync server exists, its surface (authentication, per-user
isolation, rate limiting, conflict handling) is described in ADR 0005 and
must be reviewed again before it ships. No server-side secret storage is
planned.

## 4. Out of scope

* Vulnerabilities in dependencies without a Termru-specific aggravating
  factor (report them upstream; we track advisories through `cargo deny` and
  Dependabot).
* Physical access to an unlocked machine.
* A malicious user against their own installation.

## 5. Residual risks (summary)

| Risk | Severity | Mitigation today |
|---|---|---|
| First-use MITM (TOFU without out-of-band verification) | High | Explicit fingerprint confirmation; documented limitation |
| Local attacker reads process memory of a live session | Medium | Accepted (same as every SSH client); secrets zeroized on drop |
| Local database is not encrypted | Low–Medium | 0600 permissions; documentation recommends disk encryption |
| XSS in the frontend executes in the webview | Medium | Minimal capabilities, CSP, text-only rendering of remote data |
| CI actions referenced by tag, not by SHA | Low | Pin before the release pipeline is enabled |
| No code signing of release artifacts | Medium | Documented as a trust limitation; to be addressed in Phase 12 |
| Single instance not enforced (two processes may share the database) | Low | SQLite WAL tolerates it; a single-instance plugin is planned |

## 6. Reporting a vulnerability

See [SECURITY.md](../../SECURITY.md): use GitHub's private vulnerability
reporting, not a public issue.
