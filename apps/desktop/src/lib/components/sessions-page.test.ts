// @vitest-environment jsdom
/**
 * The Sessions page (frames 2 and 3): the terminal context menu, find in the
 * terminal, the multi-line paste question, the question before a live
 * session closes, and what the pane header, footer and toolbar say in each
 * state.
 */

import { flushSync, mount, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import SessionsPage from '../pages/SessionsPage.svelte';
import { acceleratorFor, compactChord } from '../keybindings';
import { hosts } from '../stores/hosts.svelte';
import { defaultPrefs, prefs } from '../stores/prefs.svelte';
import { route } from '../stores/route.svelte';
import { sessions, type Pane, type Tab } from '../stores/sessions.svelte';
import { terminalUi } from '../stores/terminal-ui.svelte';
import { notices } from '../notify.svelte';
import { pasteGate } from '../terminal/paste.svelte';
import { getTerminal, pruneTerminals } from '../terminal/registry';
import { click, findButton, newTarget, press, stubBrowserApis, type } from '../test-support/dom';
import { answers, callsOf, resetTauriMock } from '../test-support/tauri-core-mock';
import { resetXtermMock, searchAddons, terminals } from '../test-support/xterm-mock';
import type { Host, SessionStatus } from '../types';

vi.mock('@tauri-apps/api/core', () => import('../test-support/tauri-core-mock'));
vi.mock('@tauri-apps/api/event', () => ({ listen: async () => () => {} }));
vi.mock('@xterm/xterm', () => import('../test-support/xterm-mock'));
vi.mock('@xterm/addon-fit', () => import('../test-support/xterm-mock'));
vi.mock('@xterm/addon-webgl', () => import('../test-support/xterm-mock'));
vi.mock('@xterm/addon-search', () => import('../test-support/xterm-mock'));
vi.mock('@xterm/addon-web-links', () => import('../test-support/xterm-mock'));

function host(name: string, extra: Partial<Host> = {}): Host {
  const now = new Date().toISOString();
  return {
    id: `host-${name}`,
    name,
    address: `${name}.test`,
    port: 22,
    username: 'deploy',
    auth: { type: 'agent' },
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
    ...extra,
  };
}

let target: HTMLElement;
let component: ReturnType<typeof mount> | null = null;
let written: string[];
let clipboardText = '';

beforeEach(() => {
  resetTauriMock();
  // Every connection the terminals start gets this session id.
  answers.set('session_open', 's1');
  resetXtermMock();
  pruneTerminals(new Set());
  stubBrowserApis();
  sessions.tabs = [];
  sessions.activeId = null;
  sessions.layoutLoaded = true;
  sessions.closeRequest = null;
  hosts.hosts = [];
  hosts.loaded = true;
  notices.reset();
  route.route = 'sessions';
  terminalUi.menu = null;
  pasteGate.pending = null;
  prefs.prefs = defaultPrefs();
  prefs.prefs = { ...prefs.prefs, keybindings: {} };
  written = [];
  clipboardText = 'ls -la';
  Object.defineProperty(navigator, 'clipboard', {
    configurable: true,
    value: {
      writeText: vi.fn(async (text: string) => void written.push(text)),
      readText: vi.fn(async () => clipboardText),
    },
  });
  target = newTarget();
});

afterEach(() => {
  if (component) unmount(component);
  component = null;
  target.remove();
  document.body.replaceChildren();
  vi.useRealTimers();
});

function mountPage(): void {
  component = mount(SessionsPage, { target });
  flushSync();
}

function ready(pane: Pane, sessionId = 's1'): void {
  pane.sessionId = sessionId;
  sessions.applyStatus(pane, { state: 'ready' });
  flushSync();
}

function openTab(name = 'web-1', status: SessionStatus | null = { state: 'ready' }) {
  const tab = sessions.open(host(name), {});
  const stored = sessions.tabs.find((entry) => entry.id === tab.id) as Tab;
  const pane = sessions.activePaneOf(stored) as Pane;
  if (status?.state === 'ready') ready(pane);
  else if (status) sessions.applyStatus(pane, status);
  flushSync();
  return { tab: stored, pane };
}

/** The pane's xterm fake and its element. */
function terminalOf(pane: Pane) {
  const handle = getTerminal(pane.id);
  if (!handle) throw new Error('no terminal');
  return { handle, term: handle.term as unknown as (typeof terminals)[number] };
}

function rightClick(pane: Pane, x = 120, y = 90): void {
  const { handle } = terminalOf(pane);
  (handle.host as HTMLElement).dispatchEvent(
    new MouseEvent('contextmenu', {
      bubbles: true,
      cancelable: true,
      clientX: x,
      clientY: y,
      button: 2,
    }),
  );
  flushSync();
}

const menu = () => document.querySelector<HTMLElement>('[role="menu"]');
const items = () =>
  Array.from(document.querySelectorAll<HTMLElement>('[role="menuitem"]')).map(
    (item) => item.querySelector('.text')?.textContent?.trim() ?? '',
  );
const item = (label: string) =>
  Array.from(document.querySelectorAll<HTMLElement>('[role="menuitem"]')).find(
    (entry) => entry.querySelector('.text')?.textContent?.trim() === label,
  ) as HTMLElement;

describe('the terminal context menu', () => {
  it('opens on a right click with copy, paste, select all, clear, find and the splits', () => {
    const { pane } = openTab();
    mountPage();
    expect(menu()).toBeNull();
    rightClick(pane);
    expect(menu()).not.toBeNull();
    expect(menu()?.getAttribute('aria-label')).toBe('Terminal');
    expect(items()).toEqual([
      'Copy',
      'Paste',
      'Select all',
      'Clear scrollback',
      'Find…',
      'Split right',
      'Split down',
    ]);
  });

  it('keeps the browser menu away', () => {
    const { pane } = openTab();
    mountPage();
    const { handle } = terminalOf(pane);
    const event = new MouseEvent('contextmenu', { bubbles: true, cancelable: true, button: 2 });
    (handle.host as HTMLElement).dispatchEvent(event);
    expect(event.defaultPrevented).toBe(true);
  });

  it('disables Copy with a reason until something is selected', () => {
    const { pane } = openTab();
    mountPage();
    rightClick(pane);
    expect(item('Copy').getAttribute('aria-disabled')).toBe('true');
    expect(item('Copy').title).toBe('Select some text first');

    // A disabled entry does nothing and leaves the menu open.
    click(item('Copy'));
    expect(written).toEqual([]);
    expect(menu()).not.toBeNull();
  });

  it('copies the selection', async () => {
    const { pane } = openTab();
    mountPage();
    const { term } = terminalOf(pane);
    term.selection = 'ops@atlas';
    rightClick(pane);
    expect(item('Copy').getAttribute('aria-disabled')).toBeNull();
    click(item('Copy'));
    await vi.waitFor(() => expect(written).toEqual(['ops@atlas']));
  });

  it('pastes one line from the clipboard at once', async () => {
    const { pane } = openTab();
    mountPage();
    rightClick(pane);
    click(item('Paste'));
    await vi.waitFor(() => expect(terminalOf(pane).term.pasted).toEqual(['ls -la']));
  });

  it('asks before it pastes several lines', async () => {
    clipboardText = 'a\nb\nc';
    const { pane } = openTab();
    mountPage();
    rightClick(pane);
    click(item('Paste'));
    await vi.waitFor(() => expect(pasteGate.pending).not.toBeNull());
    expect(terminalOf(pane).term.pasted).toEqual([]);
  });

  it('says so when the clipboard cannot be read', async () => {
    (navigator.clipboard.readText as ReturnType<typeof vi.fn>).mockRejectedValueOnce(
      new Error('denied'),
    );
    const { pane } = openTab();
    mountPage();
    rightClick(pane);
    click(item('Paste'));
    await vi.waitFor(() => expect(notices.visible.map((toast) => toast.kind)).toEqual(['error']));
  });

  it('selects all and clears the scrollback', () => {
    const { pane } = openTab();
    mountPage();
    const { term } = terminalOf(pane);
    rightClick(pane);
    click(item('Select all'));
    expect(term.selectAlls).toBe(1);
    rightClick(pane);
    click(item('Clear scrollback'));
    expect(term.clears).toBe(1);
  });

  it('opens the find bar', () => {
    const { pane } = openTab();
    mountPage();
    rightClick(pane);
    click(item('Find…'));
    expect(terminalUi.isSearchOpen(pane.id)).toBe(true);
    expect(document.querySelector('[role="search"]')).not.toBeNull();
  });

  it('splits the pane to the right and below', async () => {
    const { tab, pane } = openTab();
    mountPage();
    rightClick(pane);
    click(item('Split right'));
    expect(tab.panes).toHaveLength(2);
    expect(tab.root.kind === 'split' && tab.root.direction).toBe('columns');

    // The new pane is empty (a picker): split the first one again.
    await vi.waitFor(() => expect(menu()).toBeNull());
    rightClick(pane);
    click(item('Split down'));
    expect(tab.panes).toHaveLength(3);
  });

  it('closes after a choice, and the terminal gets the keyboard back', async () => {
    const { pane } = openTab();
    mountPage();
    const { term } = terminalOf(pane);
    const before = term.focuses;
    rightClick(pane);
    click(item('Select all'));
    expect(terminalUi.menu).toBeNull();
    // The menu plays its short exit before it leaves the page.
    await vi.waitFor(() => expect(menu()).toBeNull());
    expect(term.focuses).toBeGreaterThan(before);
  });

  it('offers to open and copy a link that is under the pointer', async () => {
    const { pane } = openTab();
    mountPage();
    terminalOf(pane).handle.hoverLink = 'https://example.com/docs';
    rightClick(pane);
    expect(items().slice(0, 2)).toEqual(['Open link', 'Copy link address']);

    click(item('Copy link address'));
    await vi.waitFor(() => expect(written).toEqual(['https://example.com/docs']));
  });

  it('does not offer link entries away from a link', () => {
    const { pane } = openTab();
    mountPage();
    rightClick(pane);
    expect(items()).not.toContain('Open link');
  });

  it('is a keyboard menu: arrows move, Escape closes and gives the focus back', async () => {
    const { pane } = openTab();
    mountPage();
    const opener = document.createElement('button');
    document.body.appendChild(opener);
    opener.focus();

    // A key-opened menu (not a right click) highlights the first available entry.
    const { handle } = terminalOf(pane);
    (handle.host as HTMLElement).dispatchEvent(
      new MouseEvent('contextmenu', { bubbles: true, cancelable: true, button: 0 }),
    );
    flushSync();
    await Promise.resolve();
    await Promise.resolve();
    // Copy is disabled: the first entry that can be chosen is Paste.
    expect(document.activeElement).toBe(item('Paste'));

    press(menu() as HTMLElement, 'ArrowDown');
    expect(document.activeElement).toBe(item('Select all'));
    press(menu() as HTMLElement, 'ArrowUp');
    expect(document.activeElement).toBe(item('Paste'));
    press(menu() as HTMLElement, 'End');
    expect(document.activeElement).toBe(item('Split down'));
    press(menu() as HTMLElement, 'ArrowDown');
    expect(document.activeElement).toBe(item('Copy'));

    window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    flushSync();
    expect(terminalUi.menu).toBeNull();
    await vi.waitFor(() => expect(menu()).toBeNull());
    expect(document.activeElement).toBe(opener);
    opener.remove();
  });

  it('closes on a click outside', () => {
    const { pane } = openTab();
    mountPage();
    rightClick(pane);
    document.body.dispatchEvent(new Event('pointerdown', { bubbles: true }));
    flushSync();
    expect(terminalUi.menu).toBeNull();
  });
});

describe('find in the terminal', () => {
  function pressFind(pane: Pane, init: KeyboardEventInit = { ctrlKey: true, shiftKey: true }) {
    const { handle } = terminalOf(pane);
    const event = new KeyboardEvent('keydown', {
      key: 'F',
      bubbles: true,
      cancelable: true,
      ...init,
    });
    handle.element.dispatchEvent(event);
    flushSync();
    return event;
  }

  const bar = () => document.querySelector<HTMLElement>('[role="search"]');
  const field = () => bar()?.querySelector('input') as HTMLInputElement;

  it('opens on the find shortcut and keeps the shortcut from the rest of the app', () => {
    const { pane } = openTab();
    mountPage();
    const reachedWindow = vi.fn();
    window.addEventListener('keydown', reachedWindow);
    const event = pressFind(pane);
    window.removeEventListener('keydown', reachedWindow);

    expect(bar()).not.toBeNull();
    expect(event.defaultPrevented).toBe(true);
    // Ctrl+Shift+F also means "search hosts" in the app: not in a terminal.
    expect(reachedWindow).not.toHaveBeenCalled();
    expect(document.activeElement).toBe(field());
  });

  it('ignores other chords', () => {
    const { pane } = openTab();
    mountPage();
    pressFind(pane, { ctrlKey: true });
    pressFind(pane, { shiftKey: true });
    expect(bar()).toBeNull();
  });

  it('searches as you type, then steps with Enter and Shift+Enter', () => {
    const { pane } = openTab();
    mountPage();
    pressFind(pane);
    type(field(), 'error');
    const addon = searchAddons[0];
    expect(addon.searches.at(-1)).toMatchObject({ term: 'error', direction: 'next' });
    expect(addon.searches.at(-1)?.options).toMatchObject({ caseSensitive: false });

    press(field(), 'Enter');
    expect(addon.searches.at(-1)?.direction).toBe('next');
    press(field(), 'Enter', { shiftKey: true });
    expect(addon.searches.at(-1)?.direction).toBe('previous');
  });

  it('shows where the active match is, and when there is none', () => {
    const { pane } = openTab();
    mountPage();
    pressFind(pane);
    type(field(), 'ok');
    searchAddons[0].report(1, 4);
    flushSync();
    expect(bar()?.textContent).toContain('2 of 4');

    searchAddons[0].report(-1, 1000);
    flushSync();
    expect(bar()?.textContent).toContain('1000+');

    searchAddons[0].report(-1, 0);
    flushSync();
    expect(bar()?.textContent).toContain('No results');
  });

  it('matches case when asked', () => {
    const { pane } = openTab();
    mountPage();
    pressFind(pane);
    type(field(), 'Ok');
    const toggle = bar()?.querySelector('[aria-label="Match case"]') as HTMLElement;
    expect(toggle.getAttribute('aria-pressed')).toBe('false');
    click(toggle);
    expect(toggle.getAttribute('aria-pressed')).toBe('true');
    expect(searchAddons[0].searches.at(-1)?.options).toMatchObject({ caseSensitive: true });
  });

  it('clears the highlights for an empty search', () => {
    const { pane } = openTab();
    mountPage();
    pressFind(pane);
    type(field(), 'x');
    const before = searchAddons[0].clears;
    type(field(), '');
    expect(searchAddons[0].clears).toBeGreaterThan(before);
  });

  it('closes on Escape: highlights cleared, the terminal has the keyboard again', () => {
    const { pane } = openTab();
    mountPage();
    pressFind(pane);
    type(field(), 'ok');
    const { term } = terminalOf(pane);
    const focuses = term.focuses;
    const clears = searchAddons[0].clears;

    press(field(), 'Escape');
    expect(bar()).toBeNull();
    expect(terminalUi.isSearchOpen(pane.id)).toBe(false);
    expect(searchAddons[0].clears).toBeGreaterThan(clears);
    expect(term.focuses).toBeGreaterThan(focuses);
  });

  it('closes with its button, and the shortcut brings it back', () => {
    const { pane } = openTab();
    mountPage();
    pressFind(pane);
    type(field(), 'ok');
    click(bar()?.querySelector('[aria-label="Close find"]') as HTMLElement);
    expect(bar()).toBeNull();

    pressFind(pane);
    expect(bar()).not.toBeNull();
    expect(document.activeElement).toBe(field());
    // Pressing it again while the bar is open only takes the focus back.
    (document.activeElement as HTMLElement).blur();
    pressFind(pane);
    expect(document.querySelectorAll('[role="search"]')).toHaveLength(1);
    expect(document.activeElement).toBe(field());
  });

  it('has a find bar per pane', () => {
    const { tab, pane } = openTab();
    mountPage();
    sessions.splitActive('columns');
    flushSync();
    expect(tab.panes).toHaveLength(2);
    terminalUi.openSearch(pane.id);
    flushSync();
    expect(document.querySelectorAll('[role="search"]')).toHaveLength(1);
  });
});

describe('pasting into the terminal', () => {
  function nativePaste(pane: Pane, text: string): Event {
    const { handle } = terminalOf(pane);
    const event = new Event('paste', { bubbles: true, cancelable: true });
    Object.defineProperty(event, 'clipboardData', { value: { getData: () => text } });
    handle.element.dispatchEvent(event);
    flushSync();
    return event;
  }

  const dialog = () => document.querySelector<HTMLElement>('[role="dialog"]');

  it('sends one line straight through', () => {
    const { pane } = openTab();
    mountPage();
    const event = nativePaste(pane, 'uptime');
    expect(event.defaultPrevented).toBe(true);
    expect(terminalOf(pane).term.pasted).toEqual(['uptime']);
    expect(dialog()).toBeNull();
  });

  it('asks about several lines, naming the count and showing them', () => {
    const { pane } = openTab();
    mountPage();
    nativePaste(pane, Array.from({ length: 9 }, (_, i) => `cmd ${i + 1}`).join('\n'));
    expect(terminalOf(pane).term.pasted).toEqual([]);
    expect(dialog()?.textContent).toContain('Paste 9 lines?');
    expect(dialog()?.textContent).toContain('cmd 1');
    expect(dialog()?.textContent).toContain('and 3 more lines');
  });

  it('pastes when confirmed', () => {
    const { pane } = openTab();
    mountPage();
    nativePaste(pane, 'a\nb');
    click(findButton(dialog() as HTMLElement, 'Paste'));
    expect(terminalOf(pane).term.pasted).toEqual(['a\nb']);
  });

  it('puts the focus on Paste, and Escape cancels', async () => {
    const { pane } = openTab();
    mountPage();
    nativePaste(pane, 'a\nb');
    await Promise.resolve();
    expect(document.activeElement).toBe(findButton(dialog() as HTMLElement, 'Paste'));
    press(dialog() as HTMLElement, 'Escape');
    expect(terminalOf(pane).term.pasted).toEqual([]);
    expect(pasteGate.pending).toBeNull();
  });

  it('cancels with Cancel', () => {
    const { pane } = openTab();
    mountPage();
    nativePaste(pane, 'a\nb');
    click(findButton(dialog() as HTMLElement, 'Cancel'));
    expect(terminalOf(pane).term.pasted).toEqual([]);
  });

  it("turns the question off in Preferences after 'Don't ask again'", async () => {
    const { pane } = openTab();
    mountPage();
    nativePaste(pane, 'a\nb');
    expect(dialog()?.textContent).toContain('turn the question back on in Preferences');
    const box = dialog()?.querySelector('input[type="checkbox"]') as HTMLInputElement;
    box.checked = true;
    box.dispatchEvent(new Event('change', { bubbles: true }));
    flushSync();
    click(findButton(dialog() as HTMLElement, 'Paste'));
    nativePaste(pane, 'c\nd\ne');
    expect(terminalOf(pane).term.pasted).toEqual(['a\nb', 'c\nd\ne']);
    expect(prefs.prefs.terminal.confirm_multiline_paste).toBe(false);
    await vi.waitFor(() => expect(callsOf('prefs_set')).toHaveLength(1));
  });

  it('does not ask at all while Preferences has the question off', () => {
    prefs.prefs = {
      ...defaultPrefs(),
      terminal: { ...defaultPrefs().terminal, confirm_multiline_paste: false },
    };
    const { pane } = openTab();
    mountPage();
    nativePaste(pane, 'a\nb');
    expect(dialog()).toBeNull();
    expect(terminalOf(pane).term.pasted).toEqual(['a\nb']);
  });

  it('traps the focus in the dialog', () => {
    const { pane } = openTab();
    mountPage();
    nativePaste(pane, 'a\nb');
    const buttons = Array.from(dialog()?.querySelectorAll('button, input') ?? []);
    (buttons.at(-1) as HTMLElement).focus();
    press(buttons.at(-1) as HTMLElement, 'Tab');
    expect(dialog()?.contains(document.activeElement)).toBe(true);
  });
});

describe('closing a tab', () => {
  const alert = () => document.querySelector<HTMLElement>('[role="alertdialog"]');
  const tabClose = (title: string) =>
    document.querySelector<HTMLElement>(`[aria-label="Close ${title}"]`) as HTMLElement;

  it('asks first when the session is live, and keeps it on Cancel', () => {
    const { tab } = openTab('web-1');
    mountPage();
    click(tabClose('web-1'));
    expect(alert()?.textContent).toContain('Close “web-1”?');
    expect(alert()?.textContent).toContain('ends the SSH session');
    expect(callsOf('session_close')).toHaveLength(0);

    click(findButton(alert() as HTMLElement, 'Cancel'));
    expect(sessions.closeRequest).toBeNull();
    expect(sessions.tabs.map((entry) => entry.id)).toEqual([tab.id]);
    expect(callsOf('session_close')).toHaveLength(0);
  });

  it('closes the session when confirmed', async () => {
    openTab('web-1');
    mountPage();
    click(tabClose('web-1'));
    click(findButton(alert() as HTMLElement, 'Close session'));
    await vi.waitFor(() => expect(callsOf('session_close')).toHaveLength(1));
    expect(callsOf('session_close')[0].args).toEqual({ id: 's1' });
    await vi.waitFor(() => expect(sessions.tabs).toHaveLength(0));
  });

  it('starts with Cancel focused, so Enter does not close anything by accident', async () => {
    openTab('web-1');
    mountPage();
    click(tabClose('web-1'));
    await Promise.resolve();
    expect(document.activeElement).toBe(findButton(alert() as HTMLElement, 'Cancel'));
  });

  it('Escape cancels', () => {
    openTab('web-1');
    mountPage();
    click(tabClose('web-1'));
    press(alert() as HTMLElement, 'Escape');
    expect(sessions.closeRequest).toBeNull();
    expect(sessions.tabs).toHaveLength(1);
  });

  it('also asks while the session is still connecting', () => {
    openTab('web-1', { state: 'connecting' });
    mountPage();
    click(tabClose('web-1'));
    expect(alert()).not.toBeNull();
  });

  it('closes at once when nothing is live (failed, disconnected, restored)', async () => {
    openTab('web-1', { state: 'failed', category: 'network', message: 'down' });
    mountPage();
    click(tabClose('web-1'));
    expect(alert()).toBeNull();
    await vi.waitFor(() => expect(sessions.tabs).toHaveLength(0));
  });

  it('counts every live session of a split tab', () => {
    const { tab } = openTab('web-1');
    mountPage();
    sessions.splitActive('columns');
    const second = sessions.activePane as Pane;
    void sessions.connectPane(second, host('db-1'));
    flushSync();
    ready(second, 's2');
    click(tabClose('web-1'));
    expect(alert()?.textContent).toContain('its 2 SSH sessions');
    expect(findButton(alert() as HTMLElement, 'Close sessions')).toBeTruthy();
    expect(tab.panes).toHaveLength(2);
  });

  it('Close session in the More menu and the shortcut path ask too', () => {
    const { tab } = openTab('web-1');
    mountPage();
    click(document.querySelector('[role="toolbar"] [aria-label="More"]') as HTMLElement);
    click(item('Close session'));
    expect(sessions.closeRequest?.tabId).toBe(tab.id);
    sessions.cancelClose();
    flushSync();
    sessions.requestClose(tab);
    expect(sessions.closeRequest?.kind).toBe('tab');
  });

  it('asks before closing a live pane, and says the others keep running', () => {
    const { tab } = openTab('web-1');
    mountPage();
    sessions.splitActive('columns');
    flushSync();
    const first = tab.panes[0];
    click(
      document.querySelector(
        `[data-pane-id="${first.id}"] [aria-label="Close the pane"]`,
      ) as HTMLElement,
    );
    expect(alert()?.textContent).toContain('Close this pane?');
    expect(alert()?.textContent).toContain('The other panes keep running');
    click(findButton(alert() as HTMLElement, 'Cancel'));
    expect(tab.panes).toHaveLength(2);
  });

  it('closes an empty pane without asking', async () => {
    const { tab } = openTab('web-1');
    mountPage();
    const created = sessions.splitActive('columns') as Pane;
    flushSync();
    click(
      document.querySelector(
        `[data-pane-id="${created.id}"] [aria-label="Close the pane"]`,
      ) as HTMLElement,
    );
    expect(alert()).toBeNull();
    await vi.waitFor(() => expect(tab.panes).toHaveLength(1));
  });
});

describe('pane header and footer', () => {
  const pathOf = (pane: Pane) =>
    document.querySelector(`[data-pane-id="${pane.id}"] .path`)?.textContent?.trim();
  const footerOf = (pane: Pane) =>
    document
      .querySelector(`[data-pane-id="${pane.id}"] footer`)
      ?.textContent?.replace(/\s+/g, ' ')
      .trim();
  const headerOf = (pane: Pane) =>
    document.querySelector(`[data-pane-id="${pane.id}"] header`) as HTMLElement;
  const paneEl = (pane: Pane) =>
    document.querySelector(`[data-pane-id="${pane.id}"]`) as HTMLElement;

  it('alone in its tab: `host · title` and the grid in the header, no split controls', () => {
    const { pane } = openTab('atlas-prod');
    mountPage();
    terminalOf(pane).term.resize(132, 32);
    flushSync();
    expect(pathOf(pane)).toBe('atlas-prod · shell');
    expect(headerOf(pane).textContent).toContain('132 × 32');
    expect(headerOf(pane).querySelector('button')).toBeNull();
    // Nothing to tell it from: no focus mark.
    expect(paneEl(pane).classList.contains('focused')).toBe(false);
    expect(paneEl(pane).hasAttribute('aria-current')).toBe(false);
  });

  it('has a footer with the state, in the one-pane tab as well', () => {
    const { pane } = openTab('atlas-prod');
    mountPage();
    expect(footerOf(pane)).toBe('Ready · UTF-8');
  });

  it('says where the session is in the tooltip of the header: user@host:port and the folder', () => {
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
      {
        id: 'g2',
        name: 'Europe',
        parent_id: 'g1',
        sort_order: 0,
        created_at: '',
        updated_at: '',
        deleted: false,
      },
    ];
    const tab = sessions.open(
      host('atlas-prod', { group_id: 'g2', address: '192.0.2.10', username: 'ops' }),
      {},
    );
    const pane = sessions.activePaneOf(sessions.tabs.find((entry) => entry.id === tab.id) as Tab);
    mountPage();
    expect(
      document.querySelector(`[data-pane-id="${pane?.id}"] .path`)?.getAttribute('title'),
    ).toBe('ops@192.0.2.10:22 · Production / Europe');
    hosts.groups = [];
  });

  it('in a split: the focused pane is marked, and the footer carries the state', () => {
    const { tab, pane } = openTab('atlas-prod');
    mountPage();
    const second = sessions.splitActive('columns') as Pane;
    flushSync();
    void sessions.connectPane(second, host('atlas-prod'));
    flushSync();
    ready(second, 's2');
    sessions.focusPane(pane);
    flushSync();
    terminalOf(pane).term.resize(66, 24);
    flushSync();

    expect(tab.panes).toHaveLength(2);
    expect(pathOf(pane)).toBe('atlas-prod · shell');
    expect(pathOf(second)).toBe('atlas-prod · shell');
    expect(footerOf(pane)).toBe('Ready · UTF-8');
    expect(headerOf(pane).textContent).toContain('66 × 24');
    expect(paneEl(pane).classList.contains('focused')).toBe(true);
    expect(paneEl(pane).getAttribute('aria-current')).toBe('true');
    expect(paneEl(second).classList.contains('focused')).toBe(false);
    expect(paneEl(second).hasAttribute('aria-current')).toBe(false);
    // Expand and Close, with names.
    expect(headerOf(pane).querySelector('[aria-label="Expand the pane"]')).not.toBeNull();
    expect(headerOf(pane).querySelector('[aria-label="Close the pane"]')).not.toBeNull();
  });

  it('uses the title the remote shell set', () => {
    const { pane } = openTab('atlas-prod');
    mountPage();
    sessions.setPaneTitle(pane.id, 'ops@atlas: /srv/atlas\u0007');
    flushSync();
    expect(pathOf(pane)).toBe('atlas-prod · ops@atlas: /srv/atlas');
    sessions.setPaneTitle(pane.id, '');
    flushSync();
    expect(pathOf(pane)).toBe('atlas-prod · shell');
  });

  it('says why a pane takes no input, in sentence case', () => {
    const { pane } = openTab('atlas-prod');
    mountPage();
    sessions.splitActive('columns');
    flushSync();
    sessions.applyStatus(pane, {
      state: 'reconnecting',
      attempt: 2,
      delay_ms: 1000,
      last_error: 'reset',
    });
    flushSync();
    expect(footerOf(pane)).toContain('Reconnecting · attempt 2');

    sessions.applyStatus(pane, { state: 'failed', category: 'auth', message: 'denied' });
    flushSync();
    expect(footerOf(pane)).toContain('Connection failed · input off');
  });

  it('shows the forwards of the host in the footer, the first one and how many more', () => {
    const forwards = [
      {
        kind: 'local' as const,
        bind_address: '127.0.0.1',
        bind_port: 5432,
        target_host: 'db',
        target_port: 5432,
        enabled: true,
      },
      { kind: 'dynamic' as const, bind_port: 1080, enabled: true },
    ];
    const { pane } = openTab('atlas-prod');
    pane.host = host('atlas-prod', { forwards });
    mountPage();
    expect(footerOf(pane)).toBe('Ready · UTF-8 L 127.0.0.1:5432 → db:5432 +1');
  });

  it('offers Reconnect in the footer of a session that is over, and only then', () => {
    const { pane } = openTab('atlas-prod');
    mountPage();
    const again = () =>
      document.querySelector<HTMLButtonElement>(`[data-pane-id="${pane.id}"] footer button`);
    expect(again()).toBeNull();

    sessions.applyStatus(pane, { state: 'failed', category: 'network', message: 'down' });
    flushSync();
    expect(again()?.textContent?.trim()).toBe('Reconnect');
    expect(footerOf(pane)).toContain('Connection failed');
  });

  it('shows the pane picker for an empty pane', () => {
    openTab('atlas-prod');
    mountPage();
    const created = sessions.splitActive('columns') as Pane;
    flushSync();
    expect(pathOf(created)).toBe('empty pane');
    expect(footerOf(created)).toContain('No session');
    // An empty pane has nothing to expand.
    expect(headerOf(created).querySelector('[aria-label="Expand the pane"]')).toBeNull();
  });

  it('moves the focus to a pane that was clicked', () => {
    const { tab, pane } = openTab('atlas-prod');
    mountPage();
    const second = sessions.splitActive('columns') as Pane;
    flushSync();
    expect(tab.activePaneId).toBe(second.id);
    document
      .querySelector(`[data-pane-id="${pane.id}"]`)
      ?.dispatchEvent(new Event('pointerdown', { bubbles: true }));
    flushSync();
    expect(tab.activePaneId).toBe(pane.id);
  });

  it('expands one pane over the others and restores them', () => {
    const { tab, pane } = openTab('atlas-prod');
    mountPage();
    sessions.splitActive('columns');
    flushSync();
    click(
      document.querySelector(
        `[data-pane-id="${pane.id}"] [aria-label="Expand the pane"]`,
      ) as HTMLElement,
    );
    expect(tab.zoomedPaneId).toBe(pane.id);
    expect(document.querySelectorAll('section.pane')).toHaveLength(1);
    // The other terminal is not destroyed: the session goes on behind it.
    expect(terminals.every((term) => !term.disposed)).toBe(true);

    click(document.querySelector('[aria-label="Restore the panes"]') as HTMLElement);
    expect(tab.zoomedPaneId).toBeNull();
    expect(document.querySelectorAll('section.pane')).toHaveLength(2);
  });
});

describe('the tab strip and the tools share one row', () => {
  const topbar = () => document.querySelector('.topbar') as HTMLElement;

  it('holds the tabs, the new-session button and the toolbar of the open session', () => {
    openTab('web-1');
    openTab('web-2');
    mountPage();
    const bar = topbar();
    expect(bar.querySelectorAll('nav[aria-label="Open sessions"] li.tab')).toHaveLength(2);
    expect(bar.querySelector('nav [aria-label="New session"]')).not.toBeNull();
    expect(bar.querySelector('[role="toolbar"][aria-label="Session"]')).not.toBeNull();
    // One toolbar, whatever the number of tabs.
    expect(document.querySelectorAll('[role="toolbar"]')).toHaveLength(1);
  });

  it('says N panes after the name of a split tab, and the state of one that is not connected', () => {
    const { tab, pane } = openTab('web-1');
    mountPage();
    const label = () => document.querySelector('li.tab .label') as HTMLElement;
    expect(label().textContent).not.toContain('panes');
    // Connected is said to a screen reader only: the tab stays quiet.
    expect(label().querySelector('.state.quiet')?.textContent).toBe('Connected');
    sessions.splitActive('columns');
    flushSync();
    expect(tab.panes).toHaveLength(2);
    expect(label().textContent).toContain('2 panes');

    sessions.focusPane(pane);
    sessions.applyStatus(pane, { state: 'failed', category: 'network', message: 'down' });
    flushSync();
    expect(label().querySelector('.state')?.classList.contains('quiet')).toBe(false);
    expect(label().textContent).toContain('Error');
  });

  it('has neither a split hint banner nor a bar at the foot', () => {
    openTab('web-1');
    mountPage();
    sessions.splitActive('columns');
    flushSync();
    expect(document.querySelector('[aria-label="Split view hint"]')).toBeNull();
    expect(document.querySelector('.bar')).toBeNull();
    expect(document.querySelector('[aria-label="Session actions"]')).toBeNull();
  });
});

describe('the toolbar', () => {
  const button = (label: string) =>
    document.querySelector<HTMLButtonElement>(
      `[role="toolbar"] [aria-label^="${label}"]`,
    ) as HTMLButtonElement;

  it('has the tools of the session: split right and down, duplicate, detach, more', () => {
    openTab('web-1');
    mountPage();
    const names = Array.from(
      document.querySelectorAll<HTMLElement>('[role="toolbar"] > button'),
    ).map((tool) => tool.getAttribute('aria-label'));
    expect(names).toEqual([
      'Split right, new pane to the right',
      'Split down, new pane below',
      'Duplicate',
      'Detach',
      'More',
    ]);
    // No file browser exists yet: no button pretends to be one.
    expect(document.querySelector('[role="toolbar"] [aria-label*="Files"]')).toBeNull();
  });

  it('Split down puts a pane below, Split right to the right', () => {
    const { tab } = openTab('web-1');
    mountPage();
    click(button('Split right'));
    expect(tab.root.kind === 'split' && tab.root.direction).toBe('columns');
    click(button('Split down'));
    expect(tab.panes).toHaveLength(3);
  });

  it('stops splitting at sixteen panes, and says why', () => {
    const { tab } = openTab('web-1');
    mountPage();
    for (let i = 0; i < 15; i += 1) sessions.splitActive('columns');
    flushSync();
    expect(tab.panes).toHaveLength(16);
    expect(button('Split right').getAttribute('aria-disabled')).toBe('true');
    expect(button('Split right').title).toBe('A tab holds at most 16 panes');
  });

  it('the More menu: entries, reasons, keyboard, and the focus coming back', async () => {
    openTab('web-1');
    mountPage();
    const more = button('More');
    more.focus();
    click(more);
    await Promise.resolve();
    await Promise.resolve();
    expect(menu()?.getAttribute('aria-label')).toBe('More session actions');
    expect(items()).toEqual([
      'Reconnect',
      'Find in terminal',
      'Clear scrollback',
      'Copy SSH command',
      'Close session',
    ]);
    // Connected: nothing to reconnect.
    expect(item('Reconnect').getAttribute('aria-disabled')).toBe('true');
    expect(item('Reconnect').title).toBe('The session is connected');
    expect(more.getAttribute('aria-expanded')).toBe('true');
    // Opened by a key: the first entry that can be chosen has the focus.
    expect(document.activeElement).toBe(item('Find in terminal'));

    window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    flushSync();
    await vi.waitFor(() => expect(menu()).toBeNull());
    expect(document.activeElement).toBe(more);
  });

  it('the More menu reconnects a session that is over, and waits for one that is connecting', () => {
    const { pane } = openTab('web-1');
    mountPage();
    sessions.applyStatus(pane, { state: 'connecting' });
    flushSync();
    click(button('More'));
    expect(item('Reconnect').getAttribute('aria-disabled')).toBe('true');
    expect(item('Reconnect').title).toBe('The session is still connecting');
    window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    flushSync();

    sessions.applyStatus(pane, { state: 'failed', category: 'network', message: 'down' });
    flushSync();
    click(button('More'));
    expect(item('Reconnect').getAttribute('aria-disabled')).toBeNull();
  });

  it('the More menu copies the SSH command and asks before closing', async () => {
    const { tab } = openTab('web-1');
    mountPage();
    click(button('More'));
    click(item('Copy SSH command'));
    await vi.waitFor(() => expect(written).toEqual(['ssh deploy@web-1.test']));

    click(button('More'));
    click(item('Close session'));
    expect(sessions.closeRequest?.tabId).toBe(tab.id);
  });

  it('the More menu lists Close pane in a split', () => {
    openTab('web-1');
    mountPage();
    sessions.splitActive('columns');
    flushSync();
    click(button('More'));
    expect(items()).toContain('Close pane');
  });

  it('the More menu can review synced settings that are not approved', () => {
    const { pane } = openTab('web-1');
    mountPage();
    sessions.applyUpdate(pane, { kind: 'exec_pending' });
    flushSync();
    // The chip and the menu entry both reach the review.
    expect(
      Array.from(document.querySelectorAll('[role="toolbar"] button')).some((b) =>
        b.textContent?.includes('Synced settings off'),
      ),
    ).toBe(true);
    click(button('More'));
    expect(items()).toContain('Review synced settings…');
  });
});

describe('leaving the page', () => {
  it('leaves no menu or find bar waiting for the way back', () => {
    const { pane } = openTab();
    mountPage();
    rightClick(pane);
    terminalUi.openSearch(pane.id);
    flushSync();
    expect(terminalUi.menu).not.toBeNull();

    unmount(component as ReturnType<typeof mount>);
    component = null;
    expect(terminalUi.menu).toBeNull();
    expect(terminalUi.isSearchOpen(pane.id)).toBe(false);
    expect(searchAddons[0].clears).toBeGreaterThan(0);
    // The terminal itself stays: the session goes on.
    expect(getTerminal(pane.id)).toBeDefined();
  });
});

describe('the page without sessions', () => {
  it('offers a new session, the palette and the Hosts page', () => {
    mountPage();
    expect(document.body.textContent).toContain('No open sessions');
    // The chord is the one the palette is bound to (Ctrl Shift P on a PC, as the sidebar says).
    expect(document.querySelector('.empty kbd')?.textContent).toBe(
      compactChord(acceleratorFor('palette.open', {})),
    );
    expect(findButton(document.body, 'New session')).toBeTruthy();
    // No toolbar without a session to act on.
    expect(document.querySelector('[role="toolbar"]')).toBeNull();
    click(findButton(document.body, 'Open Hosts'));
    expect(route.route).toBe('hosts');
  });
});
