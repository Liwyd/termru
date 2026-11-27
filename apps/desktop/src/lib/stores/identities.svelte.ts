import { api } from '../ipc';
import { hosts } from './hosts.svelte';
import type { GeneratedIdentity, Host, Identity, PublicKeyInfo } from '../types';

/** What is known about the key an identity is bound to. */
export type KeyState =
  | { status: 'loading' }
  /** No key file is bound on this device. */
  | { status: 'none' }
  | { status: 'ready'; info: PublicKeyInfo }
  /** A key file is bound but its public key cannot be read. */
  | { status: 'error'; message: string };

/** The fields of the "generate a key" form. */
export interface GenerateInput {
  name: string;
  comment: string;
  passphrase: string;
  rememberPassphrase: boolean;
}

function messageOf(err: unknown): string {
  if (typeof err === 'string') return err;
  if (err && typeof err === 'object' && 'message' in err) {
    const message = (err as { message: unknown }).message;
    if (typeof message === 'string' && message.length > 0) return message;
  }
  return String(err);
}

/**
 * The identities page's data beyond the list itself (which `hosts` holds,
 * because the host editor picks from it): the device-local key bindings and
 * the public key of each bound file, read on demand and cached.
 *
 * Every mutation throws the backend's message; the page shows it next to
 * its form, so a refusal is never swallowed.
 */
export class IdentitiesStore {
  /** identity id -> key file path (device-local, never synced). */
  bindings = $state.raw<Record<string, string>>({});
  /** identity id -> what the bound key file says (see `ensureKey`). */
  keys = $state.raw<Record<string, KeyState>>({});
  /** The identity the identities page has selected (it survives a visit to another page). */
  selectedId = $state<string | null>(null);
  /** The bindings were read at least once. */
  bindingsLoaded = $state(false);

  async loadBindings(): Promise<void> {
    const list = (await api.identityBindings()) ?? [];
    const map: Record<string, string> = {};
    for (const binding of list) map[binding.identityId] = binding.keyPath;
    this.bindings = map;
    this.bindingsLoaded = true;
  }

  /**
   * Reads the public key of an identity's bound file once: the answer (or the
   * reason it could not be read) is cached, and a read in flight is not
   * started twice. `force` reads again (a new key was bound).
   */
  async ensureKey(identity: Identity, force = false): Promise<void> {
    if (!force && this.keys[identity.id]) return;
    this.keys = { ...this.keys, [identity.id]: { status: 'loading' } };
    try {
      const info = await api.identityPublicKey(identity.id);
      this.#setKey(identity.id, info ? { status: 'ready', info } : { status: 'none' });
    } catch (err) {
      this.#setKey(identity.id, { status: 'error', message: messageOf(err) });
    }
  }

  #setKey(id: string, state: KeyState): void {
    this.keys = { ...this.keys, [id]: state };
  }

  #forget(id: string): void {
    const bindings = { ...this.bindings };
    delete bindings[id];
    this.bindings = bindings;
    const keys = { ...this.keys };
    delete keys[id];
    this.keys = keys;
  }

  /** Creates an identity without a key (a key file is chosen afterwards). */
  async create(name: string, comment: string): Promise<Identity> {
    const now = new Date().toISOString();
    const saved = await api.identitySave({
      id: crypto.randomUUID(),
      name: name.trim(),
      comment: comment.trim() ? comment.trim() : null,
      fingerprint: null,
      created_at: now,
      updated_at: now,
      deleted: false,
    });
    await hosts.reloadIdentities();
    return saved;
  }

  /** Renames an identity and changes its comment. */
  async rename(identity: Identity, name: string, comment: string): Promise<Identity> {
    const saved = await api.identitySave({
      ...identity,
      name: name.trim(),
      comment: comment.trim() ? comment.trim() : null,
    });
    await hosts.reloadIdentities();
    return saved;
  }

  /** Binds the key file at `path` to the identity. */
  async bind(identity: Identity, path: string): Promise<void> {
    await api.identityBind(identity.id, path);
    this.bindings = { ...this.bindings, [identity.id]: path };
    await this.ensureKey(identity, true);
  }

  async unbind(identity: Identity): Promise<void> {
    await api.identityUnbind(identity.id);
    this.#forget(identity.id);
  }

  async remove(identity: Identity): Promise<void> {
    await api.identityDelete(identity.id);
    this.#forget(identity.id);
    // Hosts keep their (now dangling) reference until the next read.
    await hosts.reloadIdentities();
  }

  /** Generates an ed25519 key and the identity for it. */
  async generate(input: GenerateInput): Promise<GeneratedIdentity> {
    const passphrase = input.passphrase.length > 0 ? input.passphrase : null;
    const made = await api.identityGenerate(
      input.name.trim(),
      input.comment.trim() ? input.comment.trim() : null,
      passphrase,
      passphrase !== null && input.rememberPassphrase,
    );
    this.bindings = { ...this.bindings, [made.identity.id]: made.key_path };
    // The answer already carries the key, so it is not read again.
    this.#setKey(made.identity.id, {
      status: 'ready',
      info: {
        public_key: made.public_key,
        fingerprint: made.fingerprint,
        algorithm: 'ssh-ed25519',
        encrypted: passphrase !== null,
        key_path: made.key_path,
      },
    });
    await hosts.reloadIdentities();
    return made;
  }

  /** The hosts that sign in with an identity. */
  hostsUsing(identity: Identity): Host[] {
    return hosts.hosts.filter(
      (host) => host.auth.type === 'private_key' && host.auth.identity_id === identity.id,
    );
  }

  /** The sentence that says what deleting the identity affects. */
  deleteBody(identity: Identity): string {
    const users = this.hostsUsing(identity);
    const kept = this.bindings[identity.id]
      ? ` The key file stays on this device at ${this.bindings[identity.id]}.`
      : ' No key file is bound, so no file is touched.';
    if (users.length === 0) return `No host uses this identity.${kept}`;
    const names = users.slice(0, 5).map((host) => host.name);
    const more = users.length > names.length ? ` and ${users.length - names.length} more` : '';
    const count = users.length === 1 ? '1 host uses' : `${users.length} hosts use`;
    return `${count} this identity: ${names.join(', ')}${more}. They can no longer sign in with a key until you pick another identity.${kept}`;
  }
}

export const identities = new IdentitiesStore();
