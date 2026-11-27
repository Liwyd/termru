import type { Host } from '../types';
import { unsaved } from './unsaved.svelte';

/**
 * The unsaved state of the host editor, kept while another page is open.
 * The password typed into the form is never parked.
 */
export interface HostEditDraft {
  host: Host;
  authType: Host['auth']['type'];
  identityId: string;
  groupId: string;
  tags: string[];
  /** The port and keepalive as typed (the host holds them as numbers). */
  portText?: string;
  keepaliveText?: string;
  /** Whether the "keep the password on this device" box was ticked. */
  savePassword?: boolean;
}

/** How many previous pages `back()` remembers. */
const HISTORY_LIMIT = 20;

/** The pages the shell can show. Dialogs are not routes. */
export const ROUTE_IDS = [
  'hosts',
  'sessions',
  'folders',
  'identities',
  'sync',
  'preferences',
  'host-edit',
] as const;

export type RouteId = (typeof ROUTE_IDS)[number];

/** Human names for the navigation. */
export const ROUTE_TITLES: Record<RouteId, string> = {
  hosts: 'Hosts',
  sessions: 'Sessions',
  folders: 'Folders',
  identities: 'Identities',
  sync: 'Sync',
  preferences: 'Preferences',
  'host-edit': 'Host',
};

export function isRouteId(value: string | null | undefined): value is RouteId {
  return typeof value === 'string' && (ROUTE_IDS as readonly string[]).includes(value);
}

function parseHash(hash: string): RouteId | null {
  const id = hash.replace(/^#/, '');
  return isRouteId(id) ? id : null;
}

/**
 * The current page, kept in the URL hash so a dev reload lands on the same
 * page and links can address a page directly. The last route is also
 * persisted in the workspace document (see the sessions store).
 */
export class RouteStore {
  route = $state<RouteId>('hosts');
  /**
   * The host being edited when the route is `host-edit`; null means a new
   * host. This is the only state of the editor route: opening the editor is
   * `edit()`, nothing else has to be switched on.
   */
  editHost = $state<Host | null>(null);
  /** Counts `edit()` calls: the editor starts a fresh form each time it changes. */
  editToken = $state(0);
  /** Unsaved editor state parked while the user visits another page. */
  draft = $state.raw<HostEditDraft | null>(null);
  /** The pages visited before this one (most recent last). */
  #history: RouteId[] = [];

  constructor() {
    if (typeof window !== 'undefined') {
      const fromHash = parseHash(window.location.hash);
      if (fromHash) this.route = fromHash;
      window.addEventListener('hashchange', () => {
        const next = parseHash(window.location.hash);
        if (!next || next === this.route) return;
        // The address bar changed under a page with unsaved work: put the
        // address back and ask, instead of leaving silently.
        if (unsaved.ask(() => this.go(next, { force: true }))) {
          window.location.hash = `#${this.route}`;
          return;
        }
        this.#enter(next);
      });
    }
  }

  /** Switches page and remembers the one we leave. */
  #enter(next: RouteId): void {
    if (next === this.route) return;
    this.#history.push(this.route);
    if (this.#history.length > HISTORY_LIMIT) this.#history.shift();
    this.route = next;
  }

  /**
   * Navigates to a page and syncs the hash. A page with unsaved work is not
   * left silently: the unsaved-changes question opens instead, and the
   * navigation happens when the user agrees to discard (`force` skips the
   * question, for the caller that has already asked or saved).
   */
  go(route: RouteId, options: { force?: boolean } = {}): void {
    if (
      !options.force &&
      route !== this.route &&
      unsaved.ask(() => this.go(route, { force: true }))
    ) {
      return;
    }
    this.#enter(route);
    const target = `#${route}`;
    if (typeof window !== 'undefined' && window.location.hash !== target) {
      window.location.hash = target;
    }
  }

  /**
   * Returns to the page the user came from (`fallback` when there is none).
   * A page that is open as a full page uses this for its Close and Cancel
   * buttons instead of always going to the hosts page.
   */
  back(fallback: RouteId = 'hosts', options: { force?: boolean } = {}): void {
    if (!options.force && unsaved.ask(() => this.back(fallback, { force: true }))) return;
    let previous = this.#history.pop();
    // A page that was left through itself (a reload on the same hash) is not
    // a place to return to.
    while (previous === this.route) previous = this.#history.pop();
    const target = previous ?? fallback;
    const targetHash = `#${target}`;
    this.route = target;
    if (typeof window !== 'undefined' && window.location.hash !== targetHash) {
      window.location.hash = targetHash;
    }
  }

  /** Opens the host editor for a host, or for a new host when null. */
  edit(host: Host | null, options: { force?: boolean } = {}): void {
    // Opening the editor on another host would replace the form in front of
    // the user: that is a way to lose changes too.
    if (!options.force && unsaved.ask(() => this.edit(host, { force: true }))) return;
    this.editHost = host;
    this.editToken += 1;
    this.draft = null;
    this.go('host-edit', { force: true });
  }

  /**
   * Parks the editor's unsaved state and opens another page. `back()` from
   * that page returns to the editor, which restores the draft.
   */
  leaveEditor(draft: HostEditDraft, to: RouteId): void {
    this.draft = draft;
    // The form is parked, not lost: no question.
    this.go(to, { force: true });
  }

  /** Restores the persisted route (workspace document). Old documents
   * without a route leave the current page untouched. The URL hash (a dev
   * reload or an explicit link) wins over the persisted value. */
  restore(last: string | null | undefined): void {
    if (typeof window !== 'undefined' && window.location.hash) return;
    if (isRouteId(last) && last !== 'host-edit') {
      this.route = last;
    }
  }
}

export const route = new RouteStore();
