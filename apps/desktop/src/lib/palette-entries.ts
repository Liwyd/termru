import {
  COMMANDS,
  acceleratorFor,
  compactChord,
  expandAccelerator,
  type CommandId,
} from './keybindings';
import { hosts } from './stores/hosts.svelte';
import { intents } from './stores/intents.svelte';
import type { PaletteEntry } from './stores/palette.svelte';
import { prefs } from './stores/prefs.svelte';
import { route } from './stores/route.svelte';
import { sessions } from './stores/sessions.svelte';
import { sync } from './stores/sync.svelte';
import type { Host, ThemePreference } from './types';

/** What the entries do that is not a command of the registry. */
export interface PaletteDeps {
  /** Runs a command (keyboard or palette). */
  run: (id: CommandId) => void;
  /** Starts a connection to a host. */
  connect: (host: Host) => void;
  /** Goes to the session that is already running for a host. */
  openRunning: (host: Host) => void;
  setTheme: (theme: ThemePreference) => void;
}

/** The hosts in the start list, before the user types (the rest are found by typing). */
const START_HOSTS = 8;
/** The folders in the start list. */
const START_FOLDERS = 6;

/** The sections of the Preferences page, by the id of their heading. */
const PREFERENCE_SECTIONS: { id: string; title: string; keywords: string }[] = [
  {
    id: 'appearance-label',
    title: 'Terminal appearance',
    keywords: 'font size line height theme cursor scrollback copy paste',
  },
  { id: 'shortcuts-label', title: 'Keyboard shortcuts', keywords: 'keybindings keys bind record' },
  { id: 'motion-label', title: 'Motion and accessibility', keywords: 'reduce motion focus ring' },
  { id: 'notices-label', title: 'Notifications', keywords: 'notices toasts routine' },
  { id: 'about-label', title: 'About', keywords: 'version licence license' },
];

/** The section a command belongs to in the palette (default: its own group). */
const SECTION: Partial<Record<CommandId, string>> = {
  'host.new': 'Create',
  'host.import': 'Create',
  'folder.new': 'Create',
  'identity.new': 'Create',
  'tab.close': 'Session',
  'tab.next': 'Session',
  'tab.previous': 'Session',
  'tab.duplicate': 'Session',
  'tab.detach': 'Session',
  'pane.splitRight': 'Session',
  'pane.splitDown': 'Session',
  'pane.close': 'Session',
  'pane.find': 'Session',
  'pane.next': 'Session',
  'pane.previous': 'Session',
};

/** What a command says to the palette when it is the one chosen. */
const VERB: Partial<Record<CommandId, string>> = {
  'host.search': 'Search',
  'tab.close': 'Close',
  'pane.close': 'Close',
};

/** The order of the sections when the user has not typed anything. */
const SECTION_ORDER = [
  'Live',
  'Hosts',
  'Go to',
  'Create',
  'Session',
  'Library',
  'Folders',
  'View',
  'Preferences',
  'Sync',
];

/**
 * Everything the palette can do, as entries: the running sessions, every
 * host (Enter connects or opens, Shift+Enter edits), every page, every action
 * of the app that exists, the library views and folders, the sections of
 * Preferences and the theme. The commands the current page cannot use are
 * left out (a split with no session open).
 */
export function buildPaletteEntries(deps: PaletteDeps): PaletteEntry[] {
  const overrides = prefs.prefs.keybindings;
  const chord = (id: CommandId): string => compactChord(acceleratorFor(id, overrides));
  const entries: PaletteEntry[] = [];
  const add = (entry: PaletteEntry): void => void entries.push(entry);

  // --- Live sessions -------------------------------------------------------
  for (const tab of sessions.tabs) {
    const live = tab.panes.some((pane) => pane.status.state === 'ready');
    add({
      id: `live:${tab.id}`,
      title: `Go to ${tab.title}`,
      group: 'Live',
      detail: `${tab.panes.length} ${tab.panes.length === 1 ? 'pane' : 'panes'} · ${live ? 'connected' : 'not connected'}`,
      verb: 'Focus',
      run: () => {
        sessions.setActive(tab.id);
        route.go('sessions');
      },
    });
  }

  // --- Hosts ---------------------------------------------------------------
  const liveHostIds = new Set(
    sessions.tabs.flatMap((tab) =>
      tab.panes
        .filter(
          (pane) => pane.host && pane.status.state !== 'closed' && pane.status.state !== 'failed',
        )
        .map((pane) => pane.host?.id ?? ''),
    ),
  );
  const folderLabels = new Map(hosts.folderOptions.map((option) => [option.id, option.label]));
  const tagNames = new Map(hosts.tags.map((tag) => [tag.id, tag.name]));
  hosts.sortedAll.forEach((host, index) => {
    const running = liveHostIds.has(host.id);
    const port = host.port === 22 ? '' : `:${host.port}`;
    add({
      id: `host:${host.id}`,
      title: running ? `Open ${host.name}` : `Connect to ${host.name}`,
      group: 'Hosts',
      detail: `${host.username}@${host.address}${port}${running ? ' · live' : ''}`,
      keywords: [
        host.group_id ? (folderLabels.get(host.group_id) ?? '') : '',
        ...host.tags.map((id) => tagNames.get(id) ?? ''),
      ].join(' '),
      verb: running ? 'Open' : 'Connect',
      searchOnly: index >= START_HOSTS,
      run: () => (running ? deps.openRunning(host) : deps.connect(host)),
      alt: { verb: 'Edit', run: () => route.edit(host) },
    });
  });
  add(commandEntry('host.search', 'Hosts'));

  // --- Pages ---------------------------------------------------------------
  const pages: [CommandId, CommandId | null][] = [
    ['route.hosts', null],
    ['route.sessions', null],
    ['route.folders', null],
    ['route.identities', null],
    ['route.sync', 'sync.open'],
    ['route.preferences', 'settings.open'],
  ];
  for (const [id, shortcutOf] of pages) add(commandEntry(id, 'Go to', shortcutOf ?? undefined));

  // --- Create --------------------------------------------------------------
  for (const id of ['host.new', 'host.import', 'folder.new', 'identity.new'] as const) {
    add(commandEntry(id));
  }

  // --- The session you are looking at -------------------------------------
  const tab = sessions.active;
  const pane = sessions.activePane;
  if (tab && pane) {
    const connected = pane.status.state === 'ready' && pane.sessionId !== null;
    const full = tab.panes.length >= 16;
    if (!full) {
      add(commandEntry('pane.splitRight'));
      add(commandEntry('pane.splitDown'));
    }
    if (pane.host) add(commandEntry('pane.find'));
    if (connected) {
      add(commandEntry('tab.duplicate'));
      add(commandEntry('tab.detach'));
    }
    if (tab.panes.length > 1) {
      add(commandEntry('pane.next'));
      add(commandEntry('pane.previous'));
      add(commandEntry('pane.close'));
    }
    if (sessions.tabs.length > 1) {
      add(commandEntry('tab.next'));
      add(commandEntry('tab.previous'));
    }
    add(commandEntry('tab.close'));
  }

  // --- Library and folders -------------------------------------------------
  const views: { id: string; title: string; show: () => void }[] = [
    { id: 'all', title: 'Show all hosts', show: () => hosts.showLibrary('all') },
    { id: 'favorites', title: 'Show favorites', show: () => hosts.showLibrary('favorites') },
    { id: 'recent', title: 'Show recent hosts', show: () => hosts.showLibrary('recent') },
    {
      id: 'unfiled',
      title: 'Show hosts without a folder',
      show: () => hosts.showLibrary('unfiled'),
    },
  ];
  for (const view of views) {
    add({
      id: `library:${view.id}`,
      title: view.title,
      group: 'Library',
      verb: 'Show',
      run: () => {
        view.show();
        route.go('hosts');
      },
    });
  }
  hosts.folderTree.nodes.forEach((node, index) => {
    add({
      id: `folder:${node.group.id}`,
      title: `Show hosts in ${node.path}`,
      group: 'Folders',
      detail: `${node.count} ${node.count === 1 ? 'host' : 'hosts'}`,
      keywords: 'folder',
      verb: 'Show',
      searchOnly: index >= START_FOLDERS,
      run: () => {
        hosts.showFolder(node.group.id);
        route.go('hosts');
      },
    });
  });

  // --- View ----------------------------------------------------------------
  add(commandEntry('view.toggleTheme'));
  for (const [theme, label] of [
    ['dark', 'Dark'],
    ['light', 'Light'],
    ['system', 'Follow the system'],
  ] as const) {
    add({
      id: `theme:${theme}`,
      title: `Theme: ${label}`,
      group: 'View',
      keywords: 'appearance colour color',
      verb: 'Use',
      run: () => deps.setTheme(theme),
    });
  }
  add(commandEntry('view.increaseFont'));
  add(commandEntry('view.decreaseFont'));
  add(commandEntry('view.toggleSidebar'));
  if (route.route === 'hosts') add(commandEntry('view.toggleInspector'));

  // --- Preferences ---------------------------------------------------------
  for (const section of PREFERENCE_SECTIONS) {
    add({
      id: `preferences:${section.id}`,
      title: `Preferences: ${section.title}`,
      group: 'Preferences',
      keywords: section.keywords,
      verb: 'Open',
      run: () => {
        intents.request({ kind: 'preferences-section', id: section.id });
        route.go('preferences');
      },
    });
  }

  // --- Sync ----------------------------------------------------------------
  if (sync.signedIn) add(commandEntry('sync.now'));
  else {
    add({
      id: 'sync:sign-in',
      title: 'Sign in to sync',
      group: 'Sync',
      keywords: 'cloud account',
      verb: 'Open',
      run: () => route.go('sync'),
    });
  }

  return arrange(entries);

  /** A command of the registry as an entry. */
  function commandEntry(id: CommandId, group?: string, shortcutOf: CommandId = id): PaletteEntry {
    const command = COMMANDS.find((candidate) => candidate.id === id);
    const accelerator = acceleratorFor(shortcutOf, overrides);
    return {
      id,
      title: command?.title ?? id,
      group: group ?? SECTION[id] ?? command?.group ?? 'General',
      accelerator: accelerator ? chord(shortcutOf) : undefined,
      keywords: expandAccelerator(accelerator),
      verb: VERB[id] ?? (id.startsWith('route.') ? 'Open' : 'Run'),
      run: () => deps.run(id),
    };
  }
}

/** The entries in the order of the sections (a stable sort: each keeps its place in its section). */
function arrange(entries: PaletteEntry[]): PaletteEntry[] {
  const rank = (group: string): number => {
    const index = SECTION_ORDER.indexOf(group);
    return index === -1 ? SECTION_ORDER.length : index;
  };
  return entries
    .map((entry, index) => ({ entry, index }))
    .sort((a, b) => rank(a.entry.group) - rank(b.entry.group) || a.index - b.index)
    .map(({ entry }) => entry);
}
