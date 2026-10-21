# Known limitations and residual risks

What Termru does **not** do yet, and what a user has to live with. The
security-relevant part of this list is also tracked in the
[threat model](security/threat-model.md).

## Product

* **No packaged installers yet.** The desktop app is built from source; the
  `.deb`, RPM, AppImage and NSIS packages are still to come. Until then
  there is no "one command" install for a desktop user.
* **No automatic background sync.** The sync runs when the user asks for it
  (the sidebar entry or the palette). There is no timer, no automatic token
  refresh and no automatic retry with backoff.
* **No split panes yet.** A window shows one terminal per tab; the split
  layout, the per-window tab sets and the window-close semantics are open.
* **No key file picker.** The path of a private key is typed in; there is no
  native file dialog yet.
* **No settings dialog.** The appearance preferences (font, size, cursor,
  theme) are stored and applied, but there is no UI to change them beyond
  the palette commands.
* **No remapping UI.** The keybinding registry detects conflicts and the
  palette shows the bindings, but a binding cannot be changed by the user
  yet.
* **No tags, folders or context menus in the host tree.** Hosts can be
  favourited, searched and grouped by the *group* model, but the tree
  itself is a flat list.
* **Restored tabs are placeholders.** After a restart the layout comes back,
  but the sessions do not: the user reconnects. A remote shell can not be
  restored, and Termru never claims otherwise.

## Environment

* **The interactive UI was never driven by a human on the build machine.**
  The machine has no window manager, so the window could not be shown,
  clicked through or screenshotted. Everything automated is green; the
  visual pass is still open. A checklist for it is in the TODO.
* **The cloud server was never exercised against a container runtime
  locally** (no Docker daemon): the image and the compose file are built in
  CI.
* **Only Linux was built and tested.** The Windows and macOS builds exist
  in the pipeline plan, but "built on" is not "tested on".

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
* **No code signing.** The release artifacts are not signed, and there are
  no checksums published for the desktop packages yet. A user has to trust
  the download channel.
* **The CI actions are pinned to SHAs**, but the build still runs on
  GitHub-hosted runners.

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
