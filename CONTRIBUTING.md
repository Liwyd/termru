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

## Releasing

The release pipeline (`.github/workflows/release.yml`) is triggered by a tag
that starts with `v` and nothing else. It never runs for a pull request.

**Protected tags.** The `v*` tag pattern is a protected tag in the
repository settings: only a maintainer with the *write* permission may
create or delete such a tag, and the rule covers the deletion as well, so
an accidental `git push --delete` can not rewrite a published release. The
rule is intentionally simple — one pattern, no exception list — because an
exception list is where tag rules rot.

**What the pipeline does.** It validates the tag (a semantic version that
matches the version in `Cargo.toml` and in `tauri.conf.json`), runs the
whole test suite, builds the desktop packages for Debian, RPM, AppImage and
NSIS, installs each package on the runner to prove it is installable,
publishes the cloud server image in the GitHub container registry (the
version tag, plus `latest` for a stable release only), writes the
checksums and uploads the assets to the GitHub release. The pipeline
verifies afterwards that the assets and the image are really there.

**Release recovery.** A release that failed halfway is not a disaster: the
pipeline is idempotent per job.

* *A failed platform* — the other platforms still publish; the matrix runs
  with `fail-fast: false`, so one broken platform does not cancel the
  others. Re-run the failed job from the GitHub interface ("Re-run failed
  jobs"), or delete the tag and push it again.
* *A failed image push* — re-run the `cloud-image-publish` job. The image
  is versioned, so re-pushing the same tag replaces it.
* *A failed asset upload* — re-run the `publish` job; it rewrites the
  checksums and uploads the assets again.
* *A broken release that was already published* — the tag has to be moved,
  which needs the protected-tag rule to be lifted for that one operation.
  Do it deliberately: delete the tag, publish a fix, then restore the rule
  and note the replacement in the release notes. A published release is
  never edited in place: the checksums would not match any more.
* *A release that must not be published at all* — delete the GitHub
  release and the tag, and fix the cause before tagging again. The images
  in the registry stay behind; they are harmless, because the version
  tag of a deleted release is never referenced.

**Publishing needs no extra credentials.** The GitHub release uses the
`GITHUB_TOKEN` of the workflow, and the container image is published in the
GitHub container registry with the same token (`packages: write`). There is
no Docker Hub account involved.

## License

By contributing you agree that your contributions are licensed under the
Apache-2.0 license, same as the project.
