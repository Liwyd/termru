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

---

## Operational Rules & Conventions

1. **Core Stack & Architecture:**
   - Rust 1.99 workspace (`termru-core`, `termru-store`, `termru-ssh`, `termru-desktop`, `termru-cloud-server`).
   - Tauri 2 stable + Svelte 5 + Vite 8 + TypeScript 5.x + `@xterm/xterm` 6.
   - SQLite locally (rusqlite bundled, WAL mode, foreign keys, versioned migrations) + PostgreSQL for cloud sync.
   - Pure Rust SSH engine (`russh 0.64`) with TOFU host keys and changed-key warnings.

2. **Security & Privacy:**
   - No secret synchronization: secrets live exclusively in the OS Keyring (`keyring` crate).
   - Parameterized SQL queries only; strict authorization checks per user/device.
   - Strict Content Security Policy (CSP), least-privilege Tauri capabilities (`core:default` + specific window/dialog permissions).
   - No remote font loading (bundled fontsource assets).

3. **Code Quality & Testing:**
   - Zero compiler warnings (`-D warnings` for clippy).
   - Comprehensive test coverage across workspace crates and frontend components.
   - Adherence to tokens, motion, accessibility, and UI design specifications (`re-design/`).
