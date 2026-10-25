import { api } from '../ipc';
import type { Group, Host, Identity, RecentHost, Tag } from '../types';

/** The filter tabs of the hosts page (frame 1). */
export type HostFilterTab = 'all' | 'favorites' | 'recent' | 'unfiled';

/** Hosts + identities, loaded from the local store. */
export class HostsStore {
  /**
   * The host lists are raw state: 10 000 hosts must be filterable and
   * sortable inside a frame budget, and a deep proxy around every host
   * costs ~50 ms per pass. Every mutation therefore replaces the array.
   */
  hosts = $state.raw<Host[]>([]);
  identities = $state.raw<Identity[]>([]);
  /** The folders the hosts can be put in. */
  groups = $state.raw<Group[]>([]);
  /** The labels of the hosts. */
  tags = $state.raw<Tag[]>([]);
  loading = $state(false);
  error = $state<string | null>(null);
  query = $state('');
  /** The active filter tab of the hosts table. */
  filterTab = $state<HostFilterTab>('all');
  /** The folder the rows are restricted to ('' = every folder). */
  folderFilter = $state('');
  /** Last connection time per host id (ms since the epoch). */
  recentAt = $state.raw<Record<string, number>>({});
  /** Known state of the stored password per host id; unknown = absent. */
  passwordStored = $state.raw<Record<string, boolean>>({});
  /** Editor dialog state: `editing` = host being edited (`null` = new host). */
  editorOpen = $state(false);
  editing = $state<Host | null>(null);

  /**
   * Hosts matching the search box, in table order (the command palette
   * connects to any host, so this is the only filter it applies).
   */
  get visible(): Host[] {
    const needle = this.query.trim().toLowerCase();
    if (!needle) return this.sortedAll;
    const index = this.searchIndex;
    return this.sortedAll.filter((host) => (index.get(host.id) ?? '').includes(needle));
  }

  /**
   * The lowercase search text of every host (name, address, username,
   * description, tag names — the same fields `matches` covers), joined so
   * that one field can never match across into the next one. Derived: it
   * is rebuilt when the data changes, not on every keystroke (10 000
   * `toLowerCase` calls per keystroke are ~40 ms).
   */
  searchIndex = $derived.by(() => {
    const index = new Map<string, string>();
    for (const host of this.hosts) {
      const tagNames = host.tags
        .map((tagId) => this.tags.find((entry) => entry.id === tagId)?.name ?? '')
        .join('\n');
      index.set(
        host.id,
        [host.name, host.address, host.username, host.description ?? '', tagNames]
          .join('\n')
          .toLowerCase(),
      );
    }
    return index;
  });

  /**
   * Every host in table order (see `byRecent`). The full set is sorted
   * once per data change, so every keystroke is a pure filter: re-sorting
   * 10 000 hosts costs ~150 ms, filtering them costs ~6 ms.
   */
  sortedAll = $derived.by(() => [...this.hosts].sort((a, b) => this.byRecent(a, b)));

  /**
   * The hosts matching the search box and the folder dropdown. The folder
   * includes the folders nested under it (choosing "Production" also
   * shows "Production / Europe"). Derived: one pass per change, not one
   * per template expression.
   */
  searched = $derived.by(() => {
    // The index is read even without a needle: it is data work, and it
    // belongs to the moment the data changes — not to the first keystroke.
    const index = this.searchIndex;
    const needle = this.query.trim().toLowerCase();
    let bySearch = this.sortedAll;
    if (needle) {
      bySearch = this.sortedAll.filter((host) => (index.get(host.id) ?? '').includes(needle));
    }
    if (!this.folderFilter) return bySearch;
    const byId = new Map(this.groups.map((group) => [group.id, group]));
    return bySearch.filter((host) => this.inFolder(host, this.folderFilter, byId));
  });

  /** True when the folder of a host is (or is nested under) `folderId`. */
  inFolder(host: Host, folderId: string, byId?: Map<string, Group>): boolean {
    if (!host.group_id) return false;
    const index = byId ?? new Map(this.groups.map((group) => [group.id, group]));
    let current: Group | undefined = index.get(host.group_id);
    const seen = new Set<string>();
    while (current && !seen.has(current.id)) {
      if (current.id === folderId) return true;
      seen.add(current.id);
      current = current.parent_id ? index.get(current.parent_id) : undefined;
    }
    return false;
  }

  /**
   * The sort of the hosts table: most recent connection first, never-used
   * hosts last; favorites before the rest; then by name. The name compare
   * is a codepoint comparison on purpose: `localeCompare` costs ~4 µs per
   * call, which is half a second over 10 000 hosts.
   */
  byRecent(a: Host, b: Host): number {
    const ra = this.recentAt[a.id] ?? 0;
    const rb = this.recentAt[b.id] ?? 0;
    if (ra !== rb) return rb - ra;
    if (a.favorite !== b.favorite) return a.favorite ? -1 : 1;
    if (a.name < b.name) return -1;
    if (a.name > b.name) return 1;
    return 0;
  }

  /**
   * The rows of the hosts table: the search box, the folder dropdown and
   * the active filter tab, in recent-activity order (the order of
   * `sortedAll`: filtering a sorted list keeps its order, so no tab
   * switch re-sorts). Derived: the flush of a keystroke computes it once,
   * not once per rendered expression.
   */
  rows = $derived.by(() => {
    const base = this.searched;
    switch (this.filterTab) {
      case 'favorites':
        return base.filter((host) => host.favorite);
      case 'recent':
        return base.filter((host) => this.recentAt[host.id] !== undefined);
      case 'unfiled':
        return base.filter((host) => !host.group_id);
      default:
        return base;
    }
  });

  /**
   * The counts of the filter tabs (over the search + folder selection).
   * One pass, with `recentAt` hoisted: three `filter` passes over 10 000
   * hosts cost ~30 ms, one loop costs a few.
   */
  counts = $derived.by(() => {
    const base = this.searched;
    const recentAt = this.recentAt;
    let favorites = 0;
    let recent = 0;
    let unfiled = 0;
    for (const host of base) {
      if (host.favorite) favorites += 1;
      if (recentAt[host.id] !== undefined) recent += 1;
      if (!host.group_id) unfiled += 1;
    }
    return { all: base.length, favorites, recent, unfiled };
  });

  /** The last connection time of a host (ms), `null` when never used. */
  lastUsed(host: Host): number | null {
    return this.recentAt[host.id] ?? null;
  }

  /** Resets the search box, the folder dropdown and the filter tab. */
  clearFilters(): void {
    this.query = '';
    this.folderFilter = '';
    this.filterTab = 'all';
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
      this.recentAt = recentMap(recents);
      this.groups = (groups ?? []).filter((group) => !group.deleted);
      this.tags = (tags ?? []).filter((tag) => !tag.deleted);
    } catch (err) {
      this.error = errorMessage(err);
    } finally {
      this.loading = false;
    }
  }

  /** Re-reads the last connection times (the table shows them live). */
  async refreshRecents(): Promise<void> {
    try {
      this.recentAt = recentMap(await api.recentList());
    } catch {
      // The dates are decoration: a failing refresh keeps the old ones.
    }
  }

  /**
   * Reads whether the password of a host sits in the OS credential store
   * (for the Authentication cell). The answer is cached per host; the
   * lookup only runs for the rows actually on screen.
   */
  async ensurePasswordState(host: Host): Promise<void> {
    if (host.auth.type !== 'password') return;
    if (this.passwordStored[host.id] !== undefined) return;
    try {
      this.markPasswordStored(host.id, (await api.secretHas(host.id, 'password')) === true);
    } catch {
      // Unknown stays unknown: the label keeps its "Ask on connect" default.
    }
  }

  /** Records the known state of the stored password of a host. */
  markPasswordStored(hostId: string, stored: boolean): void {
    this.passwordStored = { ...this.passwordStored, [hostId]: stored };
  }

  async save(host: Host): Promise<Host | null> {
    try {
      const saved = await api.hostSave(host);
      // Raw state: the array is replaced, never written into.
      this.hosts = this.hosts.some((entry) => entry.id === saved.id)
        ? this.hosts.map((entry) => (entry.id === saved.id ? saved : entry))
        : [...this.hosts, saved];
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
      // Raw state: the array is replaced, never written into.
      this.hosts = this.hosts.map((entry) =>
        entry.id === host.id ? { ...entry, favorite: next } : entry,
      );
    } catch (err) {
      this.error = errorMessage(err);
    }
  }
}

/** The `last_used_at` of every recent host, by host id. */
function recentMap(entries: RecentHost[] | null | undefined): Record<string, number> {
  const map: Record<string, number> = {};
  for (const entry of entries ?? []) {
    if (entry?.host && !entry.host.deleted) map[entry.host.id] = entry.last_used_at;
  }
  return map;
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
