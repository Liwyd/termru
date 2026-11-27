// @vitest-environment jsdom
/**
 * Accessibility: axe-core over every route and every dialog of the app, as a
 * screen reader meets them (the whole document, shell included), in the
 * states a user can actually reach: loaded, empty, failed, and with form
 * errors showing.
 *
 * jsdom cannot evaluate the rules that need layout or colours; the list is in
 * `test-support/axe.ts`, and contrast is covered by `design-tokens.test.ts`.
 * The keyboard checklist of every page is `docs/accessibility.md`.
 */

import { flushSync, mount, tick, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import App from '../App.svelte';
import { notify, notices } from './notify.svelte';
import { connectFlow } from './stores/connect.svelte';
import { hosts } from './stores/hosts.svelte';
import { palette, commandEntries } from './stores/palette.svelte';
import { prefs } from './stores/prefs.svelte';
import { route } from './stores/route.svelte';
import { sessions, type Pane, type Tab } from './stores/sessions.svelte';
import { shortcuts } from './stores/shortcuts.svelte';
import { sync } from './stores/sync.svelte';
import { pasteGate } from './terminal/paste.svelte';
import { pruneTerminals } from './terminal/registry';
import { click, findButton, stubBrowserApis, type } from './test-support/dom';
import { axeViolations, formatViolations } from './test-support/axe';
import { answers, failures, resetTauriMock } from './test-support/tauri-core-mock';
import { resetXtermMock } from './test-support/xterm-mock';
import type {
  AuthPromptMessage,
  ConflictView,
  Group,
  Host,
  HostKeyPromptMessage,
  Identity,
  Preferences,
  SyncStatus,
} from './types';

const bus = vi.hoisted(() => ({
  handlers: new Map<string, (event: { payload: unknown }) => void>(),
}));

vi.mock('@tauri-apps/api/core', () => import('./test-support/tauri-core-mock'));
vi.mock('@tauri-apps/api/event', () => ({
  listen: async (name: string, handler: (event: { payload: unknown }) => void) => {
    bus.handlers.set(name, handler);
    return () => bus.handlers.delete(name);
  },
}));
vi.mock('@xterm/xterm', () => import('./test-support/xterm-mock'));
vi.mock('@xterm/addon-fit', () => import('./test-support/xterm-mock'));
vi.mock('@xterm/addon-webgl', () => import('./test-support/xterm-mock'));
vi.mock('@xterm/addon-search', () => import('./test-support/xterm-mock'));
vi.mock('@xterm/addon-web-links', () => import('./test-support/xterm-mock'));

const STAMP = '2026-01-01T00:00:00Z';

function host(id: string, over: Partial<Host> = {}): Host {
  return {
    id,
    name: id,
    address: `${id}.example.test`,
    port: 22,
    username: 'ops',
    auth: { type: 'password' },
    group_id: null,
    tags: [],
    favorite: false,
    description: null,
    term: 'xterm-256color',
    keepalive_seconds: 0,
    sort_order: 0,
    created_at: STAMP,
    updated_at: STAMP,
    deleted: false,
    ...over,
  };
}

function group(id: string, name: string, parent: string | null = null): Group {
  return {
    id,
    name,
    parent_id: parent,
    sort_order: 0,
    created_at: STAMP,
    updated_at: STAMP,
    deleted: false,
  };
}

function identity(id: string, name: string): Identity {
  return {
    id,
    name,
    comment: 'Work laptop key',
    fingerprint: 'SHA256:Zk3qN0yQm1v8cTt2uRk5GxJ7aWb4HsD9eLp6OiXnYfE',
    created_at: STAMP,
    updated_at: STAMP,
    deleted: false,
  };
}

const HOSTS: Host[] = [
  host('atlas-prod', {
    group_id: 'g-eu',
    favorite: true,
    auth: { type: 'private_key', identity_id: 'i-work' },
  }),
  host('relay-edge', { group_id: 'g-edge', favorite: true }),
  host('staging-api', { group_id: 'g-sand' }),
  host('archive'),
];
const GROUPS: Group[] = [
  group('g-prod', 'Production'),
  group('g-eu', 'Europe', 'g-prod'),
  group('g-edge', 'Edge', 'g-prod'),
  group('g-sand', 'Sandbox'),
];
const IDENTITIES: Identity[] = [identity('i-work', 'work-ed25519')];

function status(over: Partial<SyncStatus> = {}): SyncStatus {
  return {
    settings: {
      server_url: 'https://sync.example.test',
      email: 'ops@example.test',
      device_name: 'workstation',
      device_id: 'device-1234',
    },
    signed_in: true,
    pending: 0,
    conflicts: 0,
    cursor: 5,
    last_sync_at: '2026-10-24T09:00:00Z',
    last_error: null,
    ...over,
  };
}

function conflict(): ConflictView {
  return {
    record_id: 'h1',
    kind: 'host',
    server_rev: 9,
    observed_at: STAMP,
    local: { id: 'h1', name: 'staging', address: '10.0.0.1', port: 22, updated_at: 'a' },
    remote: { id: 'h1', name: 'staging', address: '10.0.0.9', port: 22, updated_at: 'b' },
  };
}

function storedPrefs(): Preferences {
  return {
    theme: 'light',
    terminal: {
      font_family: 'IBM Plex Mono, monospace',
      font_size: 14,
      scrollback: 10_000,
      cursor_blink: true,
      cursor_style: 'block',
      copy_on_select: false,
      confirm_multiline_paste: true,
      line_height: 1.3,
    },
    keybindings: {},
    reduced_motion: 'system',
    focus_visibility: false,
    routine_notices: true,
  };
}

let target: HTMLElement;
let app: ReturnType<typeof mount> | null = null;

/** The app as it starts, with data behind it; `hosts.load()` fills the stores. */
async function start(
  options: { empty?: boolean; sync?: SyncStatus; conflicts?: ConflictView[] } = {},
): Promise<void> {
  answers.set('host_list', options.empty ? [] : HOSTS);
  answers.set('group_list', options.empty ? [] : GROUPS);
  answers.set('identity_list', options.empty ? [] : IDENTITIES);
  answers.set('prefs_get', storedPrefs());
  answers.set('sync_status', options.sync ?? status({ signed_in: false, last_sync_at: null }));
  answers.set('sync_conflicts', options.conflicts ?? []);
  answers.set('app_info', { name: 'termru-desktop', version: '0.2.0' });
  app = mount(App, { target });
  await vi.waitFor(() => expect(hosts.loaded).toBe(true));
  await vi.waitFor(() => expect(sync.loading).toBe(false));
  await tick();
}

async function visit(id: Parameters<typeof route.go>[0]): Promise<void> {
  route.go(id);
  flushSync();
  await tick();
  await tick();
}

/** Fails with the rule, its impact and the offending nodes. */
async function expectAccessible(): Promise<void> {
  const failures = await axeViolations(document);
  expect(formatViolations(failures)).toBe('');
}

beforeEach(() => {
  resetTauriMock();
  resetXtermMock();
  pruneTerminals(new Set());
  stubBrowserApis();
  bus.handlers.clear();
  connectFlow.cancel();
  palette.hide();
  shortcuts.cancel();
  notices.reset();
  pasteGate.pending = null;
  hosts.hosts = [];
  hosts.loaded = false;
  hosts.error = null;
  hosts.query = '';
  hosts.filterTab = 'all';
  hosts.folderFilter = '';
  hosts.passwordStored = {};
  sessions.tabs = [];
  sessions.activeId = null;
  sessions.windowLabel = null;
  sessions.layoutLoaded = false;
  sessions.closeRequest = null;
  sync.status = null;
  sync.error = null;
  route.route = 'hosts';
  route.editHost = null;
  route.draft = null;
  window.location.hash = '#hosts';
  // What `index.html` gives the document: the language and the title.
  document.documentElement.lang = 'en';
  document.title = 'Termru';
  target = document.createElement('div');
  target.id = 'app';
  document.body.appendChild(target);
});

afterEach(() => {
  if (app) unmount(app);
  app = null;
  target.remove();
  document.body.replaceChildren();
  history.pushState({}, '', '/');
});

describe('axe: every route', () => {
  it('Hosts', async () => {
    await start();
    await vi.waitFor(() => expect(target.textContent).toContain('atlas-prod'));
    await expectAccessible();
  });

  it('Hosts with sessions running (banner, live rows)', async () => {
    await start();
    sessions.open(HOSTS[0], {});
    flushSync();
    await tick();
    await expectAccessible();
  });

  it('Hosts, empty', async () => {
    await start({ empty: true });
    expect(target.textContent).toContain('No hosts yet');
    await expectAccessible();
  });

  it('Hosts, failed to load', async () => {
    answers.set('host_list', () => {
      throw new Error('the store is locked');
    });
    answers.set('prefs_get', storedPrefs());
    app = mount(App, { target });
    await vi.waitFor(() => expect(hosts.error).not.toBeNull());
    await tick();
    await expectAccessible();
  });

  it('Sessions', async () => {
    await start();
    const tab = sessions.open(HOSTS[0], {});
    const stored = sessions.tabs.find((entry) => entry.id === tab.id) as Tab;
    const pane = sessions.activePaneOf(stored) as Pane;
    pane.sessionId = 's1';
    sessions.applyStatus(pane, { state: 'ready' });
    await visit('sessions');
    await expectAccessible();
  });

  it('Sessions, nothing open', async () => {
    await start();
    await visit('sessions');
    await expectAccessible();
  });

  it('Host editor, new and filled', async () => {
    await start();
    route.edit(null);
    flushSync();
    await tick();
    await expectAccessible();
    route.go('hosts');
    flushSync();
    route.edit(HOSTS[0]);
    flushSync();
    await tick();
    await expectAccessible();
  });

  it('Host editor with errors showing', async () => {
    await start();
    route.edit(null);
    flushSync();
    await tick();
    // Leave the required fields empty and blur them: the errors appear.
    for (const id of ['host-name', 'host-address']) {
      const input = document.getElementById(id) as HTMLInputElement;
      input.focus();
      input.dispatchEvent(new FocusEvent('blur'));
      input.dispatchEvent(new FocusEvent('focusout', { bubbles: true }));
    }
    type(document.getElementById('host-port') as HTMLInputElement, '99999');
    (document.getElementById('host-port') as HTMLInputElement).dispatchEvent(
      new FocusEvent('focusout', { bubbles: true }),
    );
    flushSync();
    await expectAccessible();
  });

  it('Folders', async () => {
    await start();
    await visit('folders');
    await expectAccessible();
  });

  it('Folders, empty', async () => {
    await start({ empty: true });
    await visit('folders');
    await expectAccessible();
  });

  it('Identities', async () => {
    await start();
    await visit('identities');
    await expectAccessible();
  });

  it('Identities, empty', async () => {
    await start({ empty: true });
    await visit('identities');
    await expectAccessible();
  });

  it('Sync, signed out (the sign-in form)', async () => {
    await start();
    await visit('sync');
    await expectAccessible();
  });

  it('Sync, signed out, with the form errors showing', async () => {
    await start();
    await visit('sync');
    for (const input of Array.from(target.querySelectorAll<HTMLInputElement>('input'))) {
      input.focus();
      input.dispatchEvent(new FocusEvent('focusout', { bubbles: true }));
    }
    flushSync();
    await expectAccessible();
  });

  it('Sync, signed in, with conflicts', async () => {
    await start({ sync: status({ conflicts: 1 }), conflicts: [conflict()] });
    await visit('sync');
    await vi.waitFor(() => expect(target.textContent).toContain('Conflicts'));
    await expectAccessible();
  });

  it('Preferences', async () => {
    await start();
    await visit('preferences');
    await expectAccessible();
  });
});

describe('axe: dialogs and layers', () => {
  it('the delete confirmation of a host', async () => {
    await start();
    await vi.waitFor(() => expect(target.textContent).toContain('atlas-prod'));
    const more = target.querySelector('button.more') as HTMLButtonElement;
    click(more);
    const del = Array.from(document.querySelectorAll<HTMLElement>('[role="menuitem"]')).find(
      (entry) => entry.textContent?.trim() === 'Delete',
    ) as HTMLElement;
    click(del);
    await tick();
    expect(document.querySelector('[role="alertdialog"]')).not.toBeNull();
    await expectAccessible();
  });

  it('the row menu', async () => {
    await start();
    await vi.waitFor(() => expect(target.textContent).toContain('atlas-prod'));
    click(target.querySelector('button.more') as HTMLButtonElement);
    expect(document.querySelector('[role="menu"]')).not.toBeNull();
    await expectAccessible();
  });

  it('the connect dialog', async () => {
    await start();
    void connectFlow.request(HOSTS[2]);
    await vi.waitFor(() => expect(connectFlow.pending).not.toBeNull());
    await tick();
    expect(document.querySelector('[role="dialog"], [role="alertdialog"]')).not.toBeNull();
    await expectAccessible();
  });

  it('the host key dialog: unknown and changed', async () => {
    await start();
    await vi.waitFor(() => expect(bus.handlers.has('host-key-prompt')).toBe(true));
    const unknown: HostKeyPromptMessage = {
      id: 'k1',
      session_id: 's1',
      request: {
        state: 'unknown',
        host: 'new.example.test',
        port: 22,
        key: { key_type: 'ssh-ed25519', fingerprint: 'SHA256:abc' },
      },
    };
    bus.handlers.get('host-key-prompt')?.({ payload: unknown });
    flushSync();
    await tick();
    await expectAccessible();
    bus.handlers.get('host-key-prompt-closed')?.({ payload: { id: 'k1' } });
    flushSync();
    await tick();
    const changed: HostKeyPromptMessage = {
      id: 'k2',
      session_id: 's1',
      request: {
        state: 'changed',
        host: 'rebuilt.example.test',
        port: 22,
        old: { key_type: 'ssh-ed25519', fingerprint: 'SHA256:old' },
        new: { key_type: 'ssh-ed25519', fingerprint: 'SHA256:new' },
      },
    };
    bus.handlers.get('host-key-prompt')?.({ payload: changed });
    flushSync();
    await tick();
    expect(document.querySelector('[role="alertdialog"], [role="dialog"]')).not.toBeNull();
    await expectAccessible();
  });

  it('the authentication question: keyboard-interactive and passphrase', async () => {
    await start();
    await vi.waitFor(() => expect(bus.handlers.has('auth-prompt')).toBe(true));
    const kbd: AuthPromptMessage = {
      id: 'a1',
      session_id: 's1',
      host_id: 'h',
      request: {
        kind: { type: 'keyboard_interactive' },
        name: 'Two-factor',
        instructions: 'Enter the code from your app.',
        prompts: [
          { text: 'Verification code: ', echo: true },
          { text: 'PIN: ', echo: false },
        ],
      },
    };
    bus.handlers.get('auth-prompt')?.({ payload: kbd });
    flushSync();
    await tick();
    await expectAccessible();
    bus.handlers.get('auth-prompt-closed')?.({ payload: { id: 'a1' } });
    flushSync();
    await tick();
    const phrase: AuthPromptMessage = {
      id: 'a2',
      session_id: 's1',
      host_id: 'h',
      request: {
        kind: { type: 'passphrase', identity: { id: 'i-work', name: 'work-ed25519' }, retry: true },
        name: '',
        instructions: '',
        prompts: [{ text: 'Passphrase for work-ed25519', echo: false }],
      },
    };
    bus.handlers.get('auth-prompt')?.({ payload: phrase });
    flushSync();
    await tick();
    await expectAccessible();
  });

  it('the close-session question', async () => {
    await start();
    const tab = sessions.open(HOSTS[0], {});
    const stored = sessions.tabs.find((entry) => entry.id === tab.id) as Tab;
    const pane = sessions.activePaneOf(stored) as Pane;
    pane.sessionId = 's1';
    sessions.applyStatus(pane, { state: 'ready' });
    await visit('sessions');
    sessions.closeRequest = { kind: 'tab', tabId: stored.id, paneId: null, live: 1 };
    flushSync();
    await tick();
    expect(document.querySelector('[role="alertdialog"]')).not.toBeNull();
    await expectAccessible();
  });

  it('the multi-line paste question', async () => {
    await start();
    await visit('sessions');
    pasteGate.pending = {
      paneId: 'p1',
      text: 'ls\npwd\nwhoami',
      lines: 3,
      preview: ['ls', 'pwd', 'whoami'],
    };
    flushSync();
    await tick();
    expect(document.querySelector('[role="dialog"]')).not.toBeNull();
    await expectAccessible();
  });

  it('the command palette', async () => {
    await start();
    palette.show(
      commandEntries(
        () => {},
        (command) => command.accelerator,
      ),
    );
    flushSync();
    await tick();
    expect(document.querySelector('[aria-label="Command palette"]')).not.toBeNull();
    await expectAccessible();
  });

  it('the restore-defaults question of Preferences', async () => {
    await start();
    await visit('preferences');
    click(findButton(target, 'Restore defaults'));
    await tick();
    expect(document.querySelector('[role="alertdialog"], [role="dialog"]')).not.toBeNull();
    await expectAccessible();
  });

  it('notices', async () => {
    await start();
    notify({ key: 'host.deleted', entityId: 'h', values: { name: 'atlas-prod' } });
    notify({ key: 'sync.failed', values: { detail: 'the server did not answer' } });
    flushSync();
    await tick();
    expect(document.querySelectorAll('[data-notice-key]').length).toBeGreaterThan(0);
    await expectAccessible();
  });
});

/**
 * Every control that reports an error says so (`aria-invalid`) and points at
 * the text that explains it (`aria-describedby`); every field message is
 * pointed at by a control. Returns how many controls were invalid, so a
 * scenario can prove that the error state was really reached.
 */
function expectErrorsWired(): number {
  const invalid = Array.from(document.querySelectorAll('[aria-invalid="true"]'));
  for (const control of invalid) {
    const ids = (control.getAttribute('aria-describedby') ?? '').split(/\s+/).filter(Boolean);
    const texts = ids.map((id) => document.getElementById(id)?.textContent?.trim() ?? '');
    const label = `${control.tagName.toLowerCase()}#${control.id}`;
    expect(ids.length, `${label} has no aria-describedby`).toBeGreaterThan(0);
    expect(
      texts.some((text) => text.length > 0),
      `${label} points at an empty message`,
    ).toBe(true);
  }
  const messages = document.querySelectorAll('.field .error, .field-error, p.error:not(.conflict)');
  for (const message of Array.from(messages)) {
    expect(message.id, `an error message without an id: ${message.textContent}`).not.toBe('');
    expect(
      document.querySelector(`[aria-describedby~="${message.id}"]`),
      `nothing is described by ${message.id}`,
    ).not.toBeNull();
  }
  return invalid.length;
}

function leave(element: HTMLElement): void {
  element.dispatchEvent(new FocusEvent('focusout', { bubbles: true }));
  element.dispatchEvent(new Event('blur'));
  flushSync();
}

describe('form errors are wired to their fields (aria-invalid + aria-describedby)', () => {
  it('host editor: the required fields, the port and a refused tag', async () => {
    await start();
    route.edit(null);
    flushSync();
    await tick();
    click(findButton(target, 'Save host'));
    await tick();
    type(document.getElementById('host-port') as HTMLInputElement, '70000');
    leave(document.getElementById('host-port') as HTMLInputElement);
    failures.set('tag_save', 'the tag name is not allowed');
    type(target.querySelector('input[aria-label="New tag"]') as HTMLInputElement, 'bad');
    click(findButton(target, 'Add tag'));
    await vi.waitFor(() => expect(target.querySelector('#host-tag-error')).not.toBeNull());
    expect(expectErrorsWired()).toBeGreaterThanOrEqual(4);
    expect(document.getElementById('host-tag-error')?.textContent).toContain('not allowed');
    await expectAccessible();
  });

  it('sync sign-in: server, email and password', async () => {
    await start();
    await visit('sync');
    type(document.getElementById('sync-server') as HTMLInputElement, '');
    type(document.getElementById('sync-email') as HTMLInputElement, '');
    target
      .querySelector('form')
      ?.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }));
    flushSync();
    expect(expectErrorsWired()).toBe(3);
  });

  it('preferences: size, line height and scrollback', async () => {
    await start();
    await visit('preferences');
    type(document.getElementById('pref-size') as HTMLInputElement, '90');
    type(document.getElementById('pref-line') as HTMLInputElement, '5');
    type(document.getElementById('pref-scrollback') as HTMLInputElement, 'lots');
    expect(expectErrorsWired()).toBe(3);
    // The scrollback keeps its description and gains the error.
    const described = document.getElementById('pref-scrollback')?.getAttribute('aria-describedby');
    expect(described).toBe('pref-scroll-desc pref-scroll-error');
    await expectAccessible();
  });

  it('folders: an empty name', async () => {
    await start();
    await visit('folders');
    const item = Array.from(target.querySelectorAll<HTMLButtonElement>('button.item')).find(
      (button) => button.querySelector('.label')?.textContent?.trim() === 'Sandbox',
    ) as HTMLButtonElement;
    click(item);
    type(document.getElementById('folder-name') as HTMLInputElement, '   ');
    click(findButton(target, 'Save folder'));
    await tick();
    expect(expectErrorsWired()).toBe(1);
    await expectAccessible();
  });

  it('identities: an empty name', async () => {
    await start();
    await visit('identities');
    const item = Array.from(target.querySelectorAll<HTMLButtonElement>('button.item')).find(
      (button) => button.querySelector('.item-name')?.textContent?.trim() === 'work-ed25519',
    ) as HTMLButtonElement;
    click(item);
    type(document.getElementById('identity-name') as HTMLInputElement, '  ');
    click(findButton(target, 'Save identity'));
    await tick();
    expect(expectErrorsWired()).toBe(1);
    await expectAccessible();
  });

  it('a passphrase that was refused: the field says so', async () => {
    await start();
    await vi.waitFor(() => expect(bus.handlers.has('auth-prompt')).toBe(true));
    bus.handlers.get('auth-prompt')?.({
      payload: {
        id: 'a3',
        session_id: 's1',
        host_id: 'h',
        request: {
          kind: {
            type: 'passphrase',
            identity: { id: 'i-work', name: 'work-ed25519' },
            retry: true,
          },
          name: '',
          instructions: '',
          prompts: [{ text: 'Passphrase for work-ed25519', echo: false }],
        },
      } satisfies AuthPromptMessage,
    });
    flushSync();
    await tick();
    expect(expectErrorsWired()).toBe(1);
  });
});

describe('the preference store is what the pages read', () => {
  it('uses the stored preferences', async () => {
    await start();
    expect(prefs.prefs.theme).toBe('light');
  });
});
