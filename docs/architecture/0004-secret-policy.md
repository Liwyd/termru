# 0004. Secret policy: OS keyring only; secrets are never synced

* Status: Accepted
* Date: 2026-10-19

## Context

SSH credentials (passwords, key passphrases) are the highest-value secrets
the product touches. The product wants two things that conflict:

* **Convenience**: a saved host should be connectable without retyping a
  password every time.
* **Safety**: credentials must not leak via the local database, logs,
  backups, sync payloads, or crash dumps — and the product makes E2E-style
  claims about sync that a server-side secret store would contradict.

There is also no existing, well-audited *recovery* story for a forgotten
master password. Inventing one (client-side encryption key derived from a
master password) creates a real risk of shipping a recovery flow that
silently weakens the security model.

## Decision

1. **Secrets live only in the OS credential vault** (Secret Service /
   D-Bus keyring on Linux, Windows Credential Manager, macOS Keychain) via
   the `keyring` crate, keyed per secret (e.g. host password, key
   passphrase), never in SQLite.
2. **The local database schema has no field that can hold a secret.**
   Hosts store *references* (auth method, key file *path*, keyring
   entry name), not values. Backups therefore contain no secrets by
   construction.
3. **Secrets are never part of the sync protocol.** The sync payload builder
   has no access to keyring APIs; `RecordPayload` has no secret-bearing
   variant (enforced by type design and tested).
4. **Storing a passphrase in the keyring is an explicit, per-secret user
   choice**, defaulting to off ("ask every time").
5. Secrets are zeroized on drop (`SecretValue`), redacted in `Debug`, and
   never logged; a logging audit test asserts no secret patterns reach log
   files.

## Consequences

* **Documented limitation**: a user with two devices must re-enter
  credentials on the second device. This is stated in README/security docs
  as intended behavior, not a bug.
* Keyring availability becomes an environment dependency: headless Linux
  without a Secret Service must degrade gracefully (fall back to prompting
  per connection, with a clear explanation — never plaintext storage).
* If a "sync my secrets" feature is ever required, it needs a new ADR with
  a real threat model (client-side encryption + recovery), not an extension
  of this one.

## Alternatives considered

* **Encrypt secrets with a user master password and sync ciphertext**:
  requires a recovery story we cannot honestly provide; a weak one (email
  reset, security questions) defeats the encryption. Deferred until a real
  requirement exists.
* **Store secrets in SQLite plaintext with file permissions**: fails the
  backup/log/sync-leak analysis above; rejected outright.
* **OS-level file encryption only**: no per-secret access control, and
  backups/exports bypass it; rejected.
