import { describe, expect, it } from 'vitest';
import { HostsStore } from './hosts.svelte';
import type { Group, Host, Tag } from '../types';

function group(id: string, name: string, parent: string | null = null): Group {
  const now = new Date().toISOString();
  return {
    id,
    name,
    parent_id: parent,
    sort_order: 0,
    created_at: now,
    updated_at: now,
    deleted: false,
  };
}

function tag(id: string, name: string): Tag {
  const now = new Date().toISOString();
  return {
    id,
    name,
    color: null,
    created_at: now,
    updated_at: now,
    deleted: false,
  };
}

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

  it('searches the description and the tags', () => {
    const store = new HostsStore();
    const tagged = host('web');
    tagged.description = 'the frontend server';
    tagged.tags = ['tag-1'];
    store.hosts = [tagged, host('db')];
    store.tags = [tag('tag-1', 'production')];

    store.query = 'frontend';
    expect(store.visible.map((entry) => entry.name)).toEqual(['web']);
    store.query = 'production';
    expect(store.visible.map((entry) => entry.name)).toEqual(['web']);
    // The tag id is not the name: it does not match.
    store.query = 'tag-1';
    expect(store.visible).toHaveLength(0);
  });

  it('shows the path of a nested folder', () => {
    const store = new HostsStore();
    store.groups = [group('g1', 'work'), group('g2', 'servers', 'g1')];
    expect(store.folderOptions).toEqual([
      { id: 'g1', label: 'work' },
      { id: 'g2', label: 'work / servers' },
    ]);
  });

  it('breaks a cycle in the folder tree', () => {
    const store = new HostsStore();
    // g1 is the parent of g2 and g2 the parent of g1: the path must stop.
    store.groups = [group('g1', 'a', 'g2'), group('g2', 'b', 'g1')];
    expect(store.folderOptions).toHaveLength(2);
    for (const option of store.folderOptions) {
      expect(option.label.split(' / ').length).toBeLessThanOrEqual(2);
    }
  });

  it('sorts the rows by recent activity, favorites before the rest, then by name', () => {
    const store = new HostsStore();
    const zeta = host('zeta');
    const alpha = host('alpha');
    const used = host('used');
    store.hosts = [zeta, alpha, used];
    store.recentAt = { 'host-used': 500 };
    expect(store.rows.map((entry) => entry.name)).toEqual(['used', 'alpha', 'zeta']);

    // A second, older connection stays below the newer one.
    store.recentAt = { 'host-used': 500, 'host-zeta': 100 };
    expect(store.rows.map((entry) => entry.name)).toEqual(['used', 'zeta', 'alpha']);
  });

  it('keeps only the favorites in the favorites tab (with their counts)', () => {
    const store = new HostsStore();
    store.hosts = [host('a', true), host('b'), host('c', true)];
    store.filterTab = 'favorites';
    expect(store.rows.map((entry) => entry.name)).toEqual(['a', 'c']);
    expect(store.counts).toEqual({ all: 3, favorites: 2, recent: 0, unfiled: 3 });
  });

  it('keeps the hosts in the recents tab, newest first', () => {
    const store = new HostsStore();
    store.hosts = [host('a'), host('b'), host('c')];
    store.recentAt = { 'host-b': 200, 'host-c': 100 };
    store.filterTab = 'recent';
    expect(store.rows.map((entry) => entry.name)).toEqual(['b', 'c']);
    expect(store.counts.recent).toBe(2);
  });

  it('keeps the hosts without a folder in the unfiled tab', () => {
    const store = new HostsStore();
    store.groups = [group('g1', 'production')];
    const filed = host('filed');
    filed.group_id = 'g1';
    store.hosts = [filed, host('loose')];
    store.filterTab = 'unfiled';
    expect(store.rows.map((entry) => entry.name)).toEqual(['loose']);
    expect(store.counts.unfiled).toBe(1);
  });

  it('restricts the rows to a folder, nested folders included', () => {
    const store = new HostsStore();
    store.groups = [group('g1', 'work'), group('g2', 'servers', 'g1')];
    const nested = host('nested');
    nested.group_id = 'g2';
    const direct = host('direct');
    direct.group_id = 'g1';
    store.hosts = [nested, direct, host('loose')];

    store.folderFilter = 'g1';
    expect(store.rows.map((entry) => entry.name).sort()).toEqual(['direct', 'nested']);
    expect(store.counts.all).toBe(2);

    store.folderFilter = 'g2';
    expect(store.rows.map((entry) => entry.name)).toEqual(['nested']);
  });

  it('combines the search, the folder and the tab', () => {
    const store = new HostsStore();
    store.groups = [group('g1', 'production')];
    const api = host('api-prod');
    api.group_id = 'g1';
    api.favorite = true;
    const web = host('web-prod');
    web.group_id = 'g1';
    store.hosts = [api, web, host('api-dev')];

    store.query = 'prod';
    store.folderFilter = 'g1';
    store.filterTab = 'favorites';
    expect(store.rows.map((entry) => entry.name)).toEqual(['api-prod']);
  });

  it('searches the description and the tag names in the rows too', () => {
    const store = new HostsStore();
    const tagged = host('web');
    tagged.description = 'the frontend server';
    tagged.tags = ['tag-1'];
    store.hosts = [tagged, host('db')];
    store.tags = [tag('tag-1', 'production')];

    store.query = 'frontend';
    expect(store.rows.map((entry) => entry.name)).toEqual(['web']);
    store.query = 'production';
    expect(store.rows.map((entry) => entry.name)).toEqual(['web']);
    // The tag id is not the name: it does not match.
    store.query = 'tag-1';
    expect(store.rows).toHaveLength(0);
  });

  it('clears the search, the folder and the tab at once', () => {
    const store = new HostsStore();
    store.hosts = [host('a', true)];
    store.query = 'zzz';
    store.folderFilter = 'g1';
    store.filterTab = 'favorites';

    store.clearFilters();

    expect(store.query).toBe('');
    expect(store.folderFilter).toBe('');
    expect(store.filterTab).toBe('all');
    expect(store.rows.map((entry) => entry.name)).toEqual(['a']);
  });

  it('reports the last connection time of a host', () => {
    const store = new HostsStore();
    const used = host('used');
    const fresh = host('fresh');
    store.hosts = [used, fresh];
    store.recentAt = { 'host-used': 1_760_000_000_000 };
    expect(store.lastUsed(used)).toBe(1_760_000_000_000);
    expect(store.lastUsed(fresh)).toBeNull();
  });
});
