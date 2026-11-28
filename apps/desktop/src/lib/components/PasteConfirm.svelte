<script lang="ts">
  import { pasteGate, PASTE_PREVIEW_LINES, type PendingPaste } from '../terminal/paste.svelte';
  import SessionDialog from './SessionDialog.svelte';

  /**
   * "Paste N lines?": a paste of several lines runs each of them as soon as
   * the shell reads it, so it is shown first. The checkbox turns the
   * question off in Preferences (`terminal.confirm_multiline_paste`).
   */
  const request = $derived(pasteGate.pending);

  let last = $state.raw<PendingPaste | null>(null);
  $effect(() => {
    if (request) last = request;
  });
  const shown = $derived(request ?? last);

  let dontAsk = $state(false);
  $effect(() => {
    // A new question starts unchecked.
    if (request) dontAsk = false;
  });

  const hidden = $derived(shown ? shown.lines - shown.preview.length : 0);
</script>

<SessionDialog
  open={request !== null}
  role="dialog"
  title={shown ? `Paste ${shown.lines} lines?` : 'Paste?'}
  description="The shell runs each line as soon as it reads it."
  onclose={() => pasteGate.cancel()}
>
  {#if shown}
    <div class="preview" aria-label="The first {PASTE_PREVIEW_LINES} lines of the paste">
      {#each shown.preview as line, index (index)}
        <div class="line">{line === '' ? ' ' : line}</div>
      {/each}
      {#if hidden > 0}
        <div class="more">and {hidden} more line{hidden === 1 ? '' : 's'}</div>
      {/if}
    </div>
  {/if}
  <label class="remember">
    <input type="checkbox" bind:checked={dontAsk} />
    <span>
      Don’t ask again
      <span class="note">You can turn the question back on in Preferences.</span>
    </span>
  </label>

  {#snippet actions()}
    <button type="button" class="quiet sx-press" onclick={() => pasteGate.cancel()}>Cancel</button>
    <button
      type="button"
      class="sx-press primary"
      data-autofocus
      onclick={() => pasteGate.confirm(dontAsk)}
    >
      Paste
    </button>
  {/snippet}
</SessionDialog>

<style>
  .preview {
    padding: var(--termru-space-12);
    border: var(--termru-border-w) solid var(--termru-border);
    border-radius: var(--termru-radius-button);
    background: var(--termru-terminal);
    font-family: var(--termru-font-mono);
    font-size: var(--termru-text-secondary-size);
    line-height: var(--termru-text-secondary-lh);
  }

  .line {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: pre;
  }

  .more {
    margin-top: var(--termru-space-4);
    color: var(--termru-text-2);
    font-family: var(--termru-font-ui);
  }

  .remember {
    display: flex;
    align-items: flex-start;
    gap: var(--termru-space-8);
    color: var(--termru-text-2);
    font-size: var(--termru-text-secondary-size);
    line-height: var(--termru-text-secondary-lh);
    cursor: pointer;
  }

  .remember input {
    margin: 2px 0 0;
  }

  .note {
    display: block;
    color: var(--termru-text-3);
  }
</style>
