// @vitest-environment jsdom
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { RouteStore } from './route.svelte';
import { UnsavedGuard } from './unsaved.svelte';

describe('the unsaved-changes guard', () => {
  let guard: UnsavedGuard;
  beforeEach(() => {
    guard = new UnsavedGuard();
  });

  it('lets everything through while nothing is held', () => {
    const proceed = vi.fn();
    expect(guard.dirty).toBe(false);
    expect(guard.ask(proceed)).toBe(false);
    expect(guard.question).toBeNull();
  });

  it('lets everything through while what is held is clean', () => {
    guard.hold(() => false);
    expect(guard.ask(vi.fn())).toBe(false);
  });

  it('holds the action back and asks while something is unsaved', () => {
    const proceed = vi.fn();
    guard.hold(() => true);
    expect(guard.dirty).toBe(true);
    expect(guard.ask(proceed)).toBe(true);
    expect(proceed).not.toHaveBeenCalled();
    expect(guard.question?.kind).toBe('leave');
  });

  it('"keep" drops the question and the action', () => {
    const proceed = vi.fn();
    guard.hold(() => true);
    guard.ask(proceed);
    guard.keep();
    expect(guard.question).toBeNull();
    expect(proceed).not.toHaveBeenCalled();
  });

  it('"discard" runs what the user wanted', () => {
    const proceed = vi.fn();
    guard.hold(() => true);
    guard.ask(proceed, 'close');
    expect(guard.question?.kind).toBe('close');
    guard.discard();
    expect(proceed).toHaveBeenCalledTimes(1);
    expect(guard.question).toBeNull();
  });

  it('withdraws its hold and its question when the page goes', () => {
    const release = guard.hold(() => true);
    guard.ask(vi.fn());
    release();
    expect(guard.dirty).toBe(false);
    expect(guard.question).toBeNull();
  });

  it('an older page cannot withdraw the hold of a newer one', () => {
    const first = guard.hold(() => true);
    guard.hold(() => true);
    first();
    expect(guard.dirty).toBe(true);
  });
});

describe('the route and the guard', () => {
  /** A route store whose pages ask the shared guard, as the app's do. */
  async function setup() {
    const { unsaved } = await import('./unsaved.svelte');
    window.location.hash = '';
    const store = new RouteStore();
    store.go('folders');
    return { store, unsaved };
  }

  it('asks before go() and goes only after "discard"', async () => {
    const { store, unsaved } = await setup();
    const release = unsaved.hold(() => true);
    store.go('sync');
    expect(store.route).toBe('folders');
    expect(unsaved.question).not.toBeNull();
    unsaved.discard();
    expect(store.route).toBe('sync');
    release();
  });

  it('asks before back() too', async () => {
    const { store, unsaved } = await setup();
    store.go('sync');
    const release = unsaved.hold(() => true);
    store.back();
    expect(store.route).toBe('sync');
    unsaved.discard();
    expect(store.route).toBe('folders');
    release();
  });

  it('does not ask to go to the page it is on, nor when forced', async () => {
    const { store, unsaved } = await setup();
    const release = unsaved.hold(() => true);
    store.go('folders');
    expect(unsaved.question).toBeNull();
    store.go('sync', { force: true });
    expect(store.route).toBe('sync');
    expect(unsaved.question).toBeNull();
    release();
  });

  it('parking the editor for another page asks nothing', async () => {
    const { store, unsaved } = await setup();
    const release = unsaved.hold(() => true);
    store.leaveEditor(
      { host: { id: 'h' } as never, authType: 'agent', identityId: '', groupId: '', tags: [] },
      'identities',
    );
    expect(store.route).toBe('identities');
    expect(unsaved.question).toBeNull();
    release();
  });

  it('opening the editor on another host is a way to lose changes too', async () => {
    const { store, unsaved } = await setup();
    store.edit(null);
    const release = unsaved.hold(() => true);
    store.edit({ id: 'other' } as never);
    expect(store.editHost).toBeNull();
    expect(unsaved.question).not.toBeNull();
    unsaved.discard();
    expect(store.editHost?.id).toBe('other');
    release();
  });

  it('every edit() starts a fresh form (the token changes)', async () => {
    const { store } = await setup();
    const before = store.editToken;
    store.edit(null);
    store.edit(null);
    expect(store.editToken).toBe(before + 2);
  });
});
