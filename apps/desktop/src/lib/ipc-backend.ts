/**
 * The one place the frontend touches Tauri.
 *
 * Every Tauri call of the app (commands, channels, events, the window, the
 * file dialog) goes through this seam, so the backend can be swapped. In
 * production it is the real Tauri API. In a dev server opened with
 * `?mock` (for example `http://127.0.0.1:1420/?mock#hosts`) it is the
 * in-memory fake of `dev/mock-ipc.ts`, which lets the whole UI run in a
 * plain browser with realistic data.
 *
 * The mock is loaded by a dynamic import behind `import.meta.env.DEV`:
 * Vite replaces that constant with `false` in a production build, the
 * branch is dropped, and neither the mock nor its fixtures reach the
 * bundle (`scripts/check-no-mock-in-dist.sh` greps the build output).
 *
 * The exports are thin functions (and a lazy `Channel`) instead of eager
 * re-exports on purpose: a test that mocks only part of
 * `@tauri-apps/api/core` must still be able to load this module.
 */

import { Channel as TauriChannel, invoke as tauriInvoke } from '@tauri-apps/api/core';
import type { InvokeArgs } from '@tauri-apps/api/core';
import { listen as tauriListen } from '@tauri-apps/api/event';
import type { EventCallback, UnlistenFn } from '@tauri-apps/api/event';
import { open as tauriOpen } from '@tauri-apps/plugin-dialog';

export type { EventCallback, InvokeArgs, UnlistenFn };

/** The window operations the app uses (the title bar controls). */
export interface WindowHandle {
  minimize(): Promise<void>;
  toggleMaximize(): Promise<void>;
  close(): Promise<void>;
}

/** The options of the single-file picker. */
export interface OpenFileOptions {
  title?: string;
}

/** What `dev/mock-ipc.ts` provides; it mirrors the exports below. */
export type MockBackend = typeof import('./dev/mock-ipc');

/**
 * The fake backend, only when this is a dev server and the URL asks for it.
 * `import.meta.env.DEV` is the first operand so a production build folds
 * the whole expression to `null` and drops the dynamic import.
 */
const mock: MockBackend | null =
  import.meta.env.DEV &&
  typeof location !== 'undefined' &&
  new URLSearchParams(location.search).has('mock')
    ? await import('./dev/mock-ipc')
    : null;

/** Calls a backend command (a Tauri command, or its mock). */
export function invoke<T>(cmd: string, args?: InvokeArgs): Promise<T> {
  return mock ? mock.invoke<T>(cmd, args) : tauriInvoke<T>(cmd, args);
}

/**
 * A stream from the backend (Tauri's `Channel`, or its mock). It is passed
 * to a command as an argument; `onmessage` receives the updates.
 *
 * The constructor resolves the implementation when it runs, not when this
 * module loads.
 */
export type Channel<T = unknown> = TauriChannel<T>;
export const Channel = class BackendChannel<T> {
  constructor(onmessage?: (response: T) => void) {
    // A constructor that returns an object hands that object to `new`.
    return mock
      ? (new mock.Channel<T>(onmessage) as unknown as TauriChannel<T>)
      : new TauriChannel<T>(onmessage);
  }
} as unknown as new <T = unknown>(onmessage?: (response: T) => void) => TauriChannel<T>;

/** Subscribes to a backend event; resolves to the function that unsubscribes. */
export function listen<T>(event: string, handler: EventCallback<T>): Promise<UnlistenFn> {
  return mock ? mock.listen<T>(event, handler) : tauriListen<T>(event, handler);
}

/** Opens the native file picker for one file; `null` when cancelled. */
export function openFile(options: OpenFileOptions = {}): Promise<string | null> {
  if (mock) return mock.openFile(options);
  return tauriOpen({ multiple: false, directory: false, title: options.title });
}

/**
 * The current window. Loaded on demand so tests and non-Tauri builds never
 * touch the window API.
 */
export async function currentWindow(): Promise<WindowHandle> {
  if (mock) return mock.currentWindow();
  const { getCurrentWindow } = await import('@tauri-apps/api/window');
  return getCurrentWindow();
}
