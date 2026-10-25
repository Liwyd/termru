import { describe, expect, it } from 'vitest';
import { authLabel } from './auth-label';
import type { Host, Identity } from './types';

function host(auth: Host['auth']): Host {
  const now = new Date().toISOString();
  return {
    id: 'host-1',
    name: 'web',
    address: 'web.test',
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

function identity(id: string, name: string, deleted = false): Identity {
  const now = new Date().toISOString();
  return { id, name, comment: null, fingerprint: null, created_at: now, updated_at: now, deleted };
}

describe('authLabel', () => {
  it('names the ssh agent', () => {
    expect(authLabel(host({ type: 'agent' }), [])).toBe('SSH agent');
  });

  it('tells an asked-for password from a stored one', () => {
    const password = host({ type: 'password' });
    expect(authLabel(password, [])).toBe('Password · Ask on connect');
    expect(authLabel(password, [], true)).toBe('Password · This device');
  });

  it('names the identity of a private key', () => {
    const identities = [identity('identity-1', 'work laptop')];
    expect(authLabel(host({ type: 'private_key', identity_id: 'identity-1' }), identities)).toBe(
      'Identity · work laptop',
    );
  });

  it('reports a missing identity instead of inventing one', () => {
    const identities = [identity('identity-1', 'work laptop')];
    expect(authLabel(host({ type: 'private_key', identity_id: 'gone' }), identities)).toBe(
      'Identity · missing',
    );
  });

  it('reports a deleted identity as missing', () => {
    const identities = [identity('identity-1', 'work laptop', true)];
    expect(authLabel(host({ type: 'private_key', identity_id: 'identity-1' }), identities)).toBe(
      'Identity · missing',
    );
  });
});
