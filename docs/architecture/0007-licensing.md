# 0007. Licensing: Apache-2.0 for Termru, permissive dependency policy

* Status: Accepted
* Date: 2026-10-19

## Context

Termru is intended for open-source publication (GitHub Releases, Docker
Hub). Licensing choices affect downstream adoption (companies are the
likely users of a self-hosted SSH manager), redistribution of packages
(.deb/.rpm/NSIS/AppImage) and of the cloud container image, and the freedom
to relicense later if needed.

## Decision

* **Project code: Apache License 2.0** (root `LICENSE`, `license = "Apache-2.0"`
  in the workspace). Rationale: explicit patent grant, industry-standard for
  infrastructure tooling, no copyleft obligations on downstream users.
* **Dependency license policy: allowlist, enforced by `cargo-deny` in CI**
  (`deny.toml`): MIT, Apache-2.0 (+LLVM-exception), BSD-2/3, ISC, Zlib,
  MPL-2.0, CC0-1.0, 0BSD, Unlicense, Unicode-3.0/DFS-2016, CDLA-Permissive-2.0,
  OpenSSL license text. No GPL/AGPL/LGPL dependencies, no unknown licenses,
  no git-sourced or unknown-registry crates.
* npm dependencies are gated by `npm audit` and reviewed for license
  (frontend deps: MIT/BSD-class only expected); recorded in
  `docs/security/dependency-review.md` when written.
* **No code-signing credentials exist** for desktop artifacts; package
  signing limitations are documented rather than faked (checksums
  `SHA256SUMS` are published; signing is a future item).
* Third-party notices: Apache-2.0 allows binary distribution without the
  full text of permissive deps, but release notes/NOTICE will credit
  significant dependencies (russh, Tauri, xterm).

## Consequences

* Contributors' changes are Apache-2.0; CLA is not used (patent grant in
  the license text is sufficient at this scale).
* Adding a copyleft dependency is a policy exception requiring an ADR
  update, not an ad-hoc decision.
* If a permissive-allowlist crate is ever needed but unlicensed (e.g. a
  dependency with a non-standard license), `cargo deny` fails CI and forces
  an explicit decision.

## Alternatives considered

* **MIT**: equally permissive but no explicit patent grant.
* **GPL-3.0**: deters the primary audience (companies running internal
  tools) with no security benefit; rejected.
* **MPL-2.0 (file copyleft)**: reasonable, but adds obligations that buy
  nothing for a greenfield tool; rejected.
* **BSL/SSPL for "open core"**: no commercialization plan requires it;
  rejected.
