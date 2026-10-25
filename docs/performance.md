# Performance

Measured numbers, not guesses. Every measurement can be repeated with the
command below it.

**The machine these numbers come from:** 2 CPU cores, 8 GB RAM, Fedora 44,
Linux 6.x, Rust 1.99, PostgreSQL 18.6. The Rust measurements use the `dev`
profile (`opt-level = 1`), so they are *not* release numbers — a release
build is faster. The frontend numbers are from a debug webview build.

The absolute values matter less than the shape: what a user notices is the
latency of an interaction, not a throughput number.

## The local store

`cargo test -p termru-store --test performance -- --nocapture`

With 10,000 hosts (the upper end of what a user will ever have):

| Operation | Time | Note |
|---|---|---|
| Write a host (row + sync queue, one transaction) | 0.16 ms | 6,100 hosts/s |
| List all hosts | 62 ms | one query, the sidebar redraws in one frame budget on a modern machine |
| Read one host | 0.05 ms | |
| Search ("host-07777") | 7 ms | `LIKE` over 10,000 rows |
| Materialize the push payload of 10,000 queued mutations | 489 ms | only when 10,000 changes are pending at once |
| Backup (`VACUUM INTO`) | 28 ms | 4.7 MiB archive |
| Database on disk | 4.9 MiB | for 10,000 hosts |

**Budget:** a host list of 10,000 entries must be usable. 62 ms for the list
and 7 ms for a search are both well inside a frame budget on a modern
machine; the *rendering* of 10,000 rows is the real limit and is why the
hosts table renders only the window of rows on screen (fixed row height,
6 rows of overscan — see below).

## The hosts table (frontend)

`npx vitest run src/lib/pages/hosts-page.test.ts`

With 10,000 hosts in the page (jsdom, Svelte dev mode — no webview, so the
DOM times are a pessimistic test-environment number, not a browser number):

| Operation | Time | Note |
|---|---|---|
| Mount the hosts page | ~400 ms | one-time: builds the search index and the sorted set |
| Filter keystroke (store work) | 4.7–4.8 ms | one filter pass over the index; inside the 16.7 ms frame budget |
| Clear the search (store work) | 7.7 ms | the rows reuse the pre-sorted set; no re-sort |
| Switch a filter tab (store work) | 3.5 ms | one pass over the already sorted rows |
| DOM rows at 10,000 hosts | 16 | windowed: 10 visible + 6 overscan, regardless of the total |

The sort of the full set and the search index are derived from the data:
they are rebuilt when the hosts change (~200 ms for 10,000 — one-time),
never on a keystroke. Every keystroke is a pure filter, and the tab counts
are a single pass, not four. The test asserts the store's own work at 50 ms
(the design goal is one frame, 16.7 ms — measured 4.7–8 ms idle and ~26 ms
under the load of the full test suite running 21 workers on 2 cores; the
50 ms budget stays robust on a loaded CI machine and still catches the
classes it replaced: a re-sort per keystroke cost ~155 ms, the original
unwindowed table cost 2.2 s) plus a coarse total guard (150 ms including
the jsdom flush, which is unstable at 10–90 ms).

**Budget:** filtering stays inside a frame at 10,000 hosts, and the DOM
holds a constant number of rows.

## The SSH session

`cargo test -p termru-ssh --test sshd large_output_flood -- --nocapture`

| Operation | Time |
|---|---|
| 4 MiB of terminal output (through a real `sshd`, PTY included) | 2.64 s — about 1.5 MiB/s |

The measurement includes the round trip through the remote shell, the PTY
and the backpressure of a deliberately small queue (64 entries), so it is a
pessimistic number. The session stays responsive during the flood (the test
runs a command afterwards and waits for its output).

**Budget:** a `cat` of a large file must not freeze the UI. At 1.5 MiB/s
the terminal keeps up with everything a shell prints; a bulk transfer is
better done with `scp`/`rsync`.

## The cloud server

The numbers come from the built binary (`target/debug/termru-cloud-server`)
against a local PostgreSQL.

| Operation | Time |
|---|---|
| Start-up to ready (process start → `/readyz` answers) | 0.23 s (migrations included) |
| `/healthz` | 41,500 requests/s (keep-alive, one connection) |
| `/readyz` (a database round trip) | < 20 ms |
| Resident memory, idle | 7 MiB |

**Budget:** the server has to start fast enough for a container restart to
be invisible (healthcheck gating, 10 s grace) and to stay small enough to
run next to the database. 7 MiB of RSS and a quarter of a second to ready
are both comfortable.

## What was not measured

* **The desktop app's start-up and idle memory.** The build machine has no
  window manager, so the window could not be shown; the numbers would be
  meaningless. The measurement has to be taken on a real desktop session.
* **The UI responsiveness during a flood.** Same reason: it needs a visible
  window and a human eye (or a screen recording).
* **The sync round trip with many records.** The end-to-end test measures
  the path, not its latency at scale; a measurement with 10,000 records on
  both sides is still open.
* **Windows and macOS.** Everything above was measured on Linux only.
