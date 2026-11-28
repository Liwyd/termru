<script lang="ts">
  import { autofocus } from '../actions/autofocus';
  import { sessions } from '../stores/sessions.svelte';
  import { statusLabel, statusTone } from '../terminal/session-view';
  import SessionIcon from './SessionIcon.svelte';
  import type { Tab } from '../stores/sessions.svelte';

  /**
   * One tab of the strip: the dot, the name, the status word and the close
   * button. Duplicate and Detach live in the toolbar, where they act on the
   * session you are looking at.
   */
  let { tab, active }: { tab: Tab; active: boolean } = $props();

  /** The pane that carries the tab: its status is the status of the tab. */
  const pane = $derived(sessions.activePaneOf(tab));

  const tone = $derived(pane ? statusTone(pane.status) : 'closed');
  const label = $derived(pane ? statusLabel(pane) : '');

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

  /** Keyboard on the name button: F2 renames (Enter and Space press the button). */
  function onLabelKeyDown(event: KeyboardEvent): void {
    if (event.key === 'F2') {
      event.preventDefault();
      startRename();
    }
  }
</script>

<!-- One item of the list of open sessions: a name button and a close button.
     (Not role="tab": a tab may not contain buttons, and the strip does not
     have the arrow-key behaviour of a tab list.) -->
<li
  class="tab"
  class:active
  class:drop-target={dropIndex !== null}
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
      aria-current={active ? 'true' : undefined}
      title="Double-click or press F2 to rename"
      onclick={() => sessions.setActive(tab.id)}
      ondblclick={startRename}
      onkeydown={onLabelKeyDown}
    >
      <span class="dot {tone}" aria-hidden="true"></span>
      <span class="name">{tab.title}</span>
      {#if tab.panes.length > 1}
        <span class="panes">/ {tab.panes.length} panes</span>
      {/if}
      {#if pane}
        <span class="state">{label}</span>
      {/if}
    </button>
  {/if}

  <button
    type="button"
    class="close sx-press"
    aria-label={`Close ${tab.title}`}
    title="Close session"
    onclick={() => sessions.requestClose(tab)}
  >
    <SessionIcon name="close" size={12} />
  </button>
</li>

<style>
  .tab {
    position: relative;
    list-style: none;
    display: flex;
    align-items: center;
    gap: var(--termru-space-4);
    min-width: 0;
    max-width: 300px;
    height: 100%;
    /* The 2px rule sits in the tab; it only shows on the active one. */
    padding: 0 var(--termru-space-12) 0 var(--termru-space-16);
    border-top: var(--termru-pane-rule-w) solid transparent;
    color: var(--termru-text-2);
    /* No transition: switching tabs is often a keyboard action. */
  }

  /* The active tab is the toolbar's own surface: it opens into it. */
  .tab.active {
    margin-bottom: -1px;
    padding-bottom: 1px;
    border-top-color: var(--termru-accent);
    background: var(--termru-base);
    color: var(--termru-accent-text);
  }

  .tab.drop-target {
    box-shadow: inset 2px 0 0 var(--termru-accent);
  }

  @media (hover: hover) and (pointer: fine) {
    .tab:not(.active):hover {
      background: color-mix(in srgb, var(--termru-base) 60%, transparent);
      color: var(--termru-text);
    }
  }

  .label {
    display: flex;
    align-items: center;
    gap: 10px;
    min-width: 0;
    min-height: 0;
    height: 100%;
    padding: 0;
    border: none;
    border-radius: 0;
    background: none;
    color: inherit;
    font-size: var(--termru-text-body-size);
    line-height: var(--termru-text-body-lh);
    font-weight: 450;
    text-align: left;
    /* A tab is switched from the keyboard as often as with the pointer: the
       name does not press, fade or move. */
    transition: none;
  }

  .label:active {
    transform: none;
  }

  .tab.active .label {
    font-weight: var(--termru-text-control-weight);
  }

  .name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .panes {
    flex: none;
    white-space: nowrap;
  }

  .state {
    flex: none;
    font-size: var(--termru-text-meta-size);
    line-height: var(--termru-text-meta-lh);
    font-weight: 450;
    white-space: nowrap;
  }

  .dot {
    flex: none;
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--termru-text-3);
  }

  .tab.active .dot.ready {
    background: var(--termru-accent);
  }

  .dot.busy {
    background: transparent;
    border: 1.5px solid currentColor;
  }

  .dot.failed {
    background: var(--termru-err-text);
  }

  .dot.closed {
    background: transparent;
    border: 1.5px solid var(--termru-text-3);
  }

  .rename {
    width: 10rem;
    min-height: 32px;
    padding: 0 var(--termru-space-8);
    border: var(--termru-border-w) solid var(--termru-accent);
    border-radius: var(--termru-radius-input);
    background: var(--termru-base);
    color: var(--termru-text);
    font: inherit;
    font-size: var(--termru-text-control-size);
  }

  .close {
    flex: none;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    min-height: 0;
    width: 28px;
    height: 28px;
    padding: 0;
    border: none;
    border-radius: var(--termru-radius-button);
    background: none;
    color: var(--termru-text-3);
  }

  @media (hover: hover) and (pointer: fine) {
    .close:hover {
      background: color-mix(in srgb, var(--termru-text) 7%, transparent);
      color: var(--termru-text);
    }
  }
</style>
