// @vitest-environment jsdom
/**
 * The hosts page (frame 1): header, filters, tabs, the running-sessions
 * banner, the state matrix (Doc A.05) and the 10 000 host windowing.
 */

import { flushSync, mount, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import HostsPage from './HostsPage.svelte';
import { leafTree } from '../pane-tree';
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
  it('spells the live count line like the frame', () => {
    hosts.hosts = [host('a'), host('b')];
    sessions.tabs = [liveTab('a')];
    render();

    expect(document.body.textContent).toContain('Two saved connections. One session running.');
    expect(document.body.querySelector('nav[aria-label="Breadcrumb"]')?.textContent).toMatch(
      /Workspace\s*\/\s*Connection library/,
    );
    expect(document.body.textContent).toContain('2 hosts');
    expect(document.body.textContent).toContain('Recent activity');
  });

  it('offers the filter tabs with their counts', () => {
    hosts.hosts = [host('a', { favorite: true }), host('b')];
    hosts.recentAt = { 'host-b': 100 };
    render();

    expect(findButton('All hosts 2').getAttribute('aria-pressed')).toBe('true');
    expect(findButton('Favorites 1').getAttribute('aria-pressed')).toBe('false');
    expect(findButton('Unfiled 2')).toBeTruthy();

    findButton('Favorites 1').click();
    flushSync();
    expect(hosts.filterTab).toBe('favorites');
    expect(findButton('Favorites 1').getAttribute('aria-pressed')).toBe('true');
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
    const button = findButton('New host');
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

describe('hosts page banner', () => {
  it('is absent while no session runs', () => {
    hosts.hosts = [host('atlas-prod')];
    render();
    expect(document.body.textContent).not.toContain('Your sessions are still running');
  });

  it('names the running hosts and returns to the first one', () => {
    hosts.hosts = [host('atlas-prod'), host('relay-edge')];
    sessions.tabs = [liveTab('atlas-prod'), liveTab('relay-edge')];
    const rendered = render();

    expect(document.body.textContent).toContain('Your sessions are still running');
    expect(document.body.textContent).toContain(
      'atlas-prod and relay-edge stay connected while you organize your workspace.',
    );

    findButton('Return to atlas-prod').click();
    expect(rendered.onopen).toHaveBeenCalledWith(sessions.tabs[0]?.panes[0]?.host);
  });

  it('uses the singular verb for a single running host', () => {
    hosts.hosts = [host('atlas-prod')];
    sessions.tabs = [liveTab('atlas-prod')];
    render();

    expect(document.body.textContent).toContain(
      'atlas-prod stays connected while you organize your workspace.',
    );
    expect(findButton('Return to atlas-prod')).toBeTruthy();
  });
});

describe('hosts page with a lot of everything', () => {
  it('counts a big library with separators', () => {
    hosts.hosts = Array.from({ length: 10_005 }, (_, i) => host(`h-${i}`));
    render();
    expect(document.body.textContent).toContain('10,005 saved connections.');
    expect(document.body.textContent).toContain('10,005 hosts · Recent activity');
    expect(findButton('All hosts 10,005').getAttribute('aria-pressed')).toBe('true');
  });

  it('names three sessions at most in the banner, and keeps its button on one line', () => {
    const longName =
      'customer-facing-reporting-service-eu-central-1-blue-green-canary-deployment-primary';
    hosts.hosts = ['a', 'b', 'c', 'd', 'e', longName].map((name) => host(name));
    sessions.tabs = [longName, 'b', 'c', 'd', 'e', 'a'].map((name) => liveTab(name));
    render();
    expect(document.body.textContent).toContain(
      `${longName}, b and 4 more stay connected while you organize your workspace.`,
    );
    const back = document.body.querySelector('.banner-return') as HTMLButtonElement;
    expect(back.title).toBe(`Return to ${longName}`);
    expect(back.querySelector('.return-label')?.textContent).toBe(`Return to ${longName}`);
  });
});

describe('hosts page banner motion', () => {
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
