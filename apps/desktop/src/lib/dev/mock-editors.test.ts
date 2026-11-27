/**
 * The dev mock backend, as far as the host editor, the folders page and the
 * identities page use it: it has to refuse what the real backend refuses,
 * in its words, or the harness would hide the states those pages handle.
 */

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { Group, Host } from '../types';

vi.mock('../ipc-backend', async () => {
  const mock = await import('./mock-ipc');
  return {
    invoke: mock.invoke,
    Channel: mock.Channel,
    listen: mock.listen,
    openFile: mock.openFile,
    currentWindow: mock.currentWindow,
  };
});

const { api } = await import('../ipc');
const mockIpc = await import('./mock-ipc');

async function refusal(promise: Promise<unknown>): Promise<string> {
  try {
    await promise;
  } catch (err) {
    return (err as { message: string }).message;
  }
  throw new Error('expected a refusal');
}

describe('the mock backend: hosts', () => {
  beforeEach(() => mockIpc.resetMock());
  afterEach(() => mockIpc.resetMock());

  it('refuses a host the way validate.rs does', async () => {
    const [host] = await api.hostList();
    const refused = (changes: Partial<Host>) => refusal(api.hostSave({ ...host, ...changes }));
    expect(await refused({ name: '  ' })).toBe('name must not be empty');
    expect(await refused({ address: 'host name' })).toContain(
      'address contains invalid characters',
    );
    expect(await refused({ address: '-bad' })).toBe("address must not start or end with '-'");
    expect(await refused({ port: 0 })).toBe('port must be between 1 and 65535');
    expect(await refused({ username: 'a;b' })).toContain('username contains invalid characters');
    expect(await refused({ term: 'x y' })).toContain('term contains invalid characters');
    expect(await refused({ keepalive_seconds: 4000 })).toContain('keepalive interval');
    // A valid change goes through.
    expect((await api.hostSave({ ...host, name: 'renamed' })).name).toBe('renamed');
  });
});

describe('the mock backend: folders', () => {
  beforeEach(() => mockIpc.resetMock());
  afterEach(() => mockIpc.resetMock());

  async function folder(name: string): Promise<Group> {
    const found = (await api.groupList()).find((group) => group.name === name);
    if (!found) throw new Error(name);
    return found;
  }

  it('refuses a folder under itself or under its own descendant', async () => {
    const production = await folder('Production');
    const europe = await folder('Europe');
    expect(await refusal(api.groupSave({ ...production, parent_id: production.id }))).toBe(
      'a group cannot be its own parent',
    );
    expect(await refusal(api.groupSave({ ...production, parent_id: europe.id }))).toBe(
      'group hierarchy contains a cycle',
    );
    // A legal move is accepted.
    const sandbox = await folder('Sandbox');
    expect((await api.groupSave({ ...europe, parent_id: sandbox.id })).parent_id).toBe(sandbox.id);
  });

  it('refuses a nesting deeper than the limit', async () => {
    let parent: string | null = null;
    const now = new Date().toISOString();
    for (let level = 0; level <= 8; level += 1) {
      const made: Group = await api.groupSave({
        id: `deep-${level}`,
        name: `level ${level}`,
        parent_id: parent,
        sort_order: 0,
        created_at: now,
        updated_at: now,
        deleted: false,
      });
      parent = made.id;
    }
    const message = await refusal(
      api.groupSave({
        id: 'too-deep',
        name: 'too deep',
        parent_id: parent,
        sort_order: 0,
        created_at: now,
        updated_at: now,
        deleted: false,
      }),
    );
    expect(message).toBe('group nesting exceeds the maximum depth of 8');
  });

  it('moves the hosts and the subfolders of a deleted folder up, as the store does', async () => {
    const production = await folder('Production');
    const europe = await folder('Europe');
    await api.groupDelete(europe.id);
    const hosts = await api.hostList();
    expect(hosts.find((host) => host.name === 'atlas-prod')?.group_id).toBe(production.id);

    await api.groupDelete(production.id);
    const after = await api.groupList();
    expect(after.find((group) => group.name === 'Edge')?.parent_id).toBeNull();
    expect((await api.hostList()).find((host) => host.name === 'atlas-prod')?.group_id).toBeNull();
  });
});

describe('the mock backend: identity keys', () => {
  beforeEach(() => mockIpc.resetMock());
  afterEach(() => mockIpc.resetMock());

  it('generates a key, an identity for it and its binding', async () => {
    const made = await api.identityGenerate('new laptop', ' for work ', null, false);
    expect(made.identity.name).toBe('new laptop');
    expect(made.identity.comment).toBe('for work');
    expect(made.fingerprint).toMatch(/^SHA256:[A-Za-z0-9]{43}$/);
    expect(made.public_key).toMatch(/^ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAI\S+ new laptop$/);
    expect(made.passphrase_remembered).toBe(false);
    expect((await api.identityList()).some((identity) => identity.id === made.identity.id)).toBe(
      true,
    );
    expect(
      (await api.identityBindings()).find((b) => b.identityId === made.identity.id)?.keyPath,
    ).toBe(made.key_path);
    const info = await api.identityPublicKey(made.identity.id);
    expect(info).toMatchObject({
      public_key: made.public_key,
      fingerprint: made.fingerprint,
      algorithm: 'ssh-ed25519',
      encrypted: false,
    });
  });

  it('encrypts with a passphrase and refuses a short one', async () => {
    const made = await api.identityGenerate('locked', null, 'correct horse', true);
    expect(made.passphrase_remembered).toBe(true);
    expect((await api.identityPublicKey(made.identity.id))?.encrypted).toBe(true);
    expect(await refusal(api.identityGenerate('weak', null, 'abc', false))).toContain('at least 5');
    expect(await refusal(api.identityGenerate('  ', null, null, false))).toContain('name');
  });

  it('shows the public key of a bound fixture, and nothing for an unbound identity', async () => {
    const [work] = await api.identityList();
    const info = await api.identityPublicKey(work.id);
    expect(info?.fingerprint).toBe(work.fingerprint);
    await api.identityUnbind(work.id);
    expect(await api.identityPublicKey(work.id)).toBeNull();
  });
});

describe('the mock backend: stress flags', () => {
  afterEach(() => mockIpc.resetMock());

  it('&many adds that many generated hosts to the fixtures, with recent connections on the first', async () => {
    mockIpc.resetMock({ many: 1_200 });
    const hosts = await api.hostList();
    expect(hosts).toHaveLength(5 + 1_200);
    expect(new Set(hosts.map((host) => host.id)).size).toBe(hosts.length);
    const recents = await api.recentList();
    expect(recents.length).toBeGreaterThan(40);
  });

  it('&long keeps every value inside what the backend accepts (validate.rs, store)', async () => {
    mockIpc.resetMock({ long: true });
    const hosts = await api.hostList();
    const groups = await api.groupList();
    // The worst values are really there ...
    expect(Math.max(...hosts.map((host) => [...host.name].length))).toBeGreaterThan(80);
    expect(hosts.some((host) => host.name === 'x')).toBe(true);
    expect(hosts.some((host) => (host.description ?? '').length > 1_900)).toBe(true);
    expect(hosts.filter((host) => host.name === 'archive')).toHaveLength(2);
    // ... and none passes a limit of `validation.ts`.
    for (const host of hosts) {
      expect([...host.name].length).toBeLessThanOrEqual(128);
      expect([...host.address].length).toBeLessThanOrEqual(253);
      expect([...host.username].length).toBeLessThanOrEqual(128);
      expect([...(host.description ?? '')].length).toBeLessThanOrEqual(2_000);
      expect(host.tags.length).toBeLessThanOrEqual(32);
    }
    // A folder five levels deep (the limit is eight).
    const byId = new Map(groups.map((group) => [group.id, group]));
    const depth = (id: string | null): number =>
      id ? 1 + depth(byId.get(id)?.parent_id ?? null) : 0;
    expect(Math.max(...groups.map((group) => depth(group.id)))).toBe(5);
    for (const group of groups) expect([...group.name].length).toBeLessThanOrEqual(128);
  });

  it('is the plain fixtures without the flags', async () => {
    mockIpc.resetMock();
    expect(await api.hostList()).toHaveLength(5);
  });
});
