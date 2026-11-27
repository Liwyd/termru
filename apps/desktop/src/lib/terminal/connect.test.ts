// @vitest-environment jsdom
/**
 * Connecting a pane's terminal: the rules that keep a remount, a restored
 * layout and a moved tab from starting, closing or duplicating sessions.
 */

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { hosts } from '../stores/hosts.svelte';
import { sessions, type Pane } from '../stores/sessions.svelte';
import {
  answers,
  callsOf,
  channels,
  failures,
  resetTauriMock,
} from '../test-support/tauri-core-mock';
import { resetXtermMock, terminals } from '../test-support/xterm-mock';
import type { Host, SessionInfo, SessionUpdate, WorkspaceDoc } from '../types';
import { appearanceFromPrefs } from './appearance';
import { sendInput, startConnection } from './connect';
import { createTerminal } from './create';
import { pruneTerminals, type TerminalHandle } from './registry';
import { clearWriteQueue } from './write-queue';

vi.mock('@tauri-apps/api/core', () => import('../test-support/tauri-core-mock'));
vi.mock('@tauri-apps/api/event', () => ({ listen: async () => () => {} }));
vi.mock('@xterm/xterm', () => import('../test-support/xterm-mock'));
vi.mock('@xterm/addon-fit', () => import('../test-support/xterm-mock'));
vi.mock('@xterm/addon-webgl', () => import('../test-support/xterm-mock'));
vi.mock('@xterm/addon-search', () => import('../test-support/xterm-mock'));
vi.mock('@xterm/addon-web-links', () => import('../test-support/xterm-mock'));

function host(name = 'web-1', auth: Host['auth'] = { type: 'password' }): Host {
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

const hooks = { onData: () => {}, isAppKey: () => false };
let surface: HTMLElement;

/** A terminal for a pane, shown in the document. */
function terminalFor(pane: Pane): TerminalHandle {
  return createTerminal(pane.id, surface, appearanceFromPrefs(), hooks);
}

/** The pane of a new tab, as the store holds it. */
function openPane(target: Host = host(), secrets = {}): Pane {
  const tab = sessions.open(target, secrets);
  const pane = sessions.activePaneOf(tab);
  if (!pane) throw new Error('no pane');
  return pane;
}

function sessionInfo(id: string, state: SessionInfo['status']['state']): SessionInfo {
  const status =
    state === 'closed'
      ? ({ state, reason: 'gave up after 8 attempts' } as const)
      : ({ state } as SessionInfo['status']);
  return { id, host_id: 'host-web-1', status };
}

beforeEach(() => {
  resetTauriMock();
  resetXtermMock();
  pruneTerminals(new Set());
  sessions.tabs = [];
  sessions.activeId = null;
  sessions.windowLabel = null;
  sessions.layoutLoaded = false;
  hosts.hosts = [];
  surface = document.createElement('div');
  document.body.appendChild(surface);
});

afterEach(() => {
  surface.remove();
  pruneTerminals(new Set());
});

describe('opening a session', () => {
  it('opens one session for a new pane and records its id', async () => {
    const pane = openPane(host(), { password: 'pw' });
    await startConnection(pane, terminalFor(pane), pane.generation);

    const open = callsOf('session_open');
    expect(open).toHaveLength(1);
    expect(open[0].args?.hostId).toBe('host-web-1');
    expect((open[0].args?.options as { secrets: unknown }).secrets).toEqual({ password: 'pw' });
    expect(pane.sessionId).toBe('session-1');
    expect(callsOf('session_close')).toHaveLength(0);
  });

  it('does nothing for a generation this terminal already connected for', async () => {
    const pane = openPane();
    const handle = terminalFor(pane);
    await startConnection(pane, handle, pane.generation);
    await startConnection(pane, handle, pane.generation);
    expect(callsOf('session_open')).toHaveLength(1);
    expect(callsOf('session_close')).toHaveLength(0);
  });

  it('tells the session the size of the terminal after it opened', async () => {
    const pane = openPane();
    const handle = terminalFor(pane);
    handle.term.resize(120, 33);
    await startConnection(pane, handle, pane.generation);
    expect(callsOf('session_resize').pop()?.args).toEqual({ id: 'session-1', cols: 120, rows: 33 });
  });

  it('closes a session it opened when the pane moved on meanwhile', async () => {
    const pane = openPane();
    const handle = terminalFor(pane);
    answers.set('session_open', () => {
      // A newer attempt supersedes this one while the call is in flight.
      pane.generation += 1;
      return 'stale-session';
    });
    await startConnection(pane, handle, pane.generation);
    expect(pane.sessionId).toBeNull();
    expect(callsOf('session_close').map((call) => call.args?.id)).toEqual(['stale-session']);
  });

  it('closes a session it opened for a pane that was removed meanwhile', async () => {
    const pane = openPane();
    const handle = terminalFor(pane);
    answers.set('session_open', () => {
      sessions.tabs = [];
      return 'orphan';
    });
    await startConnection(pane, handle, pane.generation);
    expect(callsOf('session_close').map((call) => call.args?.id)).toEqual(['orphan']);
  });

  it('records a refused connection as a local failure', async () => {
    const pane = openPane();
    failures.set('session_open', 'host "web-1" requires a password');
    await startConnection(pane, terminalFor(pane), pane.generation);
    expect(pane.dead).toBe(true);
    expect(pane.errorMessage).toContain('requires a password');
  });
});

describe('typing into a pane', () => {
  it('writes in typing order, one write at a time, joining what is typed meanwhile', async () => {
    const pane = openPane();
    pane.sessionId = 'typed-session';
    sendInput(pane.id, 'l');
    sendInput(pane.id, 's');
    sendInput(pane.id, '\r');
    await new Promise((resolve) => setTimeout(resolve, 0));

    // The first keystroke left at once; the others waited for its answer and
    // went out as one write, instead of three invokes racing each other.
    const writes = callsOf('session_write').map((call) => call.args);
    expect(writes).toEqual([
      { id: 'typed-session', data: [108] },
      { id: 'typed-session', data: [115, 13] },
    ]);
    clearWriteQueue('typed-session');
  });

  it('sends nothing for a pane that has no session', async () => {
    const pane = openPane();
    sendInput(pane.id, 'x');
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(callsOf('session_write')).toHaveLength(0);
  });
});

describe('a restored pane', () => {
  async function restoredPane(): Promise<Pane> {
    const web = host();
    hosts.hosts = [web];
    const doc: WorkspaceDoc = {
      version: 1,
      windows: [
        {
          id: 'window-main',
          tabs: [{ id: 't1', title: 'web-1', root: { leaf: { pane: 'p1', host_id: web.id } } }],
          active_tab: 't1',
        },
      ],
    };
    answers.set('layout_load', doc);
    await sessions.restoreLayout();
    const pane = sessions.paneById('p1');
    if (!pane) throw new Error('the pane was not restored');
    return pane;
  }

  it('stays idle until the user asks to reconnect', async () => {
    const pane = await restoredPane();
    expect(pane.restored).toBe(true);
    expect(pane.dead).toBe(true);
    const handle = terminalFor(pane);

    await startConnection(pane, handle, pane.generation);
    await startConnection(pane, handle, pane.generation);
    expect(callsOf('session_open')).toHaveLength(0);
    expect(callsOf('session_attach')).toHaveLength(0);
    expect(pane.status).toEqual({ state: 'closed', reason: 'not connected' });

    // The Reconnect button bumps the generation: now it connects.
    sessions.retry(pane);
    await startConnection(pane, handle, pane.generation);
    expect(callsOf('session_open')).toHaveLength(1);
    expect(pane.restored).toBe(false);
  });
});

describe('a view for a session of another window', () => {
  it('attaches to it and never closes it', async () => {
    const web = host();
    const tab = sessions.attachTab('moved-session', web);
    const pane = sessions.activePaneOf(tab);
    if (!pane) throw new Error('no pane');
    // The pane starts without a session id of its own.
    expect(pane.sessionId).toBeNull();
    expect(pane.attachTo).toBe('moved-session');

    await startConnection(pane, terminalFor(pane), pane.generation);

    const attach = callsOf('session_attach');
    expect(attach).toHaveLength(1);
    expect(attach[0].args?.id).toBe('moved-session');
    expect(callsOf('session_close')).toHaveLength(0);
    expect(callsOf('session_open')).toHaveLength(0);
    expect(pane.sessionId).toBe('moved-session');
  });

  it('delivers the replay and the live updates to the pane', async () => {
    const tab = sessions.attachTab('moved-session', host());
    const pane = sessions.activePaneOf(tab);
    if (!pane) throw new Error('no pane');
    const handle = terminalFor(pane);
    await startConnection(pane, handle, pane.generation);

    const channel = channels.find((entry) => entry.cmd === 'session_attach')?.channel;
    const send = channel?.onmessage as (update: SessionUpdate) => void;
    send({ kind: 'data', base64: btoa('replayed scrollback') });
    send({ kind: 'status', status: { state: 'ready' } });
    expect(new TextDecoder().decode(terminals[0].writes[0])).toBe('replayed scrollback');
    expect(pane.status.state).toBe('ready');
  });
});

describe('reconnecting a pane that has a session id', () => {
  /** A dead pane that was connected before (so `retry` has a session to reuse). */
  function deadPane(secrets = {}): Pane {
    const pane = openPane(host(), secrets);
    pane.sessionId = 'old-session';
    pane.hadReady = true;
    sessions.applyStatus(pane, { state: 'closed', reason: 'gave up after 8 attempts' });
    return pane;
  }

  it('re-runs a stopped session instead of opening another one', async () => {
    const pane = deadPane();
    answers.set('session_list', [sessionInfo('old-session', 'closed')]);
    const handle = terminalFor(pane);
    handle.generation = 0;
    sessions.retry(pane);
    await startConnection(pane, handle, pane.generation);

    expect(callsOf('session_retry')).toHaveLength(1);
    expect(callsOf('session_retry')[0].args?.id).toBe('old-session');
    expect(callsOf('session_open')).toHaveLength(0);
    expect(callsOf('session_close')).toHaveLength(0);
    expect(pane.sessionId).toBe('old-session');
  });

  it('never closes a live session: it shows it again', async () => {
    const pane = deadPane();
    answers.set('session_list', [sessionInfo('old-session', 'ready')]);
    const handle = terminalFor(pane);
    handle.generation = 0;
    sessions.retry(pane);
    await startConnection(pane, handle, pane.generation);

    expect(callsOf('session_close')).toHaveLength(0);
    expect(callsOf('session_open')).toHaveLength(0);
    expect(callsOf('session_attach')[0].args?.id).toBe('old-session');
    // The replay would double the screen, which starts empty.
    expect(terminals[0].resets).toBe(1);
  });

  it('opens a fresh session when the backend forgot the old one', async () => {
    const pane = deadPane();
    answers.set('session_list', []);
    const handle = terminalFor(pane);
    handle.generation = 0;
    sessions.retry(pane);
    await startConnection(pane, handle, pane.generation);

    expect(callsOf('session_retry')).toHaveLength(0);
    expect(callsOf('session_open')).toHaveLength(1);
    expect(pane.sessionId).toBe('session-1');
  });

  it('replaces a stopped session when the user supplied new credentials', async () => {
    const pane = deadPane();
    answers.set('session_list', [sessionInfo('old-session', 'closed')]);
    const handle = terminalFor(pane);
    handle.generation = 0;
    sessions.retry(pane, { password: 'new-password' }, false);
    await startConnection(pane, handle, pane.generation);

    // The old session holds the old (wrong) credentials: it is not retried.
    expect(callsOf('session_retry')).toHaveLength(0);
    expect(callsOf('session_close').map((call) => call.args?.id)).toEqual(['old-session']);
    const open = callsOf('session_open')[0];
    expect((open.args?.options as { secrets: unknown }).secrets).toEqual({
      password: 'new-password',
    });
    expect(pane.freshConnect).toBe(false);
  });

  it('shows a live session again when the terminal was rebuilt', async () => {
    // A pane that has a session but whose terminal is new (never connected).
    const pane = openPane();
    pane.sessionId = 'live-session';
    answers.set('session_list', [sessionInfo('live-session', 'ready')]);
    await startConnection(pane, terminalFor(pane), pane.generation);
    expect(callsOf('session_attach')[0].args?.id).toBe('live-session');
    expect(callsOf('session_close')).toHaveLength(0);
    expect(callsOf('session_open')).toHaveLength(0);
  });
});

describe('a duplicated pane', () => {
  it('forgets its source once connected, so a retry never duplicates a gone session', async () => {
    const source = openPane();
    source.sessionId = 'source-session';
    source.status = { state: 'ready' };
    const clone = sessions.activePaneOf(sessions.duplicate(sessions.tabs[0]) as never) as Pane;
    expect(clone.duplicateFrom).toBe('source-session');

    await startConnection(clone, terminalFor(clone), clone.generation);
    expect(callsOf('session_duplicate')).toHaveLength(1);
    expect(clone.sessionId).toBe('session-dup');
    expect(clone.duplicateFrom).toBeNull();
  });

  it('clears the source when the duplicate fails, so Reconnect opens a fresh session', async () => {
    const source = openPane();
    source.sessionId = 'source-session';
    source.status = { state: 'ready' };
    const clone = sessions.activePaneOf(sessions.duplicate(sessions.tabs[0]) as never) as Pane;
    failures.set('session_duplicate', 'no session with id source-session');

    await startConnection(clone, terminalFor(clone), clone.generation);
    expect(clone.dead).toBe(true);
    expect(clone.duplicateFrom).toBeNull();

    failures.delete('session_duplicate');
    sessions.retry(clone, { password: 'pw' });
    await startConnection(clone, terminalFor(clone), clone.generation);
    expect(callsOf('session_open')).toHaveLength(1);
  });
});

describe('late updates', () => {
  it('are ignored for a pane that was removed', async () => {
    const pane = openPane();
    const handle = terminalFor(pane);
    await startConnection(pane, handle, pane.generation);
    const channel = handle.channel;
    sessions.tabs = [];
    channel?.onmessage({ kind: 'status', status: { state: 'ready' } });
    expect(pane.status.state).toBe('connecting');
  });

  it('are ignored once a newer attempt replaced the channel', async () => {
    const pane = openPane();
    const handle = terminalFor(pane);
    await startConnection(pane, handle, pane.generation);
    const first = handle.channel;
    sessions.retry(pane);
    answers.set('session_list', []);
    await startConnection(pane, handle, pane.generation);

    first?.onmessage({ kind: 'data', base64: btoa('stale') });
    expect(terminals[0].writes).toHaveLength(0);
  });
});
