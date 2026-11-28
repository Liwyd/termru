<script lang="ts">
  import SessionDialog from './SessionDialog.svelte';

  /**
   * The in-app confirmation for destructive actions (Doc A.05): title, body,
   * `Cancel` focused initially, a destructive confirm in the error colour,
   * focus returns to the trigger, Escape cancels. Replaces every
   * `window.confirm`. It is a `SessionDialog` like every other question of
   * the app, so it looks and moves like them.
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

<SessionDialog {open} {title} description={body} onclose={cancel}>
  {#snippet actions()}
    <button type="button" class="quiet sx-press" data-autofocus onclick={cancel}>Cancel</button>
    <button type="button" class="danger-fill sx-press" onclick={confirm}>{confirmLabel}</button>
  {/snippet}
</SessionDialog>
