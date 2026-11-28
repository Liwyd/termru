import { describe, expect, it } from 'vitest';
import { leafTree } from '../pane-tree';
import type { Pane, Tab } from '../stores/sessions.svelte';
import type { Host, Identity, SessionStatus } from '../types';
import {
  inputLine,
  reconnectState,
  relativeAge,
  sessionsContext,
  shellLabel,
  sizeLabel,
  sshCommand,
  sshTarget,
  statusLabel,
  statusTone,
} from './session-view';

const host = {
  id: 'h1',
  name: 'atlas-prod',
  address: '192.0.2.10',
  port: 22,
  username: 'ops',
  auth: { type: 'private_key', identity_id: 'i1' },
} as unknown as Host;

function pane(status: SessionStatus, extra: Partial<Pane> = {}): Pane {
  return {
    id: 'p1',
    sessionId: 's1',
    host,
    title: 'atlas-prod',
    status,
    secrets: {},
    generation: 0,
    exitCode: null,
    dead: false,
    errorMessage: null,
    awaitingReconnect: false,
    hadReady: false,
    lastFailure: null,
    duplicateFrom: null,
    restored: false,
    attachTo: null,
    rememberSecret: false,
    ...extra,
  };
}

describe('status words', () => {
  it('names every state and gives it a tone', () => {
    const cases: [SessionStatus, string, string][] = [
      [{ state: 'ready' }, 'Connected', 'ready'],
      [{ state: 'connecting' }, 'Connecting', 'busy'],
      [{ state: 'authenticating' }, 'Authenticating', 'busy'],
      [
        { state: 'reconnecting', attempt: 2, delay_ms: 500, last_error: 'x' },
        'Reconnecting',
        'busy',
      ],
      [{ state: 'failed', category: 'auth', message: 'x' }, 'Error', 'failed'],
      [{ state: 'closed', reason: 'x' }, 'Disconnected', 'closed'],
    ];
    for (const [status, label, tone] of cases) {
      expect(statusLabel(pane(status))).toBe(label);
      expect(statusTone(status)).toBe(tone);
    }
  });

  it('says a restored pane is not connected, and an empty one is empty', () => {
    expect(statusLabel(pane({ state: 'closed', reason: 'x' }, { restored: true }))).toBe(
      'Not connected',
    );
    expect(statusLabel(pane({ state: 'connecting' }, { host: null }))).toBe('Empty');
  });
});

describe('the pane footer line', () => {
  it('says Ready with the encoding while the session takes input', () => {
    expect(inputLine(pane({ state: 'ready' }))).toBe('Ready · UTF-8');
  });

  it('says why there is no input otherwise', () => {
    expect(inputLine(pane({ state: 'connecting' }))).toBe('Connecting · input off');
    expect(inputLine(pane({ state: 'authenticating' }))).toBe('Authenticating · input off');
    expect(
      inputLine(pane({ state: 'reconnecting', attempt: 3, delay_ms: 1000, last_error: 'x' })),
    ).toBe('Reconnecting · attempt 3');
    expect(inputLine(pane({ state: 'failed', category: 'network', message: 'x' }))).toBe(
      'Connection failed · input off',
    );
    expect(inputLine(pane({ state: 'closed', reason: 'x' }, { dead: true }))).toBe(
      'Disconnected · input off',
    );
    expect(inputLine(pane({ state: 'closed', reason: 'x' }, { awaitingReconnect: true }))).toBe(
      'Connection lost · retrying',
    );
    expect(inputLine(pane({ state: 'closed', reason: 'x' }, { restored: true, dead: true }))).toBe(
      'Not connected · input off',
    );
    expect(inputLine(pane({ state: 'closed', reason: 'x' }, { host: null }))).toBe('No session');
  });

  it('does not offer input to a ready pane that is marked dead', () => {
    expect(inputLine(pane({ state: 'ready' }, { dead: true }))).toBe('Disconnected · input off');
  });
});

describe('Reconnect', () => {
  it('is for a session that is over, and says why not otherwise', () => {
    expect(reconnectState(pane({ state: 'ready' }))).toEqual({
      can: false,
      reason: 'The session is connected',
    });
    expect(reconnectState(pane({ state: 'connecting' }))).toEqual({
      can: false,
      reason: 'The session is still connecting',
    });
    expect(reconnectState(pane({ state: 'failed', category: 'network', message: 'x' })).can).toBe(
      true,
    );
    expect(reconnectState(pane({ state: 'closed', reason: 'x' })).can).toBe(true);
    expect(reconnectState(pane({ state: 'ready' }, { dead: true })).can).toBe(true);
  });

  it('has nothing to reconnect in an empty pane', () => {
    expect(reconnectState(pane({ state: 'closed', reason: 'x' }, { host: null }))).toEqual({
      can: false,
      reason: 'There is no session to reconnect',
    });
    expect(reconnectState(null).can).toBe(false);
  });
});

describe('small labels', () => {
  it('formats the grid, and says nothing before it is measured', () => {
    expect(sizeLabel(132, 32)).toBe('132 × 32');
    expect(sizeLabel(0, 0)).toBe('');
  });

  it('uses the remote title for the shell segment', () => {
    expect(shellLabel({ terminalTitle: null })).toBe('shell');
    expect(shellLabel({ terminalTitle: 'ops@atlas: /srv' })).toBe('ops@atlas: /srv');
  });

  it('writes the SSH target and command', () => {
    expect(sshTarget(host)).toBe('ops@192.0.2.10:22');
    expect(sshTarget({ ...host, address: 'fe80::1', port: 2222 })).toBe('ops@[fe80::1]:2222');
    expect(sshCommand(host)).toBe('ssh ops@192.0.2.10');
    expect(sshCommand({ ...host, port: 2222 })).toBe('ssh -p 2222 ops@192.0.2.10');
  });

  it('says how long ago, in the coarsest honest unit', () => {
    const now = 10_000_000_000;
    expect(relativeAge(now - 5_000, now)).toBe('just now');
    expect(relativeAge(now - 12 * 60_000, now)).toBe('12 min ago');
    expect(relativeAge(now - 3 * 3_600_000, now)).toBe('3 h ago');
    expect(relativeAge(now - 2 * 86_400_000, now)).toBe('2 d ago');
    // A clock that went backwards is not a negative age.
    expect(relativeAge(now + 5_000, now)).toBe('just now');
  });
});

describe('the status bar line', () => {
  const identities = [{ id: 'i1', name: 'work-ed25519', deleted: false }] as unknown as Identity[];

  function tab(id: string, title: string, panes: Pane[]): Tab {
    return {
      id,
      title,
      panes,
      activePaneId: panes[0]?.id ?? null,
      root: leafTree(panes[0]?.id ?? 'x'),
    };
  }

  it('says there is nothing open', () => {
    expect(sessionsContext([], null, 0, identities)).toBe('No open sessions');
  });

  it('one pane: how long it has been connected, and the identity', () => {
    const now = 100_000_000;
    const single = tab('t1', 'atlas-prod', [
      pane({ state: 'ready' }, { connectedAt: now - 12 * 60_000 }),
    ]);
    expect(sessionsContext([single], single, now, identities)).toBe(
      'atlas-prod · Connected 12 min ago · Identity: work-ed25519',
    );
  });

  it('one pane that is not connected: the state instead of the age', () => {
    const single = tab('t1', 'atlas-prod', [pane({ state: 'connecting' })]);
    expect(sessionsContext([single], single, 0, identities)).toBe(
      'atlas-prod · Connecting · Identity: work-ed25519',
    );
  });

  it('split: panes and sessions, and what runs in the background', () => {
    const split = tab('t1', 'atlas-prod', [
      pane({ state: 'ready' }, { id: 'p1' }),
      pane({ state: 'ready' }, { id: 'p2', sessionId: 's2' }),
    ]);
    const other = tab('t2', 'relay-edge', [pane({ state: 'ready' }, { id: 'p3' })]);
    expect(sessionsContext([split, other], split, 0, identities)).toBe(
      'atlas-prod · 2 panes / 2 sessions · relay-edge connected in background',
    );
  });

  it('does not invent an identity for an agent or a password host', () => {
    const agent = { ...host, auth: { type: 'agent' } } as Host;
    const single = tab('t1', 'x', [pane({ state: 'ready' }, { host: agent, connectedAt: 0 })]);
    expect(sessionsContext([single], single, 0, identities)).toBe(
      'x · Connected just now · SSH agent',
    );
    const password = { ...host, auth: { type: 'password' } } as Host;
    const second = tab('t2', 'y', [pane({ state: 'ready' }, { host: password, connectedAt: 0 })]);
    expect(sessionsContext([second], second, 0, identities)).toBe('y · Connected just now');
  });
});
