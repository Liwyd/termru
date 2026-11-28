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
- [x] A per-host method order (`auth_order`, a synced field with a serde default; Phase 5a). The editor control is in the host editor (Phase 5b).
- [x] `AuthPromptDialog`, `HostKeyDialog` and `ConnectDialog` restyled on the session dialog (Phase 4c).
- [ ] Windows: try the OpenSSH named-pipe agent and the Pageant fallback on a real Windows host (implemented, never run).
- [ ] Try keyboard-interactive against a real PAM/2FA server (the tests use an in-process server; the private `sshd` of the tests runs without PAM).
- [ ] Not supported yet: GSSAPI, FIDO security keys (`sk-*`), SSH host certificates, the server-initiated password change.

### Sessions page (Phase 4a, frames 2 and 3)
- [x] Tab strip, toolbar, pane header/footer, split hint, 2px focus rule, draggable divider, expand/restore pane. (Rebuilt in Obsidian part 2: one 46 px row for tabs and tools, 12 px panes with a 6 px gap, 1 px champagne focus line, no hint banner and no bottom bar; see below.)
- [x] Terminal context menu, find in terminal, multi-line paste confirmation, close confirmation for a live session.
- [x] One resize owner per pane (fit on `requestAnimationFrame`, debounced backend resize).
- [x] Terminal links open in the system browser (`tauri-plugin-opener`, `opener:allow-open-url` scoped to http, https and mailto). Try it on a real desktop session (Linux `xdg-open`, Windows, macOS): it has only been exercised through a fake.
- [x] "Don't ask again" for multi-line paste is the preference `terminal.confirm_multiline_paste` (Rust DTO, sync, Preferences switch).
- [ ] The pane header shows the title the remote shell sets (OSC 0/2) or `shell`; the frame's `bash · /srv/atlas` needs the remote shell name and working directory (OSC 7 or a backend probe).
- [ ] Look at the page on a real desktop session (WebKitGTK, WebView2, WKWebView): menus, focus rings, divider drag and the find bar have only been checked in headless Chrome.

### Obsidian redesign, part 2: Sessions and overlays
- [x] Sessions page to the mockup (`~/.claude/plans/obsidian`): tab strip and tools in one 46 px row, forwards and synced-settings chips, split right / down, duplicate, detach, More; 12 px panes, 6 px gaps that are the dividers, 1 px champagne focus line, 34 px header, 28 px footer with the state, Reconnect and the forwards; empty state with the palette chord.
- [x] Every menu, dialog, notice and the find bar on raised surfaces (12 / 14 px, one shadow), one champagne primary action, danger confirms in the error colour (`danger-fill`), a dialog opened or closed by a key does not animate.
- [ ] Files / SFTP button of the Sessions toolbar: there is no SFTP yet (the mockup has one).
- [ ] Look at the page on a real desktop session (WebKitGTK, WebView2, WKWebView): the rounded clip of the WebGL terminal, the divider bar and the focus line have only been checked in headless Chrome with the DOM renderer (`--disable-3d-apis`).

### Editor, folders and identities pages (Phase 4b, frames 4 and 5)
- [x] Host editor page: numbered sections, rail (unsaved indicator, local credential with Forget, connection preview), Cancel / Save host / Save & connect, inline validation with aria wiring, unsaved-changes guard on a page change and on the window closing.
- [x] Folders page: tree with counts, detail list with Move, edit rail, cycle guard in the UI and in the store (Rust tests), delete confirmation that says what moves.
- [x] Identities page and `identity_generate` / `identity_public_key` (ed25519, 0600, no overwrite, optional passphrase; Rust tests, including `ssh-keygen -y` reading the output).
- [x] One background for every page: the shell draws the page on the white surface (frames 1, 4, 5 and 8).
- [ ] "Choose hosts" is a searchable list of up to 50 hosts, not a multi-select; moving several hosts at once is still one at a time.
- [ ] Generated keys are ed25519 only; RSA/ECDSA generation, importing a key into the app's folder and deleting a generated key's file with its identity are not offered (deleting the identity leaves the file).
- [ ] The window-close guard (`onCloseRequested`) is only exercised against a fake window in tests; try it on a real desktop session (WebKitGTK, WebView2, WKWebView).
- [x] Phase 5b: the Jump hosts, Port forwards and Advanced sections of the host editor, the forwards chip and menu in the session toolbar, the `~/.ssh/config` import dialog, the jump-host password prompt (`hop_secrets`) and the local approval of synced fields that act (below).
- [ ] A password typed in the editor is sent to the backend over IPC as a plain string and is not zeroized on the frontend side (as in the connect dialog).

### Data model v2, jump hosts, forwards, session setup (Phase 5a, ADR 0010)
- [x] `Host` gets `jump_chain`, `forwards`, `env`, `startup_command`, `agent_forward`, `auth_order` (serde defaults, unused fields not serialized); validation incl. IPv6 zone ids, per-kind forwards, env names, jump cycles and length; store migration 2 (v1 fixture database), `Hosts::import` in one transaction; the sync server needs no change (opaque payload, same entity validation).
- [x] Engine: jump chains (every hop verified and authenticated, errors name the hop), local / remote / dynamic (SOCKS5) forwards with per-forward status events, `SetEnv`, startup command, agent forwarding, read-only `~/.ssh/known_hosts`; sshd integration tests for all of them.
- [x] `~/.ssh/config` parser (pure) and the `ssh_config_preview` / `ssh_config_import` commands.
- [ ] A setting to turn the read-only `~/.ssh/known_hosts` source off.
- [ ] Windows: the agent-forwarding proxy (OpenSSH named pipe) and the forwards were never run on Windows.
- [ ] Preserve unknown host fields across versions (an older app editing a v2 host drops them).
- [ ] Re-check the routes of the hosts that depend on an edited or deleted jump host at save time (today: at connect time).
- [ ] The cloud-server test for v2 host payloads needs PostgreSQL; it was compiled but not run locally.

### Host editor v2, forwards status, import and approval (Phase 5b, ADR 0010)
- [x] Host editor: `04 / JUMP HOSTS` (ordered picker, reorder, remove, the resolved route `you → bastion → app`, cycle / missing hop / over 8 hops), `05 / PORT FORWARDS` (L/R/D rows, bind address with a non-loopback warning, enabled switch, inline validation mirrored from `validate.rs`), `06 / ADVANCED` (environment variables, startup command, agent forwarding with its risk, authentication order); the command preview in the rail shows `-J`, `-L`/`-R`/`-D` and `-A`.
- [x] Session toolbar: a forwards chip (`2 forwards`, `1 of 2 failed`, `1 needs approval`) with a menu of one line per forward (`L 127.0.0.1:8080 → db:5432 · listening`, or the reason it failed); choosing a listening one copies its address.
- [x] Jump-host secrets: the connect dialog has one password field per password hop with nothing stored, and a "keep in the credential store" box per hop; sent as `hop_secrets`, stored only after the session is ready.
- [x] Import from `~/.ssh/config`: a secondary action on the Hosts page, a dialog with the candidates (new / already in the library / cannot import, with the reason), jump routes, key files that are referenced and never copied, file warnings, a summary and a notice; missing file, empty file and read error states.
- [x] Local approval of synced fields that act (`host_exec_review`, `host_exec_approve`, `host_save` with `approve_exec`, migration 3): a host whose startup command, agent forwarding, environment or non-loopback forwards this device has not approved connects without them and says so; the review dialog shows exactly the fields.
- [ ] Approval applies to the next connection: a session that is open keeps its parameters, and so do its reconnects and duplicates. A "reconnect with the approved settings" action would need the connection parameters rebuilt from the store (they hold secrets today).
- [ ] The approval hash is of the fields as stored; a host edited on another device and approved there asks again here (per device, by design). A "approve all synced hosts" action does not exist.
- [ ] Look at the new editor sections, the forwards menu and the import and review dialogs on a real desktop session (WebKitGTK, WebView2, WKWebView): they have only been checked in headless Chrome and jsdom.
- [ ] The jump-host picker is a native select (it lists every host by name and address); a library of thousands of hosts needs a searchable list.
- [ ] Hosts that only exist in the import file as `ProxyJump user@host:port` (not an alias of the file) are imported as the address they name; the dialog lists the route as written.

### Sync, Preferences, dialogs and notices (Phase 4c, frame 8 and the specification C.03-C.04)
- [x] Sync page (status, conflicts, this device, sign in, change account, sign out, offline copy), Preferences page (every field, live preview, shortcut table with record / unbind / reset, Restore defaults, About), connect / host-key / sign-in dialogs, the notify facade (ADR 0009) and the opener plugin.
- [ ] Sync: the other devices of the account. The server has `GET /v1/devices` and `DELETE /v1/devices/{id}`, but the desktop app has no command for them, so the page shows this device only. Needs `sync_devices` / `sync_revoke_device` commands.
- [ ] Preferences: the frame's "Theme: Chalk" is a terminal palette name; Termru has one light and one dark palette, so the control is System / Light / Dark. The frame's line height of 1.65 is a multiple of the font size; the stored value multiplies xterm's cell height (default 1.3), which gives the same 23 px rows.
- [ ] Preferences: a font that is not installed falls back silently (the list says Geist Mono and IBM Plex Mono ship with Termru); there is no way to detect that yet.
- [ ] Notices: the specification suggests a Sileo toast through a React island; the facade draws native Svelte notices instead. Adopting a library needs the maintainer's approval (second UI runtime) and changes only `components/Toasts.svelte`.
- [ ] Notices: a notice shows in the window that raised it only; there is no notification centre or history.
- [ ] Look at the new pages and dialogs on a real desktop session (WebKitGTK, WebView2, WKWebView): focus rings, the select arrow, the shortcut recorder and the notice position have only been checked in headless Chrome.

### Hosts page, accessibility, motion, stress and copy (Phase 4d, frame 1)
- [x] Hosts page body rebuilt to frame 1: `PageHeader`, filter row, plain tabs, band header, 70 px rows on fixed columns, the running-sessions banner under the rows, the row menu on `PopMenu`. With this the redesign items R4 (Sessions), R5 (editor), R6 (Folders and Identities), R7 (Hosts), R8 and R9 (Sync, Preferences, dialogs, notices) are built to their frames; what is still open for each is listed under its own heading above, and none of it has been seen in a real webview.
- [x] axe-core over every route and dialog (jsdom), form errors wired with `aria-invalid` / `aria-describedby`, `docs/accessibility.md` with the keyboard checklist.
- [x] Motion pass (press feedback, hover gate, arriving blocks, the row menu) and the stress pass (10 000 hosts, long names, empty and error states, 760 px, dark): see the CHANGELOG.
- [ ] Hosts table: the Folder column is hidden when the list is under 760 px wide and the Sign-in column under 560 px (the inspector and the editor have them); a narrow window loses them. Showing the folder under the name there would keep it.
- [ ] Hosts table scroll: measured at 54 fps in a headless, software-rendered Chrome against the mock (`scripts/scroll-perf.mjs`, `docs/performance.md`); not measured on WebKitGTK, WebView2 or WKWebView, nor against the real keyring (a lookup per row at rest).
- [ ] Accessibility, see `docs/accessibility.md` (Known gaps): no run with a real screen reader (Orca, NVDA, VoiceOver); the terminal output is not announced (xterm screen reader mode is off); session tabs can only be reordered by dragging; the session toolbar has no arrow-key roving; no skip link; contrast and target size are not measured in a browser; Windows forced colours and 200 % zoom are untried.
- [ ] Whether an arriving block moves depends on the last input (a key press or a pointer), not on what caused it: a session started with a key whose banner appears after the user touched the mouse still moves. Decide whether that is right or whether the cause should be passed along.

### Obsidian redesign (O1 of 4: foundation, shell, Hosts)
- [x] Tokens (dark default, light, "System"), Geist and Geist Mono bundled (Inter removed), terminal palettes, the left sidebar with Live, Library and Folders and an icon rail, the 28 px status bar, the one-line page header on every page, the command palette with every action of the app, the Hosts page with rows, sort chips, search and the 340 px inspector. See the CHANGELOG.
- [ ] O2: the Sessions page to the brief (one 46 px row of tabs and tools, rounded panes with 6 px gaps, focused pane outline, find bar, menus, paste and close confirmations). Today it runs on the new tokens with its old layout.
- [ ] O3: the host editor (a full-width page with a left section index and a sticky footer), Folders, Identities, Sync and Preferences as list plus inspector pages; dialogs, menus and notices on one raised surface. Their section labels are sentence case already; their layouts, numbered editor sections and the `section-label` / `wp-label` classes (written in capitals in the markup of Folders and the editor, shown as sentences by a CSS rule) are still the old ones.
- [ ] Hosts inspector: no **Files** action (there is no SFTP) and no **host key** row (no command reads `known_hosts` for one host: add `host_key_info(id)` to show the fingerprint the engine would check).
- [ ] The light theme's text colours (`text-3`, `accent-text`, `live-text`, `warn-text`, `err-text`) are darker than the approved light palette so every text pair holds 4.5:1; fills are as approved. Decide whether to keep that or lighten the surfaces.
- [ ] Look at the sidebar, the rail, the palette and the inspector on a real desktop session (WebKitGTK, WebView2, WKWebView): `@container` queries, `color-mix()` and variable fonts have only been checked in headless Chrome and jsdom.
- [ ] The sidebar is a list of buttons, not a roving tree: with many folders it is many Tab stops (the folders scroll inside it). An arrow-key roving would make the nav one stop.
- [ ] A stored terminal font of "IBM Plex Mono" (the old default) is kept; only new installs get Geist Mono at 13 px.

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
   - No remote font loading (bundled fonts: Geist and Geist Mono in `src/assets/fonts/`, IBM Plex Mono through fontsource).

3. **Code Quality & Testing:**
   - Zero compiler warnings (`-D warnings` for clippy).
   - Comprehensive test coverage across workspace crates and frontend components.
   - Adherence to tokens, motion, accessibility, and UI design specifications (`re-design/`).
