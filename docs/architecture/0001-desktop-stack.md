# 0001. Desktop stack: Tauri 2 + Svelte 5 + Vite 8 + TypeScript 5 + xterm 6

* Status: Accepted
* Date: 2026-10-19

## Context

Termru is a desktop SSH terminal. Requirements that shape the stack:

* native-feeling windowing, system tray-adjacent integration later, small
  footprint relative to Electron;
* a real terminal emulator in the UI (escape sequences, selection,
  scrollback, Unicode width handling) — we must not reimplement one;
* long-lived bidirectional streaming between Rust (SSH engine) and the UI
  without polling;
* a UI framework with a small learning curve and good TypeScript
  interop, since most future contributors will touch the frontend first;
* packaging targets: Linux (.deb/.rpm/AppImage), Windows (NSIS). macOS is
  not a v1 target but must not be architecturally excluded.

## Decision

* **Tauri 2.12 stable** (not 3.0-alpha) for the desktop shell.
* **Svelte 5 + Vite 8 + TypeScript 5.x** for the frontend.
* **`@xterm/xterm` 6** (+ `fit` addon, WebGL addon with DOM fallback) as the
  terminal emulator.

Version constraints that are part of this decision:

* `@sveltejs/vite-plugin-svelte@7` requires vite ^8 and svelte ^5.46.
* `svelte-check@4` requires typescript ^5 || ^6 → **TypeScript is pinned to
  5.x**, not a newer major, until `svelte-check` supports it.

## Consequences

* One Rust binary ships the app; webview is the platform webview
  (WebKitGTK on Linux, WebView2 on Windows).
* SSH logic stays in Rust; the terminal emulator runs in the webview and
  receives raw bytes over a Tauri `Channel`. Backpressure is owned by Rust
  (bounded channels), not the webview.
* Webview hardening (CSP, no remote content, strict IPC validation) becomes
  a first-class security work item — see Phase 8.
* We depend on the platform webview's behavior differing between Linux
  distributions; CI builds on Ubuntu with `webkit2gtk-4.1` and packaging
  declares that dependency.

## Alternatives considered

* **Electron**: heavier (≈100 MB+ runtime, much higher idle RSS), and the
  SSH engine would still be Rust via a sidecar or native module — more
  complexity for no UI benefit.
* **Tauri 3.0 alpha**: no stability guarantee; v1 ships on stable only.
* **React/Vue**: viable, but Svelte 5's runes give the reactive session/tab
  state model with less boilerplate and smaller bundles; no requirement
  pulls toward React.
* **xterm.js from npm directly**: `@xterm/xterm` is the maintained package
  name of the same project; use it.
