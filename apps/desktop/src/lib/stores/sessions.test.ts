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

describe('SessionStore', () => {
  it('opens a tab and makes it active', () => {
    const store = new SessionStore();
    const tab = store.open(host(), { password: 'x' });
    expect(store.tabs).toHaveLength(1);
    expect(store.activeId).toBe(tab.id);
    expect(tab.sessionId).toBeNull();
    expect(tab.status.state).toBe('connecting');
  });

  it('marks the tab ready when the backend reports it', () => {
    const store = new SessionStore();
    const tab = store.open(host(), {});
    store.applyStatus(tab, { state: 'ready' });
    expect(tab.hadReady).toBe(true);
    expect(tab.dead).toBe(false);
    expect(tab.awaitingReconnect).toBe(false);
  });

  it('waits for the backend auto-reconnect after a network drop', () => {
    const store = new SessionStore();
    const tab = store.open(host(), {});
    store.applyStatus(tab, { state: 'ready' });
    store.applyStatus(tab, { state: 'failed', category: 'network', message: 'connection reset' });
    store.applyStatus(tab, { state: 'closed', reason: 'connection reset' });
    expect(tab.dead).toBe(false);
    expect(tab.awaitingReconnect).toBe(true);
  });

  it('treats a give-up as final', () => {
    const store = new SessionStore();
    const tab = store.open(host(), {});
    store.applyStatus(tab, { state: 'ready' });
    store.applyStatus(tab, { state: 'failed', category: 'network', message: 'refused' });
    store.applyStatus(tab, { state: 'closed', reason: 'refused (gave up after 8 attempts)' });
    expect(tab.dead).toBe(true);
    expect(tab.awaitingReconnect).toBe(false);
  });

  it('treats an auth failure as final', () => {
    const store = new SessionStore();
    const tab = store.open(host(), {});
    store.applyStatus(tab, { state: 'failed', category: 'auth', message: 'permission denied' });
    store.applyStatus(tab, { state: 'closed', reason: 'permission denied' });
    expect(tab.dead).toBe(true);
  });

  it('clears the reconnect wait when a new attempt starts', () => {
    const store = new SessionStore();
    const tab = store.open(host(), {});
    store.applyStatus(tab, { state: 'ready' });
    store.applyStatus(tab, { state: 'failed', category: 'network', message: 'reset' });
    store.applyStatus(tab, { state: 'closed', reason: 'reset' });
    expect(tab.awaitingReconnect).toBe(true);
    store.applyStatus(tab, {
      state: 'reconnecting',
      attempt: 1,
      delay_ms: 500,
      last_error: 'reset',
    });
    expect(tab.awaitingReconnect).toBe(false);
    expect(tab.dead).toBe(false);
  });

  it('retry resets the terminal state and bumps the generation', () => {
    const store = new SessionStore();
    const tab = store.open(host(), {});
    store.applyStatus(tab, { state: 'failed', category: 'auth', message: 'nope' });
    store.applyStatus(tab, { state: 'closed', reason: 'nope' });
    expect(tab.dead).toBe(true);
    const before = tab.generation;
    store.retry(tab);
    expect(tab.generation).toBe(before + 1);
    expect(tab.dead).toBe(false);
    expect(tab.errorMessage).toBeNull();
  });

  it('records a local failure as terminal', () => {
    const store = new SessionStore();
    const tab = store.open(host(), {});
    store.fail(tab, 'host not found');
    expect(tab.dead).toBe(true);
    expect(tab.errorMessage).toBe('host not found');
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
