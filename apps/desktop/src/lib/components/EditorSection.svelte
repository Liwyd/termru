<script lang="ts">
  import type { Snippet } from 'svelte';

  /**
   * One numbered section of the host editor (frame 4): the mono step label,
   * the 18 px heading and a muted sentence on the left, the fields on the
   * right. It collapses to one column under 860 px.
   */
  let {
    step,
    label,
    id,
    title,
    blurb,
    children,
  }: {
    /** `04` */
    step: string;
    /** `JUMP HOSTS` */
    label: string;
    /** The id of the heading (the section is labelled by it). */
    id: string;
    title: string;
    blurb: string;
    children: Snippet;
  } = $props();
</script>

<section class="section" aria-labelledby={id}>
  <div class="meta">
    <p class="wp-label step"><span>{step}</span> / {label}</p>
    <h2 {id}>{title}</h2>
    <p class="blurb">{blurb}</p>
  </div>
  <div class="fields">
    {@render children()}
  </div>
</section>

<style>
  .section {
    display: grid;
    grid-template-columns: 208px minmax(0, 1fr);
    column-gap: 30px;
    padding: var(--termru-space-24) 0;
    border-top: 1px solid var(--termru-border);
  }

  .section:first-child {
    border-top: none;
    padding-top: 0;
  }

  .meta {
    display: flex;
    flex-direction: column;
    gap: var(--termru-space-8);
    min-width: 0;
  }

  .step span {
    color: var(--termru-accent-text);
  }

  .step {
    color: var(--termru-accent-text);
  }

  h2 {
    margin: 0;
    font-size: 18px;
    line-height: 28px;
    font-weight: var(--termru-text-heading-weight);
  }

  .blurb {
    margin: 0;
    font-size: var(--termru-text-control-size);
    line-height: var(--termru-text-control-lh);
    color: var(--termru-text-2);
  }

  .fields {
    display: flex;
    flex-direction: column;
    gap: var(--termru-space-16);
    min-width: 0;
  }

  @media (max-width: 860px) {
    .section {
      grid-template-columns: minmax(0, 1fr);
      row-gap: var(--termru-space-16);
    }
  }
</style>
