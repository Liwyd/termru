import type { Host, Identity } from './types';

/**
 * The Authentication cell of a host row (frame 1), derived from the auth
 * method of the host:
 *
 * - `SSH agent`
 * - `Password · This device` when the password sits in the OS credential
 *   store, otherwise `Password · Ask on connect`
 * - `Identity · <name>` for a private key (the identity is resolved by id;
 *   a removed identity is reported, never invented)
 */
export function authLabel(host: Host, identities: Identity[], passwordStored = false): string {
  if (host.auth.type === 'agent') return 'SSH agent';
  if (host.auth.type === 'password') {
    return passwordStored ? 'Password · This device' : 'Password · Ask on connect';
  }
  const { identity_id: identityId } = host.auth;
  const identity = identities.find((entry) => entry.id === identityId && !entry.deleted);
  return identity ? `Identity · ${identity.name}` : 'Identity · missing';
}
