import type { Host } from '../types';

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
  /** The host being edited when the route is `host-edit`. */
  editHost = $state<Host | null>(null);

  constructor() {
    if (typeof window !== 'undefined') {
      const fromHash = parseHash(window.location.hash);
      if (fromHash) this.route = fromHash;
      window.addEventListener('hashchange', () => {
        const next = parseHash(window.location.hash);
        if (next) this.route = next;
      });
    }
  }

  /** Navigates to a page and syncs the hash. */
  go(route: RouteId): void {
    this.route = route;
    const target = `#${route}`;
    if (typeof window !== 'undefined' && window.location.hash !== target) {
      window.location.hash = target;
    }
  }

  /** Opens the host editor for a host, or for a new host when null. */
  edit(host: Host | null): void {
    this.editHost = host;
    this.go('host-edit');
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
