/**
 * The registry of live terminals.
 *
 * An xterm instance holds the scrollback of a session: it must outlive the
 * Svelte component that shows it. Leaving the Sessions page, splitting a
 * pane or closing a split all destroy and recreate the pane components, and
 * a terminal that died with them would lose its content (and, worse, would
 * have reconnected a new session on return).
 *
 * The registry owns the terminals, keyed by pane id. A component only
 * *hosts* one: it re-parents the terminal's element into its container on
 * mount and detaches it (without disposing) on unmount. A terminal is
 * disposed only when its pane is really removed (see the session store).
 *
 * This module is deliberately free of runtime imports (xterm is only
 * referenced as a type), so the stores can use it without loading xterm.
 */

import type { FitAddon } from '@xterm/addon-fit';
import type { WebglAddon } from '@xterm/addon-webgl';
import type { Terminal } from '@xterm/xterm';
import type { SessionUpdate } from '../types';

/** The backend channel of a connection, as the terminal needs it. */
export interface ChannelHandle {
  onmessage: (update: SessionUpdate) => void;
}

export interface TerminalHandle {
  paneId: string;
  term: Terminal;
  fit: FitAddon;
  /** The element xterm renders into; it survives re-parenting. */
  element: HTMLElement;
  /** The container that shows `element` right now (null while detached). */
  host: HTMLElement | null;
  /** The GPU renderer, loaded only while the terminal is visible. */
  webgl: WebglAddon | null;
  /** The channel of the current connection attempt (null before it). */
  channel: ChannelHandle | null;
  /**
   * The pane generation this terminal last started a connection for;
   * -1 = it never connected. A remount compares it with the pane's
   * generation, so it never starts (or closes) a session by itself.
   */
  generation: number;
  /** The `copy_on_select` preference, kept current by the host. */
  copyOnSelect: boolean;
  /** True once the web fonts were awaited for the first fit. */
  fontsReady: boolean;
  disposed: boolean;
  /** Releases everything the terminal holds (set by the factory). */
  dispose: () => void;
}

const handles = new Map<string, TerminalHandle>();

/** Adds a terminal to the registry (the factory calls this). */
export function registerTerminal(handle: TerminalHandle): void {
  handles.set(handle.paneId, handle);
}

/** The terminal of a pane, if it has one. */
export function getTerminal(paneId: string): TerminalHandle | undefined {
  return handles.get(paneId);
}

/** The ids of the panes that have a terminal. */
export function terminalIds(): string[] {
  return [...handles.keys()];
}

/** The number of terminals alive. */
export function terminalCount(): number {
  return handles.size;
}

/** How many terminals currently hold a GPU renderer. */
export function webglCount(): number {
  let count = 0;
  for (const handle of handles.values()) if (handle.webgl) count += 1;
  return count;
}

/** Shows a terminal inside a container (moves its element there). */
export function attachTerminal(handle: TerminalHandle, host: HTMLElement): void {
  if (handle.element.parentElement !== host) host.appendChild(handle.element);
  handle.host = host;
}

/**
 * Takes a terminal out of a container without disposing it. A no-op when
 * another container has taken the terminal over meanwhile: during a layout
 * change the new component may mount before the old one is torn down.
 */
export function detachTerminal(handle: TerminalHandle, host: HTMLElement): void {
  if (handle.host !== host) return;
  if (handle.element.parentElement === host) host.removeChild(handle.element);
  handle.host = null;
}

/** Disposes the terminal of a pane (the pane is really gone). */
export function disposeTerminal(paneId: string): void {
  const handle = handles.get(paneId);
  if (!handle) return;
  handles.delete(paneId);
  handle.dispose();
}

/** Disposes every terminal whose pane is not in `keep`. */
export function pruneTerminals(keep: ReadonlySet<string>): void {
  for (const id of terminalIds()) {
    if (!keep.has(id)) disposeTerminal(id);
  }
}
