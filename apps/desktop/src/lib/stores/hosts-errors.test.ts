// @vitest-environment jsdom
/**
 * What the hosts store does when the backend refuses something: a failed
 * mutation is a notice, never the error page of the whole table.
 */

import { beforeEach, describe, expect, it, vi } from 'vitest';
import { HostsStore } from './hosts.svelte';
import { notices } from '../notify.svelte';
import { noticeText } from '../test-support/notices';
import { answers, callsOf, failures, resetTauriMock } from '../test-support/tauri-core-mock';
import type { Host } from '../types';

vi.mock('@tauri-apps/api/core', () => import('../test-support/tauri-core-mock'));

function host(name: string): Host {
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

function loaded(): HostsStore {
  const store = new HostsStore();
  store.hosts = [host('a'), host('b')];
  store.loaded = true;
  return store;
}

beforeEach(() => {
  resetTauriMock();
  notices.reset();
});

describe('a failed mutation', () => {
  it('on save keeps the table and reports the reason to the editor', async () => {
    const store = loaded();
    failures.set('host_save', 'name already used');
    const saved = await store.save(host('c'));
    expect(saved).toBeNull();
    expect(store.saveError).toBe('name already used');
    expect(store.error).toBeNull();
    expect(store.hosts).toHaveLength(2);
  });

  it('on delete keeps the table, says so with a toast and returns false', async () => {
    const store = loaded();
    failures.set('host_delete', 'the store is locked');
    expect(await store.remove('host-a')).toBe(false);
    expect(store.error).toBeNull();
    expect(store.hosts).toHaveLength(2);
    expect(notices.visible.map((toast) => toast.kind)).toEqual(['error']);
    expect(noticeText(notices.visible[0])).toContain('the store is locked');
  });

  it('on delete succeeds quietly and returns true', async () => {
    const store = loaded();
    expect(await store.remove('host-a')).toBe(true);
    expect(store.hosts.map((entry) => entry.name)).toEqual(['b']);
  });

  it('on favorite keeps the table and shows a toast', async () => {
    const store = loaded();
    failures.set('host_set_favorite', 'read-only');
    await store.toggleFavorite(store.hosts[0]);
    expect(store.error).toBeNull();
    expect(store.hosts[0].favorite).toBe(false);
    expect(notices.visible[0].kind).toBe('error');
  });
});

describe('loading', () => {
  it('shows the skeleton on the first load only', async () => {
    const store = new HostsStore();
    answers.set('host_list', [host('a')]);
    const first = store.load();
    expect(store.loading).toBe(true);
    await first;
    expect(store.loading).toBe(false);
    expect(store.loaded).toBe(true);

    // A refresh keeps the data on screen: no skeleton.
    const refresh = store.load();
    expect(store.loading).toBe(false);
    expect(store.hosts).toHaveLength(1);
    await refresh;
    expect(callsOf('host_list')).toHaveLength(2);
  });

  it('makes a failed first load the page error, and a retry shows the skeleton again', async () => {
    const store = new HostsStore();
    failures.set('host_list', 'database is locked');
    await store.load();
    expect(store.error).toBe('database is locked');
    expect(store.loaded).toBe(false);

    failures.delete('host_list');
    const retry = store.load();
    expect(store.loading).toBe(true);
    expect(store.error).toBeNull();
    await retry;
    expect(store.error).toBeNull();
  });

  it('reports a failed refresh as a notice and keeps the hosts', async () => {
    const store = loaded();
    failures.set('host_list', 'database is locked');
    await store.load();
    expect(store.error).toBeNull();
    expect(store.hosts).toHaveLength(2);
    expect(notices.visible[0].kind).toBe('error');
  });
});
