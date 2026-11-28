// @vitest-environment jsdom
/**
 * The inspector of the hosts page: the route through jump hosts, the sign-in
 * and its second step, the forwards with what became of them in the running
 * session, tags, what happens on connect and the approval of the synced
 * settings that act.
 */

import { flushSync, mount, tick, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import HostInspector from './HostInspector.svelte';
import { execApproval } from '../stores/exec-approval.svelte';
import { hosts } from '../stores/hosts.svelte';
import type { Pane } from '../stores/sessions.svelte';
import { click, findButton, newTarget, stubBrowserApis } from '../test-support/dom';
import { answers, callsOf, failures, resetTauriMock } from '../test-support/tauri-core-mock';
import type { Forward, Host, HostExecReview } from '../types';

vi.mock('@tauri-apps/api/core', () => import('../test-support/tauri-core-mock'));

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

const LOCAL: Forward = {
  kind: 'local',
  bind_port: 5432,
  target_host: 'db',
  target_port: 5432,
  enabled: true,
};
const SOCKS: Forward = { kind: 'dynamic', bind_port: 1080, enabled: true };

function paneFor(h: Host, over: Partial<Pane> = {}): Pane {
  return {
    id: 'pane-1',
    sessionId: 's1',
    host: h,
    title: h.name,
    status: { state: 'ready' },
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
    ...over,
  };
}

function review(over: Partial<HostExecReview> = {}): HostExecReview {
  return {
    host_id: 'host-atlas',
    host_name: 'atlas',
    hash: 'h',
    pending: false,
    fields: { startup_command: null, agent_forward: false, env: [], forwards: [] },
    ...over,
  };
}

type HostHandler = ReturnType<typeof vi.fn<(host: Host) => void>>;

interface Handlers {
  onconnect: HostHandler;
  onopen: HostHandler;
  onedit: HostHandler;
  ondelete: HostHandler;
  ontogglefavorite: HostHandler;
}

let target: HTMLElement;
let component: ReturnType<typeof mount> | null = null;
let handlers: Handlers;

function render(
  h: Host,
  extra: { live?: number; pane?: Pane | null; animate?: boolean } = {},
): void {
  handlers = {
    onconnect: vi.fn<(host: Host) => void>(),
    onopen: vi.fn<(host: Host) => void>(),
    onedit: vi.fn<(host: Host) => void>(),
    ondelete: vi.fn<(host: Host) => void>(),
    ontogglefavorite: vi.fn<(host: Host) => void>(),
  };
  component = mount(HostInspector, {
    target,
    props: {
      host: h,
      live: extra.live ?? 0,
      pane: extra.pane ?? null,
      animate: extra.animate ?? false,
      ...handlers,
    },
  });
  flushSync();
}

const text = () => target.textContent?.replace(/\s+/g, ' ') ?? '';

beforeEach(() => {
  stubBrowserApis();
  resetTauriMock();
  target = newTarget();
  hosts.hosts = [];
  hosts.identities = [];
  hosts.groups = [];
  hosts.tags = [];
  hosts.passwordStored = {};
  execApproval.close();
});

afterEach(() => {
  if (component) unmount(component);
  component = null;
  target.remove();
  document.body.replaceChildren();
});

describe('HostInspector', () => {
  it('titles the host with its address, and says how it signs in and where it lives', () => {
    hosts.identities = [
      {
        id: 'i1',
        name: 'work-ed25519',
        comment: null,
        fingerprint: null,
        created_at: STAMP,
        updated_at: STAMP,
        deleted: false,
      },
    ];
    hosts.groups = [
      {
        id: 'g1',
        name: 'Production',
        parent_id: null,
        sort_order: 0,
        created_at: STAMP,
        updated_at: STAMP,
        deleted: false,
      },
    ];
    const atlas = host('atlas', {
      port: 2222,
      auth: { type: 'private_key', identity_id: 'i1' },
      group_id: 'g1',
      keepalive_seconds: 30,
      tags: ['t1'],
    });
    hosts.tags = [
      {
        id: 't1',
        name: 'eu-west',
        color: null,
        created_at: STAMP,
        updated_at: STAMP,
        deleted: false,
      },
    ];
    render(atlas);

    expect(target.querySelector('h2')?.textContent).toBe('atlas');
    expect(target.querySelector('.sub')?.textContent).toBe('ops@atlas.test:2222');
    expect(text()).toContain('Identity · work-ed25519');
    expect(text()).toContain('One-time code if asked');
    expect(text()).toContain('Production');
    expect(text()).toContain('Every 30 s');
    expect(Array.from(target.querySelectorAll('.tag')).map((tag) => tag.textContent)).toEqual([
      'eu-west',
    ]);
    // No SFTP and no known-hosts command exist: nothing pretends to.
    expect(text()).not.toContain('Files');
    expect(text()).not.toContain('Host key');
  });

  it('draws the route as you → hops → host, resolving the hop names', () => {
    const bastion = host('bastion');
    const gateway = host('gateway');
    const app = host('app', { jump_chain: [gateway.id, bastion.id] });
    hosts.hosts = [bastion, gateway, app];
    render(app);
    const route = target.querySelector('.route') as HTMLElement;
    expect(route.textContent?.replace(/\s+/g, ' ').trim()).toBe('you gateway bastion app');
    expect(Array.from(route.querySelectorAll('b')).map((b) => b.textContent)).toEqual([
      'gateway',
      'bastion',
      'app',
    ]);
  });

  it('shows a direct route for a host with no hops, and the reason when the route is broken', () => {
    render(host('plain'));
    expect(target.querySelector('.route')?.textContent?.replace(/\s+/g, ' ').trim()).toBe(
      'you plain',
    );
    unmount(component!);
    component = null;
    target.replaceChildren();

    render(host('lost', { jump_chain: ['host-deleted'] }));
    expect(target.querySelector('.route .bad')?.textContent).toContain('does not exist');
  });

  it('lists the forwards as starting on connect while no session runs', () => {
    render(host('atlas', { forwards: [LOCAL, SOCKS] }));
    const rows = Array.from(target.querySelectorAll('.fwd'));
    expect(rows.map((row) => row.querySelector('.k')?.textContent)).toEqual(['L', 'D']);
    expect(rows[0].querySelector('.line')?.textContent).toBe('127.0.0.1:5432 → db:5432');
    expect(rows.map((row) => row.querySelector('.s')?.textContent)).toEqual([
      'Starts on connect',
      'Starts on connect',
    ]);
    expect(text()).toContain('2 configured');
  });

  it('shows the live status of every forward of a running session', () => {
    const atlas = host('atlas', { forwards: [LOCAL, SOCKS, { ...LOCAL, bind_port: 5433 }] });
    const pane = paneFor(atlas, {
      forwards: [
        { state: 'listening', id: 0, kind: 'local', address: '127.0.0.1', port: 5432 },
        { state: 'failed', id: 2, kind: 'local', reason: 'address already in use' },
      ],
    });
    render(atlas, { live: 1, pane });
    const states = Array.from(target.querySelectorAll('.fwd .s')).map((s) => s.textContent);
    // Id 1 has not reported yet.
    expect(states).toEqual(['Listening', 'Starting…', 'Failed']);
    expect(target.querySelector('.fwd .s.err')?.getAttribute('title')).toBe(
      'address already in use',
    );
    expect(text()).toContain('1 listening');
  });

  it('says a forward that needs approval was left out of a running session', () => {
    const exposed: Forward = { ...LOCAL, bind_address: '0.0.0.0', bind_port: 9090 };
    const atlas = host('atlas', { forwards: [exposed] });
    render(atlas, { live: 1, pane: paneFor(atlas, { execPending: true }) });
    expect(target.querySelector('.fwd .s')?.textContent).toBe('Not approved');
  });

  it('summarises what happens on connect: environment, startup, agent forwarding', async () => {
    const atlas = host('atlas', {
      env: [
        { name: 'LANG', value: 'C' },
        { name: 'X', value: 'y' },
      ],
      startup_command: 'tmux new -A -s atlas',
      agent_forward: true,
    });
    answers.set('host_exec_review', review({ pending: false }));
    render(atlas);
    expect(text()).toContain('2 variables');
    expect(text()).toContain('tmux new -A -s atlas');
    expect(text()).toContain('Forwarded to the server');
  });

  it('says the synced settings are not approved, and opens the review', async () => {
    const atlas = host('atlas', { id: 'host-atlas', startup_command: 'tmux' });
    answers.set('host_exec_review', review({ pending: true }));
    render(atlas);
    await vi.waitFor(() =>
      expect(target.querySelector('[aria-label="Synced settings need approval"]')).not.toBeNull(),
    );
    expect(text()).toContain('Synced settings are off on this device');
    click(findButton(target, 'Review…'));
    await vi.waitFor(() => expect(callsOf('host_exec_review').length).toBeGreaterThan(1));
    expect(execApproval.open || execApproval.loading || execApproval.current !== null).toBe(true);
  });

  it('says they are approved on this device, and asks nothing for a host with none', async () => {
    const atlas = host('atlas', { id: 'host-atlas', agent_forward: true });
    answers.set('host_exec_review', review({ pending: false }));
    render(atlas);
    await vi.waitFor(() => expect(text()).toContain('Approved on this device'));
    expect(findButton(target, 'Review')).toBeTruthy();

    unmount(component!);
    component = null;
    target.replaceChildren();
    resetTauriMock();
    render(host('quiet'));
    await tick();
    expect(callsOf('host_exec_review')).toHaveLength(0);
    expect(target.querySelector('[aria-label="Synced settings need approval"]')).toBeNull();
  });

  it('keeps quiet when the approval cannot be read', async () => {
    failures.set('host_exec_review', 'locked');
    render(host('atlas', { id: 'host-atlas', agent_forward: true }));
    await vi.waitFor(() => expect(callsOf('host_exec_review')).toHaveLength(1));
    await tick();
    expect(target.querySelector('[aria-label="Synced settings need approval"]')).toBeNull();
    expect(text()).not.toContain('Approved on this device');
  });

  it('opens a running session, connects an idle host, edits, and has every action in the menu', () => {
    const atlas = host('atlas');
    render(atlas, { live: 2 });
    click(findButton(target, 'Open session'));
    expect(handlers.onopen).toHaveBeenCalledWith(atlas);
    click(findButton(target, 'Edit'));
    expect(handlers.onedit).toHaveBeenCalledWith(atlas);

    (target.querySelector('[aria-label="More actions for atlas"]') as HTMLElement).click();
    flushSync();
    const labels = Array.from(document.body.querySelectorAll('.menu button')).map((b) =>
      b.textContent?.trim(),
    );
    expect(labels).toEqual([
      'Open running session',
      'Connect',
      'Edit…',
      'Add to favorites',
      'Delete',
    ]);

    unmount(component!);
    component = null;
    target.replaceChildren();
    render(atlas, { live: 0 });
    click(findButton(target, 'Connect'));
    expect(handlers.onconnect).toHaveBeenCalledWith(atlas);
  });

  it('toggles the favorite from the title', () => {
    const atlas = host('atlas');
    render(atlas);
    click(target.querySelector('[aria-label="Add atlas to favorites"]') as Element);
    expect(handlers.ontogglefavorite).toHaveBeenCalledWith(atlas);
  });

  it('fades its content in only when it was asked to', () => {
    render(host('a'), { animate: true });
    expect(target.querySelector('.content')?.classList.contains('fade')).toBe(true);
    unmount(component!);
    component = null;
    target.replaceChildren();
    render(host('a'), { animate: false });
    expect(target.querySelector('.content')?.classList.contains('fade')).toBe(false);
  });

  it('keeps a long name and a huge note from breaking the panel', () => {
    const name =
      'customer-facing-reporting-service-eu-central-1-blue-green-canary-deployment'.repeat(2);
    render(host('x', { name, description: 'note '.repeat(400) }));
    const title = target.querySelector('h2') as HTMLElement;
    expect(title.title).toBe(name);
    // Cut by the stylesheet to two lines; the notes to six.
    expect(target.querySelector('.notes')).not.toBeNull();
  });
});
