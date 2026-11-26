// @vitest-environment jsdom
/**
 * The session store around windows and timers: the reconnect watchdog, moving
 * tabs to new windows, and the rules for who may write the saved layout.
 */

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { SessionStore, type Pane, type Tab } from './sessions.svelte';
import { answers, callsOf, failures, resetTauriMock } from '../test-support/tauri-core-mock';
import type { Host } from '../types';

vi.mock('@tauri-apps/api/core', () => import('../test-support/tauri-core-mock'));

function host(name = 'web-1'): Host {
  const now = new Date().toISOString();
  return {
    id: `host-${name}`,
    name,
    address: `${name}.test`,
    port: 22,
    username: 'deploy',
    auth: { type: 'password' },
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

function paneOf(store: SessionStore, tab: Tab): Pane {
  const pane = store.activePaneOf(tab);
  if (!pane) throw new Error('the tab has no active pane');
  return pane;
}

beforeEach(() => {
  resetTauriMock();
  vi.useFakeTimers();
});

afterEach(() => {
  vi.useRealTimers();
});

describe('the reconnect watchdog', () => {
  /** A pane that was connected and lost its network: the backend retries. */
  function droppedPane(store: SessionStore): Pane {
    const pane = paneOf(store, store.open(host(), {}));
    store.applyStatus(pane, { state: 'ready' });
    store.applyStatus(pane, { state: 'failed', category: 'network', message: 'reset' });
    return pane;
  }

  it('never stacks: a second close replaces the pending timeout', () => {
    const store = new SessionStore();
    const pane = droppedPane(store);
    store.applyStatus(pane, { state: 'closed', reason: 'reset' });
    store.applyStatus(pane, { state: 'closed', reason: 'reset' });
    store.applyStatus(pane, { state: 'closed', reason: 'reset' });
    // The only timers are the watchdog (one) and the debounced layout save.
    const watchdogs = vi.getTimerCount() - 1;
    expect(watchdogs).toBe(1);
  });

  it('is cleared when the pane becomes ready or reconnects', () => {
    const store = new SessionStore();
    const pane = droppedPane(store);
    store.applyStatus(pane, { state: 'closed', reason: 'reset' });
    const armed = vi.getTimerCount();
    store.applyStatus(pane, { state: 'ready' });
    expect(vi.getTimerCount()).toBe(armed - 1);
    expect(pane.awaitingReconnect).toBe(false);

    // A stale watchdog must not kill the pane that recovered.
    vi.advanceTimersByTime(60_000);
    expect(pane.dead).toBe(false);
  });

  it('declares the pane dead when the backend never comes back', () => {
    const store = new SessionStore();
    const pane = droppedPane(store);
    store.applyStatus(pane, { state: 'closed', reason: 'reset' });
    expect(pane.awaitingReconnect).toBe(true);
    vi.advanceTimersByTime(30_000);
    expect(pane.dead).toBe(true);
    expect(pane.awaitingReconnect).toBe(false);
  });

  it('is cleared by a retry and by closing the pane', async () => {
    const store = new SessionStore();
    const pane = droppedPane(store);
    store.applyStatus(pane, { state: 'closed', reason: 'reset' });
    store.retry(pane);
    vi.advanceTimersByTime(30_000);
    // `retry` made the pane "connecting"; the old timeout did not kill it.
    expect(pane.dead).toBe(false);

    store.applyStatus(pane, { state: 'closed', reason: 'reset again' });
    expect(pane.awaitingReconnect).toBe(true);
    await store.closePane(pane);
    // The pane is gone: its watchdog must not fire on it later.
    vi.advanceTimersByTime(30_000);
    expect(pane.dead).toBe(false);
  });

  it('ignores an update for a pane that was removed', () => {
    const store = new SessionStore();
    const tab = store.open(host(), {});
    const pane = paneOf(store, tab);
    store.tabs = [];
    store.applyStatus(pane, { state: 'ready' });
    expect(pane.status.state).toBe('connecting');
  });
});

describe('renaming a tab', () => {
  it('schedules the layout save', async () => {
    const store = new SessionStore();
    store.layoutLoaded = true;
    const tab = store.open(host(), {});
    await vi.advanceTimersByTimeAsync(1000);
    resetTauriMock();

    store.rename(tab, 'production web');
    await vi.advanceTimersByTimeAsync(1000);
    const saves = callsOf('layout_save');
    expect(saves).toHaveLength(1);
    const doc = saves[0].args?.doc as { windows: { tabs: { title: string }[] }[] };
    expect(doc.windows[0].tabs[0].title).toBe('production web');
  });
});

describe('moving a tab to new windows', () => {
  /** A tab with `ready` connected panes and one that is not connected. */
  function splitTab(store: SessionStore): Tab {
    const tab = store.open(host('a'), {});
    const first = paneOf(store, tab);
    first.sessionId = 's-a';
    store.applyStatus(first, { state: 'ready' });
    const second = store.splitActive('columns');
    if (!second) throw new Error('no split');
    store.focusPane(first);
    return tab;
  }

  it('moves every connected pane, not only the active one', async () => {
    const store = new SessionStore();
    const tab = store.open(host('a'), {});
    const first = paneOf(store, tab);
    first.sessionId = 's-a';
    store.applyStatus(first, { state: 'ready' });
    const second = store.splitActive('columns');
    if (!second) throw new Error('no split');
    store.connectPane(second, host('b'));
    second.sessionId = 's-b';
    store.applyStatus(second, { state: 'ready' });

    await store.moveToNewWindow(tab);

    expect(callsOf('window_open_session').map((call) => call.args?.id)).toEqual(['s-a', 's-b']);
    // The main window stops showing the sessions, without closing them.
    expect(callsOf('session_detach').map((call) => call.args?.id)).toEqual(['s-a', 's-b']);
    expect(callsOf('session_close')).toHaveLength(0);
    expect(store.tabs).toHaveLength(0);
  });

  it('keeps the panes that are not connected', async () => {
    const store = new SessionStore();
    const tab = splitTab(store);
    await store.moveToNewWindow(tab);

    expect(callsOf('window_open_session').map((call) => call.args?.id)).toEqual(['s-a']);
    expect(store.tabs).toHaveLength(1);
    expect(store.tabs[0].panes).toHaveLength(1);
    // The tree collapsed to the pane that stayed.
    expect(store.tabs[0].root.kind).toBe('leaf');
    expect(store.tabs[0].activePaneId).toBe(store.tabs[0].panes[0].id);
  });

  it('saves the layout after the move', async () => {
    const store = new SessionStore();
    store.layoutLoaded = true;
    const tab = splitTab(store);
    await vi.advanceTimersByTimeAsync(1000);
    resetTauriMock();

    await store.moveToNewWindow(tab);
    await vi.advanceTimersByTimeAsync(1000);
    const doc = callsOf('layout_save').pop()?.args?.doc as {
      windows: { tabs: unknown[] }[];
    };
    expect(doc.windows[0].tabs).toHaveLength(1);
  });

  it('does nothing when no pane is connected', async () => {
    const store = new SessionStore();
    const tab = store.open(host(), {});
    await store.moveToNewWindow(tab);
    expect(callsOf('window_open_session')).toHaveLength(0);
    expect(store.tabs).toHaveLength(1);
  });

  it('keeps the tab when the window cannot be opened', async () => {
    const store = new SessionStore();
    const tab = splitTab(store);
    failures.set('window_open_session', 'only a connected session can be moved');
    await store.moveToNewWindow(tab);
    expect(store.tabs).toHaveLength(1);
    expect(store.tabs[0].panes).toHaveLength(2);
    expect(callsOf('session_detach')).toHaveLength(0);
  });

  it('survives a backend without session_detach', async () => {
    const store = new SessionStore();
    const tab = splitTab(store);
    failures.set('session_detach', 'Command session_detach not found');
    await store.moveToNewWindow(tab);
    expect(callsOf('window_open_session')).toHaveLength(1);
    expect(store.tabs[0].panes).toHaveLength(1);
  });
});

describe('a session window', () => {
  it('never writes the saved layout', async () => {
    const store = new SessionStore();
    store.windowLabel = 'session-s9';
    store.layoutLoaded = true;
    expect(store.isSessionWindow).toBe(true);
    store.attachTab('s9', host());
    store.scheduleLayoutSave();
    await vi.advanceTimersByTimeAsync(2000);
    expect(callsOf('layout_save')).toHaveLength(0);
  });

  it('closes its window when its last tab is closed', async () => {
    const store = new SessionStore();
    store.windowLabel = 'session-s9';
    const tab = store.attachTab('s9', host());
    store.applyStatus(paneOf(store, tab), { state: 'ready' });
    paneOf(store, tab).sessionId = 's9';

    await store.close(tab);
    expect(callsOf('window_close').map((call) => call.args?.label)).toEqual(['session-s9']);
  });

  it('keeps its window while another tab is open', async () => {
    const store = new SessionStore();
    store.windowLabel = 'session-s9';
    const first = store.attachTab('s9', host());
    store.attachTab('s10', host('b'));
    await store.close(first);
    expect(callsOf('window_close')).toHaveLength(0);
  });

  it('is not what the main window is: it closes nothing by itself', async () => {
    const store = new SessionStore();
    const tab = store.open(host(), {});
    await store.close(tab);
    expect(callsOf('window_close')).toHaveLength(0);
  });
});

describe('saving the layout', () => {
  it('waits for the saved layout before an empty store may replace it', async () => {
    const store = new SessionStore();
    store.scheduleLayoutSave();
    await vi.advanceTimersByTimeAsync(1000);
    expect(callsOf('layout_save')).toHaveLength(0);

    answers.set('layout_load', null);
    await store.restoreLayout();
    expect(store.layoutLoaded).toBe(true);
    store.scheduleLayoutSave();
    await vi.advanceTimersByTimeAsync(1000);
    expect(callsOf('layout_save')).toHaveLength(1);
  });

  it('marks the layout loaded even when the load fails', async () => {
    const store = new SessionStore();
    failures.set('layout_load', 'disk error');
    await store.restoreLayout();
    expect(store.layoutLoaded).toBe(true);
  });
});
