<script lang="ts">
  import { autofocus } from '../actions/autofocus';
  import { sessions } from '../stores/sessions.svelte';
  import StatusBadge from './StatusBadge.svelte';
  import type { Tab } from '../stores/sessions.svelte';

  let { tab, active }: { tab: Tab; active: boolean } = $props();

  /** The pane that carries the tab: its status is the status of the tab. */
  const pane = $derived(sessions.activePaneOf(tab));

  /** Duplicating or moving needs a connected session to work from. */
  const canClone = $derived(pane?.status.state === 'ready' && pane.sessionId !== null);

  let renaming = $state(false);
  let draftTitle = $state('');
  /** Escape was pressed: the blur that follows must not commit. */
  let renameCancelled = false;
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
    renameCancelled = false;
    renaming = true;
  }

  /** Enter or blur: keeps the new name (an empty name keeps the old one). */
  function commitRename(): void {
    if (!renaming || renameCancelled) return;
    renaming = false;
    sessions.rename(tab, draftTitle);
  }

  /** Escape: back to the old name. */
  function cancelRename(): void {
    renameCancelled = true;
    renaming = false;
  }

  /** Keyboard on the tab itself: Enter or Space opens it, F2 renames it. */
  function onTabKeyDown(event: KeyboardEvent): void {
    if (event.target !== event.currentTarget) return;
    if (event.key === 'Enter' || event.key === ' ') {
      event.preventDefault();
      sessions.setActive(tab.id);
    } else if (event.key === 'F2') {
      event.preventDefault();
      startRename();
    }
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
  onkeydown={onTabKeyDown}
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
      aria-label="Tab name"
      bind:value={draftTitle}
      use:autofocus
      onblur={commitRename}
      onkeydown={(event) => {
        // The keys of the field stay in the field (the tab has its own).
        event.stopPropagation();
        if (event.key === 'Enter') commitRename();
        else if (event.key === 'Escape') cancelRename();
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

  {#if pane}
    <StatusBadge status={pane.status} />
  {/if}

  <button
    type="button"
    class="close"
    aria-label={`Duplicate ${tab.title}`}
    title="Duplicate session"
    disabled={!canClone}
    onclick={() => sessions.duplicate(tab)}
  >
    ⧉
  </button>

  <button
    type="button"
    class="close"
    aria-label={`Move ${tab.title} to a new window`}
    title="Move to a new window"
    disabled={!canClone}
    onclick={() => void sessions.moveToNewWindow(tab)}
  >
    ↗
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
