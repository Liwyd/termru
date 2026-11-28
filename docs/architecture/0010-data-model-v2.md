# ADR 0010: Data model v2: jump hosts, port forwards, environment, startup command, agent forwarding, authentication order

* Status: Accepted
* Date: 2026-11-28
* Deciders: Termru maintainers

## Context

A host was an address, a user, one authentication method and a few terminal
settings. Real use needs more: reaching a host through a bastion, forwarding a
port to a database, setting an environment variable for the session, running a
command when the shell starts, letting the server use the local agent, and
trying methods in an order other than the default. People also arrive with an
`~/.ssh/config` that already says all of it.

The host is the synced entity (ADR 0005), so every addition has to load old
records and old sync payloads, survive an older app editing the same record
(see the consequences), and be validated the same way on every device and on
the server.

## Decision

### The model (`termru-core`, `Host`)

All new fields use serde defaults, and **an unused field is not serialized**:
a host that uses none of them produces the same JSON as before, so syncing it
cannot look like an edit to an older device.

| field | meaning |
| --- | --- |
| `jump_chain: Vec<HostId>` | hosts to pass through, in order; each is another host of the library |
| `forwards: Vec<Forward>` | `kind` (`local`/`remote`/`dynamic`), `bind_address?`, `bind_port`, `target_host?`, `target_port?`, `enabled` |
| `env: Vec<EnvVar>` | `name`/`value` pairs sent with `SetEnv` before the shell |
| `startup_command: Option<String>` | typed into the shell once it starts |
| `agent_forward: bool` | let the server use the local agent (off by default) |
| `auth_order: Option<Vec<AuthKind>>` | `None` = the default chain derived from `auth` |

### Validation (`termru-core::validate`)

* **Addresses** accept IPv6 literals, bare (`fe80::1`, canonical) or in
  brackets (`[fe80::1]`), with an optional zone id (`fe80::1%eth0`, `%3`).
  Link-local addresses cannot be reached without one. Strings that were valid
  before stay valid (an old synced record must not fail on another device);
  the new forward fields are strict. The connection code strips brackets.
* **Forwards** are validated per kind: `local`/`remote` need a target host and
  port, `dynamic` has none. A listener on this machine (`local`, `dynamic`)
  binds an IP literal or `localhost`; a `remote` bind address names an address
  on the server. No bind address means loopback. At most 32 forwards, and no
  two may claim the same listening address and port.
* **Environment** names follow `[A-Za-z_][A-Za-z0-9_]*`; values contain no
  control characters; at most 32 variables, names unique.
* **Startup command**: one line, no control characters except tab, at most
  1024 characters.
* **Authentication order**: not empty, each kind at most once.
* **Jump chain**, in two layers. `Host::validate` (no lookup, so it also runs
  in sync and on the server) checks the length (at most 8), self-reference
  and duplicates. `resolve_jump_chain` (given a lookup of hosts) derives the
  full route and refuses missing hops, cycles and routes longer than 8. It
  runs on every local save and whenever a connection is built. It is **not**
  part of `Host::validate`, because sync delivers records in any order: a hop
  may arrive after the host that names it.

### The route

A host's `jump_chain` is the list of hops to pass through. The first hop is
reached from this machine, and **its own `jump_chain` is followed first**, as
OpenSSH does for `ProxyJump`: reaching `app` through `bastion`, where
`bastion` sits behind `gateway`, goes `gateway → bastion → app`. The later
hops of a chain are reached from the hop before them, so their own chains are
not used. The route may pass a host only once.

### Storage (`termru-store`, migration 2)

Six columns on `hosts`, with defaults that make every existing row a plain
host: `jump_chain`, `forwards`, `env`, `auth_order` as JSON text (like `auth`),
`startup_command` as text, `agent_forward` as 0/1. The migration runs when the
database is opened inside one transaction; a database from a newer release is
still refused. Tests build a database from a frozen copy of the v1 schema.

A local save checks the route against the store (hops exist, no cycle). Setting
the favorite flag does not: starring a host must not fail because a jump host
was deleted since. `Hosts::import` stores identities, key bindings and hosts of
an import in one transaction.

### Sync

The cloud server stores host payloads as opaque JSON after running the same
entity validation (`termru-core`), so it needs no change: the new fields
round-trip, and invalid ones are refused on the way in. `CLIENT_SCHEMA_VERSION`
stays 1: the change is additive.

### The engine (`termru-ssh`, see also ADR 0002)

* **Jump hosts**: every hop is dialed or reached through a `direct-tcpip`
  channel of the hop before it, runs its own key exchange with its own
  host-key decision, and authenticates with its own chain. A jump host gets
  nothing else: no environment, no forwards, no startup input and never the
  agent.
* **Forwards** start after `Ready`, report `Listening`/`Failed` per forward as
  a session event, are torn down (sockets closed) before the session reports
  its end, and are started again by a reconnect.
* **Environment**: each variable is a `SetEnv` request with a reply; a refusal
  (the server's `AcceptEnv`) is logged at debug level, name only, and ignored.
* **Startup command**: sent as input to the interactive shell, followed by
  Enter, first in the ordered input queue. It is not `exec`: the session stays
  a normal terminal. It is sent again by every reconnect (a new shell).
* **Agent forwarding**: `auth-agent-req` on the target's channel only; the
  server's agent channels are proxied to the local agent socket (Unix socket,
  or the OpenSSH agent's named pipe on Windows).
* **Known hosts**: the user's `~/.ssh/known_hosts` is a **read-only** extra
  source. Rules: `@revoked` anywhere refuses; the app's own file wins once it
  has an entry for the host (it holds what the user decided here); otherwise a
  key the system file knows is trusted without a prompt, and a different key of
  the same type is a changed key (the prompt's "accept" writes to the app's
  file only). An unreadable system file is skipped.
* **Authentication order**: with `auth_order`, the chain contains exactly the
  listed methods the host has the means for, in that order (no implicit
  keyboard-interactive); a password listed for a password host is required.

### Importing `~/.ssh/config`

`termru_ssh::ssh_config` is a pure parser (`Host` patterns with `!` and
wildcards, first value wins per option, `IdentityFile` cumulative, `HostName`
with `%h`, `User`, `Port`, `ProxyJump`, `Include` with a depth and file limit,
`Match` skipped with a warning). The desktop reads the file, resolves
includes, plans the import (a host that exists is recognised by address, user
and port and reused; a key path that is already bound reuses its identity;
`ProxyJump` aliases become `jump_chain`; a host that cannot be valid is skipped
with the reason and takes nothing half-built with it) and stores everything in
one transaction. Key files are only referenced by path.

## Security considerations

* **Synced fields that act.** `startup_command`, `agent_forward`, `forwards`
  (a listener on a non-loopback address) and `env` do things when a host is
  connected to. They are part of the synced record, so whoever can write to the
  sync account can change what a connection does on every device. This is the
  same trust placed in synced addresses and users (a changed address already
  redirects a connection), but these fields reach further. Mitigations in this
  version: strict validation everywhere, loopback by default, agent forwarding
  off by default, and the fields are shown in the host editor. A device-local
  "do not apply synced startup commands or agent forwarding" setting is an
  open item (TODO.md).
* **Agent forwarding** lets the server's administrators (and anyone who can
  read the socket it creates) use the agent for as long as the session lives.
  It is off by default, requested only on the target and never on a jump host.
* **A SOCKS forward has no authentication.** It is loopback-only unless a bind
  address says otherwise; the user who exposes it owns that choice.
* **Environment values are synced in plain text.** They are no place for
  secrets.
* **The system known_hosts is trusted** for hosts the app has no entry for.
  That is the point of the feature, and it means the user's `ssh` history
  vouches for keys here.

## Consequences

* An app version from before this change that edits a host drops the new
  fields from its copy and pushes that. There is no unknown-field
  preservation. Documented in `docs/limitations.md`.
* The jump route is resolved when the connection is built, not stored: moving a
  bastion to another address updates every host behind it.
* Deleting a jump host leaves hosts that name it with a clear error at connect
  time (and a refused edit), not a silent direct connection.
* A hop's own `jump_chain` is only followed for the first hop; a deliberately
  explicit route (`ProxyJump a,b`) stays explicit.

## Alternatives considered

* **Store the resolved route** (a flat list of hop ids): copies stale data and
  needs a recompute on every edit of every hop.
* **Always follow every hop's own chain**: a route like `a,b` would grow
  hidden hops, and a cycle check becomes a graph search with no use.
* **A single `exec` startup command**: loses the interactive shell, which is
  the whole point of the tab.
* **Trust the system known_hosts only with a user setting**: safer, but the
  requested behavior is that a key `ssh` already trusts does not prompt again.
  The rules above keep every mismatch blocking.
