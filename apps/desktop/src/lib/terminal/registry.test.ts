// @vitest-environment jsdom
/**
 * The terminal registry: an xterm instance outlives the component that shows
 * it, so a route change, a split or an unsplit never loses the scrollback and
 * never starts (or closes) a session.
 */

import { flushSync, mount, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import Terminal from '../components/Terminal.svelte';
import { prefs } from '../stores/prefs.svelte';
import { sessions } from '../stores/sessions.svelte';
import { callsOf, resetTauriMock } from '../test-support/tauri-core-mock';
import {
  fitSize,
  resetXtermMock,
  terminals,
  webglAddons,
  WebglAddon as FakeWebgl,
} from '../test-support/xterm-mock';
import type { Host } from '../types';
import { appearanceFromPrefs } from './appearance';
import { createTerminal, setAccelerated } from './create';
import {
  attachTerminal,
  detachTerminal,
  disposeTerminal,
  getTerminal,
  pruneTerminals,
  terminalCount,
} from './registry';

vi.mock('@tauri-apps/api/core', () => import('../test-support/tauri-core-mock'));
vi.mock('@tauri-apps/api/event', () => ({ listen: async () => () => {} }));
vi.mock('@xterm/xterm', () => import('../test-support/xterm-mock'));
vi.mock('@xterm/addon-fit', () => import('../test-support/xterm-mock'));
vi.mock('@xterm/addon-webgl', () => import('../test-support/xterm-mock'));

function host(name = 'web-1'): Host {
  const now = new Date().toISOString();
  return {
    id: `host-${name}`,
    name,
    address: `${name}.test`,
    port: 22,
    username: 'deploy',
    auth: { type: 'agent' },
    group_id: null,
    tags: [],
    favorite: false,
    description: null,
    term: 'xterm-256color',
    keepalive_seconds: 0,
    sort_order: 0,
    created_at: now,
    updated_at: now,
    deleted: false,
  };
}

const hooks = { onData: vi.fn<(data: string) => void>(), isAppKey: () => false };

let containers: HTMLElement[] = [];

function container(): HTMLElement {
  const element = document.createElement('div');
  document.body.appendChild(element);
  containers.push(element);
  return element;
}

beforeEach(() => {
  resetTauriMock();
  resetXtermMock();
  pruneTerminals(new Set());
  sessions.tabs = [];
  sessions.activeId = null;
  sessions.windowLabel = null;
  hooks.onData.mockClear();
  (globalThis as { ResizeObserver?: unknown }).ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  };
});

afterEach(() => {
  for (const element of containers) element.remove();
  containers = [];
  pruneTerminals(new Set());
});

describe('terminal registry', () => {
  it('re-parents the element on attach and keeps the terminal on detach', () => {
    const first = container();
    const second = container();
    const handle = createTerminal('pane-1', first, appearanceFromPrefs(), hooks);
    expect(handle.element.parentElement).toBe(first);

    detachTerminal(handle, first);
    expect(handle.element.parentElement).toBeNull();
    expect(handle.term).toBe(terminals[0]);
    expect(terminals[0].disposed).toBe(false);
    expect(getTerminal('pane-1')).toBe(handle);

    attachTerminal(handle, second);
    expect(handle.element.parentElement).toBe(second);
    expect(handle.host).toBe(second);
  });

  it('ignores a detach from a container that lost the terminal to another one', () => {
    // A split mounts the new component before the old one is torn down.
    const oldHost = container();
    const newHost = container();
    const handle = createTerminal('pane-1', oldHost, appearanceFromPrefs(), hooks);
    attachTerminal(handle, newHost);
    detachTerminal(handle, oldHost);
    expect(handle.element.parentElement).toBe(newHost);
    expect(handle.host).toBe(newHost);
  });

  it('disposes a terminal only on request, and releases its channel and renderer', () => {
    const handle = createTerminal('pane-1', container(), appearanceFromPrefs(), hooks);
    const channel = { onmessage: vi.fn() };
    handle.channel = channel;
    setAccelerated(handle, true);
    const gpu = webglAddons[0];

    disposeTerminal('pane-1');
    expect(terminals[0].disposed).toBe(true);
    expect(gpu.disposed).toBe(true);
    expect(getTerminal('pane-1')).toBeUndefined();
    // A late update from the backend no longer reaches anything.
    channel.onmessage();
    expect(handle.channel).toBeNull();
    expect(handle.element.isConnected).toBe(false);
  });

  it('prunes the terminals whose pane is gone', () => {
    createTerminal('keep', container(), appearanceFromPrefs(), hooks);
    createTerminal('drop', container(), appearanceFromPrefs(), hooks);
    pruneTerminals(new Set(['keep']));
    expect(terminalCount()).toBe(1);
    expect(getTerminal('keep')).toBeDefined();
    expect(getTerminal('drop')).toBeUndefined();
  });
});

describe('terminal renderer', () => {
  it('falls back to the DOM renderer when the GPU context is lost', () => {
    const handle = createTerminal('pane-1', container(), appearanceFromPrefs(), hooks);
    setAccelerated(handle, true);
    expect(handle.webgl).not.toBeNull();

    (webglAddons[0] as FakeWebgl).loseContext();
    expect(webglAddons[0].disposed).toBe(true);
    expect(handle.webgl).toBeNull();
    // It can be loaded again later (the terminal is still usable).
    setAccelerated(handle, true);
    expect(webglAddons).toHaveLength(2);
  });

  it('holds no GPU context while the terminal is not shown, and caps the total', () => {
    const handle = createTerminal('pane-1', container(), appearanceFromPrefs(), hooks);
    setAccelerated(handle, true);
    setAccelerated(handle, false);
    expect(webglAddons[0].disposed).toBe(true);
    expect(handle.webgl).toBeNull();

    for (let i = 0; i < 12; i += 1) {
      setAccelerated(createTerminal(`many-${i}`, container(), appearanceFromPrefs(), hooks), true);
    }
    expect(webglAddons.filter((addon) => !addon.disposed).length).toBeLessThanOrEqual(8);
  });
});

describe('terminal keyboard and clipboard', () => {
  it('leaves app shortcuts to the window and everything else to the shell', () => {
    const isAppKey = vi.fn((event: KeyboardEvent) => event.key === 'P');
    createTerminal('pane-1', container(), appearanceFromPrefs(), { ...hooks, isAppKey });
    const handler = terminals[0].keyHandler;
    expect(handler).not.toBeNull();
    expect(handler?.(new KeyboardEvent('keydown', { key: 'P', ctrlKey: true }))).toBe(false);
    expect(handler?.(new KeyboardEvent('keydown', { key: 'c', ctrlKey: true }))).toBe(true);
  });

  it('copies the selection when copy_on_select is on, and only then', () => {
    const writeText = vi.fn(async () => undefined);
    Object.defineProperty(navigator, 'clipboard', { value: { writeText }, configurable: true });
    const appearance = { ...appearanceFromPrefs(), copyOnSelect: false };
    const handle = createTerminal('pane-1', container(), appearance, hooks);

    terminals[0].selection = 'secret-ish text';
    terminals[0].selectionHandler?.();
    expect(writeText).not.toHaveBeenCalled();

    handle.copyOnSelect = true;
    terminals[0].selectionHandler?.();
    expect(writeText).toHaveBeenCalledWith('secret-ish text');

    // An emptied selection copies nothing.
    writeText.mockClear();
    terminals[0].selection = '';
    terminals[0].selectionHandler?.();
    expect(writeText).not.toHaveBeenCalled();
  });
});

describe('Terminal host component', () => {
  /** Opens a tab for a host and returns its pane (as the store holds it). */
  function paneForNewTab() {
    const tab = sessions.open(host(), {});
    const pane = sessions.activePaneOf(tab);
    if (!pane) throw new Error('no pane');
    return pane;
  }

  it('keeps the terminal and the session when the page is left and entered again', async () => {
    const pane = paneForNewTab();
    const first = container();
    const view = mount(Terminal, { target: first, props: { pane } });
    await vi.waitFor(() => expect(callsOf('session_open')).toHaveLength(1));
    await vi.waitFor(() => expect(pane.sessionId).toBe('session-1'));
    expect(terminals).toHaveLength(1);

    // Leaving the Sessions route destroys the component...
    unmount(view);
    flushSync();
    expect(terminals[0].disposed).toBe(false);
    expect(getTerminal(pane.id)?.element.isConnected).toBe(false);

    // ...and entering it again shows the same terminal, without a new
    // connection and without closing the old one.
    const second = container();
    const again = mount(Terminal, { target: second, props: { pane } });
    flushSync();
    await Promise.resolve();
    expect(terminals).toHaveLength(1);
    expect(second.contains(getTerminal(pane.id)?.element as Node)).toBe(true);
    expect(callsOf('session_open')).toHaveLength(1);
    expect(callsOf('session_close')).toHaveLength(0);
    expect(callsOf('session_attach')).toHaveLength(0);
    expect(pane.sessionId).toBe('session-1');
    unmount(again);
  });

  it('keeps the scrollback across a remount', async () => {
    const pane = paneForNewTab();
    const view = mount(Terminal, { target: container(), props: { pane } });
    await vi.waitFor(() => expect(pane.sessionId).toBe('session-1'));
    const handle = getTerminal(pane.id);
    handle?.channel?.onmessage({ kind: 'data', base64: btoa('hello from the shell') });
    expect(new TextDecoder().decode(terminals[0].writes[0])).toBe('hello from the shell');

    unmount(view);
    // Output that arrives while the page is away is still written.
    handle?.channel?.onmessage({ kind: 'data', base64: btoa(' and more') });
    expect(terminals[0].writes).toHaveLength(2);
  });

  it('survives a split: the pane component is rebuilt around the same terminal', async () => {
    const pane = paneForNewTab();
    const target = container();
    const view = mount(Terminal, { target, props: { pane } });
    await vi.waitFor(() => expect(pane.sessionId).toBe('session-1'));
    const surface = getTerminal(pane.id)?.element;

    // The new component mounts before the old one is torn down.
    const rebuilt = container();
    const next = mount(Terminal, { target: rebuilt, props: { pane } });
    flushSync();
    unmount(view);
    flushSync();

    expect(terminals).toHaveLength(1);
    expect(getTerminal(pane.id)?.element).toBe(surface);
    expect(surface?.isConnected).toBe(true);
    expect(rebuilt.contains(surface as Node)).toBe(true);
    expect(callsOf('session_close')).toHaveLength(0);
    unmount(next);
  });

  it('is disposed with its pane by the session store, and by nothing else', async () => {
    const tab = sessions.open(host(), {});
    const pane = sessions.activePaneOf(tab);
    if (!pane) throw new Error('no pane');
    const view = mount(Terminal, { target: container(), props: { pane } });
    await vi.waitFor(() => expect(pane.sessionId).toBe('session-1'));
    unmount(view);
    expect(terminals[0].disposed).toBe(false);

    await sessions.closePane(pane);
    expect(terminals[0].disposed).toBe(true);
    expect(terminalCount()).toBe(0);
  });

  it('applies the scrollback preference to a live terminal', async () => {
    const pane = paneForNewTab();
    const view = mount(Terminal, { target: container(), props: { pane } });
    await vi.waitFor(() => expect(terminals).toHaveLength(1));
    prefs.prefs = { ...prefs.prefs, terminal: { ...prefs.prefs.terminal, scrollback: 4321 } };
    flushSync();
    expect(terminals[0].options.scrollback).toBe(4321);
    unmount(view);
  });

  it('sends the size the terminal has once the session is open', async () => {
    // The container has a size in this test, so the fit addon runs.
    const sizes = { width: 900, height: 600 };
    const originalWidth = Object.getOwnPropertyDescriptor(HTMLElement.prototype, 'offsetWidth');
    const originalHeight = Object.getOwnPropertyDescriptor(HTMLElement.prototype, 'offsetHeight');
    Object.defineProperty(HTMLElement.prototype, 'offsetWidth', {
      configurable: true,
      get: () => sizes.width,
    });
    Object.defineProperty(HTMLElement.prototype, 'offsetHeight', {
      configurable: true,
      get: () => sizes.height,
    });
    try {
      fitSize.cols = 132;
      fitSize.rows = 41;
      const pane = paneForNewTab();
      const view = mount(Terminal, { target: container(), props: { pane } });
      await vi.waitFor(() => expect(pane.sessionId).toBe('session-1'));

      // The session was opened with the fitted grid...
      const open = callsOf('session_open')[0];
      expect((open.args?.options as { cols: number; rows: number }).cols).toBe(132);
      // ...and told again after it resolved, for a grid that changed meanwhile.
      await vi.waitFor(() => {
        const resize = callsOf('session_resize').pop();
        expect(resize?.args).toEqual({ id: 'session-1', cols: 132, rows: 41 });
      });
      unmount(view);
    } finally {
      if (originalWidth) Object.defineProperty(HTMLElement.prototype, 'offsetWidth', originalWidth);
      if (originalHeight) {
        Object.defineProperty(HTMLElement.prototype, 'offsetHeight', originalHeight);
      }
    }
  });
});
