# 0006. Session ownership: one Rust-side `SessionManager`, reattachable views

* Status: Accepted
* Date: 2026-10-19

## Context

The desktop app supports multiple native windows, each with tabs and split
panes. An SSH session is a live TCP connection + remote shell that must
survive:

* closing a pane or tab that is merely *viewing* it (accidental close must
  not kill a shell — v1 policy: closing the last view closes the session
  only on explicit user action);
* **moving a tab between windows** without dropping the connection;
* window close/crash of the viewing UI;
* app quit (then sessions must be terminated cleanly — no orphaned
  reconnect loops or leaked tasks).

If any window could own sessions directly, two windows could race to
control the same channel, double-write input, or both believe they own a
session after a move.

## Decision

**All SSH sessions are owned by a single `SessionManager` in the Rust
backend** (Tauri app state), never by a webview/window:

* `SessionManager` is the only component that creates/destroys session
  actors and holds their handles; windows are *views* that attach.
* Each session runs in its own tokio task (the actor in
  `crates/termru-ssh`); commands and output flow through bounded channels.
* Attaching streams output through a Tauri `Channel` to exactly one view at
  a time; **moving a tab = detach from window A, attach to window B**, with
  the actor's bounded replay buffer replayed before live bytes — the remote
  shell never notices.
* Session identity is a stable `SessionId` in Rust; the frontend only ever
  references ids and never owns transport state.
* Status changes (ready/failed/reconnecting/closed) are events fanned out
  to interested views; a view with no session shows stale UI as a state,
  never as an error.
* **Graceful shutdown**: app exit explicitly closes every session (bounded
  wait), so no background tasks outlive the process.
* Layout (window geometry, tabs, splits) is persisted as *metadata* only;
  on startup, stale sessions are cleaned up and **the app never claims to
  recover remote shells** — only the layout.

## Consequences

* Single owner ⇒ ordering and lifecycle invariants are testable in Rust
  (no double-free of channels, no duplicate ownership) without a display.
* All multi-window races concentrate in one component with a test surface
  (rapid open/close/move/quit interleavings are unit-tested).
* The frontend must be written defensively: it can receive events for
  sessions it no longer shows (e.g. after a move) and must ignore them by id.
* Because the manager is global app state, per-window "private" behaviors
  (e.g. a window-local scratch session) are not possible in v1 — not a
  current requirement.

## Alternatives considered

* **Sessions owned by the window that opened them**: tab move requires
  handing a live connection between webviews — an ownership transfer with
  no single arbiter; classic source of double-ownership and leaks; rejected.
* **A background process separate from the UI process**: survives window
  close but adds IPC/process supervision complexity for a desktop app whose
  UX expects sessions to end with the app; rejected for v1 (could be revisited
  for a "background terminal service" feature).
* **Frontend-side session state with Rust as dumb pipe**: webviews are
  recreated/hidden at will and cannot be trusted for lifecycle; rejected.
