import { beforeEach, describe, expect, it, vi } from 'vitest';
import { SyncStore } from './sync.svelte';
import type { ConflictView, SyncStatus } from '../types';

const status: SyncStatus = {
  settings: {
    server_url: 'https://sync.example.com',
    email: 'user@example.com',
    device_name: 'laptop',
    device_id: 'device-1',
  },
  signed_in: true,
  pending: 2,
  conflicts: 1,
  cursor: 7,
  last_sync_at: null,
  last_error: null,
};

const conflict: ConflictView = {
  record_id: 'host-1',
  kind: 'host',
  server_rev: 9,
  observed_at: '2026-01-01T00:00:00Z',
  local: { id: 'host-1', name: 'local' },
  remote: { id: 'host-1', name: 'remote' },
};

interface InvokeCall {
  cmd: string;
  args: Record<string, unknown> | undefined;
}

/** The answers the fake backend gives, per command. */
const answers = vi.hoisted(() => ({
  calls: [] as InvokeCall[],
  table: {} as Record<string, unknown>,
  failure: null as string | null,
}));

vi.mock('@tauri-apps/api/core', () => ({
  invoke: async (cmd: string, args?: Record<string, unknown>) => {
    answers.calls.push({ cmd, args });
    if (answers.failure && cmd === 'sync_now') {
      throw new Error(answers.failure);
    }
    return answers.table[cmd];
  },
}));

describe('SyncStore', () => {
  beforeEach(() => {
    answers.calls = [];
    answers.failure = null;
    answers.table = {
      sync_status: status,
      sync_conflicts: [conflict],
      sync_sign_in: status,
      sync_sign_out: null,
      sync_now: { pushed: 1, pulled: 2, applied: 2, conflicts: 0, cursor: 9 },
      sync_resolve_conflict: null,
    };
  });

  it('reads the status and the conflicts', async () => {
    const store = new SyncStore();
    await store.load();
    expect(store.signedIn).toBe(true);
    expect(store.pending).toBe(2);
    expect(store.conflictCount).toBe(1);
    expect(store.conflicts).toHaveLength(1);
  });

  it('survives a backend that answers with null', async () => {
    answers.table = { sync_status: null, sync_conflicts: null };
    const store = new SyncStore();
    await store.load();
    expect(store.signedIn).toBe(false);
    expect(store.conflictCount).toBe(0);
  });

  it('reports the failure of a cycle instead of throwing', async () => {
    answers.failure = 'the sync server could not be reached';
    const store = new SyncStore();
    const outcome = await store.syncNow();
    expect(outcome).toBeNull();
    expect(store.error).toContain('could not be reached');
  });

  it('resolves a conflict and reloads the list', async () => {
    const store = new SyncStore();
    await store.resolve('host-1', true);
    const call = answers.calls.find((entry) => entry.cmd === 'sync_resolve_conflict');
    expect(call?.args).toEqual({ recordId: 'host-1', keepLocal: true });
  });

  it('signs in and reloads the status', async () => {
    const store = new SyncStore();
    const ok = await store.signIn('https://sync.example.com', 'user@example.com', 'pw', 'laptop');
    expect(ok).toBe(true);
    const call = answers.calls.find((entry) => entry.cmd === 'sync_sign_in');
    expect(call?.args).toEqual({
      serverUrl: 'https://sync.example.com',
      email: 'user@example.com',
      password: 'pw',
      deviceName: 'laptop',
    });
  });
});
