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

  it('keeps only favorites in the favorites list', () => {
    const store = new HostsStore();
    store.hosts = [host('a', true), host('b')];
    expect(store.favorites.map((entry) => entry.name)).toEqual(['a']);
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

  it('groups the hosts by their folder', () => {
    const store = new HostsStore();
    store.groups = [group('g1', 'production'), group('g2', 'staging')];
    const inFolder = host('web');
    inFolder.group_id = 'g1';
    store.hosts = [inFolder, host('loose')];

    const sections = store.grouped;
    expect(sections.map((section) => section.label)).toEqual(['Hosts', 'production']);
    expect(sections[0].hosts.map((entry) => entry.name)).toEqual(['loose']);
    expect(sections[1].hosts.map((entry) => entry.name)).toEqual(['web']);
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
});
