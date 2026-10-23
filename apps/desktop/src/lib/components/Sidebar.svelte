<script lang="ts">
  import { hosts } from '../stores/hosts.svelte';
  import { sessions } from '../stores/sessions.svelte';
  import { sync } from '../stores/sync.svelte';
  import { toasts } from '../stores/toasts.svelte';
  import HostCard from './HostCard.svelte';
  import type { Host } from '../types';

  let {
    onconnect,
    onopensync,
    onopenidentities,
    onopenfolders,
  }: {
    onconnect: (host: Host) => void;
    onopensync: () => void;
    onopenidentities: () => void;
    onopenfolders: () => void;
  } = $props();

  function edit(host: Host | null): void {
    hosts.editing = host;
    hosts.editorOpen = true;
  }

  async function remove(host: Host): Promise<void> {
    if (!window.confirm(`Delete host "${host.name}"?`)) return;
    await hosts.remove(host.id);
    toasts.success(`Deleted ${host.name}`);
  }
</script>

<aside class="sidebar">
  <header>
    <h2>Hosts</h2>
    <button type="button" class="primary" onclick={() => edit(null)}>New</button>
  </header>

  <input
    type="search"
    placeholder="Search hosts"
    aria-label="Search hosts"
    bind:value={hosts.query}
  />

  {#if hosts.loading}
    <p class="hint">Loading…</p>
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

  <footer>
    <button type="button" class="identities" onclick={onopenfolders}>
      <span>Folders</span>
    </button>
    <button type="button" class="identities" onclick={onopenidentities}>
      <span>Identities</span>
    </button>
    <button type="button" class="sync" onclick={onopensync}>
      <span>Sync</span>
      {#if sync.conflictCount > 0}
        <span class="badge danger" title="unresolved conflicts">{sync.conflictCount}</span>
      {:else if sync.pending > 0}
        <span class="badge" title="changes waiting to be pushed">{sync.pending}</span>
      {:else if sync.signedIn}
        <span class="dot" title="signed in"></span>
      {/if}
    </button>
    {#if sessions.tabs.length > 0}
      <span>{sessions.tabs.length} open session(s)</span>
    {/if}
  </footer>
</aside>

<style>
  .sidebar {
    display: flex;
    flex-direction: column;
    gap: 0.6rem;
    width: 20rem;
    min-width: 16rem;
    padding: 0.85rem;
    border-right: 1px solid var(--termru-border);
    background: var(--termru-surface);
    overflow-y: auto;
  }

  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }

  h2 {
    margin: 0;
    font-size: 0.95rem;
    font-weight: 600;
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

  footer {
    margin-top: auto;
    display: flex;
    flex-direction: column;
    gap: 0.35rem;
    font-size: 0.75rem;
    color: var(--termru-muted);
  }

  .sync {
    display: flex;
    align-items: center;
    gap: 0.35rem;
    width: 100%;
    padding: 0.3rem 0.4rem;
    border: 1px solid var(--termru-border);
    border-radius: var(--termru-radius);
    background: var(--termru-bg);
    color: var(--termru-fg);
    font-size: 0.8rem;
    cursor: pointer;
  }

  .sync:hover {
    border-color: var(--termru-accent);
  }

  .badge {
    margin-left: auto;
    padding: 0 0.35rem;
    border-radius: 999px;
    background: var(--termru-accent);
    color: var(--termru-bg);
    font-size: 0.68rem;
    font-weight: 600;
  }

  .badge.danger {
    background: var(--termru-danger);
    color: var(--termru-on-danger);
  }

  .dot {
    margin-left: auto;
    width: 0.45rem;
    height: 0.45rem;
    border-radius: 50%;
    background: var(--termru-success);
  }
</style>
