/**
 * The dev-only mock backend (`?mock`): it must cover every command of the
 * `api` object, return the shapes the stores read, speak the session
 * protocol of the real backend, and never reach a production build.
 */

import { readdirSync, readFileSync } from 'node:fs';
import { join, relative } from 'node:path';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { Channel } from './ipc-backend';
import type { Host, SessionStatus, SessionUpdate } from './types';

// ipc.ts talks to the seam; here the seam is the mock backend itself.
vi.mock('./ipc-backend', async () => {
  const mock = await import('./dev/mock-ipc');
  return {
    invoke: mock.invoke,
    Channel: mock.Channel,
    listen: mock.listen,
    openFile: mock.openFile,
    openUrl: mock.openUrl,
    currentWindow: mock.currentWindow,
  };
});

const { api } = await import('./ipc');
const mockIpc = await import('./dev/mock-ipc');

const SRC = new URL('..', import.meta.url).pathname;

function sourceFiles(directory: string): string[] {
  const out: string[] = [];
  for (const entry of readdirSync(directory, { withFileTypes: true })) {
    const path = join(directory, entry.name);
    if (entry.isDirectory()) {
      if (entry.name !== 'node_modules') out.push(...sourceFiles(path));
    } else if (/\.(ts|svelte)$/.test(entry.name) && !entry.name.endsWith('.test.ts')) {
      out.push(path);
    }
  }
  return out;
}

/** A channel that records what the backend sends. */
function recordingChannel(): { channel: Channel<SessionUpdate>; updates: SessionUpdate[] } {
  const updates: SessionUpdate[] = [];
  const channel = new mockIpc.Channel<SessionUpdate>((update) => updates.push(update));
  return { channel: channel as unknown as Channel<SessionUpdate>, updates };
}

function decode(updates: SessionUpdate[]): string {
  const chunks = updates.flatMap((update) =>
    update.kind === 'data' ? [Uint8Array.from(atob(update.base64), (c) => c.charCodeAt(0))] : [],
  );
  return new TextDecoder().decode(Uint8Array.from(chunks.flatMap((chunk) => [...chunk])));
}

function states(updates: SessionUpdate[]): SessionStatus['state'][] {
  return updates.flatMap((update) => (update.kind === 'status' ? [update.status.state] : []));
}

describe('mock backend: command coverage', () => {
  beforeEach(() => mockIpc.resetMock());
  // Closes the sessions the probes opened (and their timers).
  afterEach(() => mockIpc.resetMock());

  const host: Host = {
    id: '00000000-0000-4000-8000-0000000000ff',
    name: 'probe',
    address: 'probe.test',
    port: 22,
    username: 'probe',
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
  };

  /** One call per `api` function: adding a command to `api` fails to compile until it is listed. */
  const calls: Record<keyof typeof api, () => Promise<unknown>> = {
    appInfo: () => api.appInfo(),
    hostList: () => api.hostList(),
    recentList: () => api.recentList(),
    hostSave: () => api.hostSave(host),
    hostExecReview: async () => {
      await api.hostSave(host);
      return api.hostExecReview(host.id);
    },
    hostExecApprove: async () => {
      await api.hostSave(host);
      const review = await api.hostExecReview(host.id);
      return api.hostExecApprove(host.id, review.hash);
    },
    hostDelete: () => api.hostDelete(host.id),
    hostSetFavorite: () => api.hostSetFavorite(host.id, true),
    groupList: () => api.groupList(),
    groupSave: () =>
      api.groupSave({
        id: 'g1',
        name: 'g',
        parent_id: null,
        sort_order: 0,
        created_at: host.created_at,
        updated_at: host.updated_at,
        deleted: false,
      }),
    groupDelete: () => api.groupDelete('g1'),
    tagList: () => api.tagList(),
    tagSave: () =>
      api.tagSave({
        id: 't1',
        name: 't',
        color: null,
        created_at: host.created_at,
        updated_at: host.updated_at,
        deleted: false,
      }),
    tagDelete: () => api.tagDelete('t1'),
    identityList: () => api.identityList(),
    identitySave: () =>
      api.identitySave({
        id: 'i1',
        name: 'i',
        comment: null,
        fingerprint: null,
        created_at: host.created_at,
        updated_at: host.updated_at,
        deleted: false,
      }),
    identityDelete: () => api.identityDelete('i1'),
    identityBind: () => api.identityBind('i1', '/home/demo/.ssh/key'),
    identityUnbind: () => api.identityUnbind('i1'),
    identityBindings: () => api.identityBindings(),
    identityGenerate: () => api.identityGenerate('probe key', null, null, false),
    identityPublicKey: () => api.identityPublicKey('i1'),
    prefsGet: () => api.prefsGet(),
    prefsSet: async () => api.prefsSet(await api.prefsGet()),
    sessionOpen: async () => {
      await api.hostSave(host);
      return api.sessionOpen(
        host.id,
        { cols: 80, rows: 24, secrets: {} },
        recordingChannel().channel,
      );
    },
    sessionRetry: async () => {
      const id = await openProbeSession();
      return api.sessionRetry(id, { cols: 80, rows: 24 }, recordingChannel().channel);
    },
    sessionDuplicate: async () => {
      const id = await openProbeSession();
      return api.sessionDuplicate(id, { cols: 80, rows: 24 }, recordingChannel().channel);
    },
    sessionClose: async () => api.sessionClose(await openProbeSession()),
    sessionDetach: async () => api.sessionDetach(await openProbeSession()),
    sessionWrite: async () => api.sessionWrite(await openProbeSession(), [108, 115, 13]),
    sessionResize: async () => api.sessionResize(await openProbeSession(), 100, 30),
    sessionList: () => api.sessionList(),
    hostKeyPromptResolve: () => api.hostKeyPromptResolve('none', true),
    authPromptResolve: () => api.authPromptResolve('none', ['x']),
    layoutSave: () => api.layoutSave({ version: 1, windows: [] }),
    layoutLoad: () => api.layoutLoad(),
    sessionAttach: async () =>
      api.sessionAttach(await openProbeSession(), recordingChannel().channel),
    windowOpenSession: async () => {
      // Only a connected session can move; the mock refuses like the backend.
      const id = await openProbeSession();
      return api.windowOpenSession(id, 'probe').catch((err: { message: string }) => err.message);
    },
    windowClose: () => api.windowClose('session-x'),
    secretSet: () => api.secretSet(host.id, 'password', 'secret'),
    secretDelete: () => api.secretDelete(host.id, 'password'),
    secretHas: () => api.secretHas(host.id, 'password'),
    sshConfigPreview: () => api.sshConfigPreview(),
    sshConfigImport: () => api.sshConfigImport(['bastion']),
    syncStatus: () => api.syncStatus(),
    syncSignIn: () => api.syncSignIn('https://sync.example.test', 'a@example.test', 'pw', 'dev'),
    syncSignOut: () => api.syncSignOut(),
    syncNow: async () => {
      await api.syncSignIn('https://sync.example.test', 'a@example.test', 'pw', 'dev');
      return api.syncNow();
    },
    syncConflicts: () => api.syncConflicts(),
    syncResolveConflict: () => api.syncResolveConflict('r1', true),
  };

  async function openProbeSession(): Promise<string> {
    await api.hostSave(host);
    return api.sessionOpen(
      host.id,
      { cols: 80, rows: 24, secrets: {} },
      recordingChannel().channel,
    );
  }

  it('lists every api function in this test', () => {
    expect(Object.keys(calls).sort()).toEqual(Object.keys(api).sort());
  });

  it.each(Object.keys(calls))('implements %s', async (name) => {
    const call = calls[name as keyof typeof api];
    // A void command answers `null`, like Tauri; only `undefined` is wrong.
    await expect(call()).resolves.not.toBeUndefined();
  });

  it('rejects an unknown command', async () => {
    await expect(mockIpc.invoke('no_such_command')).rejects.toThrow('unknown command');
  });

  it('implements app_info and every command it lists', () => {
    expect(mockIpc.mockCommands).toContain('app_info');
    expect(mockIpc.mockCommands).toContain('host_list');
  });
});

describe('mock backend: fixtures', () => {
  beforeEach(() => mockIpc.resetMock());

  it('holds the hosts of the design frames', async () => {
    const hosts = await api.hostList();
    expect(hosts.map((host) => host.name)).toEqual([
      'atlas-prod',
      'relay-edge',
      'staging-api',
      'build-runner',
      'archive',
    ]);
    const atlas = hosts[0];
    expect(`${atlas.username}@${atlas.address}:${atlas.port}`).toBe('ops@192.0.2.10:22');
    expect(hosts.filter((host) => host.favorite)).toHaveLength(3);
    expect(hosts.find((host) => host.name === 'build-runner')?.port).toBe(2222);
    expect(hosts.find((host) => host.name === 'archive')?.group_id).toBeNull();
  });

  it('nests Europe and Edge under Production', async () => {
    const groups = await api.groupList();
    const production = groups.find((group) => group.name === 'Production');
    expect(production).toBeDefined();
    expect(groups.filter((group) => group.parent_id === production?.id).map((g) => g.name)).toEqual(
      ['Europe', 'Edge'],
    );
    expect(groups.find((group) => group.name === 'Sandbox')?.parent_id).toBeNull();
  });

  it('knows the identities, the stored password and the recents', async () => {
    expect((await api.identityList()).map((identity) => identity.name)).toEqual([
      'work-ed25519',
      'build-ed25519',
    ]);
    const hosts = await api.hostList();
    const staging = hosts.find((host) => host.name === 'staging-api');
    const archive = hosts.find((host) => host.name === 'archive');
    expect(await api.secretHas(staging?.id ?? '', 'password')).toBe(true);
    expect(await api.secretHas(archive?.id ?? '', 'password')).toBe(false);
    const recents = await api.recentList();
    expect(recents.map((entry) => entry.host.name)).toEqual([
      'staging-api',
      'build-runner',
      'archive',
    ]);
    expect(new Date(recents[0].last_used_at).getHours()).toBe(9);
  });

  it('starts signed out of sync with the default preferences', async () => {
    expect((await api.syncStatus()).signed_in).toBe(false);
    expect((await api.prefsGet()).terminal.font_size).toBe(14);
  });

  it('mutates the in-memory state', async () => {
    const [first] = await api.hostList();
    await api.hostSetFavorite(first.id, false);
    expect((await api.hostList())[0].favorite).toBe(false);
    await api.hostDelete(first.id);
    expect((await api.hostList()).some((host) => host.id === first.id)).toBe(false);
    const saved = await api.hostSave({ ...first, id: 'new-host', name: 'new' });
    expect((await api.hostList()).some((host) => host.id === saved.id)).toBe(true);
  });

  it('returns copies: the UI never shares objects with the backend', async () => {
    const hosts = await api.hostList();
    hosts[0].name = 'changed';
    expect((await api.hostList())[0].name).toBe('atlas-prod');
  });

  it('supports the empty and error states', async () => {
    mockIpc.resetMock({ empty: true });
    expect(await api.hostList()).toEqual([]);
    expect(await api.recentList()).toEqual([]);
    mockIpc.resetMock({ error: true });
    await expect(api.hostList()).rejects.toMatchObject({ message: expect.any(String) });
  });
});

describe('mock backend: sessions', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    mockIpc.resetMock();
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  async function openFirstHost() {
    const [host] = await api.hostList();
    const { channel, updates } = recordingChannel();
    const id = await api.sessionOpen(host.id, { cols: 80, rows: 24, secrets: {} }, channel);
    return { id, updates, host };
  }

  it('streams connecting, ready and then the shell output', async () => {
    const { id, updates } = await openFirstHost();
    expect(updates).toEqual([]);
    await vi.advanceTimersByTimeAsync(2000);
    const order = states(updates);
    expect(order[0]).toBe('connecting');
    expect(order).toContain('ready');
    expect(order.indexOf('ready')).toBeGreaterThan(order.indexOf('connecting'));
    // The wire format of the real backend: {kind:'data', base64}.
    const data = updates.find((update) => update.kind === 'data');
    expect(data).toEqual({ kind: 'data', base64: expect.any(String) });
    const text = decode(updates);
    expect(text).toContain('cd /srv/atlas');
    expect(text).toContain('[ok]');
    expect(text).toContain('\x1b[');
    expect(text.endsWith('$ ')).toBe(true);
    expect(await api.sessionList()).toEqual([
      expect.objectContaining({ id, status: { state: 'ready' } }),
    ]);
  });

  it('echoes typed input and answers Enter with a new prompt', async () => {
    const { id, updates } = await openFirstHost();
    await vi.advanceTimersByTimeAsync(2000);
    updates.length = 0;
    await api.sessionWrite(id, [...new TextEncoder().encode('pwd')]);
    expect(decode(updates)).toBe('pwd');
    updates.length = 0;
    await api.sessionWrite(id, [13]);
    const answer = decode(updates);
    expect(answer.startsWith('\r\n')).toBe(true);
    expect(answer).toContain('/srv/atlas');
    expect(answer.endsWith('$ ')).toBe(true);
  });

  it('ignores typing in a log stream until Ctrl C', async () => {
    await openFirstHost();
    const [host] = await api.hostList();
    const { channel, updates } = recordingChannel();
    const id = await api.sessionOpen(host.id, { cols: 80, rows: 24, secrets: {} }, channel);
    await vi.advanceTimersByTimeAsync(2000);
    updates.length = 0;
    await api.sessionWrite(id, [...new TextEncoder().encode('x')]);
    expect(updates).toEqual([]);
    await api.sessionWrite(id, [3]);
    expect(decode(updates)).toMatch(/^\^C\r\n.*\$ $/);
  });

  it('replays the transcript on attach', async () => {
    const { id, updates } = await openFirstHost();
    await vi.advanceTimersByTimeAsync(2000);
    const attached = recordingChannel();
    await api.sessionAttach(id, attached.channel);
    expect(decode(attached.updates)).toBe(decode(updates));
    expect(states(attached.updates)).toEqual(['ready']);
  });

  it('closes sessions and rejects stale ids like the backend', async () => {
    const { id } = await openFirstHost();
    await api.sessionClose(id);
    expect(await api.sessionList()).toEqual([]);
    await expect(api.sessionWrite(id, [97])).rejects.toMatchObject({
      message: expect.stringMatching(/no session/i),
    });
  });

  it('shows a second atlas-prod session the log stream', async () => {
    await openFirstHost();
    const [host] = await api.hostList();
    const second = recordingChannel();
    await api.sessionOpen(host.id, { cols: 80, rows: 24, secrets: {} }, second.channel);
    await vi.advanceTimersByTimeAsync(2000);
    expect(decode(second.updates)).toContain('journalctl');
  });
});

describe('dev harness is dev-only', () => {
  const seam = readFileSync(new URL('./ipc-backend.ts', import.meta.url), 'utf8');

  it('loads the mock only behind import.meta.env.DEV', () => {
    // The runtime reference is one dynamic import (the other mention is a
    // type), and the DEV check comes first so a production build folds the
    // whole condition and drops the import.
    const runtime = seam.match(/await import\(\s*['"]\.\/dev\/mock-ipc['"]\s*\)/g) ?? [];
    expect(runtime).toHaveLength(1);
    expect(seam).toMatch(
      /import\.meta\.env\.DEV\s*&&[\s\S]{0,200}has\(\s*'mock'\s*\)\s*\?\s*await import\(\s*'\.\/dev\/mock-ipc'\s*\)/,
    );
    expect(seam).not.toMatch(/^import[^;]*dev\/mock-ipc/m);
  });

  it('is the only module that imports Tauri', () => {
    const offenders = sourceFiles(SRC)
      .filter((file) => /@tauri-apps\//.test(readFileSync(file, 'utf8')))
      .map((file) => relative(SRC, file));
    expect(offenders).toEqual(['lib/ipc-backend.ts']);
  });

  it('is the only module that references the mock', () => {
    const offenders = sourceFiles(SRC)
      .filter((file) => !file.includes('/lib/dev/'))
      .filter((file) => /dev\/mock-ipc/.test(readFileSync(file, 'utf8')))
      .map((file) => relative(SRC, file));
    expect(offenders).toEqual(['lib/ipc-backend.ts']);
  });
});

describe('mock backend: the sync flag', () => {
  it('starts signed out by default', async () => {
    mockIpc.resetMock();
    const status = await api.syncStatus();
    expect(status.signed_in).toBe(false);
    expect(await api.syncConflicts()).toEqual([]);
  });

  it('starts signed in with `&sync`, with conflicts on `&sync=conflicts`', async () => {
    mockIpc.resetMock({ sync: 'in' });
    expect((await api.syncStatus()).signed_in).toBe(true);
    expect((await api.syncStatus()).conflicts).toBe(0);

    mockIpc.resetMock({ sync: 'conflicts' });
    const status = await api.syncStatus();
    expect(status.conflicts).toBe(2);
    expect(await api.syncConflicts()).toHaveLength(2);
    await api.syncResolveConflict((await api.syncConflicts())[0].record_id, true);
    expect(await api.syncConflicts()).toHaveLength(1);

    mockIpc.resetMock({ sync: 'error' });
    expect((await api.syncStatus()).last_error).toContain('connection refused');
    mockIpc.resetMock();
  });

  it('reports the version and opens links without a browser', async () => {
    mockIpc.resetMock();
    expect((await api.appInfo()).version).toMatch(/\d/);
    await mockIpc.openUrl('https://example.com/');
    expect(mockIpc.openedUrls.at(-1)).toBe('https://example.com/');
  });
});
