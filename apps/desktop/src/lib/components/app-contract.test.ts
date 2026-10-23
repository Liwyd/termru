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
let identities: unknown[] = [];
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
        case 'host_save':
          // The backend answers with the saved host: the editor closes.
          return { ...(args?.host as Record<string, unknown>) };
        case 'recent_list':
          return [];
        case 'identity_list':
          return identities;
        case 'group_list':
          return [
            {
              id: 'group-1',
              name: 'production',
              parent_id: null,
              sort_order: 0,
              created_at: '2026-01-01T00:00:00Z',
              updated_at: '2026-01-01T00:00:00Z',
              deleted: false,
            },
          ];
        case 'tag_list':
          return [
            {
              id: 'tag-1',
              name: 'frontend',
              color: null,
              created_at: '2026-01-01T00:00:00Z',
              updated_at: '2026-01-01T00:00:00Z',
              deleted: false,
            },
          ];
        case 'identity_bindings':
          return [{ identityId: 'identity-1', keyPath: '/home/user/.ssh/id_ed25519' }];
        case 'plugin:dialog|open':
          return '/home/user/.ssh/id_ed25519';
        case 'prefs_get':
          return {
            theme: 'dark',
            terminal: {
              font_family: 'IBM Plex Mono, monospace',
              font_size: 14,
              scrollback: 1000,
              cursor_blink: true,
              cursor_style: 'block',
              copy_on_select: false,
              line_height: 1.65,
            },
            keybindings: {},
            reduced_motion: 'system',
            focus_visibility: false,
            routine_notices: true,
          };
        case 'session_open':
          sessionChannel = args?.channel as typeof sessionChannel;
          return 's1';
        case 'sync_status':
          return {
            settings: {
              server_url: '',
              email: '',
              device_name: '',
              device_id: '',
            },
            signed_in: false,
            pending: 0,
            conflicts: 0,
            cursor: 0,
            last_sync_at: null,
            last_error: null,
          };
        case 'sync_conflicts':
          return [];
        case 'session_close':
        case 'session_write':
        case 'session_resize':
        case 'host_key_prompt_resolve':
        case 'sync_sign_out':
        case 'sync_resolve_conflict':
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
    identities = [
      {
        id: 'identity-1',
        name: 'work laptop',
        comment: null,
        fingerprint: null,
        created_at: '2026-01-01T00:00:00Z',
        updated_at: '2026-01-01T00:00:00Z',
        deleted: false,
      },
    ];
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

  it('signs in to the sync server with the documented arguments', async () => {
    await vi.waitFor(() => {
      expect(target.textContent).toContain('web-1');
    });

    click(findButton(target, 'Sync'));
    await vi.waitFor(() => {
      expect(target.textContent).toContain('Cloud sync');
    });

    const inputs = target.querySelectorAll('input');
    const url = target.querySelector('input[type="url"]') as HTMLInputElement;
    const email = target.querySelector('input[type="email"]') as HTMLInputElement;
    const password = target.querySelector('input[type="password"]') as HTMLInputElement;
    expect(url).not.toBeNull();
    type(url, 'https://sync.example.com');
    type(email, 'user@example.com');
    type(password, 'a long passphrase');
    expect(inputs.length).toBeGreaterThan(2);

    const form = target.querySelector('form');
    form?.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }));

    await vi.waitFor(() => {
      expect(invokeCalls.some((call) => call.cmd === 'sync_sign_in')).toBe(true);
    });

    const signIn = invokeCalls.find((call) => call.cmd === 'sync_sign_in');
    expect(signIn?.args).toEqual({
      serverUrl: 'https://sync.example.com',
      email: 'user@example.com',
      password: 'a long passphrase',
      deviceName: '',
    });
  });

  it('saves the appearance settings with the documented shape', async () => {
    await vi.waitFor(() => {
      expect(target.textContent).toContain('web-1');
    });

    // Mod+Comma opens the settings.
    window.dispatchEvent(
      new KeyboardEvent('keydown', { key: ',', ctrlKey: true, bubbles: true, cancelable: true }),
    );
    await vi.waitFor(() => {
      expect(target.textContent).toContain('Settings');
    });

    const size = target.querySelector('input[type="number"]') as HTMLInputElement;
    expect(size).not.toBeNull();
    type(size, '18');

    const save = findButton(target, 'Save');
    click(save);

    await vi.waitFor(() => {
      expect(invokeCalls.some((call) => call.cmd === 'prefs_set')).toBe(true);
    });

    const call = invokeCalls.find((entry) => entry.cmd === 'prefs_set');
    const prefs = call?.args?.prefs as
      { theme: string; terminal: { font_size: number; cursor_style: string } } | undefined;
    expect(prefs?.terminal.font_size).toBe(18);
    // The theme is the one the backend reports; the dialog only changes
    // what the user edits.
    expect(prefs?.theme).toBe('dark');
    expect(prefs?.terminal.cursor_style).toBe('block');
  });

  it('splits a pane and saves the split layout', async () => {
    await vi.waitFor(() => {
      expect(target.textContent).toContain('web-1');
    });

    setTimeout(() => console.log('RAW TIMER FIRED', Date.now()), 100);
    setTimeout(() => console.log('RAW TIMER 750 FIRED', Date.now()), 750);

    // The split command of the registry.
    window.dispatchEvent(
      new KeyboardEvent('keydown', {
        key: 'E',
        ctrlKey: true,
        shiftKey: true,
        bubbles: true,
        cancelable: true,
      }),
    );
    await vi.waitFor(() => {
      expect(target.textContent).toContain('No session in this pane');
    });
    // The empty pane offers the hosts.
    expect(target.textContent).toContain('web-1');

    // The layout that is saved holds the split (the save is debounced).
    await vi.waitFor(
      () => {
        const saves = invokeCalls.filter((call) => call.cmd === 'layout_save');
        const last = saves.pop();
        const doc = last?.args?.doc as
          { windows: { tabs: { root: { split?: unknown } }[] }[] } | undefined;
        const tabs = doc?.windows?.[0]?.tabs ?? [];
        // The pane that was split is the one that had the focus.
        expect(tabs.some((tab) => tab.root.split !== undefined)).toBe(true);
      },
      { timeout: 3000 },
    );
  });

  it('opens the context menu of a host and connects it', async () => {
    await vi.waitFor(() => {
      expect(target.textContent).toContain('web-1');
    });

    const card = target.querySelector('.card') as HTMLElement;
    expect(card).not.toBeNull();
    card.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true }));

    await vi.waitFor(() => {
      expect(target.textContent).toContain('Add to favorites');
    });

    // The menu offers the actions of the host. The card has its own
    // "Connect" button, so the one of the menu is picked explicitly.
    const menuButtons = Array.from(target.querySelectorAll('.menu button')) as HTMLButtonElement[];
    const menuConnect = menuButtons.find((button) => button.textContent === 'Connect');
    expect(menuConnect).toBeDefined();
    menuConnect!.click();

    // The connection dialog opens for the host.
    await vi.waitFor(() => {
      expect(target.textContent).toContain('Password');
    });
  });

  it('saves the folder and the tags of a host', async () => {
    await vi.waitFor(() => {
      expect(target.textContent).toContain('web-1');
    });

    click(findButton(target, 'Edit'));
    await vi.waitFor(() => {
      expect(target.textContent).toContain('Edit host');
    });

    // The folder and the tag of the host are offered.
    const selects = target.querySelectorAll('.dialog select');
    const folder = selects[1] as HTMLSelectElement;
    expect(folder).not.toBeNull();
    folder.value = 'group-1';
    folder.dispatchEvent(new Event('change', { bubbles: true }));

    const check = target.querySelector('.dialog input[type="checkbox"]') as HTMLInputElement;
    expect(check).not.toBeNull();
    check.checked = true;
    check.dispatchEvent(new Event('change', { bubbles: true }));

    click(findButton(target, 'Save'));

    await vi.waitFor(() => {
      expect(invokeCalls.some((call) => call.cmd === 'host_save')).toBe(true);
    });
    const save = invokeCalls.filter((call) => call.cmd === 'host_save').pop();
    const host = save?.args?.host as { group_id: string | null; tags: string[] } | undefined;
    expect(host?.group_id).toBe('group-1');
    expect(host?.tags).toEqual(['tag-1']);
  });

  it('binds a key file to an identity with the native picker', async () => {
    await vi.waitFor(() => {
      expect(target.textContent).toContain('web-1');
    });

    click(findButton(target, 'Identities'));
    await vi.waitFor(() => {
      expect(target.textContent).toContain('work laptop');
    });

    // The bound path is shown, and it comes from the backend.
    await vi.waitFor(() => {
      expect(target.textContent).toContain('/home/user/.ssh/id_ed25519');
    });

    click(findButton(target, 'Choose key…'));
    await vi.waitFor(() => {
      expect(invokeCalls.some((call) => call.cmd === 'plugin:dialog|open')).toBe(true);
    });
    await vi.waitFor(() => {
      expect(invokeCalls.some((call) => call.cmd === 'identity_bind')).toBe(true);
    });

    const bind = invokeCalls.find((call) => call.cmd === 'identity_bind');
    expect(bind?.args).toEqual({
      id: 'identity-1',
      keyPath: '/home/user/.ssh/id_ed25519',
    });
  });

  it('creates an identity with the documented shape', async () => {
    await vi.waitFor(() => {
      expect(target.textContent).toContain('web-1');
    });

    click(findButton(target, 'Identities'));
    await vi.waitFor(() => {
      expect(target.textContent).toContain('New identity');
    });

    const inputs = target.querySelectorAll('.dialog form input');
    type(inputs[0] as HTMLInputElement, 'backup key');
    click(findButton(target, 'Create'));

    await vi.waitFor(() => {
      expect(invokeCalls.some((call) => call.cmd === 'identity_save')).toBe(true);
    });

    const save = invokeCalls.find((call) => call.cmd === 'identity_save');
    const identity = save?.args?.identity as { name: string; comment: string | null } | undefined;
    expect(identity?.name).toBe('backup key');
    expect(identity?.comment).toBeNull();
  });

  it('rebinds a command and refuses a combination that is already taken', async () => {
    await vi.waitFor(() => {
      expect(target.textContent).toContain('web-1');
    });

    window.dispatchEvent(
      new KeyboardEvent('keydown', { key: ',', ctrlKey: true, bubbles: true, cancelable: true }),
    );
    await vi.waitFor(() => {
      expect(target.textContent).toContain('Settings');
    });

    // Rebind the command palette.
    const change = findButton(target, 'Change');
    click(change);

    // A combination that another command already uses is refused.
    target.querySelector('.dialog')?.dispatchEvent(
      new KeyboardEvent('keydown', {
        key: 't',
        ctrlKey: true,
        bubbles: true,
        cancelable: true,
      }),
    );
    await vi.waitFor(() => {
      expect(target.textContent).toContain('Already used by');
    });

    // A free combination is accepted.
    target.querySelector('.dialog')?.dispatchEvent(
      new KeyboardEvent('keydown', {
        key: 'F8',
        ctrlKey: true,
        shiftKey: true,
        bubbles: true,
        cancelable: true,
      }),
    );
    await vi.waitFor(() => {
      expect(target.textContent).toContain('Ctrl+Shift+F8');
    });

    click(findButton(target, 'Save'));
    await vi.waitFor(() => {
      expect(invokeCalls.some((call) => call.cmd === 'prefs_set')).toBe(true);
    });

    const call = invokeCalls.find((entry) => entry.cmd === 'prefs_set');
    const prefs = call?.args?.prefs as { keybindings: Record<string, string> } | undefined;
    expect(prefs?.keybindings['palette.open']).toBe('Ctrl+Shift+F8');
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
