<script lang="ts">
  import { hosts } from '../stores/hosts.svelte';
  import { sessions } from '../stores/sessions.svelte';
  import { toasts } from '../stores/toasts.svelte';
  import HostCard from './HostCard.svelte';
  import type { Host } from '../types';

  let { onconnect }: { onconnect: (host: Host) => void } = $props();

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
    <ul>
      {#each hosts.visible as host (host.id)}
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

  {#if sessions.tabs.length > 0}
    <footer>
      <span>{sessions.tabs.length} open session(s)</span>
    </footer>
  {/if}
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
    font-size: 0.75rem;
    color: var(--termru-muted);
  }
</style>
