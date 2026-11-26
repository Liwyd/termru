// @vitest-environment jsdom
/**
 * Route store: transitions, hash round-trip and the restore rules.
 */

import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { RouteStore, ROUTE_IDS, isRouteId, type RouteId } from './route.svelte';

describe('route store', () => {
  beforeEach(() => {
    window.location.hash = '';
  });

  afterEach(() => {
    window.location.hash = '';
  });

  it('starts on hosts', () => {
    const store = new RouteStore();
    expect(store.route).toBe('hosts');
  });

  it('transitions between every route and syncs the hash', () => {
    const store = new RouteStore();
    for (const id of ROUTE_IDS) {
      store.go(id);
      expect(store.route).toBe(id);
      expect(window.location.hash).toBe(`#${id}`);
    }
  });

  it('round-trips through the hash', () => {
    const store = new RouteStore();
    store.go('sessions');
    // A fresh instance (dev reload) reads the hash.
    const reloaded = new RouteStore();
    expect(reloaded.route).toBe('sessions');
  });

  it('follows hash changes from outside (back/forward)', () => {
    const store = new RouteStore();
    window.location.hash = '#preferences';
    window.dispatchEvent(new HashChangeEvent('hashchange'));
    expect(store.route).toBe('preferences');
  });

  it('ignores unknown hashes', () => {
    const store = new RouteStore();
    store.go('folders');
    window.location.hash = '#not-a-route';
    window.dispatchEvent(new HashChangeEvent('hashchange'));
    expect(store.route).toBe('folders');
  });

  it('restores a persisted route', () => {
    const store = new RouteStore();
    store.restore('sync');
    expect(store.route).toBe('sync');
  });

  it('keeps hosts when the document predates routing', () => {
    const store = new RouteStore();
    store.go('identities');
    store.restore(undefined);
    store.restore(null);
    expect(store.route).toBe('identities');
  });

  it('never restores host-edit (the edit target is gone after a restart)', () => {
    const store = new RouteStore();
    store.restore('host-edit');
    expect(store.route).toBe('hosts');
  });

  it('lets the URL hash win over the persisted route', () => {
    window.location.hash = '#sessions';
    const store = new RouteStore();
    store.restore('preferences');
    expect(store.route).toBe('sessions');
  });

  it('edits a host on the host-edit route', () => {
    const store = new RouteStore();
    const host = { id: 'h1' } as Parameters<RouteStore['edit']>[0];
    store.edit(host);
    expect(store.route).toBe('host-edit');
    expect(store.editHost).toEqual(host);
  });

  it('recognizes route ids', () => {
    expect(isRouteId('hosts')).toBe(true);
    expect(isRouteId('nope')).toBe(false);
    expect(isRouteId(null)).toBe(false);
    const ids: readonly RouteId[] = ROUTE_IDS;
    expect(ids.length).toBe(7);
  });
});

describe('navigating back', () => {
  beforeEach(() => {
    window.location.hash = '';
  });

  it('returns to the page the user came from, not always to hosts', () => {
    const store = new RouteStore();
    store.go('sessions');
    store.go('preferences');
    store.back();
    expect(store.route).toBe('sessions');
    expect(window.location.hash).toBe('#sessions');
  });

  it('walks back through several pages and falls back to hosts at the start', () => {
    const store = new RouteStore();
    store.go('folders');
    store.go('identities');
    store.go('sync');
    store.back();
    expect(store.route).toBe('identities');
    store.back();
    expect(store.route).toBe('folders');
    store.back();
    expect(store.route).toBe('hosts');
    // Nothing left to return to: the fallback, whatever it is.
    store.back('sessions');
    expect(store.route).toBe('sessions');
  });

  it('remembers pages reached through the URL hash as well', () => {
    const store = new RouteStore();
    store.go('folders');
    window.location.hash = '#preferences';
    window.dispatchEvent(new HashChangeEvent('hashchange'));
    store.back();
    expect(store.route).toBe('folders');
  });

  it('does not count the page it is already on', () => {
    const store = new RouteStore();
    store.go('folders');
    store.go('folders');
    store.back();
    expect(store.route).toBe('hosts');
  });
});

describe('the host editor route', () => {
  beforeEach(() => {
    window.location.hash = '';
  });

  const host = { id: 'h1' } as Parameters<RouteStore['edit']>[0];
  const draft = {
    host: { id: 'h1', name: 'typed so far' } as NonNullable<Parameters<RouteStore['edit']>[0]>,
    authType: 'private_key' as const,
    identityId: 'i1',
    groupId: '',
    tags: ['t1'],
  };

  it('opens with edit(host) alone and edit(null) means a new host', () => {
    const store = new RouteStore();
    store.edit(null);
    expect(store.route).toBe('host-edit');
    expect(store.editHost).toBeNull();
    store.edit(host);
    expect(store.editHost).toEqual(host);
  });

  it('parks the unsaved form while another page is open, and keeps the host', () => {
    const store = new RouteStore();
    store.edit(host);
    store.leaveEditor(draft, 'identities');
    expect(store.route).toBe('identities');
    expect(store.draft).toEqual(draft);
    expect(store.editHost).toEqual(host);

    // The identities page's Close goes back to the editor.
    store.back();
    expect(store.route).toBe('host-edit');
    expect(store.draft).toEqual(draft);
  });

  it('forgets a parked form when another edit starts', () => {
    const store = new RouteStore();
    store.edit(host);
    store.leaveEditor(draft, 'identities');
    store.edit(null);
    expect(store.draft).toBeNull();
  });
});
