<script lang="ts">
  import { sessions } from '../stores/sessions.svelte';
  import { connectFlow } from '../stores/connect.svelte';
  import { hosts } from '../stores/hosts.svelte';
  import { route } from '../stores/route.svelte';
  import StatusBadge from './StatusBadge.svelte';
  import Terminal from './Terminal.svelte';
  import type { Pane } from '../stores/sessions.svelte';

  let { pane, focused }: { pane: Pane; focused: boolean } = $props();

  /** What the user typed to find a host for an empty pane. */
  let query = $state('');

  /** Every host that matches the search (not only the first few). */
  const candidates = $derived.by(() => {
    const needle = query.trim().toLowerCase();
    const all = hosts.sortedAll;
    if (!needle) return all;
    return all.filter((host) =>
      [host.name, host.address, host.username].some((field) =>
        field.toLowerCase().includes(needle),
      ),
    );
  });

  function pick(host: (typeof candidates)[number]): void {
    void connectFlow.pick(pane, host);
  }
</script>

<section class="pane" class:focused aria-label={pane.title}>
  <header>
    <button
      type="button"
      class="title"
      title={pane.host ? pane.host.address : 'empty pane'}
      onclick={() => sessions.focusPane(pane)}
    >
      {pane.title}
    </button>

    {#if pane.host}
      <StatusBadge status={pane.status} />
      <span class="spacer"></span>
      <button
        type="button"
        class="icon"
        title="Split to the right"
        aria-label="Split to the right"
        onclick={() => {
          sessions.focusPane(pane);
          sessions.splitActive('columns');
        }}
      >
        ▥
      </button>
      <button
        type="button"
        class="icon"
        title="Split below"
        aria-label="Split below"
        onclick={() => {
          sessions.focusPane(pane);
          sessions.splitActive('rows');
        }}
      >
        ▤
      </button>
      <button
        type="button"
        class="icon"
        title="Close the pane"
        aria-label="Close the pane"
        onclick={() => void sessions.closePane(pane)}
      >
        ×
      </button>
    {:else}
      <span class="spacer"></span>
      <button
        type="button"
        class="icon"
        title="Close the pane"
        aria-label="Close the pane"
        onclick={() => void sessions.closePane(pane)}
      >
        ×
      </button>
    {/if}
  </header>

  <div class="body">
    {#if pane.host}
      {#key pane.id}
        <Terminal {pane} />
      {/key}
    {:else}
      <div class="empty">
        <p>No session in this pane.</p>
        {#if hosts.hosts.length > 0}
          <input
            type="search"
            class="filter"
            placeholder="Search hosts"
            aria-label="Search hosts for this pane"
            bind:value={query}
            onkeydown={(event) => {
              if (event.key === 'Enter' && candidates[0]) {
                event.preventDefault();
                pick(candidates[0]);
              }
            }}
          />
          {#if candidates.length > 0}
            <ul>
              {#each candidates as host (host.id)}
                <li>
                  <button type="button" onclick={() => pick(host)}>
                    {host.name}
                    <span class="address">{host.username}@{host.address}</span>
                  </button>
                </li>
              {/each}
            </ul>
          {:else}
            <p class="hint">No host matches “{query.trim()}”.</p>
          {/if}
        {:else}
          <p class="hint">No saved hosts yet.</p>
          <button type="button" class="primary new" onclick={() => route.edit(null)}>
            New host
          </button>
        {/if}
      </div>
    {/if}
  </div>
</section>

<style>
  .pane {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-width: 0;
    min-height: 0;
    background: var(--termru-bg);
  }

  .pane.focused {
    outline: 1px solid var(--termru-accent);
    outline-offset: -1px;
  }

  header {
    display: flex;
    align-items: center;
    gap: 0.3rem;
    padding: 0.15rem 0.3rem;
    border-bottom: 1px solid var(--termru-border);
    background: var(--termru-surface);
    font-size: 0.75rem;
  }

  .title {
    background: none;
    border: none;
    color: var(--termru-muted);
    cursor: pointer;
    font: inherit;
    padding: 0;
    max-width: 12rem;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .pane.focused .title {
    color: var(--termru-fg);
  }

  .spacer {
    flex: 1;
  }

  .icon {
    background: none;
    border: none;
    color: var(--termru-muted);
    cursor: pointer;
    font-size: 0.8rem;
    line-height: 1;
    padding: 0 0.2rem;
  }

  .icon:hover {
    color: var(--termru-fg);
  }

  .body {
    flex: 1;
    min-height: 0;
    position: relative;
  }

  .empty {
    height: 100%;
    min-height: 0;
    padding: 0.75rem 0;
    overflow: hidden;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 0.5rem;
    color: var(--termru-muted);
    font-size: 0.85rem;
  }

  .empty p {
    margin: 0;
  }

  .empty ul {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 0.2rem;
    width: min(22rem, 90%);
    min-height: 0;
    overflow-y: auto;
  }

  .empty .filter {
    width: min(22rem, 90%);
    padding: 0.3rem 0.5rem;
    border: 1px solid var(--termru-border);
    border-radius: var(--termru-radius);
    background: var(--termru-surface);
    color: var(--termru-fg);
    font: inherit;
  }

  .empty .new {
    width: auto;
    text-align: center;
    background: var(--termru-accent);
    border-color: var(--termru-accent);
    color: var(--termru-on-accent);
  }

  .empty button {
    width: 100%;
    text-align: left;
    background: none;
    border: 1px solid var(--termru-border);
    border-radius: var(--termru-radius);
    color: var(--termru-fg);
    cursor: pointer;
    font: inherit;
    padding: 0.3rem 0.5rem;
  }

  .empty button:hover {
    border-color: var(--termru-accent);
  }

  .address {
    color: var(--termru-muted);
    font-size: 0.72rem;
    margin-left: 0.4rem;
  }

  .hint {
    font-size: 0.78rem;
  }
</style>
