# ADR 0009: Notices go through one facade, with its own copy

* Status: Accepted
* Date: 2026-11-27
* Deciders: Termru maintainers
* Source: design specification C.03 and C.04 (`re-design/10.svg`)

## Context

Until now any code could call `toasts.error(someString)`. The string could be a
backend message, a server banner or a host name, and it went to the screen as it
was. The same failure could pile up as ten identical toasts, notices of every
kind were announced the same way by a screen reader, and there was no way to
turn the routine ones off. The design specification asks for a typed facade:
messages named by key, a whitelist of copy, deduplication by entity, a bounded
queue, an `alert` for what needs action and a polite status for the rest, one
position (top right, below the navigation), and no library call outside one
adapter.

The specification also says that a React island with Sileo is a separate choice
that needs the user's approval (it adds a second UI runtime and its cost to the
bundle). That approval has not been given, so the notices stay native Svelte.

## Decision

1. **One entry point.** `notify({ key, entityId?, values?, action? })` in
   `apps/desktop/src/lib/notify.svelte.ts`. Nothing else in the app puts a
   notice on the screen. The old `toasts` store is gone.
2. **Copy whitelist.** `COPY` is the only source of titles and sentences, keyed
   (`host.saved`, `sync.failed`, ...). A key that is not in it shows nothing.
   The kind (`success`, `info`, `warning`, `error`) comes from the table, not
   from the caller, so a failure cannot be sent as a success. A caller supplies
   only values (`name`, `place`, `detail`, counts). Terminal output has no key
   and no value slot: it cannot reach a notice.
3. **Cleaned values.** Every text value has control, bidirectional-override and
   zero-width characters replaced, whitespace collapsed to one line, and a length
   cap (80 for a name, 120 for a place, 220 for a reason). Counts are clamped
   whole numbers. Notices are rendered as text nodes only (`{@html}` is banned
   by `hardening.test.ts`).
4. **Dedup.** The same notice (same key and entity, or the same key and values)
   while it is on screen or waiting is one notice: it keeps its place, takes the
   newest text and its timer restarts. Once it has left, the same event is news
   again.
5. **Bounded queue.** At most 2 notices are visible (the specification says two)
   and at most 6 wait. A failure does not wait behind a routine notice: it takes
   the slot of the oldest routine one, or goes to the front of the queue when
   every slot holds a failure. When the queue is full the oldest routine notice
   is dropped, never a failure.
6. **Time.** A routine notice stays 6 s (the specification's default), a failure
   10 s. The timer waits while the pointer or the keyboard focus is on a notice
   and while the window is hidden. Nothing is sticky: a failure that needs a
   decision is also shown inline on its page.
7. **Roles.** A failure or a warning is `role="alert"`, every other kind is
   `role="status"`. A notice never takes the focus. Its dismiss button and its
   action (`Retry` for a failed sync) are reachable with Tab.
8. **Preference.** `routine_notices` off silences `success` and `info`. Failures
   and warnings always show. The Preferences page can preview a notice
   regardless of the setting (`prefs.preview`).
9. **Placement and motion.** One fixed viewport at the top right, 24 px from the
   edge and below the title bar and navigation (`top: 36 + 64 + 24`), outside the
   route outlet, so it never covers the navigation or a dialog's buttons. A
   notice enters with opacity and an 8 px translate in 220 ms and leaves in
   160 ms, as CSS transitions on the shared motion tokens; with reduced motion
   the translate is removed and only an opacity change of at most 80 ms remains.
10. **One adapter.** `components/Toasts.svelte` is the only place that draws a
    notice. It has no policy. Swapping the drawing (or adopting a library after
    approval) means changing that file, not the 40 call sites.

## Consequences

* Adding a notice means adding a key and its copy. That is a small cost on
  purpose: the sentence is reviewed once, in one file, and tests can list it.
* A message from the backend still reaches the user, but only as the `detail` of
  a keyed failure, cleaned and capped, never as a title.
* Tests assert on keys, kinds, roles and the copy, not on a free string.
* There is no bounce, spring or blur anywhere in the notices (`motion.test.ts`
  scans for them).
* Not decided here: sound (none, by design), notices from other windows (each
  window shows its own), and a persistent notification centre.

## Related

* The opener plugin (links in terminal output) and its capability scope are
  recorded in `docs/security/threat-model.md` section 3.4.
