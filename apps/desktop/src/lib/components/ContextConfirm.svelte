<script lang="ts">
  import { tick } from 'svelte';

  /**
   * The in-app confirmation for destructive actions (Doc A.05): title,
   * body, `Cancel` focused initially, destructive confirm, focus returns to
   * the trigger, Escape cancels. Replaces every `window.confirm`.
   */
  let {
    open = $bindable(false),
    title,
    body,
    confirmLabel = 'Delete',
    onconfirm,
  }: {
    open?: boolean;
    title: string;
    body: string;
    confirmLabel?: string;
    onconfirm: () => void;
  } = $props();

  let cancelButton = $state<HTMLButtonElement | null>(null);
  /** The element that had the focus before the dialog opened. */
  let trigger: HTMLElement | null = null;

  $effect(() => {
    if (!open) return;
    trigger = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    void tick().then(() => cancelButton?.focus());
    return () => {
      trigger?.focus();
      trigger = null;
    };
  });

  function cancel(): void {
    open = false;
  }

  function confirm(): void {
    open = false;
    onconfirm();
  }

  function onkeydown(event: KeyboardEvent): void {
    if (event.key === 'Escape') {
      event.preventDefault();
      cancel();
    }
  }
</script>

{#if open}
  <div
    class="backdrop"
    role="presentation"
    onclick={(e) => e.target === e.currentTarget && cancel()}
  >
    <div
      class="dialog"
      role="alertdialog"
      aria-modal="true"
      aria-labelledby="confirm-title"
      aria-describedby="confirm-body"
      tabindex="-1"
      {onkeydown}
    >
      <h2 id="confirm-title">{title}</h2>
      <p id="confirm-body">{body}</p>
      <div class="actions">
        <button type="button" bind:this={cancelButton} onclick={cancel}>Cancel</button>
        <button type="button" class="destructive" onclick={confirm}>{confirmLabel}</button>
      </div>
    </div>
  </div>
{/if}

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    background: color-mix(in srgb, var(--termru-fg) 35%, transparent);
    z-index: 60;
  }

  .dialog {
    width: min(24rem, calc(100vw - 2rem));
    padding: var(--termru-space-5);
    border: 1px solid var(--termru-border);
    border-radius: var(--termru-radius-lg);
    background: var(--termru-surface);
    color: var(--termru-fg);
    display: flex;
    flex-direction: column;
    gap: var(--termru-space-3);
  }

  h2 {
    margin: 0;
    font-size: var(--termru-text-heading-size);
    line-height: var(--termru-text-heading-lh);
    font-weight: var(--termru-text-heading-weight);
  }

  p {
    margin: 0;
    font-size: var(--termru-text-body-size);
    line-height: var(--termru-text-body-lh);
    color: var(--termru-muted);
  }

  .actions {
    display: flex;
    justify-content: flex-end;
    gap: var(--termru-space-3);
    margin-top: var(--termru-space-2);
  }

  button {
    min-height: var(--termru-control-h);
    padding: 0 var(--termru-space-4);
    border: 1px solid var(--termru-border);
    border-radius: var(--termru-radius-button);
    background: var(--termru-surface);
    color: var(--termru-fg);
    font-family: inherit;
    font-size: var(--termru-text-control-size);
    font-weight: var(--termru-text-control-weight);
    cursor: pointer;
  }

  button.destructive {
    border-color: var(--termru-danger);
    background: var(--termru-danger);
    color: var(--termru-on-danger);
  }
</style>
