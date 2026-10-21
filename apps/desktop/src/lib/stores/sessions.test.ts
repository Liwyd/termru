import { describe, expect, it } from 'vitest';
import { SessionStore } from './sessions.svelte';
import type { Host } from '../types';

function host(name = 'web-1'): Host {
  const now = new Date().toISOString();
  return {
    id: `host-${name}`,
    name,
    address: 'web-1.test',
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

/** The pane of a tab that has the focus. */
function paneOf(store: SessionStore, tab: ReturnType<SessionStore['open']>) {
  const pane = store.activePaneOf(tab);
  if (!pane) throw new Error('the tab has no active pane');
  return pane;
}

describe('SessionStore', () => {
  it('opens a tab and makes it active', () => {
    const store = new SessionStore();
    const tab = store.open(host(), { password: 'x' });
    expect(store.tabs).toHaveLength(1);
    expect(store.activeId).toBe(tab.id);
    expect(tab.panes).toHaveLength(1);
    expect(paneOf(store, tab).sessionId).toBeNull();
    expect(paneOf(store, tab).status.state).toBe('connecting');
  });

  it('marks the pane ready when the backend reports it', () => {
    const store = new SessionStore();
    const tab = store.open(host(), {});
    const pane = paneOf(store, tab);
    store.applyStatus(pane, { state: 'ready' });
    expect(pane.hadReady).toBe(true);
    expect(pane.dead).toBe(false);
    expect(pane.awaitingReconnect).toBe(false);
  });

  it('waits for the backend auto-reconnect after a network drop', () => {
    const store = new SessionStore();
    const pane = paneOf(store, store.open(host(), {}));
    store.applyStatus(pane, { state: 'ready' });
    store.applyStatus(pane, { state: 'failed', category: 'network', message: 'connection reset' });
    store.applyStatus(pane, { state: 'closed', reason: 'connection reset' });
    expect(pane.dead).toBe(false);
    expect(pane.awaitingReconnect).toBe(true);
  });

  it('treats a give-up as final', () => {
    const store = new SessionStore();
    const pane = paneOf(store, store.open(host(), {}));
    store.applyStatus(pane, { state: 'ready' });
    store.applyStatus(pane, { state: 'failed', category: 'network', message: 'refused' });
    store.applyStatus(pane, { state: 'closed', reason: 'refused (gave up after 8 attempts)' });
    expect(pane.dead).toBe(true);
    expect(pane.awaitingReconnect).toBe(false);
  });

  it('treats an auth failure as final', () => {
    const store = new SessionStore();
    const pane = paneOf(store, store.open(host(), {}));
    store.applyStatus(pane, { state: 'failed', category: 'auth', message: 'permission denied' });
    store.applyStatus(pane, { state: 'closed', reason: 'permission denied' });
    expect(pane.dead).toBe(true);
  });

  it('clears the reconnect wait when a new attempt starts', () => {
    const store = new SessionStore();
    const pane = paneOf(store, store.open(host(), {}));
    store.applyStatus(pane, { state: 'ready' });
    store.applyStatus(pane, { state: 'failed', category: 'network', message: 'reset' });
    store.applyStatus(pane, { state: 'closed', reason: 'reset' });
    expect(pane.awaitingReconnect).toBe(true);
    store.applyStatus(pane, {
      state: 'reconnecting',
      attempt: 1,
      delay_ms: 500,
      last_error: 'reset',
    });
    expect(pane.awaitingReconnect).toBe(false);
    expect(pane.dead).toBe(false);
  });

  it('retry resets the terminal state and bumps the generation', () => {
    const store = new SessionStore();
    const pane = paneOf(store, store.open(host(), {}));
    store.applyStatus(pane, { state: 'failed', category: 'auth', message: 'nope' });
    store.applyStatus(pane, { state: 'closed', reason: 'nope' });
    expect(pane.dead).toBe(true);
    const before = pane.generation;
    store.retry(pane);
    expect(pane.generation).toBe(before + 1);
    expect(pane.dead).toBe(false);
    expect(pane.errorMessage).toBeNull();
  });

  it('records a local failure as terminal', () => {
    const store = new SessionStore();
    const pane = paneOf(store, store.open(host(), {}));
    store.fail(pane, 'host not found');
    expect(pane.dead).toBe(true);
    expect(pane.errorMessage).toBe('host not found');
  });

  it('cycles the active tab in both directions', () => {
    const store = new SessionStore();
    const a = store.open(host('a'), {});
    const b = store.open(host('b'), {});
    const c = store.open(host('c'), {});
    expect(store.activeId).toBe(c.id);
    store.cycleActive(-1);
    expect(store.activeId).toBe(b.id);
    store.cycleActive(1);
    expect(store.activeId).toBe(c.id);
    store.cycleActive(1);
    expect(store.activeId).toBe(a.id);
  });

  it('renames a tab only when the title is not empty', () => {
    const store = new SessionStore();
    const tab = store.open(host(), {});
    store.rename(tab, '  renamed  ');
    expect(tab.title).toBe('renamed');
    store.rename(tab, '   ');
    expect(tab.title).toBe('renamed');
  });
});

describe('split panes', () => {
  it('splits the focused pane into an empty one', () => {
    const store = new SessionStore();
    const tab = store.open(host(), {});
    const created = store.splitActive('columns');
    expect(created).not.toBeNull();
    expect(tab.panes).toHaveLength(2);
    // The new pane is empty and has the focus.
    expect(tab.activePaneId).toBe(created?.id);
    expect(created?.host).toBeNull();
    expect(store.activePane?.host).toBeNull();
  });

  it('splits below as well', () => {
    const store = new SessionStore();
    store.open(host(), {});
    const created = store.splitActive('rows');
    const tab = store.active;
    expect(created).not.toBeNull();
    expect(tab?.root.kind).toBe('split');
    if (tab?.root.kind === 'split') expect(tab.root.direction).toBe('rows');
  });

  it('connects a host into an empty pane', async () => {
    const store = new SessionStore();
    store.open(host(), {});
    const created = store.splitActive('columns');
    const target = host('other');
    await store.connectPane(created!, target);
    expect(created?.host?.id).toBe(target.id);
    expect(created?.title).toBe('other');
    // The generation changed, so the view opens a connection.
    expect(created?.generation).toBe(1);
  });

  it('closes a pane and collapses the tree', async () => {
    const store = new SessionStore();
    const tab = store.open(host(), {});
    const created = store.splitActive('columns');
    await store.closePane(created!);
    expect(tab.panes).toHaveLength(1);
    // The split is gone: the tree is a single leaf again.
    expect(tab.root.kind).toBe('leaf');
    expect(store.tabs).toHaveLength(1);
  });

  it('closes the tab when its last pane is closed', async () => {
    const store = new SessionStore();
    const tab = store.open(host(), {});
    await store.closePane(paneOf(store, tab));
    expect(store.tabs).toHaveLength(0);
  });

  it('moves the focus from pane to pane', () => {
    const store = new SessionStore();
    const tab = store.open(host(), {});
    const created = store.splitActive('columns');
    const first = tab.panes[0];
    store.focusPane(first);
    expect(tab.activePaneId).toBe(first.id);
    store.cyclePane(1);
    expect(tab.activePaneId).toBe(created?.id);
    store.cyclePane(1);
    expect(tab.activePaneId).toBe(first.id);
  });

  it('keeps the split ratio in the allowed range', () => {
    const store = new SessionStore();
    const tab = store.open(host(), {});
    store.splitActive('columns');
    const split = tab.root;
    if (split.kind !== 'split') throw new Error('the tab was not split');
    store.resizeSplit(tab, split.id, 0.9);
    const resized = store.tabs[0].root;
    if (resized.kind !== 'split') throw new Error('the tab was not split');
    expect(resized.ratio).toBeCloseTo(0.9);
    store.resizeSplit(tab, split.id, 5);
    const clamped = store.tabs[0].root;
    if (clamped.kind !== 'split') throw new Error('the tab was not split');
    expect(clamped.ratio).toBeLessThanOrEqual(0.9);
  });

  it('saves and restores the pane tree', async () => {
    const store = new SessionStore();
    const tab = store.open(host('a'), {});
    store.splitActive('columns');
    const doc = store.toLayoutDoc();
    expect(doc.windows[0].tabs[0].root.split).toBeDefined();
    expect(doc.windows[0].tabs[0].active_pane).toBe(tab.activePaneId);
  });
});
