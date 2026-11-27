# Termru — Roadmap & Remaining Rules

This authoritative file tracks the remaining roadmap items and operational rules for the Termru project. Completed implementation phases and historical check-ins have been archived.

---

## Remaining Roadmap Items

### Phase 17 — Cross-platform validation and performance (Interactive / Human-driven)
- [ ] Windows + Ubuntu + Fedora validation on real runners (build + launch smoke; interactive UI walkthrough).
- [ ] Manual interactive UX validation on Windows/Fedora/Ubuntu by human (environment limitation: headless build machine lacks window manager).

### Phase 18 — Final acceptance & release verification (Interactive / On-Device)
- [ ] Visual UI walkthrough on a real desktop session (add host, connect, resize, split pane, move tab to new window, store password, palette, rebind key, sync login).
- [ ] Tag `v0.1.0` (or subsequent production tag) using credentials on a push-enabled host with release permissions.

### SSH authentication follow-ups (see ADR 0008)
- [ ] Persist a per-host method order (`auth_order`, a synced field with a serde default); until then the chain is derived from the host's method.
- [ ] Restyle `AuthPromptDialog` and `HostKeyDialog` with the redesigned dialogs (they are deliberately plain).
- [ ] Windows: try the OpenSSH named-pipe agent and the Pageant fallback on a real Windows host (implemented, never run).
- [ ] Try keyboard-interactive against a real PAM/2FA server (the tests use an in-process server; the private `sshd` of the tests runs without PAM).
- [ ] Not supported yet: GSSAPI, FIDO security keys (`sk-*`), SSH host certificates, the server-initiated password change.

### Sessions page (Phase 4a, frames 2 and 3)
- [x] Tab strip, toolbar, pane header/footer, split hint, 2px focus rule, draggable divider, expand/restore pane.
- [x] Terminal context menu, find in terminal, multi-line paste confirmation, close confirmation for a live session.
- [x] One resize owner per pane (fit on `requestAnimationFrame`, debounced backend resize).
- [ ] Open terminal links in the system browser: needs the Tauri opener plugin (Rust dependency, `opener:allow-open-url` capability, `@tauri-apps/plugin-opener`). Until then a Ctrl/Cmd+click copies the address.
- [ ] "Don't ask again" for multi-line paste is per session only; a stored preference needs a new field in `Preferences` (Rust DTO and sync).
- [ ] The pane header shows the title the remote shell sets (OSC 0/2) or `shell`; the frame's `bash · /srv/atlas` needs the remote shell name and working directory (OSC 7 or a backend probe).
- [ ] Look at the page on a real desktop session (WebKitGTK, WebView2, WKWebView): menus, focus rings, divider drag and the find bar have only been checked in headless Chrome.

### Editor, folders and identities pages (Phase 4b, frames 4 and 5)
- [x] Host editor page: numbered sections, rail (unsaved indicator, local credential with Forget, connection preview), Cancel / Save host / Save & connect, inline validation with aria wiring, unsaved-changes guard on a page change and on the window closing.
- [x] Folders page: tree with counts, detail list with Move, edit rail, cycle guard in the UI and in the store (Rust tests), delete confirmation that says what moves.
- [x] Identities page and `identity_generate` / `identity_public_key` (ed25519, 0600, no overwrite, optional passphrase; Rust tests, including `ssh-keygen -y` reading the output).
- [ ] The Hosts page and the other pages still sit on the grey canvas while frames 1, 4 and 5 are white; the three pages of this phase are white. Decide on one background in the shell.
- [ ] "Choose hosts" is a searchable list of up to 50 hosts, not a multi-select; moving several hosts at once is still one at a time.
- [ ] Generated keys are ed25519 only; RSA/ECDSA generation, importing a key into the app's folder and deleting a generated key's file with its identity are not offered (deleting the identity leaves the file).
- [ ] The window-close guard (`onCloseRequested`) is only exercised against a fake window in tests; try it on a real desktop session (WebKitGTK, WebView2, WKWebView).
- [ ] Phase 5 will add the Jump host, Forwards and Advanced sections to the editor; `04 / ADVANCED` already exists for them.
- [ ] A password typed in the editor is sent to the backend over IPC as a plain string and is not zeroized on the frontend side (as in the connect dialog).

### Dependency maintenance
- [ ] Evaluate migrating `chrono` (RUSTSEC-2026-0543, unmaintained, ignored in `deny.toml`) to `jiff` or `time` before 2027-03-01; keep the stored epoch-ms / RFC 3339 wire formats and the `tokio-postgres` mapping working (see `docs/security/dependency-review.md`).

---

## Operational Rules & Conventions

1. **Core Stack & Architecture:**
   - Rust 1.99 workspace (`termru-core`, `termru-store`, `termru-ssh`, `termru-desktop`, `termru-cloud-server`).
   - Tauri 2 stable + Svelte 5 + Vite 8 + TypeScript 5.x + `@xterm/xterm` 6.
   - SQLite locally (rusqlite bundled, WAL mode, foreign keys, versioned migrations) + PostgreSQL for cloud sync.
   - Pure Rust SSH engine (`russh 0.65`) with TOFU host keys and changed-key warnings.

2. **Security & Privacy:**
   - No secret synchronization: secrets live exclusively in the OS Keyring (`keyring` crate).
   - Parameterized SQL queries only; strict authorization checks per user/device.
   - Strict Content Security Policy (CSP), least-privilege Tauri capabilities (`core:default` + specific window/dialog permissions).
   - No remote font loading (bundled fontsource assets).

3. **Code Quality & Testing:**
   - Zero compiler warnings (`-D warnings` for clippy).
   - Comprehensive test coverage across workspace crates and frontend components.
   - Adherence to tokens, motion, accessibility, and UI design specifications (`re-design/`).
