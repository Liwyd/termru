import { api } from '../ipc';
import type { Group, Host, Identity, Tag } from '../types';

/** Hosts + identities, loaded from the local store. */
export class HostsStore {
  hosts = $state<Host[]>([]);
  identities = $state<Identity[]>([]);
  recents = $state<Host[]>([]);
  /** The folders the hosts can be put in. */
  groups = $state<Group[]>([]);
  /** The labels of the hosts. */
  tags = $state<Tag[]>([]);
  loading = $state(false);
  error = $state<string | null>(null);
  query = $state('');
  /** Editor dialog state: `editing` = host being edited (`null` = new host). */
  editorOpen = $state(false);
  editing = $state<Host | null>(null);

  /**
   * Hosts matching the current search, favorites first, then by name.
   *
   * The search covers the name, the address, the username, the description
   * and the tags.
   */
  get visible(): Host[] {
    const needle = this.query.trim().toLowerCase();
    const filtered = needle ? this.hosts.filter((host) => this.matches(host, needle)) : this.hosts;
    return [...filtered].sort((a, b) => {
      if (a.favorite !== b.favorite) return a.favorite ? -1 : 1;
      return a.name.localeCompare(b.name);
    });
  }

  /** True when the host matches the search. */
  matches(host: Host, needle: string): boolean {
    if (
      host.name.toLowerCase().includes(needle) ||
      host.address.toLowerCase().includes(needle) ||
      host.username.toLowerCase().includes(needle) ||
      (host.description ?? '').toLowerCase().includes(needle)
    ) {
      return true;
    }
    // The tag names, not the tag ids.
    return host.tags.some((tagId) => {
      const tag = this.tags.find((entry) => entry.id === tagId);
      return tag ? tag.name.toLowerCase().includes(needle) : false;
    });
  }

  get favorites(): Host[] {
    return this.hosts.filter((host) => host.favorite);
  }

  /**
   * The folders, with the path of every folder (the parents joined by
   * " / "). The cycles of the data (a folder that is its own ancestor) are
   * broken: the path stops instead of looping.
   */
  get folderOptions(): { id: string; label: string }[] {
    const byId = new Map(this.groups.map((group) => [group.id, group]));
    const options: { id: string; label: string }[] = [];
    for (const group of this.groups) {
      const parts: string[] = [];
      let current: Group | undefined = group;
      const seen = new Set<string>();
      while (current && !seen.has(current.id)) {
        seen.add(current.id);
        parts.unshift(current.name);
        current = current.parent_id ? byId.get(current.parent_id) : undefined;
      }
      options.push({ id: group.id, label: parts.join(' / ') });
    }
    return options.sort((a, b) => a.label.localeCompare(b.label));
  }

  /**
   * The visible hosts, grouped by their folder. The hosts without a folder
   * come first, then the folders in alphabetical order.
   */
  get grouped(): { id: string; label: string; hosts: Host[] }[] {
    const visible = this.visible;
    const byFolder = new Map<string, Host[]>();
    const loose: Host[] = [];
    for (const host of visible) {
      if (!host.group_id) {
        loose.push(host);
        continue;
      }
      const list = byFolder.get(host.group_id) ?? [];
      list.push(host);
      byFolder.set(host.group_id, list);
    }
    const sections = [
      ...(loose.length > 0 ? [{ id: '', label: 'Hosts', hosts: loose }] : []),
      ...this.folderOptions
        .filter((option) => byFolder.has(option.id))
        .map((option) => ({
          id: option.id,
          label: option.label,
          hosts: byFolder.get(option.id) ?? [],
        })),
    ];
    return sections;
  }

  async load(): Promise<void> {
    this.loading = true;
    this.error = null;
    try {
      const [hosts, identities, recents, groups, tags] = await Promise.all([
        api.hostList(),
        api.identityList(),
        api.recentList(),
        api.groupList(),
        api.tagList(),
      ]);
      this.hosts = (hosts ?? []).filter((host) => !host.deleted);
      this.identities = (identities ?? []).filter((identity) => !identity.deleted);
      this.recents = (recents ?? []).filter((host) => !host.deleted);
      this.groups = (groups ?? []).filter((group) => !group.deleted);
      this.tags = (tags ?? []).filter((tag) => !tag.deleted);
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
