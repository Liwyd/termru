<script lang="ts">
  import { hosts } from '../stores/hosts.svelte';
  import { route } from '../stores/route.svelte';
  import { toasts } from '../stores/toasts.svelte';
  import ContextConfirm from './ContextConfirm.svelte';
  import HostCard from './HostCard.svelte';
  import type { Host } from '../types';

  let { onconnect }: { onconnect: (host: Host) => void } = $props();

  let confirmOpen = $state(false);
  let pendingDelete = $state<Host | null>(null);

  function edit(host: Host | null): void {
    hosts.editing = host;
    hosts.editorOpen = true;
    route.edit(host);
  }

  function remove(host: Host): void {
    pendingDelete = host;
    confirmOpen = true;
  }

  async function confirmedDelete(): Promise<void> {
    const host = pendingDelete;
    pendingDelete = null;
    if (!host) return;
    await hosts.remove(host.id);
    toasts.success(`Deleted ${host.name}`);
  }
</script>

<div class="hostlist">
  <input
    type="search"
    placeholder="Search hosts"
    aria-label="Search hosts"
    bind:value={hosts.query}
  />

  {#if hosts.loading}
    <p class="hint" aria-busy="true">Loading…</p>
  {:else if hosts.error}
    <p class="hint error">{hosts.error}</p>
  {:else if hosts.visible.length === 0}
    <p class="hint">
      {hosts.query ? 'No hosts match your search.' : 'No hosts yet — add your first one.'}
    </p>
  {:else}
    {#each hosts.grouped as section (section.id)}
      {#if section.id !== ''}
        <h3 class="folder">{section.label}</h3>
      {/if}
      <ul>
        {#each section.hosts as host (host.id)}
          <li>
            <HostCard
              {host}
              {onconnect}
              onedit={edit}
              ondelete={remove}
              ontogglefavorite={(target) => hosts.toggleFavorite(target)}
            />
          </li>
        {/each}
      </ul>
    {/each}
  {/if}

  {#if hosts.recents.length > 0 && hosts.query.length === 0}
    <section>
      <h3>Recent</h3>
      <ul class="recent">
        {#each hosts.recents.slice(0, 5) as host (host.id)}
          <li>
            <button type="button" class="recent-item" onclick={() => onconnect(host)}>
              <span class="recent-name">{host.name}</span>
              <span class="recent-target">{host.username}@{host.address}</span>
            </button>
          </li>
        {/each}
      </ul>
    </section>
  {/if}
</div>

<ContextConfirm
  bind:open={confirmOpen}
  title={pendingDelete ? `Delete host “${pendingDelete.name}”?` : 'Delete host?'}
  body="The host profile is removed from this device. Running sessions are not closed."
  onconfirm={() => void confirmedDelete()}
/>

<style>
  .hostlist {
    display: flex;
    flex-direction: column;
    gap: 0.6rem;
    min-width: 0;
  }

  input[type='search'] {
    width: 100%;
    padding: 0.4rem 0.55rem;
    border-radius: var(--termru-radius);
    border: 1px solid var(--termru-border);
    background: var(--termru-bg);
    color: var(--termru-fg);
  }

  ul {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 0.4rem;
  }

  section h3 {
    margin: 0.2rem 0 0.2rem;
    font-size: 0.75rem;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--termru-muted);
  }

  h3.folder {
    margin: 0.3rem 0 0.1rem;
    font-size: 0.72rem;
    font-weight: 600;
    color: var(--termru-muted);
    border-bottom: 1px solid var(--termru-border);
    padding-bottom: 0.15rem;
  }

  .recent {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 0.15rem;
  }

  .recent-item {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 0.1rem;
    width: 100%;
    border: none;
    background: none;
    color: var(--termru-fg);
    padding: 0.2rem 0.3rem;
    text-align: left;
  }

  .recent-item:hover {
    background: var(--termru-bg);
    border: none;
  }

  .recent-name {
    font-size: 0.82rem;
  }

  .recent-target {
    font-size: 0.72rem;
    color: var(--termru-muted);
  }

  .hint {
    margin: 0;
    color: var(--termru-muted);
    font-size: 0.82rem;
  }

  .hint.error {
    color: var(--termru-danger);
  }
</style>
