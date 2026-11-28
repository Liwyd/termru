// @vitest-environment jsdom
/**
 * The whole app around keyboard, navigation and connecting: the behaviours
 * that live in `App.svelte` and cross several components.
 */

import { flushSync, mount, tick, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import App from '../../App.svelte';
import { chrome } from '../stores/chrome.svelte';
import { connectFlow } from '../stores/connect.svelte';
import { hosts } from '../stores/hosts.svelte';
import { intents } from '../stores/intents.svelte';
import { palette } from '../stores/palette.svelte';
import { prefs } from '../stores/prefs.svelte';
import { route } from '../stores/route.svelte';
import { sessions } from '../stores/sessions.svelte';
import { shortcuts } from '../stores/shortcuts.svelte';
import { sshImport } from '../stores/ssh-import.svelte';
import { notices } from '../notify.svelte';
import { noticeText } from '../test-support/notices';
import { unsaved } from '../stores/unsaved.svelte';
import { pruneTerminals } from '../terminal/registry';
import { click, findButton, stubBrowserApis, type } from '../test-support/dom';
import { answers, callsOf, failures, resetTauriMock } from '../test-support/tauri-core-mock';
import { resetXtermMock } from '../test-support/xterm-mock';
import type { Host, Preferences } from '../types';

const bus = vi.hoisted(() => ({
  handlers: new Map<string, (event: { payload: unknown }) => void>(),
}));

vi.mock('@tauri-apps/api/core', () => import('../test-support/tauri-core-mock'));
vi.mock('@tauri-apps/api/event', () => ({
  listen: async (name: string, handler: (event: { payload: unknown }) => void) => {
    bus.handlers.set(name, handler);
    return () => bus.handlers.delete(name);
  },
}));
vi.mock('@xterm/xterm', () => import('../test-support/xterm-mock'));
vi.mock('@xterm/addon-fit', () => import('../test-support/xterm-mock'));
vi.mock('@xterm/addon-webgl', () => import('../test-support/xterm-mock'));
vi.mock('@xterm/addon-search', () => import('../test-support/xterm-mock'));
vi.mock('@xterm/addon-web-links', () => import('../test-support/xterm-mock'));

const WEB: Host = {
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

function storedPrefs(theme: Preferences['theme']): Preferences {
  return {
    theme,
    terminal: {
      font_family: 'IBM Plex Mono, monospace',
      font_size: 14,
      scrollback: 1000,
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

/** A chord on Linux/Windows: Ctrl (+ Shift) + key, from the given element. */
function chord(
  key: string,
  init: KeyboardEventInit = {},
  from: EventTarget = window,
): KeyboardEvent {
  const event = new KeyboardEvent('keydown', {
    key,
    ctrlKey: true,
    bubbles: true,
    cancelable: true,
    ...init,
  });
  from.dispatchEvent(event);
  flushSync();
  return event;
}

let target: HTMLElement;
let app: ReturnType<typeof mount> | null = null;

async function start(theme: Preferences['theme'] = 'dark'): Promise<void> {
  answers.set('host_list', [WEB]);
  answers.set('prefs_get', storedPrefs(theme));
  app = mount(App, { target });
  await vi.waitFor(() => expect(target.textContent).toContain('web-1'));
}

beforeEach(() => {
  resetTauriMock();
  resetXtermMock();
  pruneTerminals(new Set());
  stubBrowserApis();
  // The operating system prefers a dark theme.
  (globalThis as { matchMedia?: unknown }).matchMedia = (query: string) => ({
    matches: !/max-width/.test(query),
    addEventListener() {},
    removeEventListener() {},
  });
  bus.handlers.clear();
  connectFlow.cancel();
  palette.hide();
  sshImport.close();
  chrome.sidebarCollapsed = false;
  chrome.narrow = false;
  chrome.inspectorOpen = true;
  intents.pending = null;
  shortcuts.cancel();
  notices.reset();
  hosts.hosts = [];
  hosts.loaded = false;
  hosts.searchFocus = false;
  hosts.query = '';
  hosts.showLibrary('all');
  hosts.selectedId = null;
  hosts.passwordStored = {};
  sessions.tabs = [];
  sessions.activeId = null;
  sessions.windowLabel = null;
  sessions.layoutLoaded = false;
  route.route = 'hosts';
  route.editHost = null;
  route.draft = null;
  window.location.hash = '#hosts';
  target = document.createElement('div');
  document.body.appendChild(target);
});

afterEach(() => {
  if (app) unmount(app);
  app = null;
  target.remove();
  document.body.replaceChildren();
  history.pushState({}, '', '/');
});

describe('opening the host editor', () => {
  it('"New host" opens an empty editor and Cancel goes back', async () => {
    await start();
    click(findButton(target, 'New host'));
    await vi.waitFor(() => expect(route.route).toBe('host-edit'));
    expect(target.textContent).toContain('New connection');
    // It does not bounce back to the hosts page.
    await tick();
    expect(route.route).toBe('host-edit');

    click(findButton(target, 'Cancel'));
    expect(route.route).toBe('hosts');
  });

  it('the Add host shortcut opens it from anywhere', async () => {
    await start();
    route.go('preferences');
    flushSync();
    chord('N', { shiftKey: true });
    expect(route.route).toBe('host-edit');
    expect(route.editHost).toBeNull();
    expect(target.textContent).toContain('New connection');
  });

  it('opens a new host when #host-edit is reloaded without a draft', async () => {
    await start();
    route.go('host-edit');
    flushSync();
    await tick();
    expect(route.route).toBe('host-edit');
    expect(target.textContent).toContain('New connection');
  });

  it('"Edit" opens the host, and Save returns to the page the user came from', async () => {
    await start();
    click(findButton(target, 'Edit'));
    await vi.waitFor(() => expect(target.textContent).toContain('Edit connection'));
    answers.set('host_save', (args: unknown) => (args as { host: Host }).host);
    type(document.getElementById('host-name') as HTMLInputElement, 'renamed');
    click(findButton(target, 'Save host'));
    await vi.waitFor(() => expect(route.route).toBe('hosts'));
    expect(callsOf('host_save')).toHaveLength(1);
  });
});

describe('keyboard shortcuts', () => {
  it('keep chords out of text fields, except the palette chord', async () => {
    await start();
    const search = target.querySelector('input[type="search"]') as HTMLInputElement;
    search.focus();

    chord('N', { shiftKey: true }, search);
    expect(route.route).toBe('hosts');

    chord('P', { shiftKey: true }, search);
    expect(palette.open).toBe(true);
  });

  it('keep chords out while a dialog is open', async () => {
    await start();
    click(findButton(target, 'Connect'));
    await vi.waitFor(() => expect(connectFlow.pending).not.toBeNull());
    expect(target.querySelector('[aria-modal="true"]')).not.toBeNull();

    chord('N', { shiftKey: true });
    expect(route.route).toBe('hosts');
    chord('P', { shiftKey: true });
    expect(palette.open).toBe(false);
  });

  it('are not run by the shell chords: Ctrl+W, Ctrl+D and Ctrl+K do nothing', async () => {
    await start();
    sessions.open(WEB, {});
    for (const key of ['w', 'd', 'k', 'f', 'n', 't']) chord(key);
    expect(sessions.tabs).toHaveLength(1);
    expect(palette.open).toBe(false);
    expect(route.route).toBe('hosts');
  });

  it('open the palette with the PC chord, with every host listed', async () => {
    await start();
    hosts.hosts = Array.from({ length: 12 }, (_, i) => ({ ...WEB, id: `h${i}`, name: `srv-${i}` }));
    // The hosts page filters to a few; the palette still offers all of them.
    hosts.query = 'srv-1';
    chord('P', { shiftKey: true });
    expect(palette.open).toBe(true);
    const connects = palette.entries.filter((entry) => entry.id.startsWith('host:'));
    expect(connects).toHaveLength(12);
    hosts.query = '';
  });

  it('search the hosts from another page and focus the field once it exists', async () => {
    await start();
    route.go('sessions');
    flushSync();
    chord('F', { shiftKey: true });
    expect(route.route).toBe('hosts');
    await vi.waitFor(() =>
      expect(document.activeElement).toBe(target.querySelector('input[type="search"]')),
    );
  });

  it('toggle the theme to the opposite of the one shown, also from "system"', async () => {
    // The system is dark (see matchMedia in beforeEach): "system" shows dark.
    await start('system');
    chord('L', { shiftKey: true });
    await vi.waitFor(() => expect(callsOf('prefs_set')).toHaveLength(1));
    expect((callsOf('prefs_set')[0].args?.prefs as Preferences).theme).toBe('light');

    chord('L', { shiftKey: true });
    await vi.waitFor(() => expect(callsOf('prefs_set')).toHaveLength(2));
    expect((callsOf('prefs_set')[1].args?.prefs as Preferences).theme).toBe('dark');
  });

  it('change the font size with Ctrl+Plus and Ctrl+Minus, as typed on a keyboard', async () => {
    await start();
    // `+` is Shift+= on a US keyboard.
    chord('+', { shiftKey: true });
    await vi.waitFor(() => expect(callsOf('prefs_set')).toHaveLength(1));
    expect((callsOf('prefs_set')[0].args?.prefs as Preferences).terminal.font_size).toBe(15);
    chord('-');
    await vi.waitFor(() => expect(callsOf('prefs_set')).toHaveLength(2));
    expect((callsOf('prefs_set')[1].args?.prefs as Preferences).terminal.font_size).toBe(14);
  });

  it('report a preferences store that refuses the change', async () => {
    await start();
    failures.set('prefs_set', 'disk full');
    chord('L', { shiftKey: true });
    await vi.waitFor(() => expect(notices.visible.length).toBe(1));
    expect(notices.visible[0].kind).toBe('error');
    expect(noticeText(notices.visible[0])).toContain('disk full');
  });

  it('still honour a stored override', async () => {
    await start();
    prefs.prefs = { ...prefs.prefs, keybindings: { 'host.new': 'Ctrl+Alt+H' } };
    chord('H', { altKey: true });
    expect(route.route).toBe('host-edit');
  });
});

describe('connecting', () => {
  it('skips the dialog when the password is in the credential store', async () => {
    answers.set('secret_has', true);
    await start();
    click(findButton(target, 'Connect'));
    await vi.waitFor(() => expect(callsOf('session_open')).toHaveLength(1));
    expect(connectFlow.pending).toBeNull();
    expect(target.querySelector('[aria-modal="true"]')).toBeNull();
    const options = callsOf('session_open')[0].args?.options as { secrets: SessionSecretsLike };
    expect(options.secrets).toEqual({});
    expect(route.route).toBe('sessions');
  });

  it('asks for the password otherwise, and an empty field sends none', async () => {
    answers.set('secret_has', false);
    await start();
    click(findButton(target, 'Connect'));
    await vi.waitFor(() => expect(connectFlow.pending).not.toBeNull());
    target
      .querySelector('form')
      ?.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }));
    await vi.waitFor(() => expect(callsOf('session_open')).toHaveLength(1));
    const options = callsOf('session_open')[0].args?.options as { secrets: SessionSecretsLike };
    expect('password' in options.secrets && options.secrets.password !== undefined).toBe(false);
  });

  it('opens the dialog again for a restored pane that needs a password', async () => {
    answers.set('secret_has', false);
    answers.set('layout_load', {
      version: 1,
      windows: [
        {
          id: 'window-main',
          tabs: [{ id: 't1', title: 'web-1', root: { leaf: { pane: 'p1', host_id: WEB.id } } }],
          active_tab: 't1',
        },
      ],
      last_route: 'sessions',
    });
    window.location.hash = '';
    await start();
    await vi.waitFor(() => expect(route.route).toBe('sessions'));
    await vi.waitFor(() => expect(target.textContent).toContain('Not connected'));
    // Restored: nothing connected by itself.
    expect(callsOf('session_open')).toHaveLength(0);
    expect(callsOf('session_attach')).toHaveLength(0);

    click(findButton(target, 'Reconnect'));
    await vi.waitFor(() => expect(connectFlow.pending?.reconnecting).toBe(true));
    expect(callsOf('session_open')).toHaveLength(0);

    type(target.querySelector('input[type="password"]') as HTMLInputElement, 'pw');
    target
      .querySelector('form')
      ?.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }));
    await vi.waitFor(() => expect(callsOf('session_open')).toHaveLength(1));
    const options = callsOf('session_open')[0].args?.options as { secrets: SessionSecretsLike };
    expect(options.secrets).toEqual({ password: 'pw' });
  });
});

interface SessionSecretsLike {
  password?: string;
  passphrase?: string;
}

describe('pages in flow', () => {
  const pages: [string, () => void][] = [
    ['folders', () => route.go('folders')],
    ['identities', () => route.go('identities')],
    ['sync', () => route.go('sync')],
    ['preferences', () => route.go('preferences')],
  ];

  it.each(pages)('%s is a page: no backdrop covers the navigation', async (_name, go) => {
    await start();
    go();
    flushSync();
    await tick();
    expect(target.querySelector('.backdrop')).toBeNull();
    expect(target.querySelector('[aria-modal="true"]')).toBeNull();
    // The navigation is there and clickable.
    const nav = target.querySelector('nav[aria-label="Primary"]');
    expect(nav).not.toBeNull();
    click(findButton(nav as HTMLElement, 'All hosts'));
    expect(route.route).toBe('hosts');
  });

  it('the host editor goes back to the page the user came from, not always to hosts', async () => {
    await start();
    route.go('sessions');
    route.edit(null);
    flushSync();
    click(findButton(target, 'Cancel'));
    expect(route.route).toBe('sessions');
  });

  it('the preferences have no Save or Cancel: a change applies and saves at once', async () => {
    await start();
    route.go('sync');
    route.go('preferences');
    flushSync();
    const labels = Array.from(target.querySelectorAll('button')).map((button) =>
      button.textContent?.trim(),
    );
    expect(labels).not.toContain('Save');
    expect(labels).not.toContain('Cancel');
    expect(labels).toContain('Restore defaults');
    // Nothing is pending, so leaving the page asks nothing.
    route.go('sync');
    expect(route.route).toBe('sync');
  });

  it('the editor can visit the identities and come back with its draft', async () => {
    await start();
    route.edit(null);
    flushSync();
    // A private-key host offers the link to the identities page.
    const key = target.querySelector('input[name="auth"][value="private_key"]') as HTMLInputElement;
    key.checked = true;
    key.dispatchEvent(new Event('change', { bubbles: true }));
    flushSync();
    type(document.getElementById('host-name') as HTMLInputElement, 'draft host');
    click(findButton(target, 'Manage identities…'));
    expect(route.route).toBe('identities');
    // No question: the form is parked, not lost.
    expect(unsaved.question).toBeNull();

    // The identities page offers the way back to the host being edited.
    click(findButton(target, 'Back to host'));
    expect(route.route).toBe('host-edit');
    expect((document.getElementById('host-name') as HTMLInputElement).value).toBe('draft host');
  });
});

describe('the saved layout', () => {
  it('records the page on navigation, not only when tabs change', async () => {
    answers.set('layout_load', null);
    await start();
    await vi.waitFor(() => expect(sessions.layoutLoaded).toBe(true));
    route.go('folders');
    await vi.waitFor(
      () => {
        const doc = callsOf('layout_save').pop()?.args?.doc as { last_route?: string } | undefined;
        expect(doc?.last_route).toBe('folders');
      },
      { timeout: 4000 },
    );
  });

  it('is not replaced by an empty layout before the saved one was read', async () => {
    let release: (value: unknown) => void = () => {};
    answers.set('layout_load', () => new Promise((resolve) => (release = resolve)));
    await start();
    route.go('folders');
    await new Promise((resolve) => setTimeout(resolve, 1000));
    expect(callsOf('layout_save')).toHaveLength(0);
    release(null);
  });
});

describe('a session window', () => {
  it('attaches to its session and never owns the saved layout', async () => {
    history.pushState({}, '', '/?attach=s9');
    answers.set('session_list', [{ id: 's9', host_id: WEB.id, status: { state: 'ready' } }]);
    await start();
    await vi.waitFor(() => expect(callsOf('session_attach')).toHaveLength(1));
    expect(sessions.windowLabel).toBe('session-s9');
    expect(callsOf('session_attach')[0].args?.id).toBe('s9');
    expect(callsOf('layout_load')).toHaveLength(0);
    expect(callsOf('session_close')).toHaveLength(0);
    expect(callsOf('session_open')).toHaveLength(0);

    route.go('folders');
    await new Promise((resolve) => setTimeout(resolve, 1000));
    expect(callsOf('layout_save')).toHaveLength(0);
  });
});

describe('after a sync', () => {
  it('reloads the hosts and the preferences when the backend says records were applied', async () => {
    await start();
    await vi.waitFor(() => expect(bus.handlers.has('sync-applied')).toBe(true));
    const before = {
      hosts: callsOf('host_list').length,
      prefs: callsOf('prefs_get').length,
    };
    bus.handlers.get('sync-applied')?.({ payload: null });
    await vi.waitFor(() => expect(callsOf('host_list').length).toBeGreaterThan(before.hosts));
    expect(callsOf('prefs_get').length).toBeGreaterThan(before.prefs);
  });
});

describe('the shell: sidebar and palette', () => {
  /** Opens the palette with its chord and runs one entry, as Enter on it would. */
  function runFromPalette(id: string): void {
    chord('P', { shiftKey: true });
    expect(palette.open).toBe(true);
    const entry = palette.entries.find((candidate) => candidate.id === id);
    if (!entry) throw new Error(`the palette has no entry ${id}`);
    entry.run();
    palette.hide();
    flushSync();
  }

  it('collapses the sidebar to an icon rail with its chord and button, and brings it back', async () => {
    await start();
    expect(target.querySelector('aside.side.rail')).toBeNull();
    chord('B', { shiftKey: true });
    expect(chrome.sidebarCollapsed).toBe(true);
    expect(target.querySelector('aside.side.rail')).not.toBeNull();
    // The items keep their names as icons.
    expect(target.querySelector('[aria-label="Preferences"]')).not.toBeNull();

    click(target.querySelector('[aria-label="Expand the sidebar"]') as Element);
    flushSync();
    expect(chrome.sidebarCollapsed).toBe(false);
    expect(target.querySelector('aside.side.rail')).toBeNull();
  });

  it('opens the palette from the pill in the sidebar', async () => {
    await start();
    click(target.querySelector('[aria-label="Search or run a command"]') as Element);
    expect(palette.open).toBe(true);
    // Running sessions, hosts, pages and actions are all in it.
    const groups = new Set(palette.entries.map((entry) => entry.group));
    for (const group of ['Hosts', 'Go to', 'Create', 'View', 'Preferences', 'Sync']) {
      expect(groups, group).toContain(group);
    }
  });

  it('starts a new folder from the palette on a page that does not exist yet', async () => {
    await start();
    runFromPalette('folder.new');
    expect(route.route).toBe('folders');
    await vi.waitFor(() => expect(document.activeElement?.id).toBe('folder-name'));
    expect(intents.pending).toBeNull();
  });

  it('starts a new identity from the palette', async () => {
    await start();
    runFromPalette('identity.new');
    expect(route.route).toBe('identities');
    await vi.waitFor(() => expect(document.activeElement?.id).toBe('identity-name'));
  });

  it('imports ~/.ssh/config from the palette', async () => {
    await start();
    runFromPalette('host.import');
    await vi.waitFor(() => expect(callsOf('ssh_config_preview')).toHaveLength(1));
  });

  it('brings a section of Preferences into view from the palette', async () => {
    await start();
    runFromPalette('preferences:shortcuts-label');
    expect(route.route).toBe('preferences');
    await vi.waitFor(() => expect(document.activeElement?.id).toBe('shortcuts-label'));
  });

  it('shows the library views of the sidebar on the hosts page', async () => {
    await start();
    route.go('preferences');
    flushSync();
    click(findButton(target, 'Favorites'));
    flushSync();
    expect(route.route).toBe('hosts');
    expect(target.querySelector('header.head h1')?.textContent).toBe('Favorites');
  });

  it('hides and shows the inspector from the palette', async () => {
    await start();
    expect(target.querySelector('aside[aria-label="Host details"]')).not.toBeNull();
    runFromPalette('view.toggleInspector');
    expect(chrome.inspectorOpen).toBe(false);
    expect(target.querySelector('aside[aria-label="Host details"]')).toBeNull();
  });

  it('sets the theme from the palette', async () => {
    await start('system');
    runFromPalette('theme:light');
    await vi.waitFor(() => expect(callsOf('prefs_set')).toHaveLength(1));
    expect((callsOf('prefs_set')[0].args?.prefs as Preferences).theme).toBe('light');
  });
});
