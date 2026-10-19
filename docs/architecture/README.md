# Architecture Decision Records

Numbered ADRs for Termru. Each records one significant decision, its context,
and its consequences. Decisions referenced from code doc comments use these
exact paths.

| ADR | Decision | Status |
| --- | --- | --- |
| [0001](0001-desktop-stack.md) | Desktop stack: Tauri 2 + Svelte 5 + Vite 8 + TypeScript 5 + xterm 6 | Accepted |
| [0002](0002-ssh-transport.md) | SSH transport: `russh` (pure Rust) instead of spawning OpenSSH | Accepted |
| [0003](0003-persistence-split.md) | Persistence: local SQLite, cloud PostgreSQL | Accepted |
| [0004](0004-secret-policy.md) | Secret policy: OS keyring only, secrets never synced | Accepted |
| [0005](0005-sync-protocol.md) | Sync protocol: per-user monotonic revisions, explicit conflicts | Accepted |
| [0006](0006-multi-window-session-ownership.md) | Session ownership: single Rust-side `SessionManager` across windows | Accepted |
| [0007](0007-licensing.md) | Licensing: Apache-2.0 code, permissive dependency policy | Accepted |

Conventions: ADRs are immutable once accepted; superseding a decision means
adding a new ADR and marking the old one *Superseded by NNNN*.
