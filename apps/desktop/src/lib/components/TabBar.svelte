<script lang="ts">
  import { sessions } from '../stores/sessions.svelte';
  import StatusBadge from './StatusBadge.svelte';
  import type { Tab } from '../stores/sessions.svelte';

  let { tab, active }: { tab: Tab; active: boolean } = $props();

  let renaming = $state(false);
  let draftTitle = $state('');
  let dragIndex = $state<number | null>(null);
  let dropIndex = $state<number | null>(null);

  function onDragStart(event: DragEvent, index: number): void {
    dragIndex = index;
    event.dataTransfer?.setData('text/plain', String(index));
    if (event.dataTransfer) event.dataTransfer.effectAllowed = 'move';
  }

  function onDragOver(event: DragEvent, index: number): void {
    event.preventDefault();
    dropIndex = index;
  }

  function onDrop(event: DragEvent, index: number): void {
    event.preventDefault();
    const from = dragIndex ?? Number(event.dataTransfer?.getData('text/plain'));
    dragIndex = null;
    dropIndex = null;
    if (Number.isInteger(from)) {
      sessions.reorder(from, index);
    }
  }

  function startRename(): void {
    draftTitle = tab.title;
    renaming = true;
  }

  function commitRename(): void {
    sessions.rename(tab, draftTitle);
    renaming = false;
  }
</script>

<div
  class="tab"
  class:active
  class:drop-target={dropIndex !== null}
  role="tab"
  aria-selected={active}
  tabindex="0"
  draggable="true"
  ondragstart={(event) => onDragStart(event, sessions.tabs.indexOf(tab))}
  ondragover={(event) => onDragOver(event, sessions.tabs.indexOf(tab))}
  ondragleave={() => (dropIndex = null)}
  ondrop={(event) => onDrop(event, sessions.tabs.indexOf(tab))}
  ondragend={() => {
    dragIndex = null;
    dropIndex = null;
  }}
>
  {#if renaming}
    <input
      class="rename"
      bind:value={draftTitle}
      onblur={commitRename}
      onkeydown={(event) => {
        if (event.key === 'Enter') commitRename();
        if (event.key === 'Escape') renaming = false;
      }}
      onclick={(event) => event.stopPropagation()}
    />
  {:else}
    <button
      type="button"
      class="label"
      onclick={() => sessions.setActive(tab.id)}
      ondblclick={startRename}
    >
      {tab.title}
    </button>
  {/if}

  <StatusBadge status={tab.status} />

  <button
    type="button"
    class="close"
    aria-label={`Duplicate ${tab.title}`}
    title="Duplicate session"
    onclick={() => sessions.duplicate(tab)}
  >
    ⧉
  </button>

  <button
    type="button"
    class="close"
    aria-label={`Close ${tab.title}`}
    onclick={() => void sessions.close(tab)}
  >
    ×
  </button>
</div>

<style>
  .tab {
    display: inline-flex;
    align-items: center;
    gap: 0.4rem;
    padding: 0.3rem 0.4rem 0.3rem 0.6rem;
    border: 1px solid transparent;
    border-bottom: none;
    border-radius: var(--termru-radius) var(--termru-radius) 0 0;
    color: var(--termru-muted);
    font-size: 0.82rem;
    max-width: 16rem;
  }

  .tab.drop-target {
    border-color: var(--termru-accent);
  }

  .tab.active {
    background: var(--termru-bg);
    border-color: var(--termru-border);
    color: var(--termru-fg);
  }

  .label {
    background: none;
    border: none;
    color: inherit;
    cursor: pointer;
    font: inherit;
    padding: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .rename {
    width: 8rem;
    font: inherit;
    padding: 0.1rem 0.25rem;
    border-radius: var(--termru-radius);
    border: 1px solid var(--termru-accent);
    background: var(--termru-bg);
    color: var(--termru-fg);
  }

  .close {
    background: none;
    border: none;
    color: inherit;
    cursor: pointer;
    font-size: 0.95rem;
    line-height: 1;
    padding: 0 0.2rem;
  }

  .close:hover {
    color: var(--termru-danger);
  }
</style>
