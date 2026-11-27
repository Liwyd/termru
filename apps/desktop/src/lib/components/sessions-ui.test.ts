// @vitest-environment jsdom
/**
 * The tab bar, the pane picker, the status bar and the hosts page around
 * sessions: what is enabled, what can be renamed, what the picker offers.
 */

import { flushSync, mount, tick, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import PaneView from './PaneView.svelte';
import SessionToolbar from './SessionToolbar.svelte';
import StatusBar from './StatusBar.svelte';
import TabBar from './TabBar.svelte';
import HostsPage from '../pages/HostsPage.svelte';
import { connectFlow } from '../stores/connect.svelte';
import { hosts } from '../stores/hosts.svelte';
import { prefs } from '../stores/prefs.svelte';
import { route } from '../stores/route.svelte';
import { sessions, type Pane, type Tab } from '../stores/sessions.svelte';
import { sync } from '../stores/sync.svelte';
import { toasts } from '../stores/toasts.svelte';
import { click, findButton, newTarget, press, stubBrowserApis, type } from '../test-support/dom';
import { answers, callsOf, failures, resetTauriMock } from '../test-support/tauri-core-mock';
import { resetXtermMock } from '../test-support/xterm-mock';
import { pruneTerminals } from '../terminal/registry';
import type { Host, SessionStatus } from '../types';

vi.mock('@tauri-apps/api/core', () => import('../test-support/tauri-core-mock'));
vi.mock('@tauri-apps/api/event', () => ({ listen: async () => () => {} }));
vi.mock('@xterm/xterm', () => import('../test-support/xterm-mock'));
vi.mock('@xterm/addon-fit', () => import('../test-support/xterm-mock'));
vi.mock('@xterm/addon-webgl', () => import('../test-support/xterm-mock'));
vi.mock('@xterm/addon-search', () => import('../test-support/xterm-mock'));
vi.mock('@xterm/addon-web-links', () => import('../test-support/xterm-mock'));

function host(name: string, auth: Host['auth'] = { type: 'agent' }): Host {
  const now = new Date().toISOString();
  return {
    id: `host-${name}`,
    name,
    address: `${name}.test`,
    port: 22,
    username: 'deploy',
    auth,
    group_id: null,
    tags: [],
    favorite: false,
    description: null,
    term: 'xterm-256color',
    keepalive_seconds: 0,
    sort_order: 0,
    created_at: now,
    updated_at: now,
    deleted: false,
  };
}

let target: HTMLElement;
let component: ReturnType<typeof mount> | null = null;

beforeEach(() => {
  resetTauriMock();
  resetXtermMock();
  pruneTerminals(new Set());
  stubBrowserApis();
  connectFlow.cancel();
  sessions.tabs = [];
  sessions.activeId = null;
  sessions.layoutLoaded = true;
  hosts.hosts = [];
  hosts.error = null;
  hosts.loading = false;
  hosts.loaded = true;
  hosts.query = '';
  hosts.searchFocus = false;
  hosts.passwordStored = {};
  toasts.toasts = [];
  route.route = 'sessions';
  route.editHost = null;
  window.location.hash = '#sessions';
  target = newTarget();
});

afterEach(() => {
  if (component) unmount(component);
  component = null;
  target.remove();
  document.body.replaceChildren();
});

function openTab(name = 'web-1'): { tab: Tab; pane: Pane } {
  const tab = sessions.open(host(name), {});
  const pane = sessions.activePaneOf(tab);
  if (!pane) throw new Error('no pane');
  return { tab: sessions.tabs.find((entry) => entry.id === tab.id) as Tab, pane };
}

function setStatus(pane: Pane, status: SessionStatus, sessionId: string | null = null): void {
  pane.sessionId = sessionId;
  sessions.applyStatus(pane, status);
  flushSync();
}

describe('toolbar duplicate and detach', () => {
  function mountBar(tab: Tab) {
    component = mount(SessionToolbar, { target, props: { tab } });
    flushSync();
  }
  const duplicate = () => target.querySelector<HTMLButtonElement>('[aria-label="Duplicate"]');
  const detach = () => target.querySelector<HTMLButtonElement>('[aria-label="Detach"]');
  const disabled = (button: HTMLButtonElement | null) =>
    button?.getAttribute('aria-disabled') === 'true';

  it('are disabled until the active pane is connected, and say why', () => {
    const { tab, pane } = openTab();
    mountBar(tab);
    expect(disabled(duplicate())).toBe(true);
    expect(disabled(detach())).toBe(true);
    // A disabled control carries its reason, and stays focusable to read it.
    expect(duplicate()?.title).toBe('Duplicate needs a connected session');
    expect(detach()?.title).toBe('Detach needs a connected session');
    expect(duplicate()?.disabled).toBe(false);

    // A session id alone is not enough: it may be connecting.
    pane.sessionId = 's1';
    flushSync();
    expect(disabled(duplicate())).toBe(true);

    setStatus(pane, { state: 'ready' }, 's1');
    expect(disabled(duplicate())).toBe(false);
    expect(disabled(detach())).toBe(false);
    expect(duplicate()?.title).not.toContain('needs');
  });

  it('are disabled again while the session reconnects or after it failed', () => {
    const { tab, pane } = openTab();
    mountBar(tab);
    setStatus(pane, { state: 'ready' }, 's1');
    setStatus(
      pane,
      { state: 'reconnecting', attempt: 1, delay_ms: 500, last_error: 'reset' },
      's1',
    );
    expect(disabled(duplicate())).toBe(true);
    setStatus(pane, { state: 'failed', category: 'network', message: 'down' }, 's1');
    expect(disabled(detach())).toBe(true);
  });

  it('does nothing when a disabled button is clicked', () => {
    const { tab } = openTab();
    mountBar(tab);
    click(duplicate() as HTMLButtonElement);
    click(detach() as HTMLButtonElement);
    expect(sessions.tabs).toHaveLength(1);
    expect(callsOf('window_open_session')).toHaveLength(0);
  });

  it('duplicates into a new tab when the session is connected', () => {
    const { tab, pane } = openTab();
    mountBar(tab);
    setStatus(pane, { state: 'ready' }, 's1');
    click(duplicate() as HTMLButtonElement);
    expect(sessions.tabs).toHaveLength(2);
  });
});

describe('TabBar rename', () => {
  function mountBar(tab: Tab) {
    component = mount(TabBar, { target, props: { tab, active: true } });
    flushSync();
  }
  const label = () => target.querySelector<HTMLButtonElement>('button.label') as HTMLButtonElement;
  const input = () => target.querySelector<HTMLInputElement>('input.rename');

  function startByDoubleClick(): void {
    label().dispatchEvent(new MouseEvent('dblclick', { bubbles: true }));
    flushSync();
  }

  it('focuses and selects the field when the rename starts', () => {
    const { tab } = openTab('web-1');
    mountBar(tab);
    startByDoubleClick();
    expect(input()).not.toBeNull();
    expect(document.activeElement).toBe(input());
    expect(input()?.value).toBe('web-1');
  });

  it('commits on Enter and saves the layout', async () => {
    vi.useFakeTimers();
    try {
      const { tab } = openTab('web-1');
      mountBar(tab);
      startByDoubleClick();
      type(input() as HTMLInputElement, 'production web');
      press(input() as HTMLInputElement, 'Enter');
      expect(tab.title).toBe('production web');
      expect(input()).toBeNull();

      await vi.advanceTimersByTimeAsync(1000);
      const doc = callsOf('layout_save').pop()?.args?.doc as {
        windows: { tabs: { title: string }[] }[];
      };
      expect(doc.windows[0].tabs[0].title).toBe('production web');
    } finally {
      vi.useRealTimers();
    }
  });

  it('cancels on Escape: the old name stays, and the blur that follows does not commit', () => {
    const { tab } = openTab('web-1');
    mountBar(tab);
    startByDoubleClick();
    const field = input() as HTMLInputElement;
    type(field, 'discarded');
    press(field, 'Escape');
    field.dispatchEvent(new FocusEvent('blur'));
    flushSync();
    expect(tab.title).toBe('web-1');
    expect(input()).toBeNull();
  });

  it('commits on blur', () => {
    const { tab } = openTab('web-1');
    mountBar(tab);
    startByDoubleClick();
    const field = input() as HTMLInputElement;
    type(field, 'renamed by blur');
    field.dispatchEvent(new FocusEvent('blur'));
    flushSync();
    expect(tab.title).toBe('renamed by blur');
  });

  it('keeps the old name for an empty one', () => {
    const { tab } = openTab('web-1');
    mountBar(tab);
    startByDoubleClick();
    type(input() as HTMLInputElement, '   ');
    press(input() as HTMLInputElement, 'Enter');
    expect(tab.title).toBe('web-1');
  });

  it('is reachable from the keyboard: F2 renames, Enter and Space open the tab', () => {
    const first = openTab('web-1');
    const second = openTab('db-1');
    expect(sessions.activeId).toBe(second.tab.id);
    mountBar(first.tab);
    const tabElement = target.querySelector('[role="tab"]') as HTMLElement;

    press(tabElement, 'Enter');
    expect(sessions.activeId).toBe(first.tab.id);
    sessions.setActive(second.tab.id);
    press(tabElement, ' ');
    expect(sessions.activeId).toBe(first.tab.id);

    press(tabElement, 'F2');
    expect(input()).not.toBeNull();
    expect(document.activeElement).toBe(input());
  });

  it('does not take the keys typed in the rename field for tab keys', () => {
    const { tab } = openTab('web-1');
    mountBar(tab);
    startByDoubleClick();
    const field = input() as HTMLInputElement;
    // F2 or Enter typed in the field must not start a second rename or
    // re-activate the tab: the field handles its own keys.
    press(field, 'F2');
    expect(target.querySelectorAll('input.rename')).toHaveLength(1);
  });
});

describe('the pane picker', () => {
  function emptyPane(): Pane {
    openTab('main');
    const created = sessions.splitActive('columns');
    const pane = created ? sessions.paneById(created.id) : null;
    if (!pane) throw new Error('no empty pane');
    return pane;
  }

  function mountPicker(pane: Pane) {
    component = mount(PaneView, { target, props: { pane, focused: true } });
    flushSync();
  }

  const names = () =>
    Array.from(target.querySelectorAll('.empty li button')).map((button) =>
      button.firstChild?.textContent?.trim(),
    );

  it('offers every host, not only the first eight', () => {
    hosts.hosts = Array.from({ length: 14 }, (_, i) => host(`host-${String(i).padStart(2, '0')}`));
    mountPicker(emptyPane());
    expect(names()).toHaveLength(14);
  });

  it('searches by name, address and user', () => {
    hosts.hosts = [host('web-1'), host('web-2'), host('db-1')];
    mountPicker(emptyPane());
    const search = target.querySelector('input[type="search"]') as HTMLInputElement;
    type(search, 'web');
    expect(names()).toEqual(['web-1', 'web-2']);
    type(search, 'db-1.test');
    expect(names()).toEqual(['db-1']);
    type(search, 'deploy');
    expect(names()).toHaveLength(3);
    type(search, 'nothing matches');
    expect(names()).toHaveLength(0);
    expect(target.textContent).toContain('No host matches');
  });

  it('connects the first match on Enter', async () => {
    hosts.hosts = [host('web-1'), host('db-1')];
    const pane = emptyPane();
    mountPicker(pane);
    const search = target.querySelector('input[type="search"]') as HTMLInputElement;
    type(search, 'db');
    press(search, 'Enter');
    await vi.waitFor(() => expect(pane.host?.id).toBe('host-db-1'));
  });

  it('says there are no saved hosts, and offers a New host button', () => {
    hosts.hosts = [];
    mountPicker(emptyPane());
    expect(target.textContent).toContain('No saved hosts yet.');
    expect(target.textContent).not.toContain('Add a host in the sidebar');
    click(findButton(target, 'New host'));
    expect(route.route).toBe('host-edit');
    expect(route.editHost).toBeNull();
  });

  it('goes through the connect flow: a password host asks for its secret', async () => {
    hosts.hosts = [host('pw-host', { type: 'password' })];
    const pane = emptyPane();
    mountPicker(pane);
    click(target.querySelector('.empty li button') as HTMLElement);
    await vi.waitFor(() => expect(connectFlow.pending?.host.name).toBe('pw-host'));
    expect(connectFlow.pending?.pane?.id).toBe(pane.id);
    // Nothing was connected without the secret.
    expect(pane.host).toBeNull();
  });

  it('connects at once when the secret is stored', async () => {
    answers.set('secret_has', true);
    hosts.hosts = [host('pw-host', { type: 'password' })];
    const pane = emptyPane();
    mountPicker(pane);
    click(target.querySelector('.empty li button') as HTMLElement);
    await vi.waitFor(() => expect(pane.host?.name).toBe('pw-host'));
    expect(connectFlow.pending).toBeNull();
  });
});

describe('StatusBar', () => {
  function mountBar() {
    component = mount(StatusBar, { target });
    flushSync();
  }

  it('shows the local workspace when signed out', () => {
    sync.status = null;
    mountBar();
    expect(target.textContent).toContain('Local workspace');
  });

  it('shows the real sync state when signed in, and offline', async () => {
    sync.status = {
      settings: { server_url: 'https://s', email: 'a@b.c', device_name: 'd', device_id: 'i' },
      signed_in: true,
      pending: 0,
      conflicts: 0,
      cursor: 1,
      last_sync_at: null,
      last_error: null,
    };
    mountBar();
    expect(target.textContent).toContain('Sync · signed in');
    expect(target.textContent).not.toContain('Local workspace');

    window.dispatchEvent(new Event('offline'));
    flushSync();
    expect(target.textContent).toContain('Sync · offline');
    window.dispatchEvent(new Event('online'));
    flushSync();
    expect(target.textContent).toContain('Sync · signed in');
    sync.status = null;
  });

  it('advertises the palette chord the platform uses, and follows a rebinding', () => {
    mountBar();
    // Linux/Windows: Ctrl+K belongs to the shell.
    expect(target.textContent).toContain('Ctrl+Shift+P');
    expect(target.textContent).not.toContain('Ctrl K');

    prefs.prefs = { ...prefs.prefs, keybindings: { 'palette.open': 'Ctrl+Alt+Space' } };
    flushSync();
    expect(target.textContent).toContain('Ctrl+Alt+Space');
    prefs.prefs = { ...prefs.prefs, keybindings: {} };
  });
});

describe('HostsPage', () => {
  function mountPage() {
    component = mount(HostsPage, {
      target,
      props: { onconnect: vi.fn(), onopen: vi.fn() },
    });
    flushSync();
  }

  it('hides the count line, the filters and the tabs in the error state', () => {
    hosts.hosts = [];
    hosts.error = 'database is locked';
    mountPage();
    expect(target.textContent).toContain('database is locked');
    expect(target.textContent).not.toContain('No saved connections');
    expect(target.textContent).not.toContain('No sessions running');
    expect(target.querySelector('[aria-label="Filter hosts"]')).toBeNull();
    expect(target.querySelector('input[type="search"]')).toBeNull();
    expect(target.querySelector('[role="table"]')).toBeNull();
    expect(findButton(target, 'Retry')).toBeTruthy();
  });

  it('shows the count line and the tabs when there is no error', () => {
    hosts.hosts = [host('web-1')];
    mountPage();
    expect(target.textContent).toContain('One saved connection.');
    expect(target.querySelector('[aria-label="Filter hosts"]')).not.toBeNull();
  });

  it('does not announce a deletion that failed', async () => {
    hosts.hosts = [host('web-1')];
    failures.set('host_delete', 'the store is locked');
    mountPage();
    click(target.querySelector('[aria-label="More actions for web-1"]') as Element);
    click(
      Array.from(document.querySelectorAll('[role="menuitem"]')).find(
        (item) => item.textContent === 'Delete',
      ) as Element,
    );
    click(findButton(document.querySelector('[role="alertdialog"]') as HTMLElement, 'Delete'));
    await vi.waitFor(() => expect(toasts.toasts.length).toBeGreaterThan(0));
    const kinds = toasts.toasts.map((toast) => toast.kind);
    expect(kinds).toEqual(['error']);
    expect(hosts.hosts).toHaveLength(1);
    expect(hosts.error).toBeNull();
  });

  it('announces a deletion that worked', async () => {
    hosts.hosts = [host('web-1')];
    mountPage();
    click(target.querySelector('[aria-label="More actions for web-1"]') as Element);
    click(
      Array.from(document.querySelectorAll('[role="menuitem"]')).find(
        (item) => item.textContent === 'Delete',
      ) as Element,
    );
    click(findButton(document.querySelector('[role="alertdialog"]') as HTMLElement, 'Delete'));
    await vi.waitFor(() => expect(toasts.toasts.map((toast) => toast.kind)).toEqual(['success']));
    expect(hosts.hosts).toHaveLength(0);
  });

  it('focuses the search field when another page asked for it before this one existed', async () => {
    hosts.hosts = [host('web-1')];
    hosts.searchFocus = true;
    mountPage();
    await tick();
    const search = target.querySelector('input[type="search"]');
    expect(document.activeElement).toBe(search);
    // The request is consumed.
    expect(hosts.searchFocus).toBe(false);
  });

  it('focuses the search field when the page is already open', async () => {
    hosts.hosts = [host('web-1')];
    mountPage();
    hosts.searchFocus = true;
    await tick();
    expect(document.activeElement).toBe(target.querySelector('input[type="search"]'));
  });

  it('refreshes the recents when a session becomes ready, not only when tabs come and go', async () => {
    hosts.hosts = [host('web-1')];
    const { pane } = openTab('web-1');
    mountPage();
    await vi.waitFor(() => expect(callsOf('recent_list').length).toBeGreaterThan(0));
    const before = callsOf('recent_list').length;

    setStatus(pane, { state: 'ready' }, 's1');
    await vi.waitFor(() => expect(callsOf('recent_list').length).toBeGreaterThan(before));
  });

  it('does not show the loading skeleton again on a refresh', async () => {
    hosts.hosts = [host('web-1')];
    hosts.loaded = true;
    mountPage();
    const refresh = hosts.load();
    flushSync();
    expect(target.querySelector('[aria-busy="true"]')).toBeNull();
    expect(target.textContent).toContain('web-1');
    await refresh;
  });
});
