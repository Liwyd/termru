import { afterEach, describe, expect, it, vi } from 'vitest';
import {
  PUSH_DEBOUNCE_MS,
  claimResizeOwner,
  resizeOwnerCount,
  resizeOwnerOf,
  type ResizeEnvironment,
  type ResizeHooks,
} from './resize';

/** A fake clock: frames and timers run when the test says so. */
function fakeEnvironment() {
  let nextId = 1;
  const frames = new Map<number, () => void>();
  const timers = new Map<number, { run: () => void; ms: number }>();
  let observer: (() => void) | null = null;
  let observed = 0;
  let disconnected = 0;

  const env: ResizeEnvironment = {
    observe(_target, onResize) {
      observed += 1;
      observer = onResize;
      return () => {
        disconnected += 1;
        observer = null;
      };
    },
    requestFrame(run) {
      const id = nextId++;
      frames.set(id, run);
      return id;
    },
    cancelFrame(id) {
      frames.delete(id);
    },
    setTimer(run, ms) {
      const id = nextId++;
      timers.set(id, { run, ms });
      return id as unknown as ReturnType<typeof setTimeout>;
    },
    clearTimer(id) {
      timers.delete(id as unknown as number);
    },
  };

  return {
    env,
    /** The observer fires (the container's size changed). */
    resize: () => observer?.(),
    /** One animation frame. */
    frame: async () => {
      const due = [...frames.values()];
      frames.clear();
      for (const run of due) run();
      // Let a fit that awaits something settle.
      await Promise.resolve();
      await Promise.resolve();
    },
    /** The debounce timers fire. */
    flushTimers: () => {
      const due = [...timers.values()];
      timers.clear();
      for (const { run } of due) run();
    },
    pendingFrames: () => frames.size,
    pendingTimers: () => [...timers.values()].map((timer) => timer.ms),
    observed: () => observed,
    disconnected: () => disconnected,
  };
}

function hooks(changed: () => boolean = () => true) {
  return {
    fit: vi.fn(async () => changed()),
    push: vi.fn(),
  };
}

const target = {} as Element;

afterEach(() => {
  // Owners are module state: leave none behind.
  for (const id of ['pane-1', 'pane-2']) resizeOwnerOf(id)?.stop();
});

describe('the resize owner', () => {
  it('has one owner per pane: a second claim stops the first', () => {
    const first = fakeEnvironment();
    const second = fakeEnvironment();
    const a = claimResizeOwner('pane-1', hooks(), first.env);
    a.start(target);
    expect(resizeOwnerCount()).toBe(1);

    const b = claimResizeOwner('pane-1', hooks(), second.env);
    b.start(target);

    expect(a.stopped).toBe(true);
    expect(first.disconnected()).toBe(1);
    expect(resizeOwnerOf('pane-1')).toBe(b);
    expect(resizeOwnerCount()).toBe(1);
    // The new owner observes once; the old observer is gone.
    expect(second.observed()).toBe(1);
  });

  it('keeps one owner for each of two panes', () => {
    const env = fakeEnvironment();
    claimResizeOwner('pane-1', hooks(), env.env).start(target);
    claimResizeOwner('pane-2', hooks(), env.env).start(target);
    expect(resizeOwnerCount()).toBe(2);
  });

  it('observes the target once, however often it is started', () => {
    const env = fakeEnvironment();
    const owner = claimResizeOwner('pane-1', hooks(), env.env);
    owner.start(target);
    owner.start(target);
    expect(env.observed()).toBe(1);
  });

  it('fits once per frame, however many resizes came in (a divider drag)', async () => {
    const env = fakeEnvironment();
    const h = hooks();
    claimResizeOwner('pane-1', h, env.env).start(target);
    await env.frame();
    h.fit.mockClear();

    for (let i = 0; i < 50; i += 1) env.resize();
    expect(env.pendingFrames()).toBe(1);
    expect(h.fit).not.toHaveBeenCalled();

    await env.frame();
    expect(h.fit).toHaveBeenCalledTimes(1);
  });

  it('tells the backend only after the size settled, once', async () => {
    const env = fakeEnvironment();
    const h = hooks();
    claimResizeOwner('pane-1', h, env.env).start(target);
    await env.frame();
    env.flushTimers();
    h.push.mockClear();

    // A drag: a fit on every frame, and the push waits.
    for (let i = 0; i < 20; i += 1) {
      env.resize();
      await env.frame();
    }
    expect(h.fit.mock.calls.length).toBeGreaterThanOrEqual(20);
    expect(h.push).not.toHaveBeenCalled();
    // Each fit restarted the debounce: one timer is pending, with the delay.
    expect(env.pendingTimers()).toEqual([PUSH_DEBOUNCE_MS]);

    env.flushTimers();
    expect(h.push).toHaveBeenCalledTimes(1);
  });

  it('does not push when the grid did not change', async () => {
    const env = fakeEnvironment();
    const h = hooks(() => false);
    claimResizeOwner('pane-1', h, env.env).start(target);
    await env.frame();
    env.resize();
    await env.frame();
    expect(env.pendingTimers()).toEqual([]);
    expect(h.push).not.toHaveBeenCalled();
  });

  it('runs one fit at a time and fits again for what came in meanwhile', async () => {
    const env = fakeEnvironment();
    let release: () => void = () => {};
    let calls = 0;
    const h: ResizeHooks = {
      fit: () => {
        calls += 1;
        // The first fit waits (for the fonts), the next ones do not.
        return calls === 1
          ? new Promise<boolean>((resolve) => {
              release = () => resolve(true);
            })
          : Promise.resolve(false);
      },
      push: () => {},
    };
    claimResizeOwner('pane-1', h, env.env).start(target);
    await env.frame();
    expect(calls).toBe(1);

    env.resize();
    await env.frame();
    // The second trigger waits for the running fit.
    expect(calls).toBe(1);

    release();
    await Promise.resolve();
    await Promise.resolve();
    await env.frame();
    expect(calls).toBe(2);
  });

  it('stops cleanly: nothing fires afterwards', async () => {
    const env = fakeEnvironment();
    const h = hooks();
    const owner = claimResizeOwner('pane-1', h, env.env);
    owner.start(target);
    await env.frame();
    env.resize();
    owner.stop();
    expect(env.pendingFrames()).toBe(0);
    expect(resizeOwnerCount()).toBe(0);
    await env.frame();
    env.flushTimers();
    h.fit.mockClear();
    expect(h.push).not.toHaveBeenCalled();
    expect(h.fit).not.toHaveBeenCalled();
    // Starting a stopped owner does nothing.
    owner.start(target);
    expect(env.observed()).toBe(1);
  });

  it('cancels a pending push when it stops', async () => {
    const env = fakeEnvironment();
    const h = hooks();
    const owner = claimResizeOwner('pane-1', h, env.env);
    owner.start(target);
    await env.frame();
    expect(env.pendingTimers()).toHaveLength(1);
    owner.stop();
    expect(env.pendingTimers()).toEqual([]);
    env.flushTimers();
    expect(h.push).not.toHaveBeenCalled();
  });
});
