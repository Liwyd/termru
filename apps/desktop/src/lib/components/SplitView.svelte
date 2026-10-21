<script lang="ts">
  import type { TreeNode } from '../pane-tree';
  import { sessions } from '../stores/sessions.svelte';
  import PaneView from './PaneView.svelte';
  // The tree is recursive: the component renders itself.
  import SplitView from './SplitView.svelte';
  import type { Tab } from '../stores/sessions.svelte';

  let {
    node,
    tab,
    depth = 0,
  }: {
    node: TreeNode;
    tab: Tab;
    depth?: number;
  } = $props();

  /** True while the divider is being dragged. */
  let dragging = $state(false);

  const pane = $derived(
    node.kind === 'leaf' ? (tab.panes.find((entry) => entry.id === node.pane) ?? null) : null,
  );

  /**
   * Moves the divider. The ratio is relative to the size of the split, so
   * the pointer position is converted into a fraction of the container.
   */
  function onPointerMove(event: PointerEvent): void {
    if (!dragging || node.kind !== 'split') return;
    const container = (event.currentTarget as HTMLElement).parentElement;
    if (!container) return;
    const box = container.getBoundingClientRect();
    const ratio =
      node.direction === 'columns'
        ? (event.clientX - box.left) / Math.max(1, box.width)
        : (event.clientY - box.top) / Math.max(1, box.height);
    sessions.resizeSplit(tab, node.id, ratio);
  }
</script>

{#if node.kind === 'leaf'}
  {#if pane}
    <PaneView {pane} focused={tab.activePaneId === pane.id} />
  {/if}
{:else}
  <div
    class="split"
    class:columns={node.direction === 'columns'}
    class:rows={node.direction === 'rows'}
  >
    <div class="side" style={`flex: ${node.ratio} 1 0%`}>
      {#key node.id + ':first'}
        <SplitView node={node.first} {tab} {depth} />
      {/key}
    </div>

    <button
      type="button"
      class="divider"
      aria-label="Resize the panes"
      title="Drag to resize the panes"
      onpointerdown={(event) => {
        dragging = true;
        (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
      }}
      onpointermove={onPointerMove}
      onpointerup={(event) => {
        dragging = false;
        (event.currentTarget as HTMLElement).releasePointerCapture(event.pointerId);
      }}
      onkeydown={(event) => {
        if (event.key === 'ArrowLeft' || event.key === 'ArrowUp') {
          event.preventDefault();
          sessions.resizeSplit(tab, node.id, node.ratio - 0.05);
        } else if (event.key === 'ArrowRight' || event.key === 'ArrowDown') {
          event.preventDefault();
          sessions.resizeSplit(tab, node.id, node.ratio + 0.05);
        }
      }}
    ></button>

    <div class="side" style={`flex: ${1 - node.ratio} 1 0%`}>
      {#key node.id + ':second'}
        <SplitView node={node.second} {tab} {depth} />
      {/key}
    </div>
  </div>
{/if}

<style>
  .split {
    display: flex;
    height: 100%;
    min-width: 0;
    min-height: 0;
  }

  .split.columns {
    flex-direction: row;
  }

  .split.rows {
    flex-direction: column;
  }

  .side {
    display: flex;
    min-width: 0;
    min-height: 0;
  }

  .side > :global(*) {
    flex: 1;
    min-width: 0;
    min-height: 0;
  }

  .divider {
    flex: 0 0 4px;
    padding: 0;
    border: none;
    background: var(--termru-border);
    cursor: col-resize;
  }

  .split.rows .divider {
    cursor: row-resize;
  }

  .divider:hover,
  .divider:focus-visible {
    background: var(--termru-accent);
    outline: none;
  }
</style>
