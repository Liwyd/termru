# Known limitations and residual risks

What Termru does **not** do yet, and what a user has to live with. The
security-relevant part of this list is also tracked in the
[threat model](security/threat-model.md).

## Product

* **The interactive UI was never driven by a human on the build machine.**
  The machine has no window manager, so the window could not be shown,
  clicked through or screenshotted. Everything automated is green; the
  visual pass is still open. A checklist for it is in the TODO.
* **Only Linux was built and tested.** The Windows and macOS packages are
  built by the release pipeline, but "built on" is not "tested on".
* **The second window can only show a moved tab.** There is no per-window
  tab bar: a window that was opened to take over a session has exactly that
  session, and no tabs of its own.
* **Restored tabs are placeholders.** After a restart the layout comes back
  (the tabs, the order and the pane tree), but the sessions do not: the
  user reconnects. A remote shell can not be restored, and Termru never
  claims otherwise.
* **The port forwarding is not implemented.** No local, remote or dynamic
  forwarding, no jump hosts. The SSH engine is the pure-Rust one, so the
  feature is possible, but it was left out of the first version
  (see ADR 0002).
* **No snippets.** The domain has no snippet model, so the search can not
  cover one and the palette can not insert one.
* **No context menus and no drag & drop of a host onto a tab.** The tabs can
  be reordered by dragging, and a host is opened with a click or from the
  palette.
* **The terminal follows the theme of the app.** A host can not have a
  terminal theme of its own.
* **The sync covers the configuration, not the sessions.** The open
  terminals, the window geometry and the layout are device-local.

## Environment

* **The cloud server was never exercised against a container runtime
  locally** (no Docker daemon on the build machine). The image and the
  compose file are built and smoke-tested in CI.
* **The packages were never installed on a real desktop.** The pipeline
  installs the `.deb` and the `.rpm` on the runner and queries the package
  manager, but the application was never launched by a user.

## Security

* **TOFU on first use.** The first connection to an unknown host asks for a
  confirmation. A changed host key is a hard warning and is never accepted
  automatically — but the first use is still a decision the user makes
  without a second source of truth.
* **The local database is not encrypted.** It holds host names, addresses
  and user names, and it is protected by the file permissions (`0600`)
  only.
* **The cloud database connection is not encrypted** (`NoTls`). The database
  is expected to live on a private network next to the server.
* **The rate limiter is per process.** With several replicas behind a load
  balancer each enforces its own budget.
* **Registering reveals whether an account exists** (the server answers
  `409`). Logging in does not: an unknown account and a wrong password are
  indistinguishable.
* **The webview is a browser.** The content security policy is strict and
  the frontend never inserts raw markup, but a browser engine is still a
  large attack surface.
* **No code signing.** The release artifacts are not signed; the
  `SHA256SUMS` file of the release is the trust anchor.
* **The CI actions are pinned to SHAs**, but the build still runs on
  GitHub-hosted runners.
* **The first use of a private key trusts the file on disk.** The path is
  stored, the key is never copied, but the file itself is not verified
  against a fingerprint in the UI.

## Protocol

* **The sync protocol has no encryption of its own.** It relies on the
  transport (TLS) and on the server's access control.
* **Conflicts are never resolved automatically.** When both sides changed
  the same record, the user has to choose. That is deliberate, but it means
  a busy account can accumulate conflicts.
* **The protocol version is not negotiated.** A client that speaks a
  different payload version must be updated in lockstep with the server
  (the schema version is carried in the payload, but there is no downgrade
  path yet).
* **The revision counter is per user.** Deleting an account and creating a
  new one with the same name starts at 1 again.
