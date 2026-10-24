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
