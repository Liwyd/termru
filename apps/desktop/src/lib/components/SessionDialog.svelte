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
   *
   * The connect, host-key and sign-in dialogs are built on it too: `onsubmit`
   * makes the body a form (Enter submits), `wide` gives a long fingerprint
   * room, `tone="danger"` is for a question about a changed host key, and
   * `backdropCloses={false}` keeps a question that must be answered with a
   * button from going away on a stray click.
   */
  let {
    open,
    title,
    description,
    onclose,
    children,
    actions,
    role = 'alertdialog',
    wide = false,
    xwide = false,
    monoDescription = false,
    tone = 'default',
    backdropCloses = true,
    onsubmit,
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
    wide?: boolean;
    /** Room for a list (the import of the SSH config). */
    xwide?: boolean;
    /** The description is an address or a path: mono. */
    monoDescription?: boolean;
    tone?: 'default' | 'danger';
    backdropCloses?: boolean;
    onsubmit?: (event: SubmitEvent) => void;
  } = $props();

  const pop = presence(() => open);
  const uid = $props.id();
</script>

{#snippet body()}
  <h2 id="{uid}-title">{title}</h2>
  {#if description}
    <p id="{uid}-description" class="description" class:mono={monoDescription}>
      {description}
    </p>
  {/if}
  {@render children?.()}
  <div class="actions">
    {@render actions()}
  </div>
{/snippet}

{#if pop.mounted}
  <div
    class="backdrop"
    class:shown={pop.shown}
    role="presentation"
    onclick={(event) => backdropCloses && event.target === event.currentTarget && onclose()}
  >
    <div
      class="dialog sx-dialog"
      class:wide
      class:xwide
      class:danger={tone === 'danger'}
      {role}
      aria-modal={open ? 'true' : undefined}
      aria-labelledby="{uid}-title"
      aria-describedby={description ? `${uid}-description` : undefined}
      tabindex="-1"
      use:focusTrap={{ onescape: onclose }}
    >
      {#if onsubmit}
        <form {onsubmit}>
          {@render body()}
        </form>
      {:else}
        {@render body()}
      {/if}
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
    padding: var(--termru-space-16);
    background: var(--termru-scrim);
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
    max-height: calc(100vh - 2 * var(--termru-space-16));
    overflow: auto;
    padding: var(--termru-space-24);
    border: var(--termru-border-w) solid var(--termru-border);
    border-radius: var(--termru-radius-lg);
    background: var(--termru-raised);
    color: var(--termru-text);
    box-shadow: var(--termru-shadow-float);
    display: flex;
    flex-direction: column;
    gap: var(--termru-space-12);
    transform: scale(var(--dialog-scale));
    transition: transform var(--motion-fast) var(--ease-out);
  }

  .backdrop.shown .dialog {
    transform: none;
    transition: transform var(--motion-context) var(--ease-out);
  }

  .dialog.wide {
    width: min(32rem, 100%);
  }

  .dialog.xwide {
    width: min(44rem, 100%);
  }

  .dialog.danger {
    border-color: color-mix(in srgb, var(--termru-err-text) 50%, var(--termru-border));
  }

  .dialog.danger h2 {
    color: var(--termru-err-text);
  }

  /* The body is the dialog's own column, form or not. */
  form {
    display: flex;
    flex-direction: column;
    gap: var(--termru-space-12);
  }

  h2 {
    margin: 0;
    font-size: var(--termru-text-heading-size);
    line-height: var(--termru-text-heading-lh);
    font-weight: var(--termru-text-heading-weight);
  }

  .description {
    margin: 0;
    color: var(--termru-text-2);
    font-size: var(--termru-text-body-size);
    line-height: var(--termru-text-body-lh);
  }

  .description.mono {
    font-family: var(--termru-font-mono);
    font-size: var(--termru-text-secondary-size);
    line-height: var(--termru-text-secondary-lh);
    overflow-wrap: anywhere;
  }

  .actions {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: var(--termru-space-12);
    margin-top: var(--termru-space-8);
  }
</style>
