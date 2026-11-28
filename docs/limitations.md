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
* **Forwarding, jump hosts and the other session options have no screens
  yet.** The engine, the data model and the import of `~/.ssh/config` work
  (see ADR 0010), but the host editor does not show the Jump host, Forwards
  and Advanced sections until the next phase: they can only come from an
  import or a synced record.
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

## Jump hosts, forwards and session setup

See [ADR 0010](architecture/0010-data-model-v2.md) and ADR 0002.

* **An older Termru drops the new fields.** A version from before data model
  v2 that edits a host keeps no jump chain, forwards, environment, startup
  command, agent flag or authentication order in its copy and pushes that.
  There is no unknown-field preservation. Update every device before editing
  hosts that use them.
* **Synced fields act.** A startup command, agent forwarding and forwards come
  with the synced record: whoever can write to the sync account can change
  what a connection does on every device. They are validated everywhere and
  off or loopback-only by default, but there is no per-device switch that
  ignores them yet.
* **Agent forwarding is a risk by nature.** The server's administrators can use
  the agent while the session lives. It is off by default and only requested
  on the target, never on a jump host. On Windows only the OpenSSH agent's
  named pipe is proxied (Pageant can authenticate but not be forwarded), and
  that path was never run.
* **The startup command is typed input, not `exec`.** It reaches an
  interactive shell like keystrokes: a shell whose startup files read from the
  terminal (a prompt in `.profile`) may consume it, and it runs again after
  every automatic reconnect (a new shell).
* **Environment variables only reach what the server accepts** (`AcceptEnv`;
  most servers take `LANG` and `LC_*`). A refusal is silent in the UI and
  logged at debug level (the name, never the value).
* **A SOCKS forward has no authentication,** and a forward bound to a
  non-loopback address is reachable from the network. Both are the user's
  explicit choice.
* **Forward listeners are IPv4 loopback by default** (`localhost` binds
  `127.0.0.1`); an IPv6 listener needs an explicit `::1` bind address.
* **The route is not rechecked for the hosts that depend on an edited host.**
  Editing a jump host so that another host's route becomes a cycle or too long
  is reported when that host is connected, not when the jump host is saved.
  Deleting a jump host does the same.
* **Each hop has its own time budgets.** A chain of N hops can take N times the
  connect and authentication time before it gives up; cancelling is immediate.
  A question asked by a jump host is shown in the session's dialog, marked with
  the host's name.
* **The user's `~/.ssh/known_hosts` is trusted read-only.** A key it holds is
  accepted without a prompt for a host the app has no entry for. There is no
  setting to turn that off yet. Wildcard and hashed entries work like
  OpenSSH's; `@cert-authority` is ignored.
* **The `~/.ssh/config` import is a subset.** It understands `Host` (patterns,
  negation), `HostName`, `User`, `Port`, `IdentityFile`, `ProxyJump` and
  `Include`. `Match` blocks are skipped with a warning; `ProxyCommand`,
  forwards, `SendEnv` and every other option are not imported. A host uses one
  key: only the first usable `IdentityFile` is imported. A host with none is
  imported with agent authentication. The import is not repeated on change:
  it creates what is missing and recognises what exists by address, user and
  port.
* **The jump, forward and import code was exercised against OpenSSH `sshd`
  only** (private instances, no PAM, no real firewall between the hops).

## Authentication

See [ADR 0008](architecture/0008-authentication.md).

* **Not supported:** GSSAPI/Kerberos, host-based authentication, FIDO security
  keys (`sk-*`), SSH host certificates and the server-initiated password change
  (`SSH_MSG_USERAUTH_PASSWD_CHANGEREQ`). A server that needs one of them is not
  reachable.
* **The Windows agent code was never run.** The OpenSSH agent's named pipe
  (`\\.\pipe\openssh-ssh-agent`) and Pageant are supported by the engine, but
  the machine the code was written on is Linux: the Windows branch compiles
  there only conditionally and has no test. Treat agent authentication on
  Windows as unverified until it was tried on a Windows host.
* **At most 5 agent keys are offered** to a server (`MaxAuthTries` is 6 by
  default and every rejected key counts). An agent with more keys needs the
  right key among the first five; there is no per-host key choice yet (the host
  data model has a single authentication method, the chain is derived from it).
* **The method order is not configurable.** The chain is derived from the
  host's method (password → keyboard-interactive, key → keyboard-interactive,
  agent → keyboard-interactive); a user-defined order needs a synced field of
  its own.
* **A keyboard-interactive server's questions are shown as text it chose.** The
  dialog renders them as plain text (never markup), but what they say is the
  server's word.

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
