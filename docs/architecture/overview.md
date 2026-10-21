# Architecture overview

The decisions are recorded in the [ADRs](README.md). This page ties them
together: what the system is made of, what talks to what, and why the
pieces are split the way they are.

## The shape of the system

```
┌──────────────────────────── desktop app (Tauri 2) ───────────────────────────┐
│                                                                              │
│  webview (Svelte 5)            Rust core                                     │
│  ┌────────────────────┐        ┌──────────────────────────────────────────┐   │
│  │ sidebar, tabs,     │  IPC   │ SessionManager  ── owns every SSH session  │   │
│  │ palette, settings  │◄──────►│   ├─ one task per session                │   │
│  │ xterm.js view      │        │   └─ events fan out to every view        │   │
│  └────────────────────┘        │                                          │   │
│                                │ Store (SQLite)   SyncEngine   Secrets    │   │
│                                │ hosts, groups,   queue, cursor  keyring  │   │
│                                │ tags, workspace  push/pull                │   │
│                                └────────┬───────────────────────┬─────────┘   │
└─────────────────────────────────────────┼───────────────────────┼─────────────┘
                                          │                       │
                                   local SQLite            HTTPS (optional)
                                          │                       │
                                          │              ┌────────▼─────────┐
                                          │              │ cloud server     │
                                          │              │ Axum + PostgreSQL│
                                          │              │ accounts, devices│
                                          │              │ revisions        │
                                          │              └────────┬─────────┘
                                          │                       │
┌─────────────────────────────────────────┼───────────────────────┼─────────────┐
│                                         ▼                       ▼             │
│                              ┌────────────────────┐   ┌────────────────────┐   │
│                              │ SSH server         │   │ PostgreSQL         │   │
│                              │ (russh, no OpenSSH)│   │ (the cloud state)  │   │
│                              └────────────────────┘   └────────────────────┘   │
└──────────────────────────────────────────────────────────────────────────────┘
```

Three things are true at once and they are the reason for the structure:

1. **The app works without the network.** The local store is the source of
   truth; the cloud is an optional mirror of the configuration.
2. **The SSH session belongs to the app, not to a window.** A tab can move
   to another window, and the session keeps running ([ADR 0006](0006-multi-window-session-ownership.md)).
3. **The secrets never leave the machine.** They live in the OS credential
   store, and the sync payload physically cannot carry one
   ([ADR 0004](0004-secret-policy.md)).

## The crates

| Crate | What lives there | Why it is separate |
|---|---|---|
| `termru-core` | The domain models (host, identity, group, tag, preferences), their validation, and the wire types of the sync protocol | Both sides of the wire (the app and the server) depend on it, so the protocol can not drift |
| `termru-store` | SQLite: the schema, the versioned migrations, the repositories, the sync queue, the backup | The persistence rules (dirty flags, tombstones, conflict semantics) are the subtle part of the product; they deserve their own tests |
| `termru-ssh` | The SSH engine: the connection state machine, the authentication, the known-hosts store, the PTY, the reconnect policy | It is the part with the most edge cases (a half-open connection looks like a healthy one), so it is isolated from the UI |
| `termru-desktop` (app) | The Tauri commands, the session manager, the sync engine, the frontend | The boundary between "the engine" and "the product" stays visible |
| `termru-cloud-server` (app) | The HTTP API, the accounts, the devices, the revision counter | The server is optional; the app must build and work without it |

The desktop app and the cloud server are *applications* in the same cargo
workspace, not libraries: they are built and shipped separately.

## The data flow of a change

A local edit (the user renames a host):

1. the frontend calls the Tauri command;
2. the command validates the input and writes the row **and** the sync
   mutation in one transaction (`termru-store`);
3. the row is marked *dirty* — this is what turns a concurrent remote change
   into a conflict instead of a silent overwrite;
4. the frontend refreshes the list.

The mutation sits in the queue until the next sync cycle. The cycle pushes
the queued mutations with their base revision, then pulls everything newer
than the cursor. A mutation that the server has already applied (a retried
request) is answered with the revision of the first application, so a retry
is harmless.

## The boundaries that matter

| Boundary | What crosses it | What never crosses it |
|---|---|---|
| webview ↔ Rust | IPC commands with validated arguments | The webview has no access to the file system, the network or the shell (the capability file grants `core:default` only) |
| app ↔ SSH server | The SSH protocol, a key or a password | Nothing else; there is no agent forwarding, no port forwarding |
| app ↔ cloud | The sync protocol: hosts, groups, tags, preferences | Secrets, private keys, the log files, the workspace geometry of other devices |
| app ↔ OS | The credential store (secrets), the data directory (the database, the logs) | The database never holds a secret |

## Why these decisions

The short version of the reasoning behind each ADR:

* **[0001 Desktop stack](0001-desktop-stack.md)** — a webview gives a
  familiar UI toolkit and a small memory footprint; Tauri keeps the
  privilege boundary in Rust instead of in a browser process.
* **[0002 SSH transport](0002-ssh-transport.md)** — implementing the client
  instead of spawning `ssh` is more work, but it is the only way to give
  the user a real host-key trust flow and to behave the same on every
  platform.
* **[0003 Persistence split](0003-persistence-split.md)** — SQLite for the
  app (a single file, no service to run), PostgreSQL for the server (safe
  concurrent writes, a backup story that already exists).
* **[0004 Secret policy](0004-secret-policy.md)** — no secret synchronization
  in v1. Inventing a recovery mechanism for encrypted secrets would have
  broken the end-to-end guarantee for a feature nobody asked for.
* **[0005 Sync protocol](0005-sync-protocol.md)** — a per-user monotonic
  revision with explicit conflicts. No silent last-writer-wins, no CRDT:
  a configuration is small, and a wrong merge is worse than a question.
* **[0006 Session ownership](0006-multi-window-session-ownership.md)** — one
  owner for every session, so that a tab can move between windows without
  dropping the connection and without two windows claiming the same shell.
* **[0007 Licensing](0007-licensing.md)** — Apache-2.0 for the code, a
  permissive dependency policy, no copyleft in the dependency tree.

## The invariants the tests protect

These are the properties the code must never break. Each one has a test.

* A local edit marks the row dirty, so a concurrent remote change becomes a
  conflict instead of an overwrite.
* A secret can not be pushed: the payload builder has no access to the
  credential store, and the payload has no field that could hold one.
* A session has exactly one owner; closing it releases the SSH channel.
* A changed host key is never accepted automatically.
* The revision of a user is monotonic; the cursor never moves backwards.
* The migrations run once; running them again changes nothing.
