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

Use **`russh` 0.65 (pure Rust)** as the SSH client implementation, in
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
* **Concurrency**: one tokio actor task owns the russh handle and the
  *read half* of the shell channel; the *write half* belongs to two small
  tasks — an input pump draining an ordered, bounded input queue, and a
  resize pump (latest size wins). The actor never awaits anything on the
  russh side, so it keeps draining output (which is what lets russh process
  the window adjusts a large paste is waiting for), and resize/close never
  queue behind a blocked write.
* **Dead-connection detection**: russh's own keepalive is the single source
  (default 30 s, 3 unanswered → transport closed), plus TCP keepalive and
  `TCP_NODELAY` on the socket. A host that stores `0` uses the default:
  keepalives cannot be switched off entirely.
* **Exit semantics**: only a transport that vanished *without* an EOF, exit
  status or channel close from the remote is a retriable network failure.
  Shell exit, exit-signal (`kill -9 $$`), remote EOF/close are final
  (`Closed` with a readable reason, never reconnected).
* **Connect deadline**: one budget covers DNS, TCP (addresses raced with a
  250 ms stagger, first success wins), key exchange and shell setup; it is
  paused while a host-key prompt waits for the user, and everything honors
  the cancel token. Authentication has its own timeout.
* **Bounded output path**: one consumer through a bounded channel; the actor
  never drops it because it is slow — events wait in a small pending queue
  (reading pauses when it is full, i.e. ordinary flow control) while output
  also fills a bounded, offset-counting replay ring (default 512 KiB). The
  desktop manager keeps its own ring next to the view list so snapshot and
  view registration are atomic (no gap, no duplicate).
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

## Amendment 2026-11-28: jump hosts, port forwards, session setup

The decisions above stand. ADR 0010 adds what a host may ask of its session;
this is how the transport carries it.

* **Jump hosts** (`jump.rs`). The first hop is dialed; every later hop and the
  target are reached through a `direct-tcpip` channel opened on the hop before,
  turned into a stream and handed to `russh::client::connect_stream`. Each hop
  has its **own** key exchange, host-key decision (known_hosts, the trust
  prompt, `@revoked`), keepalive and authentication chain; a jump host cannot
  vouch for what lies behind it. One cancellation signal covers the whole
  chain; each hop runs under its own connect budget and authentication
  timeout, so the worst case is that of every hop in turn. Errors of a jump
  host are wrapped in `SshError::Hop` (which hop, of how many, its name) and
  keep the category of their cause: a refused password on a jump host is still
  an authentication failure, a dropped link is still retriable, and a
  forwarding ban (`AllowTcpForwarding no`) is final. The jump connections stay
  open as long as the target's, and are closed after it. Automatic reconnects
  start the whole chain again.
* **Port forwards** (`forward.rs`). `local` (`-L`) and `dynamic` (`-D`, a
  minimal SOCKS5 server: no authentication, `CONNECT` only, IPv4/IPv6/domain
  names resolved by the server) listen here; `remote` (`-R`) asks the server to
  listen (`tcpip-forward`) and answers its `forwarded-tcpip` channels by
  connecting to the target, which is connected *before* the channel is
  confirmed. Loopback unless a bind address says otherwise. A forward reports
  `Listening` (with the address actually bound) or `Failed` (with a reason in
  words) as `SessionEvent::Forward`, after `Ready`. They belong to the session
  actor: on any end they are aborted and awaited (listening sockets closed)
  before `Closed` is reported, so a reconnect can bind the same ports. At most
  128 connections per forward.
* **Environment, startup command, agent**. `SetEnv` requests ask for a reply:
  a refusal is logged (name only) and ignored. The startup command is the first
  chunk of the ordered input queue (Enter is `\r`, as a terminal sends it).
  Agent forwarding sends `auth-agent-req` on the target's channel and proxies
  the server's agent channels to the local agent socket; the handler of a jump
  host refuses them.
* **Read-only known_hosts** (`KnownHosts::with_readonly`). See ADR 0010 for the
  rules; the writable file stays the only one that is written.
