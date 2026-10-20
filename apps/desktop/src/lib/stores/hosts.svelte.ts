import { api } from '../ipc';
import type { Host, Identity } from '../types';

/** Hosts + identities, loaded from the local store. */
export class HostsStore {
  hosts = $state<Host[]>([]);
  identities = $state<Identity[]>([]);
  loading = $state(false);
  error = $state<string | null>(null);
  query = $state('');
  /** Editor dialog state: `editing` = host being edited (`null` = new host). */
  editorOpen = $state(false);
  editing = $state<Host | null>(null);

  /** Hosts matching the current search, favorites first, then by name. */
  get visible(): Host[] {
    const needle = this.query.trim().toLowerCase();
    const filtered = needle
      ? this.hosts.filter(
          (host) =>
            host.name.toLowerCase().includes(needle) ||
            host.address.toLowerCase().includes(needle) ||
            host.username.toLowerCase().includes(needle),
        )
      : this.hosts;
    return [...filtered].sort((a, b) => {
      if (a.favorite !== b.favorite) return a.favorite ? -1 : 1;
      return a.name.localeCompare(b.name);
    });
  }

  get favorites(): Host[] {
    return this.hosts.filter((host) => host.favorite);
  }

  async load(): Promise<void> {
    this.loading = true;
    this.error = null;
    try {
      const [hosts, identities] = await Promise.all([api.hostList(), api.identityList()]);
      this.hosts = hosts.filter((host) => !host.deleted);
      this.identities = identities.filter((identity) => !identity.deleted);
    } catch (err) {
      this.error = errorMessage(err);
    } finally {
      this.loading = false;
    }
  }

  async save(host: Host): Promise<Host | null> {
    try {
      const saved = await api.hostSave(host);
      const index = this.hosts.findIndex((entry) => entry.id === saved.id);
      if (index >= 0) {
        this.hosts[index] = saved;
      } else {
        this.hosts = [...this.hosts, saved];
      }
      return saved;
    } catch (err) {
      this.error = errorMessage(err);
      return null;
    }
  }

  async remove(id: string): Promise<void> {
    try {
      await api.hostDelete(id);
      this.hosts = this.hosts.filter((host) => host.id !== id);
    } catch (err) {
      this.error = errorMessage(err);
    }
  }

  async toggleFavorite(host: Host): Promise<void> {
    const next = !host.favorite;
    try {
      await api.hostSetFavorite(host.id, next);
      const index = this.hosts.findIndex((entry) => entry.id === host.id);
      if (index >= 0) this.hosts[index] = { ...this.hosts[index], favorite: next };
    } catch (err) {
      this.error = errorMessage(err);
    }
  }
}

/** Normalizes an unknown thrown value into a user-facing message. */
export function errorMessage(err: unknown): string {
  if (typeof err === 'string') return err;
  if (err && typeof err === 'object' && 'message' in err) {
    const message = (err as { message: unknown }).message;
    if (typeof message === 'string' && message.length > 0) return message;
  }
  return String(err);
}

export const hosts = new HostsStore();
