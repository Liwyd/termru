<script lang="ts">
  import type { Snippet } from 'svelte';

  /**
   * The body of a list and inspector page: the content on the left and the
   * inspector (340 px) on the right, the way the Hosts page lays them out.
   * Both scroll on their own. When the room is short of what the two need (by
   * the width of this box, not of the window: the sidebar takes its share
   * first) the inspector goes under the content and the whole body scrolls, so
   * nothing the inspector offers is ever out of reach.
   *
   * The box is the container `body`: a page that lays its own content out in
   * columns asks `@container body (...)` the same question.
   */
  let {
    main,
    inspector,
    label,
  }: {
    main: Snippet;
    inspector?: Snippet;
    /** The accessible name of the content region. */
    label?: string;
  } = $props();
</script>

<div class="body-wrap">
  <div class="body" class:with-inspector={inspector !== undefined}>
    <div class="main" role={label ? 'region' : undefined} aria-label={label}>
      {@render main()}
    </div>
    {#if inspector}
      <div class="insp-slot">{@render inspector()}</div>
    {/if}
  </div>
</div>

<style>
  .body-wrap {
    display: flex;
    flex: 1;
    min-width: 0;
    min-height: 0;
    container: body / inline-size;
  }

  .body {
    display: grid;
    grid-template-columns: minmax(0, 1fr);
    flex: 1;
    min-width: 0;
    min-height: 0;
  }

  .body.with-inspector {
    grid-template-columns: minmax(0, 1fr) var(--termru-inspector-w);
  }

  .main {
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
    overflow: auto;
  }

  .insp-slot {
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
  }

  .insp-slot > :global(.insp) {
    flex: 1;
  }

  @container body (max-width: 860px) {
    .body,
    .body.with-inspector {
      grid-template-columns: minmax(0, 1fr);
      grid-auto-rows: max-content;
      overflow-y: auto;
    }

    .main {
      overflow: visible;
    }

    .insp-slot > :global(.insp) {
      flex: none;
      margin: 0 var(--termru-space-10) var(--termru-space-10);
      overflow: visible;
    }
  }
</style>
