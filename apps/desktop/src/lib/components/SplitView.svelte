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

  /** At the top of the tree: the pane shown alone, when one is expanded. */
  const zoomed = $derived(
    depth === 0 && tab.zoomedPaneId
      ? (tab.panes.find((entry) => entry.id === tab.zoomedPaneId) ?? null)
      : null,
  );

  const single = $derived(tab.panes.length === 1);

  /**
   * Moves the divider. The ratio is relative to the size of the split, so
   * the pointer position is converted into a fraction of the container. The
   * panes follow the pointer directly (no transition): a drag that lagged
   * behind the hand would feel broken.
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

  function endDrag(event: PointerEvent): void {
    if (!dragging) return;
    dragging = false;
    const target = event.currentTarget as HTMLElement;
    if (target.hasPointerCapture(event.pointerId)) target.releasePointerCapture(event.pointerId);
  }
</script>

{#if zoomed}
  <PaneView pane={zoomed} focused={tab.activePaneId === zoomed.id} single={false} zoomed />
{:else if node.kind === 'leaf'}
  {#if pane}
    <PaneView {pane} focused={tab.activePaneId === pane.id} {single} />
  {/if}
{:else}
  <div
    class="split"
    class:columns={node.direction === 'columns'}
    class:rows={node.direction === 'rows'}
    class:dragging
  >
    <div class="side" style={`flex: ${node.ratio} 1 0%`}>
      {#key node.id + ':first'}
        <SplitView node={node.first} {tab} depth={depth + 1} />
      {/key}
    </div>

    <!--
      The window-splitter pattern (WAI-ARIA): a focusable separator that
      arrow keys move. Svelte's lint knows `separator` only as static.
    -->
    <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
    <div
      class="divider"
      class:dragging
      role="separator"
      tabindex="0"
      aria-orientation={node.direction === 'columns' ? 'vertical' : 'horizontal'}
      aria-label="Resize the panes"
      aria-valuemin={10}
      aria-valuemax={90}
      aria-valuenow={Math.round(node.ratio * 100)}
      title="Drag to resize the panes, double-click to even them out"
      onpointerdown={(event) => {
        if (event.button !== 0) return;
        dragging = true;
        (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
      }}
      onpointermove={onPointerMove}
      onpointerup={endDrag}
      onpointercancel={endDrag}
      ondblclick={() => sessions.resizeSplit(tab, node.id, 0.5)}
      onkeydown={(event) => {
        if (event.key === 'ArrowLeft' || event.key === 'ArrowUp') {
          event.preventDefault();
          sessions.resizeSplit(tab, node.id, node.ratio - 0.05);
        } else if (event.key === 'ArrowRight' || event.key === 'ArrowDown') {
          event.preventDefault();
          sessions.resizeSplit(tab, node.id, node.ratio + 0.05);
        }
      }}
    ></div>

    <div class="side" style={`flex: ${1 - node.ratio} 1 0%`}>
      {#key node.id + ':second'}
        <SplitView node={node.second} {tab} depth={depth + 1} />
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

  .split.dragging {
    user-select: none;
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

  /* The divider is the gap between two panes (6 px, on the page colour). A few
     pixels either side take the pointer too; the line that shows under the
     pointer, on focus and while it is held is a 2 px champagne bar inside the
     gap. */
  .divider {
    position: relative;
    flex: 0 0 var(--termru-divider-w);
    background: transparent;
    cursor: col-resize;
    touch-action: none;
  }

  .split.rows > .divider {
    cursor: row-resize;
  }

  .divider::before {
    content: '';
    position: absolute;
    inset: 0 -3px;
  }

  .split.rows > .divider::before {
    inset: -3px 0;
  }

  .divider::after {
    content: '';
    position: absolute;
    inset: 0 calc(50% - 1px);
    border-radius: 1px;
    background: var(--termru-accent);
    opacity: 0;
  }

  .split.rows > .divider::after {
    inset: calc(50% - 1px) 0;
  }

  @media (hover: hover) and (pointer: fine) {
    .divider::after {
      transition: opacity var(--motion-fast) ease;
    }

    .divider:hover::after {
      opacity: 0.7;
    }
  }

  .divider:focus-visible {
    outline: none;
  }

  .divider:focus-visible::after,
  .divider.dragging::after {
    opacity: 1;
  }
</style>
