// @vitest-environment jsdom
/**
 * Shell contract: the sidebar (live sessions, library, folders, workspace,
 * the icon rail), the status bar and the ContextConfirm dialog.
 */

import { flushSync, mount, tick, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import Sidebar from './Sidebar.svelte';
import StatusBar from './StatusBar.svelte';
import ContextConfirm from './ContextConfirm.svelte';
import { chrome } from '../stores/chrome.svelte';
import { hosts } from '../stores/hosts.svelte';
import { intents } from '../stores/intents.svelte';
import { route, ROUTE_IDS } from '../stores/route.svelte';
import { sessions, type Pane } from '../stores/sessions.svelte';
import { sync } from '../stores/sync.svelte';
import type { Group, Host, SessionStatus } from '../types';

function click(element: Element): void {
  element.dispatchEvent(new MouseEvent('click', { bubbles: true }));
}

function host(name: string, over: Partial<Host> = {}): Host {
  const stamp = '2026-01-01T00:00:00Z';
  return {
    id: `host-${name}`,
    name,
    address: `${name}.test`,
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
    created_at: stamp,
    updated_at: stamp,
    deleted: false,
    ...over,
  };
}

function group(id: string, name: string, parent: string | null = null): Group {
  const stamp = '2026-01-01T00:00:00Z';
  return {
    id,
    name,
    parent_id: parent,
    sort_order: 0,
    created_at: stamp,
    updated_at: stamp,
    deleted: false,
  };
}

/** A pane in a given state, for the counters. */
function pane(id: string, status: SessionStatus): Pane {
  return {
    id,
    sessionId: status.state === 'ready' ? `s-${id}` : null,
    host: null,
    title: id,
    status,
    secrets: {},
    generation: 0,
    exitCode: null,
    dead: false,
    errorMessage: null,
    awaitingReconnect: false,
    hadReady: status.state === 'ready',
    lastFailure: null,
    duplicateFrom: null,
    restored: false,
    attachTo: null,
    rememberSecret: false,
  };
}

describe('Sidebar', () => {
  let target: HTMLElement;
  let component: ReturnType<typeof mount>;
  let opened: number;

  const FOLDERS: Group[] = [
    group('g-prod', 'Production'),
    group('g-eu', 'Europe', 'g-prod'),
    group('g-sand', 'Sandbox'),
  ];

  function item(label: string): HTMLButtonElement {
    const found = Array.from(target.querySelectorAll<HTMLButtonElement>('button')).find(
      (button) =>
        button.getAttribute('aria-label') === label || button.textContent?.trim().startsWith(label),
    );
    if (!found) throw new Error(`no sidebar item "${label}"`);
    return found;
  }

  beforeEach(() => {
    route.route = 'hosts';
    route.editHost = null;
    sessions.tabs = [];
    sessions.activeId = null;
    hosts.hosts = [
      host('a', { favorite: true, group_id: 'g-eu' }),
      host('b', { favorite: true, group_id: 'g-sand' }),
      host('c'),
    ];
    hosts.groups = FOLDERS;
    hosts.identities = [];
    hosts.recentAt = { 'host-a': 10 };
    hosts.filterTab = 'all';
    hosts.folderFilter = '';
    chrome.sidebarCollapsed = false;
    chrome.narrow = false;
    opened = 0;
    target = document.createElement('div');
    document.body.appendChild(target);
    component = mount(Sidebar, { target, props: { onpalette: () => (opened += 1) } });
  });

  afterEach(() => {
    unmount(component);
    target.remove();
    hosts.hosts = [];
    hosts.groups = [];
    hosts.recentAt = {};
  });

  it('is the primary navigation, with the page you are on marked', () => {
    expect(target.querySelector('nav[aria-label="Primary"]')).not.toBeNull();
    const current = Array.from(target.querySelectorAll('[aria-current="page"]'));
    expect(current).toHaveLength(1);
    expect(current[0].textContent).toContain('All hosts');

    route.go('identities');
    flushSync();
    const moved = Array.from(target.querySelectorAll('[aria-current="page"]'));
    expect(moved).toHaveLength(1);
    expect(moved[0].textContent).toContain('Identities');
    // The page is told apart by more than colour: the item is "on".
    expect(moved[0].className).toContain('on');
  });

  it('counts the library and the folders', () => {
    expect(item('All hosts').textContent).toContain('3');
    expect(item('Favorites').textContent).toContain('2');
    expect(item('Recent').textContent).toContain('1');
    // A folder counts the hosts under it too.
    expect(item('Production, 1').getAttribute('aria-label')).toBe('Production, 1');
    expect(item('Production / Europe, 1')).toBeTruthy();
    expect(item('Sandbox, 1')).toBeTruthy();
    expect(item('Unfiled, 1')).toBeTruthy();
  });

  it('opens the command palette from the search pill', () => {
    click(item('Search or run a command'));
    expect(opened).toBe(1);
    expect(item('Search or run a command').textContent).toContain('Search or run');
  });

  it('shows the running sessions and focuses one on click', () => {
    expect(target.textContent).toContain('No live sessions');
    sessions.tabs = [
      {
        id: 't1',
        title: 'atlas-prod',
        panes: [pane('p1', { state: 'ready' }), pane('p2', { state: 'ready' })],
        activePaneId: 'p1',
        root: {
          kind: 'split',
          id: 's',
          direction: 'columns',
          ratio: 0.5,
          first: { kind: 'leaf', pane: 'p1' },
          second: { kind: 'leaf', pane: 'p2' },
        },
      },
      {
        id: 't2',
        title: 'relay-edge',
        panes: [pane('p3', { state: 'connecting' })],
        activePaneId: 'p3',
        root: { kind: 'leaf', pane: 'p3' },
      },
    ];
    flushSync();

    const first = item('atlas-prod, 2 panes, connected');
    expect(first.textContent).toContain('2');
    expect(item('relay-edge, 1 pane, connecting')).toBeTruthy();

    click(item('relay-edge, 1 pane, connecting'));
    flushSync();
    expect(sessions.activeId).toBe('t2');
    expect(route.route).toBe('sessions');
    expect(item('relay-edge, 1 pane, connecting').getAttribute('aria-current')).toBe('page');
  });

  it('drives the hosts filter: Favorites, Recent, a folder, Unfiled', () => {
    route.go('sessions');
    flushSync();

    click(item('Favorites'));
    flushSync();
    expect(route.route).toBe('hosts');
    expect(hosts.filterTab).toBe('favorites');
    expect(hosts.rows.map((entry) => entry.name)).toEqual(['a', 'b']);
    expect(item('Favorites').getAttribute('aria-current')).toBe('page');

    click(item('Production / Europe, 1'));
    flushSync();
    expect(hosts.folderFilter).toBe('g-eu');
    expect(hosts.filterTab).toBe('all');
    expect(hosts.rows.map((entry) => entry.name)).toEqual(['a']);

    click(item('Unfiled, 1'));
    flushSync();
    expect(hosts.filterTab).toBe('unfiled');
    expect(hosts.folderFilter).toBe('');

    click(item('All hosts'));
    flushSync();
    expect(hosts.rows).toHaveLength(3);
  });

  it('asks for a new folder, and reaches every page of the workspace', () => {
    click(item('New folder'));
    flushSync();
    expect(route.route).toBe('folders');
    expect(intents.pending).toEqual({ kind: 'new-folder' });
    intents.pending = null;

    for (const [label, id] of [
      ['Identities', 'identities'],
      ['Sync', 'sync'],
      ['Preferences', 'preferences'],
      ['Manage folders', 'folders'],
      ['Live sessions', 'sessions'],
    ] as const) {
      click(item(label));
      flushSync();
      expect(route.route, label).toBe(id);
    }
  });

  it('collapses to an icon rail that keeps every item named', () => {
    expect(target.querySelector('.rail')).toBeNull();
    click(item('Collapse the sidebar'));
    flushSync();
    expect(chrome.sidebarCollapsed).toBe(true);
    expect(target.querySelector('.rail')).not.toBeNull();
    // Icons only: the names are on the buttons.
    expect(item('All hosts, 3')).toBeTruthy();
    expect(item('Identities, 0')).toBeTruthy();
    expect(item('Preferences')).toBeTruthy();
    expect(item('Folders')).toBeTruthy();

    click(item('Expand the sidebar'));
    flushSync();
    expect(chrome.sidebarCollapsed).toBe(false);
    expect(target.querySelector('.rail')).toBeNull();
  });

  it('is a rail on a narrow window whatever was chosen, and cannot be expanded there', () => {
    chrome.narrow = true;
    flushSync();
    expect(target.querySelector('.rail')).not.toBeNull();
    expect(item('Expand the sidebar').disabled).toBe(true);
  });
});

describe('StatusBar', () => {
  let target: HTMLElement;
  let component: ReturnType<typeof mount>;

  beforeEach(() => {
    route.route = 'hosts';
    route.editHost = null;
    sessions.tabs = [];
    sessions.activeId = null;
    sync.status = null;
    target = document.createElement('div');
    document.body.appendChild(target);
    component = mount(StatusBar, { target });
  });

  afterEach(() => {
    unmount(component);
    target.remove();
    hosts.hosts = [];
  });

  it('shows the live count, the sync state and the palette hint', () => {
    expect(target.textContent).toContain('No live sessions');
    expect(target.textContent).toContain('Local workspace');
    expect(target.textContent).toContain('command');

    sessions.tabs = [
      {
        id: 't1',
        title: 'web-1',
        panes: [pane('p1', { state: 'ready' }), pane('p2', { state: 'ready' })],
        activePaneId: 'p1',
        root: { kind: 'leaf', pane: 'p1' },
      },
      {
        id: 't2',
        title: 'db-1',
        panes: [pane('p3', { state: 'failed', category: 'network', message: 'refused' })],
        activePaneId: 'p3',
        root: { kind: 'leaf', pane: 'p3' },
      },
    ];
    flushSync();
    // Tabs with a connected pane, like the live list of the sidebar.
    expect(target.textContent).toContain('1 live');
  });

  it('says when the sync is signed in, and how long ago it ran', () => {
    sync.status = {
      settings: { server_url: 'https://s', email: 'a@b.c', device_name: 'd', device_id: 'x' },
      signed_in: true,
      pending: 0,
      conflicts: 0,
      cursor: 1,
      last_sync_at: new Date(Date.now() - 2 * 60_000).toISOString(),
      last_error: null,
    };
    flushSync();
    expect(target.textContent).toContain('Synced 2 min ago');
  });

  it('changes the hints per page, and keeps a line for the editor and the sessions', () => {
    hosts.hosts = [host('web')];
    flushSync();
    expect(target.textContent).toContain('↑↓ select');

    route.go('folders');
    flushSync();
    expect(target.textContent).not.toContain('↑↓ select');

    route.go('sessions');
    flushSync();
    expect(target.textContent).toContain('No open sessions');
  });
});

describe('ContextConfirm', () => {
  let target: HTMLElement;
  let component: ReturnType<typeof mount>;

  beforeEach(() => {
    target = document.createElement('div');
    document.body.appendChild(target);
  });

  afterEach(() => {
    unmount(component);
    target.remove();
  });

  it('focuses Cancel, cancels on Escape and returns focus to the trigger', async () => {
    let confirmed = 0;
    const trigger = document.createElement('button');
    trigger.textContent = 'Delete';
    document.body.appendChild(trigger);
    trigger.focus();

    component = mount(ContextConfirm, {
      target,
      props: {
        open: true,
        title: 'Delete host “web-1”?',
        body: 'The host profile is removed.',
        onconfirm: () => (confirmed += 1),
      },
    });

    await tick();
    // The focus moves after the dialog element exists (one more flush).
    await tick();
    const cancel = Array.from(target.querySelectorAll('button')).find(
      (button) => button.textContent === 'Cancel',
    ) as HTMLButtonElement;
    expect(document.activeElement).toBe(cancel);

    // Escape cancels without confirming.
    target
      .querySelector('[role="alertdialog"]')
      ?.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    await tick();
    expect(confirmed).toBe(0);
    expect(target.textContent).toBe('');

    // Focus returns to the trigger.
    expect(document.activeElement).toBe(trigger);
    trigger.remove();
  });

  it('runs the confirm handler', async () => {
    let confirmed = 0;
    component = mount(ContextConfirm, {
      target,
      props: {
        open: true,
        title: 'Delete folder “prod”?',
        body: 'The hosts stay.',
        onconfirm: () => (confirmed += 1),
      },
    });
    await tick();
    const confirm = Array.from(target.querySelectorAll('button')).find(
      (button) => button.textContent === 'Delete',
    ) as HTMLButtonElement;
    click(confirm);
    await tick();
    expect(confirmed).toBe(1);
    expect(target.textContent).toBe('');
  });

  it('renders nothing while closed', () => {
    component = mount(ContextConfirm, {
      target,
      props: { open: false, title: 'T', body: 'B', onconfirm: () => {} },
    });
    expect(target.textContent).toBe('');
  });
});

describe('route coverage', () => {
  it('keeps the six pages of the app, and the editor that is reached from Hosts', () => {
    // host-edit is reached from Hosts, not the sidebar; the rest are all there.
    const navRoutes = ROUTE_IDS.filter((id) => id !== 'host-edit');
    expect(navRoutes.length).toBe(6);
  });
});
