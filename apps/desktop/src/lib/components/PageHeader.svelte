<script lang="ts">
  import type { Snippet } from 'svelte';

  /** One step of the breadcrumb; a step with `onclick` is a way back. */
  export interface Crumb {
    label: string;
    onclick?: () => void;
    /** The full text, when `label` is a shortened form of it (shown on hover). */
    title?: string;
  }

  /**
   * The head every route has (the page anatomy of the frames): a mono
   * breadcrumb, the 28/42 title, one muted line under it and the page's
   * primary action on the right.
   */
  let {
    crumbs,
    title,
    description,
    actions,
  }: {
    crumbs: Crumb[];
    title: string;
    description: string;
    /** The primary action (or the page's status), top right. */
    actions?: Snippet;
  } = $props();
</script>

<header class="head">
  <nav aria-label="Breadcrumb">
    <ol class="crumbs">
      {#each crumbs as crumb, index (index)}
        <li>
          {#if index > 0}<span class="sep" aria-hidden="true">/</span>{/if}
          {#if crumb.onclick}
            <button type="button" class="crumb-link" title={crumb.title} onclick={crumb.onclick}
              >{crumb.label}</button
            >
          {:else}
            <span
              class="crumb-text"
              title={crumb.title ?? crumb.label}
              aria-current={index === crumbs.length - 1 ? 'page' : undefined}>{crumb.label}</span
            >
          {/if}
        </li>
      {/each}
    </ol>
  </nav>
  <div class="title-row">
    <div class="titles">
      <h1>{title}</h1>
      <p class="description">{description}</p>
    </div>
    {#if actions}
      <div class="actions">{@render actions()}</div>
    {/if}
  </div>
</header>

<style>
  .head {
    display: flex;
    flex-direction: column;
    gap: var(--termru-space-1);
    min-width: 0;
  }

  .crumbs {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: var(--termru-space-2);
    margin: 0;
    padding: 0;
    list-style: none;
    font-family: var(--termru-font-mono);
    font-size: var(--termru-text-meta-size);
    line-height: var(--termru-text-meta-lh);
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--termru-muted);
  }

  .crumbs li {
    display: inline-flex;
    gap: var(--termru-space-2);
    min-width: 0;
  }

  /* A crumb is one line, whatever it holds (a 128 character host name, a
     folder five levels deep): it ends in an ellipsis and its title has the rest. */
  .crumbs li > .crumb-text,
  .crumbs li > .crumb-link {
    max-width: 36ch;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .sep {
    color: var(--termru-faint);
  }

  /* A step you can go back to: it reads as a link, not as a button. */
  .crumb-link {
    min-height: 0;
    padding: 0;
    border: none;
    border-radius: 2px;
    background: none;
    color: inherit;
    font: inherit;
    letter-spacing: inherit;
    text-transform: inherit;
    cursor: pointer;
  }

  @media (hover: hover) and (pointer: fine) {
    .crumb-link:hover {
      color: var(--termru-accent);
      text-decoration: underline;
    }
  }

  .title-row {
    display: flex;
    align-items: flex-end;
    justify-content: space-between;
    gap: var(--termru-space-4);
    min-width: 0;
  }

  .titles {
    min-width: 0;
  }

  h1 {
    margin: 0;
    font-size: var(--termru-text-title-size);
    line-height: var(--termru-text-title-lh);
    font-weight: var(--termru-text-title-weight);
    color: var(--termru-fg);
    overflow-wrap: anywhere;
  }

  .description {
    margin: 0;
    font-size: var(--termru-text-body-size);
    line-height: var(--termru-text-body-lh);
    color: var(--termru-muted);
  }

  .actions {
    display: flex;
    align-items: center;
    gap: var(--termru-space-3);
    flex: none;
  }

  @media (max-width: 720px) {
    .title-row {
      flex-direction: column;
      align-items: flex-start;
    }
  }
</style>
