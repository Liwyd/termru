<script lang="ts">
  import { sessions } from '../stores/sessions.svelte';
  import { hosts } from '../stores/hosts.svelte';
  import StatusBadge from './StatusBadge.svelte';
  import Terminal from './Terminal.svelte';
  import type { Pane } from '../stores/sessions.svelte';

  let { pane, focused }: { pane: Pane; focused: boolean } = $props();

  /** The hosts offered in an empty pane. */
  const candidates = $derived(hosts.hosts.slice(0, 8));
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
      <Terminal {pane} />
    {:else}
      <div class="empty">
        <p>No session in this pane.</p>
        {#if candidates.length > 0}
          <ul>
            {#each candidates as host (host.id)}
              <li>
                <button type="button" onclick={() => void sessions.connectPane(pane, host)}>
                  {host.name}
                  <span class="address">{host.username}@{host.address}</span>
                </button>
              </li>
            {/each}
          </ul>
        {:else}
          <p class="hint">Add a host in the sidebar first.</p>
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
    max-width: 22rem;
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
