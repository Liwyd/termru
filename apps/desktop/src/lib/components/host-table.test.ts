// @vitest-environment jsdom
/**
 * The hosts table: the row cells, the selection, the row actions and the
 * windowed rendering of a very large list.
 */

import { flushSync, mount, tick } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import HostTable from './HostTable.svelte';
import { hosts } from '../stores/hosts.svelte';
import type { Host, Identity } from '../types';

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

function identity(id: string, name: string): Identity {
  const now = new Date().toISOString();
  return {
    id,
    name,
    comment: null,
    fingerprint: null,
    created_at: now,
    updated_at: now,
    deleted: false,
  };
}

interface Handlers {
  onselect: (host: Host, via: 'pointer' | 'keyboard') => void;
  onconnect: (host: Host) => void;
  onopen: (host: Host) => void;
  onedit: (host: Host) => void;
  ondelete: (host: Host) => void;
  ontogglefavorite: (host: Host) => void;
}

function render(
  rows: Host[],
  extra: { live?: Map<string, number>; recentAt?: Record<string, number> } = {},
): {
  target: HTMLElement;
  handlers: Handlers;
} {
  const target = document.createElement('div');
  document.body.appendChild(target);
  const handlers = {
    onselect: vi.fn<(host: Host, via: 'pointer' | 'keyboard') => void>(),
    onconnect: vi.fn<(host: Host) => void>(),
    onopen: vi.fn<(host: Host) => void>(),
    onedit: vi.fn<(host: Host) => void>(),
    ondelete: vi.fn<(host: Host) => void>(),
    ontogglefavorite: vi.fn<(host: Host) => void>(),
  };
  mount(HostTable, {
    target,
    props: {
      rows,
      live: extra.live ?? new Map<string, number>(),
      recentAt: extra.recentAt ?? {},
      ...handlers,
    },
  });
  flushSync();
  return { target, handlers };
}

function findButton(root: ParentNode, label: string): HTMLButtonElement {
  // The selected row's button carries the Enter key cap: "Connect ↵".
  const button = Array.from(root.querySelectorAll('button')).find(
    (candidate) => candidate.textContent?.replace('↵', '').trim() === label,
  );
  if (!button) throw new Error(`button "${label}" not found`);
  return button;
}

let target: HTMLElement | null = null;

beforeEach(() => {
  hosts.identities = [];
  hosts.groups = [];
  hosts.passwordStored = {};
});

afterEach(() => {
  if (target) target.remove();
  target = null;
  document.body.innerHTML = '';
});

describe('HostTable rows', () => {
  it('renders the cells: host, folder, sign-in, last session', () => {
    const web = host('web', { auth: { type: 'private_key', identity_id: 'identity-1' } });
    hosts.identities = [identity('identity-1', 'work-ed25519')];
    const rendered = render([web], {
      recentAt: { 'host-web': new Date(2026, 9, 18, 8, 0).getTime() },
    });
    target = rendered.target;

    expect(target.textContent).toContain('web');
    expect(target.textContent).toContain('deploy@web.test:22');
    expect(target.textContent).toContain('Unfiled');
    // A key icon says it is an identity; the cell shows its name, the title all of it.
    expect(target.querySelector('.auth-text')?.textContent).toBe('work-ed25519');
    expect((target.querySelector('.auth-cell') as HTMLElement).title).toBe(
      'Identity · work-ed25519',
    );
    expect(target.textContent).toContain('18 Oct');
    expect(target.textContent).toContain('Last session');
    expect(target.textContent).toContain('Sign-in');
  });

  it('shows the folder path of a filed host', () => {
    hosts.groups = [
      {
        id: 'g1',
        name: 'Production',
        parent_id: null,
        sort_order: 0,
        created_at: new Date().toISOString(),
        updated_at: new Date().toISOString(),
        deleted: false,
      },
    ];
    const web = host('web');
    web.group_id = 'g1';
    const rendered = render([web]);
    target = rendered.target;
    expect(target.textContent).toContain('Production');
    expect(target.textContent).not.toContain('Unfiled');
  });

  it('shows Live (and the pane count) for a running session and the date otherwise', () => {
    const live = host('atlas');
    const two = host('relay');
    const idle = host('archive');
    const rendered = render([live, two, idle], {
      live: new Map([
        ['host-atlas', 1],
        ['host-relay', 2],
      ]),
      recentAt: { 'host-archive': new Date(2026, 9, 18, 8, 0).getTime() },
    });
    target = rendered.target;
    const cells = Array.from(target.querySelectorAll('.session-cell')).map((cell) =>
      cell.textContent?.trim(),
    );
    expect(cells).toEqual(['Live', 'Live · 2', '18 Oct']);
  });

  it('shows the last two folders of a deep path, and the whole path as the tooltip', () => {
    const stamp = new Date().toISOString();
    const folder = (id: string, name: string, parent: string | null) => ({
      id,
      name,
      parent_id: parent,
      sort_order: 0,
      created_at: stamp,
      updated_at: stamp,
      deleted: false,
    });
    hosts.groups = [
      folder('g1', 'Northwind Industries Holdings', null),
      folder('g2', 'Platform Infrastructure', 'g1'),
      folder('g3', 'Production (Frankfurt)', 'g2'),
      folder('g4', 'Legacy billing reconciliation', 'g3'),
    ];
    const web = host('web', { group_id: 'g4' });
    const rendered = render([web]);
    target = rendered.target;
    const cell = target.querySelector('.folder-cell') as HTMLElement;
    expect(cell.textContent?.trim()).toBe(
      '… / Production (Frankfurt) / Legacy billing reconciliation',
    );
    expect(cell.title).toBe(
      'Northwind Industries Holdings / Platform Infrastructure / Production (Frankfurt) / Legacy billing reconciliation',
    );
  });

  it('gives the long name and address a tooltip, and keeps both on one line', () => {
    const name = 'customer-facing-reporting-service-eu-central-1-blue-green-canary-deployment';
    const rendered = render([host(name, { address: 'ip-10-128-44-203.example.test' })]);
    target = rendered.target;
    expect((target.querySelector('.name') as HTMLElement).title).toBe(name);
    expect((target.querySelector('.target') as HTMLElement).title).toBe(
      `deploy@ip-10-128-44-203.example.test:22`,
    );
  });

  it('reads the password state from the store for the Authentication cell', () => {
    const web = host('web');
    hosts.passwordStored = { 'host-web': true };
    const rendered = render([web]);
    target = rendered.target;
    expect(target.textContent).toContain('Password · This device');
  });
});

describe('HostTable actions', () => {
  it('connects a host with the Connect button and the double-click', () => {
    const web = host('web');
    const rendered = render([web]);
    target = rendered.target;

    click(findButton(target, 'Connect'));
    expect(rendered.handlers.onconnect).toHaveBeenCalledWith(web);

    target.querySelector('.row')?.dispatchEvent(new MouseEvent('dblclick', { bubbles: true }));
    expect(rendered.handlers.onconnect).toHaveBeenCalledTimes(2);
  });

  it('opens the running session of a live host', () => {
    const web = host('web');
    const rendered = render([web], { live: new Map([['host-web', 1]]) });
    target = rendered.target;

    click(findButton(target, 'Open'));
    expect(rendered.handlers.onopen).toHaveBeenCalledWith(web);
    expect(rendered.handlers.onconnect).not.toHaveBeenCalled();
  });

  it('edits from the More menu', () => {
    const web = host('web');
    const rendered = render([web]);
    target = rendered.target;

    (target.querySelector('.more') as HTMLButtonElement).click();
    flushSync();
    const edit = Array.from(document.body.querySelectorAll('.menu button')).find(
      (button) => button.textContent?.trim() === 'Edit…',
    ) as HTMLButtonElement;
    edit.click();
    expect(rendered.handlers.onedit).toHaveBeenCalledWith(web);
  });

  it('offers Open running session first for a host that is live', () => {
    const web = host('web');
    const rendered = render([web], { live: new Map([['host-web', 1]]) });
    target = rendered.target;

    (target.querySelector('.more') as HTMLButtonElement).click();
    flushSync();
    const labels = Array.from(document.body.querySelectorAll('.menu button')).map((button) =>
      button.textContent?.trim(),
    );
    expect(labels).toEqual([
      'Open running session',
      'Connect',
      'Edit…',
      'Add to favorites',
      'Delete',
    ]);
  });

  it('selects a row by click and says it was a pointer', () => {
    const [a, b] = [host('a'), host('b')];
    const rendered = render([a, b]);
    target = rendered.target;

    const second = Array.from(target.querySelectorAll<HTMLElement>('.row'))[1];
    click(second);
    flushSync();
    expect(rendered.handlers.onselect).toHaveBeenCalledWith(b, 'pointer');
    expect(second.classList.contains('selected')).toBe(true);
    expect(second.getAttribute('aria-current')).toBe('true');
    // The Enter hint is on the selected row only.
    expect(second.querySelector('.go kbd')).not.toBeNull();
    expect(target.querySelectorAll('.go kbd')).toHaveLength(1);
  });

  it('toggles the favorite with the star', () => {
    const web = host('web');
    const rendered = render([web]);
    target = rendered.target;

    const star = target.querySelector('.star') as HTMLButtonElement;
    star.click();
    expect(rendered.handlers.ontogglefavorite).toHaveBeenCalledWith(web);
  });

  it('offers connect, edit, favorite and delete in the More menu', () => {
    const web = host('web');
    const rendered = render([web]);
    target = rendered.target;

    const more = target.querySelector('.more') as HTMLButtonElement;
    more.click();
    flushSync();

    const menu = document.body.querySelector('.menu');
    expect(menu).not.toBeNull();
    const labels = Array.from(menu!.querySelectorAll('button')).map(
      (button) => button.textContent?.trim() ?? '',
    );
    expect(labels).toEqual(['Connect', 'Edit…', 'Add to favorites', 'Delete']);

    const deleteButton = Array.from(menu!.querySelectorAll('button')).find(
      (button) => button.textContent?.trim() === 'Delete',
    );
    deleteButton?.click();
    expect(rendered.handlers.ondelete).toHaveBeenCalledWith(web);
  });

  it('shows no Edit button on the row: it is in the menu, the inspector and on the E key', () => {
    const rendered = render([host('web')]);
    target = rendered.target;
    expect(
      Array.from(target.querySelectorAll('button')).some((b) => b.textContent?.trim() === 'Edit'),
    ).toBe(false);
  });

  it('opens the More menu of a row on right-click', () => {
    const web = host('web');
    const rendered = render([web]);
    target = rendered.target;

    target
      .querySelector('.row')
      ?.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true }));
    flushSync();
    expect(document.body.querySelector('.menu')).not.toBeNull();
  });

  it('names the menu after the host and moves only for a pointer', async () => {
    const web = host('web');
    const rendered = render([web]);
    target = rendered.target;

    // The More button pressed by a pointer (detail 1): the menu scales in.
    const more = target.querySelector('.more') as HTMLButtonElement;
    more.dispatchEvent(new MouseEvent('click', { bubbles: true, detail: 1 }));
    flushSync();
    await tick();
    const menu = document.body.querySelector('.menu') as HTMLElement;
    expect(menu.getAttribute('aria-label')).toBe('Actions for web');
    expect(menu.classList.contains('still')).toBe(false);
    // The pointer highlights nothing: the menu itself has the focus.
    expect(document.activeElement).toBe(menu);
  });

  it('opens without motion from the keyboard (Enter on the button, the menu key)', async () => {
    const web = host('web');
    const rendered = render([web]);
    target = rendered.target;

    // Enter or Space on a button is a click with no pointer: detail 0.
    const more = target.querySelector('.more') as HTMLButtonElement;
    more.dispatchEvent(new MouseEvent('click', { bubbles: true, detail: 0 }));
    flushSync();
    await tick();
    const menu = document.body.querySelector('.menu') as HTMLElement;
    expect(menu.classList.contains('still')).toBe(true);
    // The keyboard lands on the first entry.
    expect(document.activeElement?.textContent?.trim()).toBe('Connect');
    document.body
      .querySelector('.menu')
      ?.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
  });

  it('a right-click moves the menu in, the menu key does not', () => {
    const web = host('web');
    const rendered = render([web]);
    target = rendered.target;
    const row = target.querySelector('.row') as HTMLElement;

    row.dispatchEvent(
      new MouseEvent('contextmenu', { bubbles: true, cancelable: true, button: 2 }),
    );
    flushSync();
    expect(document.body.querySelector('.menu')?.classList.contains('still')).toBe(false);
  });

  it('activates the row with Enter, not the buttons inside it', () => {
    const web = host('web');
    const rendered = render([web]);
    target = rendered.target;

    const row = target.querySelector('.row') as HTMLElement;
    row.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    expect(rendered.handlers.onconnect).toHaveBeenCalledTimes(1);
  });
});

describe('HostTable windowing', () => {
  it('renders only a small window of a 10 000 host table', async () => {
    const rows = Array.from({ length: 10_000 }, (_, i) => host(`host-${i}`));
    const rendered = render(rows);
    target = rendered.target;
    await Promise.resolve();

    const domRows = target.querySelectorAll('.row').length;
    expect(domRows).toBeGreaterThan(0);
    expect(domRows).toBeLessThan(100);
  });
});

describe('HostTable keyring lookups', () => {
  afterEach(() => {
    vi.useRealTimers();
    vi.restoreAllMocks();
  });

  it('asks about the rows on screen at once, and about a scrolled window only when it settles', () => {
    vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout', 'Date', 'performance'] });
    const ask = vi.spyOn(hosts, 'ensurePasswordState').mockResolvedValue();
    const rendered = render(Array.from({ length: 5_000 }, (_, i) => host(`h-${i}`)));
    target = rendered.target;

    vi.advanceTimersByTime(1);
    const first = ask.mock.calls.length;
    expect(first).toBeGreaterThan(0);
    expect(first).toBeLessThan(40);

    // A flick through the list: ten scroll events in 100 ms ask nothing.
    const body = target.querySelector('.body') as HTMLElement;
    for (let step = 1; step <= 10; step++) {
      body.scrollTop = step * 14_000;
      body.dispatchEvent(new Event('scroll'));
      flushSync();
      vi.advanceTimersByTime(10);
    }
    expect(ask.mock.calls.length).toBe(first);

    // It comes to rest: only the rows now on screen are asked about.
    vi.advanceTimersByTime(200);
    const later = ask.mock.calls.slice(first).map((call) => (call[0] as Host).name);
    expect(later.length).toBeGreaterThan(0);
    expect(later.length).toBeLessThan(40);
    const indexes = later.map((name) => Number(name.replace('h-', '')));
    expect(Math.min(...indexes)).toBeGreaterThan(1_500);
  });
});

function click(element: Element): void {
  element.dispatchEvent(new MouseEvent('click', { bubbles: true }));
}

describe('HostTable keyboard', () => {
  /** The row takes the focus a microtask after the key (it waits for the DOM). */
  async function settle(): Promise<void> {
    await tick();
    await tick();
  }

  function key(element: Element, name: string, init: KeyboardEventInit = {}): void {
    element.dispatchEvent(new KeyboardEvent('keydown', { key: name, bubbles: true, ...init }));
  }

  function rowOf(root: ParentNode, name: string): HTMLElement {
    const row = Array.from(root.querySelectorAll<HTMLElement>('.row')).find((entry) =>
      entry.textContent?.includes(name),
    );
    if (!row) throw new Error(`row "${name}" not found`);
    return row;
  }

  it('has one tab stop: the first row, and only the buttons of that row', () => {
    const rendered = render([host('a'), host('b'), host('c')]);
    target = rendered.target;
    const rows = Array.from(target.querySelectorAll<HTMLElement>('.row'));
    expect(rows.map((row) => row.tabIndex)).toEqual([0, -1, -1]);
    // The first row is the selected one until another is chosen.
    expect(rows.map((row) => row.classList.contains('selected'))).toEqual([true, false, false]);
    // The buttons of the other rows are not tab stops.
    expect(rows[0].querySelector('button')?.tabIndex).toBe(0);
    expect(rows[1].querySelector('button')?.tabIndex).toBe(-1);
    expect(rows[2].querySelector('button')?.tabIndex).toBe(-1);
  });

  it('moves the selection, the focus and the tab stop with the arrow keys', async () => {
    const rendered = render([host('a'), host('b'), host('c')]);
    target = rendered.target;
    const first = rowOf(target, 'a.test');
    first.focus();

    key(first, 'ArrowDown');
    await settle();
    const second = rowOf(target, 'b.test');
    expect(document.activeElement).toBe(second);
    expect(second.tabIndex).toBe(0);
    expect(first.tabIndex).toBe(-1);
    // The arrow drives the selection, and a key never animates what follows it.
    expect(second.classList.contains('selected')).toBe(true);
    expect(rendered.handlers.onselect).toHaveBeenLastCalledWith(
      expect.objectContaining({ name: 'b' }),
      'keyboard',
    );

    key(second, 'End');
    await settle();
    expect(document.activeElement).toBe(rowOf(target, 'c.test'));
    key(rowOf(target, 'c.test'), 'ArrowDown');
    await settle();
    // The last row stays the last.
    expect(document.activeElement).toBe(rowOf(target, 'c.test'));

    key(rowOf(target, 'c.test'), 'Home');
    await settle();
    expect(document.activeElement).toBe(rowOf(target, 'a.test'));
    key(rowOf(target, 'a.test'), 'ArrowUp');
    await settle();
    expect(document.activeElement).toBe(rowOf(target, 'a.test'));
  });

  it('connects on Enter and edits on E or F2', () => {
    const web = host('web');
    const rendered = render([web]);
    target = rendered.target;
    const row = rowOf(target, 'web.test');
    row.focus();

    key(row, 'Enter');
    expect(rendered.handlers.onconnect).toHaveBeenCalledWith(web);

    key(row, 'e');
    key(row, 'F2');
    expect(rendered.handlers.onedit).toHaveBeenCalledTimes(2);

    // A chord with a modifier belongs to the app, not to the row.
    key(row, 'e', { ctrlKey: true });
    expect(rendered.handlers.onedit).toHaveBeenCalledTimes(2);
  });

  it('leaves the keys of its buttons to them', () => {
    const web = host('web');
    const rendered = render([web]);
    target = rendered.target;
    const connect = findButton(target, 'Connect');
    key(connect, 'e');
    expect(rendered.handlers.onedit).not.toHaveBeenCalled();
  });
});
