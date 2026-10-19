# 0002. SSH transport: `russh` instead of spawning system OpenSSH

* Status: Accepted
* Date: 2026-10-19

## Context

Termru must connect to SSH servers with:

* first-class, app-owned host-key trust UX (trust-on-first-use prompt,
  hard warning with old/new fingerprints on change, `@revoked` support) —
  not the text-parsing of OpenSSH's stderr;
* three auth methods: password, private key file (+ passphrase), SSH agent;
* PTY allocation, window-change resize, keepalives, bounded output
  buffering so a stalled UI never blocks the transport;
* identical behavior on Linux and Windows (Windows has no system OpenSSH
  guarantee, and spawning it means parsing/pty emulation differences);
* a viable path to non-desktop targets later.

## Decision

Use **`russh` 0.64 (pure Rust)** as the SSH client implementation, in
`crates/termru-ssh`, with system OpenSSH as *test fixtures only* (a local
`sshd` in integration tests).

Concrete design choices that follow:

* **Known-hosts**: implement OpenSSH `known_hosts` read/write ourselves,
  including hashed entries (`|1|…`), wildcards, negation, `@cert-authority`
  / `@revoked` markers. Evaluation outcomes are typed
  (`Known` / `Unknown` / `Changed` / `Revoked`) and surfaced to the UI;
  `Changed` and `Revoked` are hard refusals that require explicit user
  action — never auto-accepted, never downgraded to a warning-only path.
* **Auth secrets** are supplied per connection by the caller via
  `SecretValue` (zeroizing wrapper); the SSH crate never persists them.
* **Concurrency**: one tokio actor task owns the russh handle and channel;
  commands (`Write`/`Resize`/`Attach`/`Close`) and SSH messages funnel
  through it, so ordering (replay buffer before live bytes) is trivially
  correct.
* **Dead-connection detection**: russh keepalives (default 3 unanswered →
  connection closed and reported as a network failure).
* **Bounded output path**: at most one attached consumer through a bounded
  channel with send timeout; a stalled UI causes a *detach*, with data
  continuing into a bounded replay buffer (default 512 KiB) — the SSH
  transport is never blocked by UI slowness.
* **Error taxonomy**: `SshError` with user-actionable messages and
  classification helpers (`is_retriable`, `is_auth_failure`,
  `ErrorCategory`) used to gate automatic reconnects.

## Consequences

* Protocol and algorithm coverage = russh's. Required algorithms (curve25519
  KEX, ed25519/rsa host keys, `ssh-ed25519`/`rsa-sha2` user keys, aes-gcm/
  chacha20-poly1305 ciphers) are verified by integration tests against a
  stock OpenSSH server; anything OpenSSH-only and unsupported is out of scope.
* We own known-hosts file compatibility: it must interoperate with files
  written by real OpenSSH (and vice versa). Covered by unit tests against
  real-world formats.
* No dependency on a system OpenSSH binary at runtime → smaller attack
  surface (no subprocess, no argv injection) and identical UX on all
  platforms.
* SSH agent support depends on `SSH_AUTH_SOCK` (Unix) / pageant-style
  sources where russh supports them; graceful degradation with an explicit
  error when unavailable.

## Alternatives considered

* **Spawn system OpenSSH with a PTY**: less code, but host-key trust UX and
  auth prompts require scraping stderr/stdout; behavior differs per platform
  and per `ssh_config`; harder to unit-test; subprocess attack surface.
* **`ssh2`/libssh2 bindings**: C dependency (build-tool risk in constrained
  CI), weaker async integration, no advantage in trust UX.
* **Implement the SSH protocol**: unjustified complexity.
