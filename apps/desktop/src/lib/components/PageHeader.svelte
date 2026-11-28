<script lang="ts">
  import type { Snippet } from 'svelte';
  import Icon from './Icon.svelte';

  /**
   * The head every page has: ONE line, 56 px. The title and its count, the
   * view chips or filters, a spacer, the secondary actions and the page's one
   * primary action (champagne). No breadcrumb, no description: the title says
   * where you are, the sidebar says how you got there.
   */
  let {
    title,
    count = null,
    back,
    chips,
    actions,
  }: {
    title: string;
    /** How many things the page lists (shown after the title, in mono). */
    count?: string | number | null;
    /** A way back, for a page that is opened from another (the host editor). */
    back?: { label: string; onclick: () => void };
    /** View chips and filters, right after the title. */
    chips?: Snippet;
    /** Secondary actions, then the primary one, at the right. */
    actions?: Snippet;
  } = $props();
</script>

<header class="head">
  {#if back}
    <button
      type="button"
      class="back ghost"
      aria-label={back.label}
      title={back.label}
      onclick={back.onclick}
    >
      <Icon name="arrow-left" size={16} />
    </button>
  {/if}
  <h1 {title}>{title}</h1>
  {#if count !== null && count !== ''}<span class="count" title={String(count)}>{count}</span>{/if}
  {#if chips}<div class="chips">{@render chips()}</div>{/if}
  <span class="grow"></span>
  {#if actions}<div class="actions">{@render actions()}</div>{/if}
</header>

<style>
  .head {
    display: flex;
    align-items: center;
    gap: 14px;
    flex: none;
    height: var(--termru-header-h);
    padding: 0 var(--termru-space-24) 0 var(--termru-page-inset-x);
    min-width: 0;
    /* Pages size their chips and actions against the width of the header. */
    container: head / inline-size;
  }

  .back {
    flex: none;
    width: 30px;
    height: 30px;
    min-height: 0;
    padding: 0;
    margin-left: -8px;
    display: inline-grid;
    place-items: center;
  }

  h1 {
    flex: 0 1 auto;
    min-width: 0;
    margin: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: var(--termru-text-title-size);
    line-height: var(--termru-text-title-lh);
    font-weight: var(--termru-text-title-weight);
    letter-spacing: -0.01em;
    color: var(--termru-text);
  }

  .count {
    flex: 0 1 auto;
    min-width: 0;
    max-width: 32ch;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font: 500 12px var(--termru-font-mono);
    color: var(--termru-text-3);
  }

  .chips {
    display: flex;
    align-items: center;
    gap: 2px;
    flex: none;
    margin-left: var(--termru-space-10);
  }

  .grow {
    flex: 1;
    min-width: 0;
  }

  .actions {
    display: flex;
    align-items: center;
    gap: var(--termru-space-8);
    flex: none;
  }

  /* The header buttons are 30 px, as in the design. */
  .actions :global(button) {
    min-height: 30px;
  }
</style>
