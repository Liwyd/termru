import { connectFlow } from './connect.svelte';
import { hosts } from './hosts.svelte';
import { palette } from './palette.svelte';
import { route } from './route.svelte';

/**
 * "+ New session": the quick switcher with every host in it, so the next
 * session is a few keystrokes away. With no saved host there is nothing to
 * pick, so it opens the host editor instead.
 */
export function startNewSession(): void {
  if (hosts.sortedAll.length === 0) {
    route.edit(null);
    return;
  }
  palette.show(
    hosts.sortedAll.map((host) => ({
      id: `host:${host.id}`,
      title: `Connect to ${host.name}`,
      group: 'Hosts',
      run: () => void connectFlow.request(host),
    })),
  );
}
