// @vitest-environment jsdom
/**
 * The forwards of a session in the toolbar: a compact `2 forwards` chip that
 * opens a menu with one line per forward and what became of it, fed by the
 * `forward` updates of the session (and `SessionInfo.forwards` on attach).
 */

import { flushSync, mount, tick, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import SessionToolbar from './SessionToolbar.svelte';
import { execApproval } from '../stores/exec-approval.svelte';
import { hosts } from '../stores/hosts.svelte';
import { sessions, type Pane, type Tab } from '../stores/sessions.svelte';
import { notices } from '../notify.svelte';
import { click, newTarget, press, stubBrowserApis } from '../test-support/dom';
import { noticeText } from '../test-support/notices';
import { answers, callsOf, resetTauriMock } from '../test-support/tauri-core-mock';
import { resetXtermMock } from '../test-support/xterm-mock';
import { pruneTerminals } from '../terminal/registry';
import type { ForwardUpdate, Host } from '../types';

vi.mock('@tauri-apps/api/core', () => import('../test-support/tauri-core-mock'));
vi.mock('@tauri-apps/api/event', () => ({ listen: async () => () => {} }));
vi.mock('@xterm/xterm', () => import('../test-support/xterm-mock'));
vi.mock('@xterm/addon-fit', () => import('../test-support/xterm-mock'));
vi.mock('@xterm/addon-webgl', () => import('../test-support/xterm-mock'));
vi.mock('@xterm/addon-search', () => import('../test-support/xterm-mock'));
vi.mock('@xterm/addon-web-links', () => import('../test-support/xterm-mock'));

const ATLAS: Host = {
  id: 'atlas',
  name: 'atlas-prod',
  address: 'atlas.test',
  port: 22,
  username: 'ops',
  auth: { type: 'agent' },
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
  forwards: [
    {
      kind: 'local',
      bind_address: '127.0.0.1',
      bind_port: 8080,
      target_host: 'db',
      target_port: 5432,
      enabled: true,
    },
    { kind: 'dynamic', bind_port: 1080, enabled: true },
    {
      kind: 'local',
      bind_address: '0.0.0.0',
      bind_port: 9090,
      target_host: 'metrics',
      target_port: 9090,
      enabled: true,
    },
    { kind: 'dynamic', bind_port: 1081, enabled: false },
  ],
};

const LISTENING_0: ForwardUpdate = {
  state: 'listening',
  id: 0,
  kind: 'local',
  address: '127.0.0.1',
  port: 8080,
};
const LISTENING_1: ForwardUpdate = {
  state: 'listening',
  id: 1,
  kind: 'dynamic',
  address: '127.0.0.1',
  port: 1080,
};
let target: HTMLElement;
let component: ReturnType<typeof mount> | null = null;
let tab: Tab;
let pane: Pane;

function mountBar(): void {
  component = mount(SessionToolbar, { target, props: { tab } });
  flushSync();
}

const chip = () =>
  target.querySelector<HTMLButtonElement>('button[title="Port forwards of this session"]');
const menuItems = () =>
  Array.from(document.body.querySelectorAll<HTMLElement>('[role="menu"] [role="menuitem"]'));

beforeEach(() => {
  resetTauriMock();
  resetXtermMock();
  pruneTerminals(new Set());
  stubBrowserApis();
  sessions.tabs = [];
  sessions.activeId = null;
  sessions.layoutLoaded = true;
  hosts.hosts = [ATLAS];
  hosts.identities = [];
  notices.reset();
  execApproval.close();
  target = newTarget();
  const opened = sessions.open(ATLAS, {});
  tab = sessions.tabs.find((entry) => entry.id === opened.id) as Tab;
  pane = sessions.activePaneOf(tab) as Pane;
  pane.sessionId = 's1';
  sessions.applyStatus(pane, { state: 'ready' });
});

afterEach(() => {
  if (component) unmount(component);
  component = null;
  target.remove();
  document.body.replaceChildren();
});

describe('the forwards chip', () => {
  it('is not there for a host without forwards', () => {
    sessions.tabs = [];
    const plain = sessions.open({ ...ATLAS, forwards: undefined }, {});
    tab = sessions.tabs.find((entry) => entry.id === plain.id) as Tab;
    mountBar();
    expect(chip()).toBeNull();
  });

  it('counts the enabled forwards and is a menu button', () => {
    mountBar();
    expect(chip()?.textContent?.replace(/\s+/g, ' ').trim()).toContain('3 forwards');
    expect(chip()?.getAttribute('aria-haspopup')).toBe('menu');
    expect(chip()?.getAttribute('aria-expanded')).toBe('false');
  });

  it('says one forward in the singular', () => {
    sessions.tabs = [];
    const one = sessions.open({ ...ATLAS, forwards: [ATLAS.forwards?.[1] as never] }, {});
    tab = sessions.tabs.find((entry) => entry.id === one.id) as Tab;
    mountBar();
    expect(chip()?.textContent).toContain('1 forward');
    expect(chip()?.textContent).not.toContain('1 forwards');
  });

  it('lists each forward with its state, as the engine reports them', async () => {
    sessions.applyUpdate(pane, { kind: 'forward', forward: LISTENING_0 });
    sessions.applyUpdate(pane, { kind: 'forward', forward: LISTENING_1 });
    sessions.applyUpdate(pane, {
      kind: 'forward',
      forward: { state: 'failed', id: 2, kind: 'local', reason: 'address already in use' },
    });
    mountBar();
    expect(chip()?.textContent).toContain('1 of 3 failed');
    click(chip() as HTMLElement);
    await tick();
    const items = menuItems();
    expect(items).toHaveLength(3);
    const lines = items.map((item) => item.textContent?.replace(/\s+/g, ' ').trim());
    expect(lines[0]).toBe('L 127.0.0.1:8080 → db:5432 listening');
    expect(lines[1]).toBe('D 127.0.0.1:1080 · SOCKS listening');
    expect(lines[2]).toContain('L 0.0.0.0:9090 → metrics:9090');
    expect(lines[2]).toContain('failed: address already in use');
    // A failed forward is read, not chosen.
    expect(items[2].getAttribute('aria-disabled')).toBe('true');
    expect(items[2].getAttribute('title')).toBe('failed: address already in use');
    expect(items[0].getAttribute('aria-disabled')).toBeNull();
  });

  it('shows a forward that has not reported yet as starting', async () => {
    sessions.applyUpdate(pane, { kind: 'forward', forward: LISTENING_0 });
    mountBar();
    click(chip() as HTMLElement);
    await tick();
    const lines = menuItems().map((item) => item.textContent?.replace(/\s+/g, ' ').trim());
    expect(lines[1]).toBe('D 127.0.0.1:1080 · SOCKS starting…');
    expect(chip()?.textContent).toContain('3 forwards');
  });

  it('follows a newer report and forgets everything when the connection is lost', async () => {
    mountBar();
    sessions.applyUpdate(pane, { kind: 'forward', forward: LISTENING_0 });
    sessions.applyUpdate(pane, {
      kind: 'forward',
      forward: { state: 'failed', id: 0, kind: 'local', reason: 'died' },
    });
    flushSync();
    expect(pane.forwards).toHaveLength(1);
    expect(chip()?.textContent).toContain('1 of 3 failed');
    sessions.applyStatus(pane, {
      state: 'reconnecting',
      attempt: 1,
      delay_ms: 100,
      last_error: 'gone',
    });
    flushSync();
    expect(pane.forwards).toEqual([]);
    expect(chip()?.textContent).toContain('3 forwards');
  });

  it('copies the address of a listening forward when it is chosen', async () => {
    const copied: string[] = [];
    Object.assign(navigator, {
      clipboard: { writeText: async (text: string) => void copied.push(text) },
    });
    sessions.applyUpdate(pane, { kind: 'forward', forward: LISTENING_0 });
    mountBar();
    click(chip() as HTMLElement);
    await tick();
    click(menuItems()[0]);
    await vi.waitFor(() => expect(copied).toEqual(['127.0.0.1:8080']));
    await vi.waitFor(() => expect(noticeText(notices.visible[0])).toContain('Address copied'));
  });

  it('is reachable from the keyboard', async () => {
    sessions.applyUpdate(pane, { kind: 'forward', forward: LISTENING_0 });
    mountBar();
    (chip() as HTMLElement).focus();
    // A keyboard click has no pointer detail: the menu opens without motion and the first entry has the focus.
    (chip() as HTMLElement).dispatchEvent(new MouseEvent('click', { bubbles: true, detail: 0 }));
    flushSync();
    await tick();
    await vi.waitFor(() => expect(document.activeElement).toBe(menuItems()[0]));
    press(menuItems()[0], 'Escape');
  });
});

describe('a session that runs without the synced settings', () => {
  beforeEach(() => {
    sessions.applyUpdate(pane, { kind: 'exec_pending' });
  });

  it('marks the exposed forward as not approved and offers the review', async () => {
    sessions.applyUpdate(pane, { kind: 'forward', forward: LISTENING_0 });
    sessions.applyUpdate(pane, { kind: 'forward', forward: LISTENING_1 });
    mountBar();
    expect(chip()?.textContent).toContain('1 needs approval');
    click(chip() as HTMLElement);
    await tick();
    const items = menuItems();
    expect(items.map((item) => item.textContent?.replace(/\s+/g, ' ').trim())).toEqual([
      'L 127.0.0.1:8080 → db:5432 listening',
      'D 127.0.0.1:1080 · SOCKS listening',
      'L 0.0.0.0:9090 → metrics:9090 Left out until you approve the synced settings. not approved',
      'Review synced settings…',
    ]);
    answers.set('host_exec_review', {
      host_id: 'atlas',
      host_name: 'atlas-prod',
      hash: 'h',
      pending: true,
      fields: { startup_command: null, agent_forward: false, env: [], forwards: [] },
    });
    click(items[3]);
    await vi.waitFor(() => expect(execApproval.open).toBe(true));
    expect(callsOf('host_exec_review')[0].args).toEqual({ id: 'atlas' });
  });

  it('has its own chip next to the target', async () => {
    mountBar();
    const button = Array.from(target.querySelectorAll('button')).find((b) =>
      b.textContent?.includes('Synced settings off'),
    );
    expect(button).toBeDefined();
    answers.set('host_exec_review', {
      host_id: 'atlas',
      host_name: 'atlas-prod',
      hash: 'h',
      pending: true,
      fields: { startup_command: 'x', agent_forward: false, env: [], forwards: [] },
    });
    click(button as HTMLElement);
    await vi.waitFor(() => expect(execApproval.open).toBe(true));
  });

  it('is told once, with a button that opens the review of that host', () => {
    expect(noticeText(notices.visible[0])).toContain(
      'Synced settings for atlas-prod need your approval on this device',
    );
    expect(notices.visible[0].action).toEqual({ id: 'review-host-exec', label: 'Review' });
    // A second report (a view that attaches) does not stack another notice.
    sessions.applyUpdate(pane, { kind: 'exec_pending' });
    expect(notices.visible).toHaveLength(1);
  });
});
