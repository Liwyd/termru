<script lang="ts">
  import type { Snippet } from 'svelte';

  /**
   * One group of the host editor: a sentence-case title, a quiet sentence under
   * it, and the fields. Groups are separated by space, not by lines; the
   * section index on the left of the page lists them by their titles.
   */
  let {
    id,
    title,
    blurb,
    children,
  }: {
    /** The id of the heading (the section is labelled by it, the index finds it by it). */
    id: string;
    title: string;
    blurb: string;
    children: Snippet;
  } = $props();
</script>

<section class="section" aria-labelledby={id}>
  <header class="meta">
    <h2 {id} tabindex="-1">{title}</h2>
    <p class="blurb">{blurb}</p>
  </header>
  <div class="fields">
    {@render children()}
  </div>
</section>

<style>
  .section {
    display: flex;
    flex-direction: column;
    gap: var(--termru-space-16);
    min-width: 0;
    /* A section reached from the index lands a little below the top edge. */
    scroll-margin-top: var(--termru-space-8);
  }

  .meta {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }

  h2 {
    margin: 0;
    font-size: var(--termru-text-title-size);
    line-height: var(--termru-text-title-lh);
    font-weight: var(--termru-text-title-weight);
    letter-spacing: -0.01em;
    color: var(--termru-text);
  }

  h2:focus {
    outline: none;
  }

  h2:focus-visible {
    outline: var(--termru-focus-w) solid var(--termru-accent-text);
    outline-offset: var(--termru-focus-offset);
    border-radius: 4px;
  }

  .blurb {
    margin: 0;
    font-size: var(--termru-text-secondary-size);
    line-height: var(--termru-text-secondary-lh);
    color: var(--termru-text-3);
  }

  .fields {
    display: flex;
    flex-direction: column;
    gap: var(--termru-space-16);
    min-width: 0;
  }
</style>
