import { describe, expect, it } from 'vitest';
import { HostsStore } from './hosts.svelte';
import type { Host } from '../types';

function host(name: string, favorite = false): Host {
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
    favorite,
    description: null,
    term: 'xterm-256color',
    keepalive_seconds: 0,
    sort_order: 0,
    created_at: now,
    updated_at: now,
    deleted: false,
  };
}

describe('HostsStore', () => {
  it('lists favorites first, then by name', () => {
    const store = new HostsStore();
    store.hosts = [host('zeta'), host('alpha'), host('mid', true)];
    expect(store.visible.map((entry) => entry.name)).toEqual(['mid', 'alpha', 'zeta']);
  });

  it('filters by name, address and username', () => {
    const store = new HostsStore();
    store.hosts = [host('web'), host('db')];
    store.query = 'web';
    expect(store.visible.map((entry) => entry.name)).toEqual(['web']);
    store.query = 'db.test';
    expect(store.visible.map((entry) => entry.name)).toEqual(['db']);
    store.query = 'deploy';
    expect(store.visible).toHaveLength(2);
    store.query = 'nothing';
    expect(store.visible).toHaveLength(0);
  });

  it('keeps only favorites in the favorites list', () => {
    const store = new HostsStore();
    store.hosts = [host('a', true), host('b')];
    expect(store.favorites.map((entry) => entry.name)).toEqual(['a']);
  });
});
