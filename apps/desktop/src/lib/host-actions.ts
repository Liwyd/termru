import type { MenuEntry } from './menu';
import type { Host } from './types';

/** What the row menu and the inspector menu of a host can do. */
export type HostActionId = 'open' | 'connect' | 'edit' | 'favorite' | 'delete';

/**
 * The entries of a host's "more" menu (the row's ⋯ and the inspector's ⋯): every
 * action a host has beyond the one button. A host with a session already
 * running can be opened (jump to it) as well as connected to again.
 */
export function hostMenuItems(host: Host, live: boolean): MenuEntry[] {
  const items: MenuEntry[] = [];
  if (live) items.push({ id: 'open', label: 'Open running session' });
  items.push(
    { id: 'connect', label: 'Connect' },
    { id: 'edit', label: 'Edit…' },
    { id: 'favorite', label: host.favorite ? 'Remove from favorites' : 'Add to favorites' },
    { id: 'delete', label: 'Delete', danger: true, separatorBefore: true },
  );
  return items;
}
