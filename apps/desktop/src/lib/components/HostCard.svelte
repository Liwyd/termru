<script lang="ts">
  import ContextMenu, { type MenuItem } from './ContextMenu.svelte';
  import type { Host } from '../types';

  let {
    host,
    onconnect,
    onedit,
    ondelete,
    ontogglefavorite,
  }: {
    host: Host;
    onconnect: (host: Host) => void;
    onedit: (host: Host) => void;
    ondelete: (host: Host) => void;
    ontogglefavorite: (host: Host) => void;
  } = $props();

  // The context menu of the card.
  let menuOpen = $state(false);
  let menuX = $state(0);
  let menuY = $state(0);

  const items: MenuItem[] = $derived([
    { id: 'connect', label: 'Connect' },
    { id: 'edit', label: 'Edit…' },
    {
      id: 'favorite',
      label: host.favorite ? 'Remove from favorites' : 'Add to favorites',
    },
    { id: 'delete', label: 'Delete', danger: true },
  ]);

  function onContextMenu(event: MouseEvent): void {
    event.preventDefault();
    menuX = event.clientX;
    menuY = event.clientY;
    menuOpen = true;
  }

  function choose(id: string): void {
    switch (id) {
      case 'connect':
        onconnect(host);
        break;
      case 'edit':
        onedit(host);
        break;
      case 'favorite':
        ontogglefavorite(host);
        break;
      case 'delete':
        ondelete(host);
        break;
    }
  }
</script>

<article class="card" oncontextmenu={onContextMenu}>
  <button
    type="button"
    class="favorite"
    aria-label={host.favorite ? 'Remove from favorites' : 'Add to favorites'}
    onclick={() => ontogglefavorite(host)}
  >
    {host.favorite ? '★' : '☆'}
  </button>
  <div class="body">
    <h3>{host.name}</h3>
    <p class="target">{host.username}@{host.address}:{host.port}</p>
    <p class="meta">
      {host.auth.type === 'password'
        ? 'password'
        : host.auth.type === 'agent'
          ? 'ssh agent'
          : 'key'}
    </p>
  </div>
  <div class="actions">
    <button type="button" class="primary" onclick={() => onconnect(host)}>Connect</button>
    <button type="button" onclick={() => onedit(host)}>Edit</button>
    <button type="button" class="danger" onclick={() => ondelete(host)}>Delete</button>
  </div>
</article>

<ContextMenu
  open={menuOpen}
  x={menuX}
  y={menuY}
  {items}
  onchoose={choose}
  onclose={() => (menuOpen = false)}
/>

<style>
  .card {
    display: flex;
    align-items: flex-start;
    gap: 0.5rem;
    padding: 0.65rem 0.75rem;
    border: 1px solid var(--termru-border);
    border-radius: var(--termru-radius);
    background: var(--termru-surface);
  }

  .favorite {
    background: none;
    border: none;
    cursor: pointer;
    color: var(--termru-accent);
    font-size: 1rem;
    line-height: 1;
    padding: 0.1rem;
  }

  .body {
    flex: 1;
    min-width: 0;
  }

  h3 {
    margin: 0;
    font-size: 0.92rem;
    font-weight: 600;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .target {
    margin: 0.1rem 0 0;
    font-size: 0.78rem;
    color: var(--termru-muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .meta {
    margin: 0.15rem 0 0;
    font-size: 0.7rem;
    color: var(--termru-muted);
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }

  .actions {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
  }
</style>
