<script lang="ts">
  import type { Snippet } from 'svelte';
  import { focusTrap } from '../actions/focus-trap';
  import { presence } from '../presence.svelte';

  /**
   * The modal of the Sessions page (closing a live session, pasting many
   * lines). It traps the focus, Escape cancels, the focus goes back to what
   * opened it, and it fades and scales in from the centre (160 ms, 100 ms
   * out; opacity only when motion is reduced).
   *
   * The caller owns `open`: it turns false on Cancel, Escape and a click on
   * the backdrop, and the dialog plays its exit before it leaves the page.
   */
  let {
    open,
    title,
    description,
    onclose,
    children,
    actions,
    role = 'alertdialog',
  }: {
    open: boolean;
    title: string;
    /** The sentence under the title (also the accessible description). */
    description?: string;
    onclose: () => void;
    /** What sits between the title and the buttons. */
    children?: Snippet;
    actions: Snippet;
    role?: 'dialog' | 'alertdialog';
  } = $props();

  const pop = presence(() => open);
  const uid = $props.id();
</script>

{#if pop.mounted}
  <div
    class="backdrop"
    class:shown={pop.shown}
    role="presentation"
    onclick={(event) => event.target === event.currentTarget && onclose()}
  >
    <div
      class="dialog"
      {role}
      aria-modal="true"
      aria-labelledby="{uid}-title"
      aria-describedby={description ? `${uid}-description` : undefined}
      tabindex="-1"
      use:focusTrap={{ onescape: onclose }}
    >
      <h2 id="{uid}-title">{title}</h2>
      {#if description}
        <p id="{uid}-description" class="description">{description}</p>
      {/if}
      {@render children?.()}
      <div class="actions">
        {@render actions()}
      </div>
    </div>
  </div>
{/if}

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 80;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: var(--termru-space-4);
    background: color-mix(in srgb, var(--termru-fg) 32%, transparent);
    opacity: 0;
    pointer-events: none;
    transition: opacity var(--motion-fast) var(--ease-out);
  }

  .backdrop.shown {
    opacity: 1;
    pointer-events: auto;
    transition: opacity var(--motion-context) var(--ease-out);
  }

  .dialog {
    width: min(26rem, 100%);
    max-height: calc(100vh - 2 * var(--termru-space-4));
    overflow: auto;
    padding: var(--termru-space-5);
    border: var(--termru-border-w) solid var(--termru-border);
    border-radius: var(--termru-radius-lg);
    background: var(--termru-surface);
    color: var(--termru-fg);
    box-shadow: var(--termru-shadow-float);
    display: flex;
    flex-direction: column;
    gap: var(--termru-space-3);
    transform: scale(var(--dialog-scale));
    transition: transform var(--motion-fast) var(--ease-out);
  }

  .backdrop.shown .dialog {
    transform: none;
    transition: transform var(--motion-context) var(--ease-out);
  }

  h2 {
    margin: 0;
    font-size: var(--termru-text-heading-size);
    line-height: var(--termru-text-heading-lh);
    font-weight: var(--termru-text-heading-weight);
  }

  .description {
    margin: 0;
    color: var(--termru-muted);
    font-size: var(--termru-text-body-size);
    line-height: var(--termru-text-body-lh);
  }

  .actions {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: var(--termru-space-3);
    margin-top: var(--termru-space-2);
  }
</style>
