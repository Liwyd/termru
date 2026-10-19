# Contributing to Termru

Thanks for your interest in contributing.

## Development setup

* Rust 1.85+ (`rustup` recommended)
* Node 22.12+ and npm 10+
* OpenSSH server tools (`sshd`, `ssh-keygen`) for SSH integration tests

```bash
git clone https://github.com/Liwyd/termru.git
cd termru
cargo test --workspace
npm install
npm run typecheck && npm run test
```

## Workflow

1. Open an issue for non-trivial changes so direction can be discussed first.
2. Branch from `main`; keep commits focused and messages in imperative mood.
3. All checks must pass before review:

   ```bash
   cargo fmt --check
   cargo clippy --workspace --all-targets
   cargo test --workspace
   npm run lint
   npm run test
   ```

4. Update `TODO.md` / docs when behavior or architecture changes.
5. Open a PR with a clear description of *what* and *why*.

## Code standards

* No `unsafe` in Rust code (workspace-enforced).
* Prefer explicit error types over panics; no `unwrap`/`expect` outside tests.
* New features need tests; bug fixes need a regression test.
* Security-relevant changes must call out residual risk in the PR description.

## License

By contributing you agree that your contributions are licensed under the
Apache-2.0 license, same as the project.
