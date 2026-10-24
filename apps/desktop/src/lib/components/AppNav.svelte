<script lang="ts">
  import { route, ROUTE_TITLES, type RouteId } from '../stores/route.svelte';
  import { sessions } from '../stores/sessions.svelte';

  const items: RouteId[] = ['hosts', 'sessions', 'folders', 'identities', 'sync', 'preferences'];

  const openCount = $derived(sessions.tabs.length);
</script>

<nav class="appnav" aria-label="Primary">
  <div class="brand" aria-hidden="true">
    <span class="prompt">&gt;_</span>
    <span class="name">termru</span>
  </div>

  <ul class="items">
    {#each items as id (id)}
      <li>
        <button
          type="button"
          class="item"
          class:active={route.route === id || (id === 'hosts' && route.route === 'host-edit')}
          aria-current={route.route === id ? 'page' : undefined}
          onclick={() => route.go(id)}
        >
          {ROUTE_TITLES[id]}
          {#if id === 'sessions' && openCount > 0}
            <span class="count" aria-label="{openCount} open">{openCount}</span>
          {/if}
        </button>
      </li>
    {/each}
  </ul>

  <span class="connected">
    {#if openCount > 0}
      {openCount} session{openCount === 1 ? '' : 's'} connected
    {:else}
      No sessions connected
    {/if}
  </span>
</nav>

<style>
  .appnav {
    display: flex;
    align-items: center;
    gap: var(--termru-space-5);
    height: var(--termru-nav-h);
    padding: 0 var(--termru-page-inset-x);
    border-bottom: 1px solid var(--termru-border);
    background: var(--termru-surface);
  }

  .brand {
    display: flex;
    align-items: baseline;
    gap: var(--termru-space-2);
    font-family: var(--termru-font-mono);
    font-size: 15px;
    color: var(--termru-fg);
  }

  .prompt {
    color: var(--termru-accent);
    font-weight: 600;
  }

  .items {
    display: flex;
    align-items: stretch;
    gap: var(--termru-space-1);
    list-style: none;
    margin: 0;
    padding: 0;
    height: 100%;
  }

  .item {
    display: flex;
    align-items: center;
    gap: var(--termru-space-2);
    height: 100%;
    padding: 0 var(--termru-space-3);
    border: none;
    border-bottom: 2px solid transparent;
    background: none;
    color: var(--termru-muted);
    font-family: inherit;
    font-size: var(--termru-text-control-size);
    font-weight: var(--termru-text-control-weight);
    line-height: var(--termru-text-control-lh);
    cursor: pointer;
    transition:
      color var(--motion-fast) var(--ease-quiet),
      border-color var(--motion-fast) var(--ease-quiet);
  }

  .item:hover {
    color: var(--termru-fg);
  }

  /* Active = weight + underline, never colour alone (Doc A.04). */
  .item.active {
    color: var(--termru-accent);
    font-weight: 600;
    border-bottom-color: var(--termru-accent);
  }

  .count {
    padding: 0 var(--termru-space-2);
    border-radius: 999px;
    background: var(--termru-accent-soft);
    color: var(--termru-accent);
    font-size: var(--termru-text-meta-size);
    font-weight: 600;
  }

  .connected {
    margin-left: auto;
    font-size: var(--termru-text-meta-size);
    line-height: var(--termru-text-meta-lh);
    color: var(--termru-muted);
  }
</style>
