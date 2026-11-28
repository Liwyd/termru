<script lang="ts">
  import { focusTrap } from '../actions/focus-trap';

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

  function cancel(): void {
    open = false;
  }

  function confirm(): void {
    open = false;
    onconfirm();
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
      use:focusTrap={{ onescape: cancel }}
    >
      <h2 id="confirm-title">{title}</h2>
      <p id="confirm-body">{body}</p>
      <div class="actions">
        <button type="button" data-autofocus onclick={cancel}>Cancel</button>
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
    background: var(--termru-scrim);
    z-index: 60;
  }

  .dialog {
    width: min(24rem, calc(100vw - 2rem));
    padding: var(--termru-space-24);
    border: 1px solid var(--termru-border);
    border-radius: var(--termru-radius-lg);
    background: var(--termru-raised);
    color: var(--termru-text);
    box-shadow: var(--termru-shadow-float);
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

  p {
    margin: 0;
    font-size: var(--termru-text-body-size);
    line-height: var(--termru-text-body-lh);
    color: var(--termru-text-2);
  }

  .actions {
    display: flex;
    justify-content: flex-end;
    gap: var(--termru-space-12);
    margin-top: var(--termru-space-8);
  }

  button {
    min-height: var(--termru-control-h);
    padding: 0 var(--termru-space-16);
    border: none;
    border-radius: var(--termru-radius-button);
    background: var(--termru-raised-2);
    color: var(--termru-text);
    font-family: inherit;
    font-size: var(--termru-text-control-size);
    font-weight: var(--termru-text-control-weight);
    cursor: pointer;
  }

  button.destructive {
    background: var(--termru-err-text);
    color: var(--termru-on-err);
  }
</style>
