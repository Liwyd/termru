// @vitest-environment jsdom
/**
 * The password of a jump host: the connect flow asks for it (one field per
 * hop) when the route passes a host that signs in with a password and the
 * credential store has none, and carries what was typed to the session.
 */

import { beforeEach, describe, expect, it, vi } from 'vitest';
import { connectFlow, hopNeedsPassword } from './connect.svelte';
import { hosts } from './hosts.svelte';
import { route } from './route.svelte';
import { sessions } from './sessions.svelte';
import { notices } from '../notify.svelte';
import { answers, callsOf, resetTauriMock } from '../test-support/tauri-core-mock';
import type { Host } from '../types';

vi.mock('@tauri-apps/api/core', () => import('../test-support/tauri-core-mock'));

function host(name: string, auth: Host['auth'], over: Partial<Host> = {}): Host {
  const now = new Date().toISOString();
  return {
    id: `host-${name}`,
    name,
    address: `${name}.test`,
    port: 22,
    username: 'ops',
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
    ...over,
  };
}

const gateway = host('gateway', { type: 'password' });
const bastion = host('bastion', { type: 'password' }, { jump_chain: ['host-gateway'] });
const keyed = host('keyed', { type: 'agent' });

/** The credential store holds a password for these hosts (by name). */
function stored(...names: string[]): void {
  answers.set('secret_has', (args: unknown) => {
    const { hostId, kind } = args as { hostId: string; kind: string };
    return kind === 'password' && names.some((name) => hostId === `host-${name}`);
  });
}

beforeEach(() => {
  resetTauriMock();
  connectFlow.cancel();
  sessions.tabs = [];
  sessions.activeId = null;
  hosts.hosts = [gateway, bastion, keyed];
  hosts.passwordStored = {};
  route.route = 'hosts';
  notices.reset();
});

describe('which jump hosts need a password', () => {
  it('only those that sign in with one, and list it in their order', () => {
    expect(hopNeedsPassword(gateway)).toBe(true);
    expect(hopNeedsPassword(keyed)).toBe(false);
    expect(hopNeedsPassword(host('k', { type: 'private_key', identity_id: 'i' }))).toBe(false);
    // An explicit order that leaves the password out never uses it.
    expect(hopNeedsPassword(host('o', { type: 'password' }, { auth_order: ['agent'] }))).toBe(
      false,
    );
    expect(
      hopNeedsPassword(host('o', { type: 'password' }, { auth_order: ['password', 'agent'] })),
    ).toBe(true);
  });
});

describe('connecting through jump hosts', () => {
  it('asks for the password of a hop that has none stored, labelled by the hop', async () => {
    stored();
    const app = host('app', { type: 'agent' }, { jump_chain: ['host-bastion'] });
    hosts.hosts = [...hosts.hosts, app];
    await connectFlow.request(app);
    expect(sessions.tabs).toHaveLength(0);
    const pending = connectFlow.pending;
    expect(pending?.askTarget).toBe(false);
    // The route is gateway → bastion: the first hop's own jump host comes first.
    expect(pending?.hops.map((hop) => [hop.host.name, hop.stored])).toEqual([
      ['gateway', false],
      ['bastion', false],
    ]);
  });

  it('does not ask when the credential store holds the hop password', async () => {
    stored('gateway', 'bastion');
    const app = host('app', { type: 'agent' }, { jump_chain: ['host-bastion'] });
    hosts.hosts = [...hosts.hosts, app];
    await connectFlow.request(app);
    expect(connectFlow.pending).toBeNull();
    expect(sessions.tabs).toHaveLength(1);
    expect(sessions.tabs[0].panes[0].hopSecrets).toBeUndefined();
  });

  it('asks only for the hop that has none', async () => {
    stored('gateway');
    const app = host('app', { type: 'agent' }, { jump_chain: ['host-bastion'] });
    hosts.hosts = [...hosts.hosts, app];
    await connectFlow.request(app);
    expect(connectFlow.pending?.hops.map((hop) => hop.host.name)).toEqual(['bastion']);
  });

  it('never asks for a hop that uses a key or the agent', async () => {
    const app = host('app', { type: 'agent' }, { jump_chain: ['host-keyed'] });
    hosts.hosts = [...hosts.hosts, app];
    await connectFlow.request(app);
    expect(connectFlow.pending).toBeNull();
    expect(callsOf('secret_has')).toHaveLength(0);
    expect(sessions.tabs).toHaveLength(1);
  });

  it('asks for the host and for its hops together', async () => {
    stored();
    const app = host('app', { type: 'password' }, { jump_chain: ['host-gateway'] });
    hosts.hosts = [...hosts.hosts, app];
    await connectFlow.request(app);
    expect(connectFlow.pending?.askTarget).toBe(true);
    expect(connectFlow.pending?.hops.map((hop) => hop.host.name)).toEqual(['gateway']);
  });

  it('asks only for the hops when the host itself has its password stored', async () => {
    stored('app');
    const app = host('app', { type: 'password' }, { jump_chain: ['host-gateway'] });
    hosts.hosts = [...hosts.hosts, app];
    await connectFlow.request(app);
    expect(connectFlow.pending?.askTarget).toBe(false);
    expect(connectFlow.pending?.secretStored).toBe(true);
  });

  it('leaves a route that cannot be resolved to the backend, which says why', async () => {
    const app = host('app', { type: 'agent' }, { jump_chain: ['host-deleted'] });
    hosts.hosts = [...hosts.hosts, app];
    await connectFlow.request(app);
    expect(connectFlow.pending).toBeNull();
    expect(sessions.tabs).toHaveLength(1);
  });

  it('opens the tab with what was typed, and which hops to remember', async () => {
    stored();
    const app = host('app', { type: 'agent' }, { jump_chain: ['host-bastion'] });
    hosts.hosts = [...hosts.hosts, app];
    await connectFlow.request(app);
    connectFlow.submit({}, false, {
      secrets: { 'host-gateway': { password: 'g' }, 'host-bastion': { password: 'b' } },
      remember: ['host-bastion'],
    });
    expect(connectFlow.pending).toBeNull();
    const pane = sessions.tabs[0].panes[0];
    expect(pane.hopSecrets).toEqual({
      'host-gateway': { password: 'g' },
      'host-bastion': { password: 'b' },
    });
    expect(pane.hopRemember).toEqual(['host-bastion']);
    expect(route.route).toBe('sessions');
  });

  it('keeps the chosen hop password in the credential store once connected, and no other', async () => {
    stored();
    const app = host('app', { type: 'agent' }, { jump_chain: ['host-bastion'] });
    hosts.hosts = [...hosts.hosts, app];
    await connectFlow.request(app);
    connectFlow.submit({}, false, {
      secrets: { 'host-gateway': { password: 'g' }, 'host-bastion': { password: 'b' } },
      remember: ['host-bastion'],
    });
    const pane = sessions.tabs[0].panes[0];
    pane.sessionId = 's1';
    // Nothing is stored while the connection is still being made.
    expect(callsOf('secret_set')).toHaveLength(0);
    sessions.applyStatus(pane, { state: 'ready' });
    await vi.waitFor(() => expect(callsOf('secret_set')).toHaveLength(1));
    expect(callsOf('secret_set')[0].args).toEqual({
      hostId: 'host-bastion',
      kind: 'password',
      value: 'b',
    });
    // What was kept is gone from memory; what was not kept stays for a retry.
    await vi.waitFor(() => expect(pane.hopSecrets?.['host-bastion'].password).toBeUndefined());
    expect(hosts.passwordStored['host-bastion']).toBe(true);
  });

  it('does not store a hop password when the connection failed', async () => {
    stored();
    const app = host('app', { type: 'agent' }, { jump_chain: ['host-gateway'] });
    hosts.hosts = [...hosts.hosts, app];
    await connectFlow.request(app);
    connectFlow.submit({}, false, {
      secrets: { 'host-gateway': { password: 'wrong' } },
      remember: ['host-gateway'],
    });
    const pane = sessions.tabs[0].panes[0];
    sessions.applyStatus(pane, { state: 'failed', category: 'auth', message: 'denied' });
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(callsOf('secret_set')).toHaveLength(0);
  });

  it('asks again on a reconnect after an authentication failure, for every password hop', async () => {
    stored('gateway', 'bastion');
    const app = host('app', { type: 'agent' }, { jump_chain: ['host-bastion'] });
    hosts.hosts = [...hosts.hosts, app];
    await connectFlow.request(app);
    const pane = sessions.tabs[0].panes[0];
    pane.lastFailure = { category: 'auth', message: 'denied' };
    pane.status = { state: 'failed', category: 'auth', message: 'denied' };
    pane.dead = true;
    await connectFlow.reconnect(pane);
    expect(connectFlow.pending?.reconnecting).toBe(true);
    expect(connectFlow.pending?.hops.map((hop) => [hop.host.name, hop.stored])).toEqual([
      ['gateway', true],
      ['bastion', true],
    ]);
  });

  it('reconnects without a question when the backend still holds the session', async () => {
    stored();
    const app = host('app', { type: 'agent' }, { jump_chain: ['host-gateway'] });
    hosts.hosts = [...hosts.hosts, app];
    await connectFlow.request(app);
    connectFlow.submit({}, false, { secrets: { 'host-gateway': { password: 'g' } }, remember: [] });
    const pane = sessions.tabs[0].panes[0];
    pane.sessionId = 'live';
    answers.set('session_list', [
      { id: 'live', host_id: app.id, status: { state: 'closed', reason: 'x' } },
    ]);
    pane.dead = true;
    await connectFlow.reconnect(pane);
    expect(connectFlow.pending).toBeNull();
    expect(pane.generation).toBe(1);
  });
});
