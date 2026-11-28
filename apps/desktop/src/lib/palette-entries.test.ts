// @vitest-environment jsdom
/**
 * What the command palette offers: the running sessions, every host (Enter
 * connects or opens, Shift+Enter edits), every page, every action of the app
 * that exists, the library views and folders, the sections of Preferences and
 * the theme.
 */

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { buildPaletteEntries, type PaletteDeps } from './palette-entries';
import { leafTree } from './pane-tree';
import { hosts } from './stores/hosts.svelte';
import { intents } from './stores/intents.svelte';
import { palette } from './stores/palette.svelte';
import { prefs } from './stores/prefs.svelte';
import { route } from './stores/route.svelte';
import { sessions, type Pane, type Tab } from './stores/sessions.svelte';
import { sync } from './stores/sync.svelte';
import type { Group, Host, SessionStatus } from './types';

const STAMP = '2026-01-01T00:00:00Z';

function host(name: string, over: Partial<Host> = {}): Host {
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
    created_at: STAMP,
    updated_at: STAMP,
    deleted: false,
    ...over,
  };
}

function folder(id: string, name: string, parent: string | null = null): Group {
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

function tabFor(name: string, status: SessionStatus = { state: 'ready' }, panes = 1): Tab {
  const list: Pane[] = Array.from({ length: panes }, (_, i) => ({
    id: `pane-${name}-${i}`,
    sessionId: status.state === 'ready' ? `s-${name}-${i}` : null,
    host: host(name),
    title: name,
    status,
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
  }));
  return {
    id: `tab-${name}`,
    title: name,
    panes: list,
    activePaneId: list[0].id,
    root: leafTree(list[0].id),
  };
}

function deps(): PaletteDeps {
  return {
    run: vi.fn(),
    connect: vi.fn(),
    openRunning: vi.fn(),
    setTheme: vi.fn(),
  };
}

const ids = (entries: { id: string }[]) => entries.map((entry) => entry.id);

beforeEach(() => {
  hosts.hosts = [host('atlas-prod', { group_id: 'g-eu' }), host('relay-edge'), host('archive')];
  hosts.groups = [folder('g-prod', 'Production'), folder('g-eu', 'Europe', 'g-prod')];
  hosts.tags = [];
  hosts.recentAt = {};
  sessions.tabs = [];
  sessions.activeId = null;
  sync.status = null;
  prefs.prefs = { ...prefs.prefs, keybindings: {} };
  route.route = 'hosts';
  intents.pending = null;
  palette.hide();
});

afterEach(() => {
  hosts.hosts = [];
  hosts.groups = [];
  sessions.tabs = [];
  sessions.activeId = null;
  intents.pending = null;
});

describe('palette entries', () => {
  it('lists every host once: Enter connects, Shift+Enter edits', () => {
    const d = deps();
    const entries = buildPaletteEntries(d);
    const forHosts = entries.filter((entry) => entry.id.startsWith('host:'));
    expect(forHosts).toHaveLength(3);

    const atlas = forHosts.find((entry) => entry.id === 'host:host-atlas-prod');
    expect(atlas?.title).toBe('Connect to atlas-prod');
    expect(atlas?.detail).toBe('ops@atlas-prod.test');
    // The folder is searchable though it is not shown.
    expect(atlas?.keywords).toContain('Production / Europe');
    atlas?.run();
    expect(d.connect).toHaveBeenCalledWith(expect.objectContaining({ name: 'atlas-prod' }));

    const editSpy = vi.spyOn(route, 'edit').mockImplementation(() => {});
    expect(atlas?.alt?.verb).toBe('Edit');
    atlas?.alt?.run();
    expect(editSpy).toHaveBeenCalledWith(expect.objectContaining({ name: 'atlas-prod' }));
    editSpy.mockRestore();
  });

  it('opens the running session of a live host instead of connecting again', () => {
    sessions.tabs = [tabFor('relay-edge')];
    const d = deps();
    const relay = buildPaletteEntries(d).find((entry) => entry.id === 'host:host-relay-edge');
    expect(relay?.title).toBe('Open relay-edge');
    expect(relay?.detail).toContain('live');
    expect(relay?.verb).toBe('Open');
    relay?.run();
    expect(d.openRunning).toHaveBeenCalled();
    expect(d.connect).not.toHaveBeenCalled();
  });

  it('keeps the long tail of hosts out of the start list, and finds it by typing', () => {
    hosts.hosts = Array.from({ length: 30 }, (_, i) => host(`srv-${i}`));
    palette.show(buildPaletteEntries(deps()));
    const start = palette.results.filter((entry) => entry.id.startsWith('host:'));
    expect(start).toHaveLength(8);

    palette.query = 'srv-27';
    expect(palette.results[0]?.id).toBe('host:host-srv-27');
  });

  it('offers every page, with the shortcut the user has', () => {
    const entries = buildPaletteEntries(deps());
    const pages = entries.filter((entry) => entry.group === 'Go to');
    expect(pages.map((entry) => entry.title)).toEqual([
      'Go to Hosts',
      'Go to Sessions',
      'Go to Folders',
      'Go to Identities',
      'Go to Sync',
      'Go to Preferences',
    ]);
    expect(pages.find((entry) => entry.title === 'Go to Preferences')?.accelerator).toBe('Ctrl ,');
  });

  it('offers the actions that create things: host, import, folder, identity', () => {
    const d = deps();
    const create = buildPaletteEntries(d).filter((entry) => entry.group === 'Create');
    expect(ids(create)).toEqual(['host.new', 'host.import', 'folder.new', 'identity.new']);
    expect(create.map((entry) => entry.title)).toEqual([
      'Add host',
      'Import hosts from ~/.ssh/config',
      'New folder',
      'New identity (generate a key)',
    ]);
    create[1].run();
    expect(d.run).toHaveBeenCalledWith('host.import');
  });

  it('offers the session actions only while a session is open', () => {
    expect(
      ids(buildPaletteEntries(deps())).filter(
        (id) => id.startsWith('pane.') || id.startsWith('tab.'),
      ),
    ).toEqual([]);

    sessions.tabs = [tabFor('atlas-prod', { state: 'ready' }, 2), tabFor('relay-edge')];
    sessions.activeId = 'tab-atlas-prod';
    const entries = buildPaletteEntries(deps());
    const session = entries.filter((entry) => entry.group === 'Session').map((entry) => entry.id);
    expect(session).toEqual([
      'pane.splitRight',
      'pane.splitDown',
      'pane.find',
      'tab.duplicate',
      'tab.detach',
      'pane.next',
      'pane.previous',
      'pane.close',
      'tab.next',
      'tab.previous',
      'tab.close',
    ]);
  });

  it('leaves out duplicate and detach for a session that is not connected', () => {
    sessions.tabs = [tabFor('atlas-prod', { state: 'connecting' })];
    sessions.activeId = 'tab-atlas-prod';
    const session = ids(buildPaletteEntries(deps()));
    expect(session).toContain('pane.splitRight');
    expect(session).toContain('pane.find');
    expect(session).not.toContain('tab.duplicate');
    expect(session).not.toContain('tab.detach');
  });

  it('lists the running sessions and focuses one', () => {
    sessions.tabs = [tabFor('atlas-prod', { state: 'ready' }, 2)];
    const live = buildPaletteEntries(deps()).filter((entry) => entry.group === 'Live');
    expect(live).toHaveLength(1);
    expect(live[0].title).toBe('Go to atlas-prod');
    expect(live[0].detail).toBe('2 panes · connected');
    live[0].run();
    expect(sessions.activeId).toBe('tab-atlas-prod');
    expect(route.route).toBe('sessions');
  });

  it('shows a library view or a folder on the hosts page', () => {
    route.go('sessions');
    const entries = buildPaletteEntries(deps());
    entries.find((entry) => entry.id === 'library:favorites')?.run();
    expect(route.route).toBe('hosts');
    expect(hosts.filterTab).toBe('favorites');

    const europe = entries.find((entry) => entry.id === 'folder:g-eu');
    expect(europe?.title).toBe('Show hosts in Production / Europe');
    europe?.run();
    expect(hosts.folderFilter).toBe('g-eu');
    expect(hosts.filterTab).toBe('all');
    hosts.showLibrary('all');
  });

  it('takes the user to a section of Preferences', () => {
    const entries = buildPaletteEntries(deps());
    const sections = entries.filter((entry) => entry.group === 'Preferences');
    expect(sections.map((entry) => entry.title)).toEqual([
      'Preferences: Appearance',
      'Preferences: Terminal',
      'Preferences: Keyboard shortcuts',
      'Preferences: Motion and accessibility',
      'Preferences: Notifications',
      'Preferences: About',
    ]);
    sections[2].run();
    expect(route.route).toBe('preferences');
    expect(intents.pending).toEqual({ kind: 'preferences-section', id: 'shortcuts-label' });
  });

  it('sets the theme and toggles it', () => {
    const d = deps();
    const entries = buildPaletteEntries(d);
    expect(ids(entries)).toContain('view.toggleTheme');
    entries.find((entry) => entry.id === 'theme:light')?.run();
    entries.find((entry) => entry.id === 'theme:system')?.run();
    expect(d.setTheme).toHaveBeenNthCalledWith(1, 'light');
    expect(d.setTheme).toHaveBeenNthCalledWith(2, 'system');
  });

  it('offers Sync now when signed in, and a way to sign in when not', () => {
    expect(ids(buildPaletteEntries(deps()))).toContain('sync:sign-in');
    expect(ids(buildPaletteEntries(deps()))).not.toContain('sync.now');

    sync.status = {
      settings: { server_url: 'https://s', email: 'a@b.c', device_name: 'd', device_id: 'x' },
      signed_in: true,
      pending: 0,
      conflicts: 0,
      cursor: 1,
      last_sync_at: null,
      last_error: null,
    };
    const signedIn = ids(buildPaletteEntries(deps()));
    expect(signedIn).toContain('sync.now');
    expect(signedIn).not.toContain('sync:sign-in');
  });

  it('orders the start list by section, and a section keeps its entries together', () => {
    sessions.tabs = [tabFor('atlas-prod')];
    sessions.activeId = 'tab-atlas-prod';
    const groups = buildPaletteEntries(deps()).map((entry) => entry.group);
    const firsts = groups.filter((group, i) => group !== groups[i - 1]);
    expect(firsts).toEqual([...new Set(firsts)]);
    expect(firsts.slice(0, 3)).toEqual(['Live', 'Hosts', 'Go to']);
  });

  it('runs the second action with Shift+Enter, and the first with Enter', () => {
    const first = vi.fn();
    const second = vi.fn();
    palette.show([
      { id: 'a', title: 'Alpha', group: 'Hosts', run: first, alt: { verb: 'Edit', run: second } },
    ]);
    palette.choose(true);
    expect(second).toHaveBeenCalledTimes(1);
    expect(first).not.toHaveBeenCalled();

    palette.show([{ id: 'b', title: 'Beta', group: 'Hosts', run: first }]);
    // An entry with no second action runs its first.
    palette.choose(true);
    expect(first).toHaveBeenCalledTimes(1);
  });
});
