// @vitest-environment jsdom
/**
 * Starting and restarting connections: the secret dialog is shown only when a
 * secret is needed and neither supplied nor stored in the credential store.
 */

import { beforeEach, describe, expect, it, vi } from 'vitest';
import { connectFlow } from './connect.svelte';
import { hosts } from './hosts.svelte';
import { route } from './route.svelte';
import { sessions, type Pane } from './sessions.svelte';
import { answers, callsOf, resetTauriMock } from '../test-support/tauri-core-mock';
import type { Host } from '../types';

vi.mock('@tauri-apps/api/core', () => import('../test-support/tauri-core-mock'));

function host(name: string, auth: Host['auth']): Host {
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

const password = host('pw', { type: 'password' });
const key = host('key', { type: 'private_key', identity_id: 'i1' });
const agent = host('agent', { type: 'agent' });

/** The secret store answers per kind. */
function stored(kinds: ('password' | 'passphrase')[]): void {
  answers.set('secret_has', (args: unknown) =>
    kinds.includes((args as { kind: 'password' | 'passphrase' }).kind),
  );
}

/** A restored pane: no secrets in memory, the user has to ask to reconnect. */
function restoredPane(target: Host): Pane {
  const tab = sessions.open(target, {});
  const pane = sessions.activePaneOf(tab);
  if (!pane) throw new Error('no pane');
  pane.restored = true;
  pane.dead = true;
  pane.status = { state: 'closed', reason: 'not connected' };
  return pane;
}

beforeEach(() => {
  resetTauriMock();
  connectFlow.cancel();
  sessions.tabs = [];
  sessions.activeId = null;
  hosts.passwordStored = {};
  route.route = 'hosts';
});

describe('a new connection', () => {
  it('skips the dialog when the credential store holds the password', async () => {
    stored(['password']);
    await connectFlow.request(password);
    expect(connectFlow.pending).toBeNull();
    expect(sessions.tabs).toHaveLength(1);
    // No secret is sent: the backend falls back to the credential store.
    expect(sessions.tabs[0].panes[0].secrets).toEqual({});
    expect(route.route).toBe('sessions');
    // The hosts table learns the same fact.
    expect(hosts.passwordStored[password.id]).toBe(true);
  });

  it('asks for the password when none is stored, and opens the tab on submit', async () => {
    stored([]);
    await connectFlow.request(password);
    expect(connectFlow.pending?.host.id).toBe(password.id);
    expect(connectFlow.pending?.secretStored).toBe(false);
    expect(sessions.tabs).toHaveLength(0);

    connectFlow.submit({ password: 'hunter2' }, true);
    expect(connectFlow.pending).toBeNull();
    expect(sessions.tabs[0].panes[0].secrets).toEqual({ password: 'hunter2' });
    expect(sessions.tabs[0].panes[0].rememberSecret).toBe(true);
  });

  it('skips the dialog for a key host whose passphrase is stored, asks otherwise', async () => {
    stored(['passphrase']);
    await connectFlow.request(key);
    expect(connectFlow.pending).toBeNull();
    expect(sessions.tabs).toHaveLength(1);

    stored([]);
    await connectFlow.request(key);
    expect(connectFlow.pending?.host.id).toBe(key.id);
    expect(callsOf('secret_has').pop()?.args).toEqual({ hostId: key.id, kind: 'passphrase' });
  });

  it('never asks an agent host for a secret', async () => {
    await connectFlow.request(agent);
    expect(connectFlow.pending).toBeNull();
    expect(sessions.tabs).toHaveLength(1);
    expect(callsOf('secret_has')).toHaveLength(0);
  });

  it('can be cancelled', async () => {
    stored([]);
    await connectFlow.request(password);
    connectFlow.cancel();
    expect(connectFlow.pending).toBeNull();
    expect(sessions.tabs).toHaveLength(0);
  });
});

describe('a host picked for an empty pane', () => {
  function emptyPane(): Pane {
    sessions.open(agent, {});
    const created = sessions.splitActive('columns');
    const pane = created ? sessions.paneById(created.id) : null;
    if (!pane) throw new Error('no split');
    return pane;
  }

  it('goes through the same flow: a dialog when a secret is needed', async () => {
    stored([]);
    const pane = emptyPane();
    await connectFlow.pick(pane, password);
    expect(connectFlow.pending?.pane?.id).toBe(pane.id);
    expect(pane.host).toBeNull();

    connectFlow.submit({ password: 'pw' }, false);
    expect(pane.host?.id).toBe(password.id);
    expect(pane.secrets).toEqual({ password: 'pw' });
    expect(pane.generation).toBe(1);
  });

  it('connects at once when the secret is stored', async () => {
    stored(['password']);
    const pane = emptyPane();
    await connectFlow.pick(pane, password);
    expect(connectFlow.pending).toBeNull();
    expect(pane.host?.id).toBe(password.id);
    expect(pane.secrets).toEqual({});
  });
});

describe('reconnecting a pane', () => {
  it('asks a restored password pane for the password, then retries with it', async () => {
    stored([]);
    const pane = restoredPane(password);
    await connectFlow.reconnect(pane);
    expect(connectFlow.pending?.pane?.id).toBe(pane.id);
    expect(connectFlow.pending?.reconnecting).toBe(true);
    // Nothing was retried before the secret was collected.
    expect(pane.generation).toBe(0);

    connectFlow.submit({ password: 'new' }, true);
    expect(pane.generation).toBe(1);
    expect(pane.secrets).toEqual({ password: 'new' });
    expect(pane.freshConnect).toBe(true);
    expect(pane.rememberSecret).toBe(true);
    expect(pane.restored).toBe(false);
  });

  it('retries at once when the credential store holds the password', async () => {
    stored(['password']);
    const pane = restoredPane(password);
    await connectFlow.reconnect(pane);
    expect(connectFlow.pending).toBeNull();
    expect(pane.generation).toBe(1);
    expect(pane.freshConnect).toBeFalsy();
  });

  it('retries at once when the pane still holds a secret', async () => {
    stored([]);
    const pane = restoredPane(password);
    pane.secrets = { password: 'still here' };
    await connectFlow.reconnect(pane);
    expect(connectFlow.pending).toBeNull();
    expect(pane.generation).toBe(1);
  });

  it('retries at once when the backend still knows the session (it keeps the credentials)', async () => {
    stored([]);
    const pane = restoredPane(password);
    pane.sessionId = 'known';
    answers.set('session_list', [
      { id: 'known', host_id: password.id, status: { state: 'closed', reason: 'gave up' } },
    ]);
    await connectFlow.reconnect(pane);
    expect(connectFlow.pending).toBeNull();
    expect(pane.generation).toBe(1);
  });

  it('asks again after an authentication failure, even with a stored secret', async () => {
    stored(['password']);
    const pane = restoredPane(password);
    pane.lastFailure = { category: 'auth', message: 'permission denied' };
    await connectFlow.reconnect(pane);
    expect(connectFlow.pending?.secretStored).toBe(true);
    expect(pane.generation).toBe(0);
  });

  it('never asks an agent host', async () => {
    const pane = restoredPane(agent);
    await connectFlow.reconnect(pane);
    expect(connectFlow.pending).toBeNull();
    expect(pane.generation).toBe(1);
  });

  it('does not ask a key host for a passphrase it may not need', async () => {
    stored([]);
    const pane = restoredPane(key);
    await connectFlow.reconnect(pane);
    expect(connectFlow.pending).toBeNull();
    expect(pane.generation).toBe(1);
  });
});
