<script lang="ts">
  import { toasts } from '../stores/toasts.svelte';
</script>

<div class="toasts" aria-live="polite">
  {#each toasts.toasts as toast (toast.id)}
    <div class="toast {toast.kind}">
      <span>{toast.message}</span>
      <button type="button" aria-label="Dismiss" onclick={() => toasts.dismiss(toast.id)}>×</button>
    </div>
  {/each}
</div>

<style>
  .toasts {
    position: fixed;
    right: 1rem;
    bottom: 1rem;
    display: flex;
    flex-direction: column;
    gap: 0.4rem;
    z-index: 60;
    max-width: 26rem;
  }

  .toast {
    display: flex;
    align-items: flex-start;
    gap: 0.5rem;
    padding: 0.5rem 0.6rem;
    border-radius: var(--termru-radius);
    border: 1px solid var(--termru-border);
    background: var(--termru-surface);
    color: var(--termru-fg);
    font-size: 0.82rem;
    box-shadow: 0 6px 18px rgba(2, 6, 23, 0.35);
  }

  .toast.error {
    border-color: var(--termru-danger);
  }

  .toast.success {
    border-color: var(--termru-success);
  }

  button {
    background: none;
    border: none;
    color: inherit;
    cursor: pointer;
    font-size: 0.9rem;
    line-height: 1;
    padding: 0;
  }
</style>
