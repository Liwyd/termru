// @vitest-environment jsdom
/**
 * The hosts page: the one-line header, the order chips, the search, the
 * inspector, the running-sessions strip, the state matrix and the 10 000
 * host windowing.
 */

import { flushSync, mount, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import HostsPage from './HostsPage.svelte';
import { leafTree } from '../pane-tree';
import { chrome } from '../stores/chrome.svelte';
import { hosts } from '../stores/hosts.svelte';
import { route } from '../stores/route.svelte';
import { sessions, type Pane, type Tab } from '../stores/sessions.svelte';
import { sync } from '../stores/sync.svelte';
import type { Host, SyncStatus } from '../types';

vi.mock('@tauri-apps/api/core', () => ({
  Channel: class {
    onmessage?: unknown;
  },
  invoke: async () => null,
}));

function host(name: string, overrides: Partial<Host> = {}): Host {
  const now = new Date().toISOString();
  return {
    id: `host-${name}`,
    name,
    address: `${name}.test`,
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
    created_at: now,
    updated_at: now,
    deleted: false,
    ...overrides,
  };
}

function liveTab(name: string): Tab {
  const pane: Pane = {
    id: `pane-${name}`,
    sessionId: 's1',
    host: host(name),
    title: name,
    status: { state: 'ready' },
    secrets: {},
    generation: 1,
    exitCode: null,
    dead: false,
    errorMessage: null,
    awaitingReconnect: false,
    hadReady: true,
    lastFailure: null,
    duplicateFrom: null,
    restored: false,
    attachTo: null,
    rememberSecret: false,
  };
  return {
    id: `tab-${name}`,
    title: name,
    panes: [pane],
    activePaneId: pane.id,
    root: leafTree(pane.id),
  };
}

function syncStatus(overrides: Partial<SyncStatus> = {}): SyncStatus {
  return {
    settings: { server_url: '', email: '', device_name: '', device_id: '' },
    signed_in: false,
    pending: 0,
    conflicts: 0,
    cursor: 0,
    last_sync_at: '2026-10-24T09:00:00Z',
    last_error: null,
    ...overrides,
  };
}

let target: HTMLElement | null = null;
let app: ReturnType<typeof mount> | null = null;

function render(onopen = vi.fn<(host: Host) => void>()): {
  onopen: ReturnType<typeof vi.fn<(host: Host) => void>>;
} {
  target = document.createElement('div');
  document.body.appendChild(target);
  app = mount(HostsPage, {
    target,
    props: { onconnect: vi.fn<(host: Host) => void>(), onopen },
  });
  flushSync();
  return { onopen };
}

/** A sort chip: its name is the full label (the short one is for a narrow header). */
function chip(label: string): HTMLButtonElement {
  const button = document.body.querySelector<HTMLButtonElement>(`.chip[aria-label="${label}"]`);
  if (!button) throw new Error(`chip "${label}" not found`);
  return button;
}

function findButton(label: string): HTMLButtonElement {
  const button = Array.from(document.body.querySelectorAll('button')).find(
    (candidate) => candidate.textContent?.trim() === label,
  );
  if (!button) throw new Error(`button "${label}" not found`);
  return button;
}

beforeEach(() => {
  hosts.hosts = [];
  hosts.identities = [];
  hosts.groups = [];
  hosts.tags = [];
  hosts.loading = false;
  hosts.error = null;
  hosts.query = '';
  hosts.filterTab = 'all';
  hosts.folderFilter = '';
  hosts.sort = 'recent';
  hosts.selectedId = null;
  chrome.inspectorOpen = true;
  hosts.recentAt = {};
  hosts.passwordStored = {};
  sessions.tabs = [];
  sessions.activeId = null;
  sync.status = null;
  sync.error = null;
  route.route = 'hosts';
  route.editHost = null;
  window.location.hash = '#hosts';
  (globalThis as { ResizeObserver?: unknown }).ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  };
});

afterEach(() => {
  if (app) unmount(app);
  app = null;
  if (target) target.remove();
  target = null;
  document.body.innerHTML = '';
});

describe('hosts page header', () => {
  const head = () => document.body.querySelector('header.head') as HTMLElement;

  it('is one line: the view and its count, the order, search, import and New host', () => {
    hosts.hosts = [host('a'), host('b')];
    sessions.tabs = [liveTab('a')];
    render();

    expect(head().querySelector('h1')?.textContent).toBe('All hosts');
    expect(head().querySelector('.count')?.textContent).toBe('2');
    expect(chip('Recent activity').getAttribute('aria-pressed')).toBe('true');
    expect(head().querySelector('input[type="search"]')).not.toBeNull();
    expect(head().textContent).toContain('Import ~/.ssh/config');
    // The one primary action is New host.
    expect(
      Array.from(head().querySelectorAll('button.primary')).map((b) => b.textContent?.trim()),
    ).toEqual(['New host']);
    // No breadcrumb, no description sentence.
    expect(document.body.querySelector('nav[aria-label="Breadcrumb"]')).toBeNull();
    expect(head().querySelector('p')).toBeNull();
    expect(document.body.textContent).not.toContain('saved connections');
  });

  it('names the view the sidebar chose', () => {
    hosts.groups = [
      {
        id: 'g1',
        name: 'Production',
        parent_id: null,
        sort_order: 0,
        created_at: '',
        updated_at: '',
        deleted: false,
      },
    ];
    hosts.hosts = [host('a', { favorite: true, group_id: 'g1' }), host('b')];
    render();
    expect(head().querySelector('h1')?.textContent).toBe('All hosts');

    hosts.showLibrary('favorites');
    flushSync();
    expect(head().querySelector('h1')?.textContent).toBe('Favorites');
    expect(head().querySelector('.count')?.textContent).toBe('1');

    hosts.showLibrary('unfiled');
    flushSync();
    expect(head().querySelector('h1')?.textContent).toBe('Unfiled');

    hosts.showFolder('g1');
    flushSync();
    expect(head().querySelector('h1')?.textContent).toBe('Production');
    expect(head().querySelector('.count')?.textContent).toBe('1');
  });

  it('orders the list by the chips: recent activity, name, folder', () => {
    hosts.groups = [
      {
        id: 'g1',
        name: 'Alpha',
        parent_id: null,
        sort_order: 0,
        created_at: '',
        updated_at: '',
        deleted: false,
      },
    ];
    hosts.hosts = [host('zulu', { group_id: 'g1' }), host('mike'), host('alpha')];
    hosts.recentAt = { 'host-mike': 200, 'host-zulu': 100 };
    render();
    const names = () =>
      Array.from(document.body.querySelectorAll('.row .name')).map((entry) => entry.textContent);

    expect(names()).toEqual(['mike', 'zulu', 'alpha']);

    chip('Name').click();
    flushSync();
    expect(hosts.sort).toBe('name');
    expect(names()).toEqual(['alpha', 'mike', 'zulu']);
    expect(chip('Name').getAttribute('aria-pressed')).toBe('true');

    // Hosts in a folder first, by folder; the unfiled ones last.
    chip('Folder').click();
    flushSync();
    expect(names()).toEqual(['zulu', 'alpha', 'mike']);

    chip('Recent activity').click();
    flushSync();
    expect(names()).toEqual(['mike', 'zulu', 'alpha']);
  });

  it('searches from the header and counts what it finds', () => {
    hosts.hosts = [host('web'), host('db')];
    render();
    const search = head().querySelector('input[type="search"]') as HTMLInputElement;
    search.value = 'db';
    search.dispatchEvent(new Event('input', { bubbles: true }));
    flushSync();
    expect(hosts.rows.map((entry) => entry.name)).toEqual(['db']);
    expect(head().querySelector('.count')?.textContent).toBe('1');
  });
});

describe('hosts page inspector', () => {
  const inspector = () => document.body.querySelector('aside[aria-label="Host details"]');

  it('shows the first host until one is chosen, then the chosen one', () => {
    hosts.hosts = [host('alpha'), host('bravo')];
    hosts.recentAt = { 'host-alpha': 100, 'host-bravo': 50 };
    render();

    expect(inspector()?.querySelector('h2')?.textContent).toBe('alpha');
    expect(inspector()?.textContent).toContain('deploy@alpha.test:22');

    const second = document.body.querySelectorAll<HTMLElement>('.row')[1];
    second.click();
    flushSync();
    expect(hosts.selectedId).toBe('host-bravo');
    expect(inspector()?.querySelector('h2')?.textContent).toBe('bravo');
  });

  it('fades in for a click and never for a key', async () => {
    hosts.hosts = [host('alpha'), host('bravo'), host('charlie')];
    hosts.recentAt = { 'host-alpha': 3, 'host-bravo': 2, 'host-charlie': 1 };
    render();

    const rows = () => Array.from(document.body.querySelectorAll<HTMLElement>('.row'));
    rows()[1].click();
    flushSync();
    expect(inspector()?.querySelector('.content')?.classList.contains('fade')).toBe(true);

    rows()[1].dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true }));
    await vi.waitFor(() => expect(hosts.selectedId).toBe('host-charlie'));
    flushSync();
    expect(inspector()?.querySelector('h2')?.textContent).toBe('charlie');
    expect(inspector()?.querySelector('.content')?.classList.contains('fade')).toBe(false);
  });

  it('collapses and comes back, and remembers it as a choice of this device', () => {
    hosts.hosts = [host('alpha')];
    render();
    expect(inspector()).not.toBeNull();

    const hide = document.body.querySelector(
      'button[aria-label="Hide the inspector"]',
    ) as HTMLButtonElement;
    hide.click();
    flushSync();
    expect(inspector()).toBeNull();
    expect(chrome.inspectorOpen).toBe(false);

    const show = document.body.querySelector(
      'button[aria-label="Show the inspector"]',
    ) as HTMLButtonElement;
    expect(show.getAttribute('aria-pressed')).toBe('false');
    show.click();
    flushSync();
    expect(inspector()).not.toBeNull();
  });

  it('has no inspector while there is nothing to inspect', () => {
    render();
    expect(inspector()).toBeNull();
    hosts.error = 'locked';
    flushSync();
    expect(inspector()).toBeNull();
  });
});

describe('hosts page state matrix', () => {
  it('shows a static placeholder while loading (aria-busy, no shimmer)', () => {
    hosts.loading = true;
    render();

    const busy = document.body.querySelector('[aria-busy="true"]');
    expect(busy).not.toBeNull();
    expect(document.body.textContent).toContain('Loading hosts…');
    expect(document.body.querySelectorAll('.placeholder').length).toBeGreaterThan(0);
  });

  it('shows the empty state with a New host button', () => {
    render();

    expect(document.body.textContent).toContain('No hosts yet');
    // Both ways in are on the empty state: a new host, and the ones in ~/.ssh/config.
    expect(document.body.querySelector('.state')?.textContent).toContain('Import ~/.ssh/config');
    const button = document.body.querySelector('.state button.primary') as HTMLButtonElement;
    expect(button.textContent?.trim()).toBe('New host');
    button.click();
    flushSync();
    expect(route.route).toBe('host-edit');
  });

  it('shows the filtered-empty state with a Clear filters button', () => {
    hosts.hosts = [host('web')];
    hosts.query = 'nothing-matches';
    render();

    expect(document.body.textContent).toContain('No hosts match your filters.');
    findButton('Clear filters').click();
    flushSync();
    expect(hosts.query).toBe('');
    expect(document.body.textContent).toContain('web');
  });

  it('shows the error with a Retry that reloads', async () => {
    hosts.error = 'the store is locked';
    render();

    expect(document.body.textContent).toContain('the store is locked');
    expect(document.body.querySelector('header.head .count')).toBeNull();
    findButton('Retry').click();
    await vi.waitFor(() => {
      expect(hosts.loading).toBe(false);
    });
    expect(hosts.error).toBeNull();
  });

  it('shows the offline note with the last sync and the local guarantee', () => {
    sync.status = syncStatus();
    render();
    expect(document.body.textContent).not.toContain('Changes are safe locally');

    window.dispatchEvent(new Event('offline'));
    flushSync();
    expect(document.body.textContent).toContain('Changes are safe locally');
    expect(document.body.textContent).toContain('Offline');

    window.dispatchEvent(new Event('online'));
    flushSync();
    expect(document.body.textContent).not.toContain('Changes are safe locally');
  });
});

describe('hosts page running-sessions strip', () => {
  const strip = () => document.body.querySelector('section[aria-label="Running sessions"]');

  it('is absent while no session runs', () => {
    hosts.hosts = [host('atlas-prod')];
    render();
    expect(strip()).toBeNull();
  });

  it('counts and names the running hosts and returns to the first one', () => {
    hosts.hosts = [host('atlas-prod'), host('relay-edge')];
    sessions.tabs = [liveTab('atlas-prod'), liveTab('relay-edge')];
    const rendered = render();

    expect(strip()?.textContent).toContain('2 sessions running');
    expect(strip()?.textContent).toContain(
      'atlas-prod and relay-edge stay connected while you browse.',
    );

    findButton('Return to atlas-prod').click();
    expect(rendered.onopen).toHaveBeenCalledWith(sessions.tabs[0]?.panes[0]?.host);
  });

  it('uses the singular for a single running host', () => {
    hosts.hosts = [host('atlas-prod')];
    sessions.tabs = [liveTab('atlas-prod')];
    render();

    expect(strip()?.textContent).toContain('1 session running');
    expect(strip()?.textContent).toContain('atlas-prod stays connected while you browse.');
    expect(findButton('Return to atlas-prod')).toBeTruthy();
  });

  it('marks the live row with Live', () => {
    hosts.hosts = [host('atlas-prod')];
    sessions.tabs = [liveTab('atlas-prod')];
    render();
    expect(document.body.querySelector('.row .session-cell')?.textContent?.trim()).toBe('Live');
  });
});

describe('hosts page with a lot of everything', () => {
  it('counts a big library with separators', () => {
    hosts.hosts = Array.from({ length: 10_005 }, (_, i) => host(`h-${i}`));
    render();
    expect(document.body.querySelector('header.head .count')?.textContent).toBe('10,005');
  });

  it('names three sessions at most in the banner, and keeps its button on one line', () => {
    const longName =
      'customer-facing-reporting-service-eu-central-1-blue-green-canary-deployment-primary';
    hosts.hosts = ['a', 'b', 'c', 'd', 'e', longName].map((name) => host(name));
    sessions.tabs = [longName, 'b', 'c', 'd', 'e', 'a'].map((name) => liveTab(name));
    render();
    expect(document.body.textContent).toContain(
      `${longName}, b and 4 more stay connected while you browse.`,
    );
    const back = document.body.querySelector('.strip-return') as HTMLButtonElement;
    expect(back.title).toBe(`Return to ${longName}`);
    expect(back.querySelector('.return-label')?.textContent).toBe(`Return to ${longName}`);
  });
});

describe('hosts page strip motion', () => {
  const frame = () => new Promise((resolve) => requestAnimationFrame(() => resolve(null)));
  const banner = () => document.body.querySelector('section[aria-label="Running sessions"]');

  it('does not move when it is part of the first paint', async () => {
    window.dispatchEvent(new Event('pointerdown'));
    hosts.hosts = [host('atlas-prod')];
    sessions.tabs = [liveTab('atlas-prod')];
    render();
    await frame();
    await frame();
    expect(banner()).not.toBeNull();
    expect(banner()?.classList.contains('appear')).toBe(false);
  });

  it('moves in when a session starts while the page is open', async () => {
    window.dispatchEvent(new Event('pointerdown'));
    hosts.hosts = [host('atlas-prod')];
    render();
    await frame();
    await frame();
    expect(banner()).toBeNull();

    sessions.tabs = [liveTab('atlas-prod')];
    flushSync();
    expect(banner()?.classList.contains('appear')).toBe(true);
    await vi.waitFor(() => expect(banner()?.classList.contains('appear-in')).toBe(true));
  });

  it('does not move when a key started the session', async () => {
    hosts.hosts = [host('atlas-prod')];
    render();
    await frame();
    await frame();
    window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter' }));
    sessions.tabs = [liveTab('atlas-prod')];
    flushSync();
    expect(banner()).not.toBeNull();
    expect(banner()?.classList.contains('appear')).toBe(false);
    window.dispatchEvent(new Event('pointerdown'));
  });
});

describe('hosts page windowing at 10 000 hosts', () => {
  it('keeps the DOM small and the filter keystroke inside a frame budget', async () => {
    hosts.hosts = Array.from({ length: 10_000 }, (_, i) => {
      const entry = host(`host-${i}`);
      if (i % 10 === 0) entry.name = `web-${i}`;
      return entry;
    });
    render();
    await vi.waitFor(() => {
      expect(document.body.querySelectorAll('.row').length).toBeGreaterThan(0);
    });

    const domRows = document.body.querySelectorAll('.row').length;
    expect(domRows).toBeLessThan(100);

    // Warm-up: the first keystroke pays for the JIT compilation of the
    // filter paths, as any first interaction does.
    hosts.query = 'web-7770';
    flushSync();
    expect(document.body.textContent).toContain('web-7770');

    // The app's own work: the deriveds compute the new rows and counts.
    hosts.query = 'host-123';
    const started = performance.now();
    void hosts.rows;
    void hosts.counts;
    const storeMs = performance.now() - started;
    flushSync();
    const totalMs = performance.now() - started;
    // The design goal is one frame (16.7 ms) for the store's own filter
    // work — measured 4.7–8 ms on an idle machine. The test budget is 50 ms
    // because this suite runs 21 workers on 2 cores and the same work
    // costs ~26 ms under that load; 50 ms still catches the regression
    // classes this replaced (a re-sort per keystroke: ~155 ms; the
    // original unwindowed table: 2.2 s). Numbers in docs/performance.md.
    expect(storeMs).toBeLessThan(50);
    expect(totalMs).toBeLessThan(150);
    expect(document.body.querySelectorAll('.row').length).toBeLessThan(100);
    expect(document.body.textContent).toContain('host-123');
  });
});
