/**
 * Connects a pane's terminal to a backend session.
 *
 * The terminal outlives its Svelte host (see `registry.ts`), so connecting is
 * driven by the pane's `generation`, never by a component mounting: a remount
 * finds the terminal already connected for that generation and does nothing.
 * Nothing here ever closes a live session; the only sessions this module
 * closes are one that is already dead, and one it opened itself that was
 * superseded meanwhile.
 */

import { api } from '../ipc';
import { Channel } from '../ipc-backend';
import { errorMessage } from '../stores/hosts.svelte';
import { sessions, type Pane } from '../stores/sessions.svelte';
import type { HopSecrets, SessionGeometry, SessionInfo, SessionUpdate } from '../types';
import { decodeBase64 } from './codec';
import { fitTerminal, terminalSize, whenFontsReady } from './create';
import type { TerminalHandle } from './registry';
import { clearWriteQueue, writeToSession } from './write-queue';

/** Sends what the user typed to the session of a pane. */
export function sendInput(paneId: string, data: string): void {
  const sessionId = sessions.paneById(paneId)?.sessionId;
  if (!sessionId) return;
  // Not a bare `session_write` per keystroke: concurrent invokes may be
  // delivered out of order, and a full input queue must not drop input.
  writeToSession(sessionId, data);
}

/** Tells the backend the grid size of a terminal (best effort). */
export function pushSize(handle: TerminalHandle, sessionId: string): void {
  const { cols, rows } = terminalSize(handle);
  void api.sessionResize(sessionId, cols, rows).catch(() => {
    // Resize failures are transient (session closing).
  });
}

/** The states in which the backend still runs (or is starting) a session. */
function isLive(info: SessionInfo): boolean {
  const state = info.status.state;
  return state === 'ready' || state === 'connecting' || state === 'authenticating';
}

interface Opened {
  sessionId: string;
  /** True when this call created a new backend session. */
  created: boolean;
}

/**
 * Starts the connection of a pane for `generation`, unless this terminal
 * already did (a remount) or the pane waits for the user (a restored pane).
 */
export async function startConnection(
  pane: Pane,
  handle: TerminalHandle,
  generation: number,
): Promise<void> {
  // An empty pane has no host: there is nothing to connect to.
  if (!pane.host) return;
  // This terminal already connected for this generation: a remount only
  // re-shows it.
  if (handle.generation === generation) return;

  // A pane restored from the saved layout stays idle until the user clicks
  // Reconnect (which bumps the generation).
  if (handle.generation < 0 && pane.restored && !pane.sessionId && generation === 0) return;

  handle.generation = generation;

  // The previous attempt's channel must not update the pane any more.
  if (handle.channel) handle.channel.onmessage = () => {};

  const channel = new Channel<SessionUpdate>();
  const paneId = pane.id;
  channel.onmessage = (update) => {
    // A stale channel or a removed pane is ignored.
    if (handle.disposed || handle.channel !== channel) return;
    if (update.kind === 'data') {
      handle.term.write(decodeBase64(update.base64));
      return;
    }
    const stored = sessions.paneById(paneId);
    if (stored) sessions.applyUpdate(stored, update);
  };
  handle.channel = channel;

  /** True while this attempt is still the one the pane wants. */
  const current = (): boolean =>
    !handle.disposed &&
    handle.channel === channel &&
    pane.generation === generation &&
    sessions.paneById(paneId) !== null;

  // The first fit waits for the fonts: the real cell size decides the grid,
  // and the backend must start with the grid the user will see.
  await whenFontsReady(handle);
  if (!current()) return;
  fitTerminal(handle);
  const geometry = terminalSize(handle);

  try {
    const opened = await openOrAttach(pane, handle, channel, geometry);
    if (!current()) {
      // Superseded (or the pane was removed): drop the session we started.
      if (opened.created) void api.sessionClose(opened.sessionId).catch(() => {});
      return;
    }
    pane.sessionId = opened.sessionId;
    pane.duplicateFrom = null;
    pane.freshConnect = false;
    // The grid may have changed while the call was in flight (layout, fonts):
    // tell the session the size the user sees now.
    fitTerminal(handle);
    pushSize(handle, opened.sessionId);
  } catch (err) {
    if (current()) {
      // A duplicate whose source is gone must not be retried as a duplicate.
      pane.duplicateFrom = null;
      sessions.fail(pane, errorMessage(err));
    }
  }
}

async function openOrAttach(
  pane: Pane,
  handle: TerminalHandle,
  channel: Channel<SessionUpdate>,
  geometry: SessionGeometry,
): Promise<Opened> {
  // The session lives in another window: attach to it (the backend sends the
  // replay buffer, then the live stream). It is never closed from here.
  if (pane.attachTo && !pane.sessionId) {
    await api.sessionAttach(pane.attachTo, channel);
    return { sessionId: pane.attachTo, created: false };
  }
  if (pane.sessionId) return reopen(pane, handle, channel, geometry);
  return openFresh(pane, channel, geometry);
}

/** The secrets typed for the jump hosts of the pane, or null when there are none. */
function typedHopSecrets(pane: Pane): HopSecrets | null {
  const typed = Object.entries(pane.hopSecrets ?? {}).filter(
    ([, secrets]) => secrets.password || secrets.passphrase,
  );
  return typed.length > 0 ? Object.fromEntries(typed) : null;
}

async function openFresh(
  pane: Pane,
  channel: Channel<SessionUpdate>,
  geometry: SessionGeometry,
): Promise<Opened> {
  if (!pane.host) throw new Error('the pane has no host');
  if (pane.duplicateFrom) {
    const id = await api.sessionDuplicate(pane.duplicateFrom, geometry, channel);
    return { sessionId: id, created: true };
  }
  const hops = typedHopSecrets(pane);
  const id = await api.sessionOpen(
    pane.host.id,
    {
      cols: geometry.cols,
      rows: geometry.rows,
      secrets: pane.secrets,
      // Only what was typed for jump hosts; the others use the credential store.
      ...(hops ? { hop_secrets: hops } : {}),
    },
    channel,
  );
  return { sessionId: id, created: true };
}

/**
 * A pane that already has a session id asks for a connection again.
 *
 * * the backend still runs the session: attach the view, never close it;
 * * the backend stopped it (failed, gave up): re-run it with its stored
 *   credentials (`session_retry`), unless the user supplied new ones;
 * * the backend forgot it, or the credentials changed: open a new session.
 */
async function reopen(
  pane: Pane,
  handle: TerminalHandle,
  channel: Channel<SessionUpdate>,
  geometry: SessionGeometry,
): Promise<Opened> {
  const previous = pane.sessionId as string;
  let known: SessionInfo | undefined;
  try {
    known = (await api.sessionList()).find((entry) => entry.id === previous);
  } catch {
    known = undefined;
  }

  if (known && isLive(known)) {
    // Never close a live session: show it again. The replay would double
    // what is on screen, so the screen starts empty.
    handle.term.reset();
    // What the session says about its forwards (the attach repeats it).
    for (const forward of known.forwards ?? []) sessions.applyForward(pane, forward);
    if (known.exec_pending) sessions.markExecPending(pane);
    await api.sessionAttach(previous, channel);
    return { sessionId: previous, created: false };
  }

  if (known && !pane.freshConnect) {
    try {
      // The backend keeps the connection parameters of a stopped session.
      await api.sessionRetry(previous, geometry, channel);
      return { sessionId: previous, created: false };
    } catch {
      // Cancelled or still busy: fall through to a new session.
    }
  }

  // Start over. The old entry is dead (or replaced by new credentials):
  // release it so the backend never keeps an orphan.
  pane.sessionId = null;
  // Input typed for the old session must not reach it (or the new one) late.
  clearWriteQueue(previous);
  if (known) {
    try {
      await api.sessionClose(previous);
    } catch {
      // Already gone.
    }
  }
  return openFresh(pane, channel, geometry);
}
