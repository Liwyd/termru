<script lang="ts">
  import { autofocus } from '../actions/autofocus';
  import { sessions } from '../stores/sessions.svelte';
  import { statusLabel, statusTone } from '../terminal/session-view';
  import SessionIcon from './SessionIcon.svelte';
  import type { Tab } from '../stores/sessions.svelte';

  /**
   * One tab of the strip: the dot, the name, `N panes` for a split tab, and
   * the close button. The state is the dot's colour and, when the session is
   * not connected, a word after the name (a connected tab says it to a screen
   * reader only: the quiet case stays quiet). Duplicate and Detach live in the
   * toolbar, where they act on the session you are looking at.
   */
  let { tab, active }: { tab: Tab; active: boolean } = $props();

  /** The pane that carries the tab: its status is the status of the tab. */
  const pane = $derived(sessions.activePaneOf(tab));

  const tone = $derived(pane ? statusTone(pane.status) : 'closed');
  const label = $derived(pane ? statusLabel(pane) : '');
  const connected = $derived(tone === 'ready');

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
        <span class="panes">{tab.panes.length} panes</span>
      {/if}
      {#if pane}
        <span class="state" class:quiet={connected}>{label}</span>
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
    flex: 0 1 auto;
    min-width: 0;
    max-width: 280px;
    height: var(--termru-tab-h);
    padding: 0 var(--termru-space-6) 0 var(--termru-space-12);
    border-radius: var(--termru-radius-input);
    color: var(--termru-text-2);
    /* No transition: switching tabs is often a keyboard action. */
  }

  /* The open tab is raised one step: no rule, no outline. */
  .tab.active {
    background: var(--termru-raised);
    color: var(--termru-text);
  }

  .tab.drop-target {
    box-shadow: inset 2px 0 0 var(--termru-accent);
  }

  @media (hover: hover) and (pointer: fine) {
    .tab:not(.active):hover {
      background: color-mix(in srgb, var(--termru-text) 5%, transparent);
      color: var(--termru-text);
    }
  }

  .label {
    display: flex;
    align-items: center;
    gap: var(--termru-space-8);
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
    font-weight: var(--termru-text-control-weight);
    text-align: left;
    /* A tab is switched from the keyboard as often as with the pointer: the
       name does not press, fade or move. */
    transition: none;
  }

  .label:active {
    transform: none;
  }

  .name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .panes,
  .state {
    flex: none;
    font-family: var(--termru-font-mono);
    font-size: var(--termru-text-meta-size);
    line-height: var(--termru-text-meta-lh);
    font-weight: var(--termru-text-meta-weight);
    white-space: nowrap;
  }

  .panes {
    color: var(--termru-text-3);
  }

  /* The word of a session that is not connected, in the colour of its dot. */
  .state {
    color: var(--termru-warn-text);
  }

  .dot.failed ~ .state {
    color: var(--termru-err-text);
  }

  .dot.closed ~ .state {
    color: var(--termru-text-3);
  }

  /* Connected is said to a screen reader only. */
  .state.quiet {
    position: absolute;
    width: 1px;
    height: 1px;
    margin: -1px;
    overflow: hidden;
    clip-path: inset(50%);
    white-space: nowrap;
  }

  /* The dot of the sidebar's Live list: a live session has a soft halo. */
  .dot {
    flex: none;
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--termru-live);
    box-shadow: 0 0 0 3px color-mix(in srgb, var(--termru-live) 16%, transparent);
  }

  .dot.busy {
    background: var(--termru-warn);
    box-shadow: 0 0 0 3px color-mix(in srgb, var(--termru-warn) 16%, transparent);
  }

  .dot.failed {
    background: var(--termru-err);
    box-shadow: 0 0 0 3px color-mix(in srgb, var(--termru-err) 16%, transparent);
  }

  .dot.closed {
    background: var(--termru-text-3);
    box-shadow: none;
  }

  /* The field of the app is 36 px tall; the name field fits the 32 px tab. */
  .tab input.rename:not([type='checkbox']):not([type='radio']) {
    width: 10rem;
    min-height: 26px;
    height: 26px;
    padding: 0 var(--termru-space-8);
    border: var(--termru-border-w) solid var(--termru-accent);
    border-radius: var(--termru-radius-sm);
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
    width: 22px;
    height: 22px;
    padding: 0;
    border: none;
    border-radius: var(--termru-radius-sm);
    background: none;
    color: var(--termru-text-3);
  }

  @media (hover: hover) and (pointer: fine) {
    button.close:hover:not(:disabled) {
      background: color-mix(in srgb, var(--termru-text) 8%, transparent);
      color: var(--termru-text);
    }
  }
</style>
