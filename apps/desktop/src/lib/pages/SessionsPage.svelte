<script lang="ts">
  import { sessions } from '../stores/sessions.svelte';
  import SplitView from '../components/SplitView.svelte';
  import TabBar from '../components/TabBar.svelte';
</script>

<div class="page">
  <div class="tabbar" role="tablist" aria-label="Open sessions">
    {#each sessions.tabs as tab (tab.id)}
      <TabBar {tab} active={tab.id === sessions.activeId} />
    {/each}
  </div>

  <div class="content">
    {#if sessions.tabs.length === 0}
      <div class="empty">
        <h1>Termru</h1>
        <p>Open a host from the Hosts page to start a terminal session.</p>
      </div>
    {:else}
      {#each sessions.tabs as tab (tab.id)}
        <div class="pane" class:hidden={tab.id !== sessions.activeId}>
          <SplitView node={tab.root} {tab} />
        </div>
      {/each}
    {/if}
  </div>
</div>

<style>
  .page {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
  }

  .tabbar {
    display: flex;
    align-items: flex-end;
    gap: 0.2rem;
    min-height: 2rem;
    padding: 0.2rem 0.4rem 0;
    border-bottom: 1px solid var(--termru-border);
    background: var(--termru-surface);
    overflow-x: auto;
  }

  .content {
    flex: 1;
    min-height: 0;
    position: relative;
  }

  .pane {
    position: absolute;
    inset: 0;
  }

  .pane.hidden {
    display: none;
  }

  .empty {
    height: 100%;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 0.4rem;
    color: var(--termru-muted);
  }

  .empty h1 {
    margin: 0;
    font-size: 1.3rem;
    color: var(--termru-fg);
  }

  .empty p {
    margin: 0;
    font-size: 0.85rem;
  }
</style>
