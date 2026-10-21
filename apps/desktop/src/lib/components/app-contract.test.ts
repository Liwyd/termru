// @vitest-environment jsdom
/**
 * Frontend ↔ backend contract test.
 *
 * The Tauri IPC layer and xterm are mocked, so this test exercises the real
 * Svelte components and stores: it asserts that the UI calls the Tauri
 * commands with the argument shapes the Rust side expects, and that channel
 * updates reach the terminal emulator.
 */

import { mount, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import App from '../../App.svelte';
import type { SessionUpdate } from '../types';

interface InvokeCall {
  cmd: string;
  args: Record<string, unknown> | undefined;
}

const invokeCalls: InvokeCall[] = [];
let sessionChannel: { onmessage?: (update: SessionUpdate) => void } | null = null;
let terminalWrites: Uint8Array[] = [];

const HOST = {
  id: 'host-1',
  name: 'web-1',
  address: 'web-1.test',
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
  created_at: '2026-01-01T00:00:00Z',
  updated_at: '2026-01-01T00:00:00Z',
  deleted: false,
};

vi.mock('@tauri-apps/api/core', () => {
  class Channel<T> {
    onmessage?: (update: T) => void;
  }
  return {
    Channel,
    invoke: async (cmd: string, args?: Record<string, unknown>) => {
      invokeCalls.push({ cmd, args });
      switch (cmd) {
        case 'host_list':
          return [HOST];
        case 'recent_list':
          return [];
        case 'identity_list':
          return [];
        case 'prefs_get':
          return {
            theme: 'dark',
            terminal: {
              font_family: 'monospace',
              font_size: 14,
              scrollback: 1000,
              cursor_blink: true,
              cursor_style: 'block',
              copy_on_select: false,
            },
            keybindings: {},
          };
        case 'session_open':
          sessionChannel = args?.channel as typeof sessionChannel;
          return 's1';
        case 'session_close':
        case 'session_write':
        case 'session_resize':
        case 'host_key_prompt_resolve':
          return null;
        default:
          return null;
      }
    },
  };
});

vi.mock('@tauri-apps/api/event', () => ({
  listen: async () => () => {},
}));

vi.mock('@xterm/xterm', () => {
  class Terminal {
    cols = 80;
    rows = 24;
    options: Record<string, unknown> = {};
    onData(_handler: (data: string) => void) {}
    loadAddon(_addon: unknown) {}
    open(_element: HTMLElement) {}
    write(data: Uint8Array) {
      terminalWrites.push(data);
    }
    getSelection() {
      return '';
    }
    paste(_text: string) {}
    dispose() {}
  }
  return { Terminal };
});

vi.mock('@xterm/addon-fit', () => {
  class FitAddon {
    loadAddon(_addon: unknown) {}
    fit() {}
  }
  return { FitAddon };
});

function click(element: Element): void {
  element.dispatchEvent(new MouseEvent('click', { bubbles: true }));
}

function type(element: HTMLInputElement, value: string): void {
  element.value = value;
  element.dispatchEvent(new Event('input', { bubbles: true }));
}

function findButton(root: ParentNode, label: string): HTMLButtonElement {
  const button = Array.from(root.querySelectorAll('button')).find(
    (candidate) => candidate.textContent?.trim() === label,
  );
  if (!button) throw new Error(`button "${label}" not found`);
  return button as HTMLButtonElement;
}

describe('desktop UI ↔ Tauri contract', () => {
  let target: HTMLElement;
  let app: ReturnType<typeof mount>;

  beforeEach(() => {
    invokeCalls.length = 0;
    terminalWrites = [];
    sessionChannel = null;
    target = document.createElement('div');
    document.body.appendChild(target);
    // jsdom lacks these browser APIs used by the terminal view.
    (globalThis as { ResizeObserver?: unknown }).ResizeObserver = class {
      observe() {}
      unobserve() {}
      disconnect() {}
    };
    (globalThis as { matchMedia?: unknown }).matchMedia = () => ({
      matches: true,
      addEventListener() {},
      removeEventListener() {},
    });
    app = mount(App, { target });
  });

  afterEach(() => {
    unmount(app);
    target.remove();
  });

  it('lists hosts from the store', async () => {
    await vi.waitFor(() => {
      expect(target.textContent).toContain('web-1');
    });
    expect(target.textContent).toContain('deploy@web-1.test:22');
    expect(invokeCalls.some((call) => call.cmd === 'host_list')).toBe(true);
  });

  it('asks for a password and opens a session with the documented arguments', async () => {
    await vi.waitFor(() => {
      expect(target.textContent).toContain('web-1');
    });

    click(findButton(target, 'Connect'));
    await vi.waitFor(() => {
      expect(target.textContent).toContain('Connect to web-1');
    });

    const passwordInput = target.querySelector('input[type="password"]');
    expect(passwordInput).not.toBeNull();
    type(passwordInput as HTMLInputElement, 'hunter2');

    const form = target.querySelector('form');
    form?.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }));

    await vi.waitFor(() => {
      expect(invokeCalls.some((call) => call.cmd === 'session_open')).toBe(true);
    });

    const open = invokeCalls.find((call) => call.cmd === 'session_open');
    expect(open?.args?.hostId).toBe('host-1');
    const options = open?.args?.options as
      { cols: number; rows: number; secrets: { password?: string } } | undefined;
    expect(options?.cols).toBeGreaterThan(0);
    expect(options?.rows).toBeGreaterThan(0);
    expect(options?.secrets.password).toBe('hunter2');
    expect(open?.args?.channel).toBeTruthy();
  });

  it('renders status updates and writes terminal output into xterm', async () => {
    await vi.waitFor(() => {
      expect(target.textContent).toContain('web-1');
    });
    click(findButton(target, 'Connect'));
    await vi.waitFor(() => {
      expect(target.textContent).toContain('Connect to web-1');
    });
    const passwordInput = target.querySelector('input[type="password"]');
    type(passwordInput as HTMLInputElement, 'hunter2');
    target
      .querySelector('form')
      ?.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }));

    await vi.waitFor(() => {
      expect(sessionChannel).not.toBeNull();
    });

    sessionChannel?.onmessage?.({ kind: 'status', status: { state: 'ready' } });
    await vi.waitFor(() => {
      expect(target.textContent).toContain('Connected');
    });

    sessionChannel?.onmessage?.({
      kind: 'data',
      base64: btoa('hello from the shell'),
    });
    await vi.waitFor(() => {
      expect(terminalWrites.length).toBeGreaterThan(0);
    });
    const written = new TextDecoder().decode(terminalWrites[0]);
    expect(written).toBe('hello from the shell');
  });

  it('closes the backend session when the tab is closed', async () => {
    await vi.waitFor(() => {
      expect(target.textContent).toContain('web-1');
    });
    click(findButton(target, 'Connect'));
    await vi.waitFor(() => {
      expect(target.textContent).toContain('Connect to web-1');
    });
    type(target.querySelector('input[type="password"]') as HTMLInputElement, 'hunter2');
    target
      .querySelector('form')
      ?.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }));

    await vi.waitFor(() => {
      expect(sessionChannel).not.toBeNull();
    });

    const closeButton = Array.from(target.querySelectorAll('button')).find((candidate) =>
      candidate.getAttribute('aria-label')?.startsWith('Close'),
    );
    expect(closeButton).toBeTruthy();
    click(closeButton as HTMLElement);

    await vi.waitFor(() => {
      expect(invokeCalls.some((call) => call.cmd === 'session_close')).toBe(true);
    });
    const close = invokeCalls.find((call) => call.cmd === 'session_close');
    expect(close?.args?.id).toBe('s1');
  });
});
