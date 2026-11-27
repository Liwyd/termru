// @vitest-environment jsdom
/**
 * `listen` registers for the window it runs in: the backend addresses the
 * questions of a session (host key, authentication) to the windows that show
 * it, and a listener without a target would hear them in every window.
 */

import { afterEach, describe, expect, it, vi } from 'vitest';

const tauriListen = vi.hoisted(() => vi.fn(async () => () => {}));

vi.mock('@tauri-apps/api/event', () => ({ listen: tauriListen }));
vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn(), Channel: class {} }));
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn() }));

const { listen } = await import('./ipc-backend');

type Internals = { metadata?: { currentWebview?: { label?: unknown } } };

function setWindowLabel(label: unknown): void {
  (globalThis as { __TAURI_INTERNALS__?: Internals }).__TAURI_INTERNALS__ = {
    metadata: { currentWebview: { label } },
  };
}

afterEach(() => {
  delete (globalThis as { __TAURI_INTERNALS__?: Internals }).__TAURI_INTERNALS__;
  tauriListen.mockClear();
});

describe('listen', () => {
  it('listens for this window only', async () => {
    setWindowLabel('session-7');
    const handler = vi.fn();
    await listen('auth-prompt', handler);
    expect(tauriListen).toHaveBeenCalledWith('auth-prompt', handler, {
      target: { kind: 'WebviewWindow', label: 'session-7' },
    });
  });

  it('falls back to an untargeted listener outside Tauri', async () => {
    const handler = vi.fn();
    await listen('sync-applied', handler);
    expect(tauriListen).toHaveBeenCalledWith('sync-applied', handler);
  });

  it('ignores a label that is not a string', async () => {
    setWindowLabel(42);
    const handler = vi.fn();
    await listen('sync-applied', handler);
    expect(tauriListen).toHaveBeenCalledWith('sync-applied', handler);
  });
});
