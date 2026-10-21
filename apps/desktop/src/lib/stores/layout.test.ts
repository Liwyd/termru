import { describe, expect, it, vi } from 'vitest';
import { SessionStore } from './sessions.svelte';
import type { Host, WorkspaceDoc } from '../types';

vi.mock('../ipc', () => ({
  api: {
    layoutSave: vi.fn(async () => undefined),
    layoutLoad: vi.fn(async (): Promise<WorkspaceDoc | null> => null),
    sessionClose: vi.fn(async () => undefined),
  },
}));

function host(id: string, name: string): Host {
  const now = new Date().toISOString();
  return {
    id,
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

describe('layout persistence', () => {
  it('builds a layout document from the open tabs', () => {
    const store = new SessionStore();
    store.open(host('h1', 'web-1'), {});
    store.open(host('h2', 'db-1'), {});

    const doc = store.toLayoutDoc();
    expect(doc.version).toBe(1);
    expect(doc.windows).toHaveLength(1);
    expect(doc.windows[0].tabs).toHaveLength(2);
    expect(doc.windows[0].tabs[0].root.leaf?.host_id).toBe('h1');
    expect(doc.windows[0].tabs[1].root.leaf?.host_id).toBe('h2');
    expect(doc.windows[0].active_tab).toBe(store.activeId);
  });

  it('reorders tabs', () => {
    const store = new SessionStore();
    const a = store.open(host('h1', 'a'), {});
    const b = store.open(host('h2', 'b'), {});
    const c = store.open(host('h3', 'c'), {});
    store.reorder(0, 2);
    expect(store.tabs.map((tab) => tab.id)).toEqual([b.id, c.id, a.id]);
    // Out-of-range moves are ignored.
    store.reorder(0, 99);
    expect(store.tabs.map((tab) => tab.id)).toEqual([b.id, c.id, a.id]);
  });
});
