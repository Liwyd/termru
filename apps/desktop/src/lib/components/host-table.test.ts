// @vitest-environment jsdom
/**
 * The hosts table (frame 1): the row cells, the row actions and the
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
  onconnect: (host: Host) => void;
  onopen: (host: Host) => void;
  onedit: (host: Host) => void;
  ondelete: (host: Host) => void;
  ontogglefavorite: (host: Host) => void;
}

function render(
  rows: Host[],
  extra: { live?: Set<string>; recentAt?: Record<string, number> } = {},
): {
  target: HTMLElement;
  handlers: Handlers;
} {
  const target = document.createElement('div');
  document.body.appendChild(target);
  const handlers = {
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
      live: extra.live ?? new Set<string>(),
      recentAt: extra.recentAt ?? {},
      ...handlers,
    },
  });
  flushSync();
  return { target, handlers };
}

function findButton(root: ParentNode, label: string): HTMLButtonElement {
  const button = Array.from(root.querySelectorAll('button')).find(
    (candidate) => candidate.textContent?.trim() === label,
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
  it('renders the cells of the frame: host, folder, authentication, last session', () => {
    const web = host('web', { auth: { type: 'private_key', identity_id: 'identity-1' } });
    hosts.identities = [identity('identity-1', 'work-ed25519')];
    const rendered = render([web], {
      recentAt: { 'host-web': new Date(2026, 9, 18, 8, 0).getTime() },
    });
    target = rendered.target;

    expect(target.textContent).toContain('web');
    expect(target.textContent).toContain('deploy@web.test:22');
    expect(target.textContent).toContain('Unfiled');
    expect(target.textContent).toContain('Identity · work-ed25519');
    expect(target.textContent).toContain('18 Oct');
    expect(target.textContent).toContain('LAST SESSION');
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

  it('shows Connected for a running session and the date otherwise', () => {
    const live = host('atlas');
    const idle = host('archive');
    const rendered = render([live, idle], {
      live: new Set(['host-atlas']),
      recentAt: { 'host-archive': new Date(2026, 9, 18, 8, 0).getTime() },
    });
    target = rendered.target;
    expect(target.textContent).toContain('Connected');
    expect(target.textContent).toContain('18 Oct');
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
    const rendered = render([web], { live: new Set(['host-web']) });
    target = rendered.target;

    click(findButton(target, 'Open'));
    expect(rendered.handlers.onopen).toHaveBeenCalledWith(web);
    expect(rendered.handlers.onconnect).not.toHaveBeenCalled();
  });

  it('edits with the Edit button', () => {
    const web = host('web');
    const rendered = render([web]);
    target = rendered.target;

    click(findButton(target, 'Edit'));
    expect(rendered.handlers.onedit).toHaveBeenCalledWith(web);
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
      (button) => button.textContent === 'Delete',
    );
    deleteButton?.click();
    expect(rendered.handlers.ondelete).toHaveBeenCalledWith(web);
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
    // The buttons of the other rows are not tab stops.
    expect(rows[0].querySelector('button')?.tabIndex).toBe(0);
    expect(rows[1].querySelector('button')?.tabIndex).toBe(-1);
    expect(rows[2].querySelector('button')?.tabIndex).toBe(-1);
  });

  it('moves the focus and the tab stop with the arrow keys', async () => {
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
