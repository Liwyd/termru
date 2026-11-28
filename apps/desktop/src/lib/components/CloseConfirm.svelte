<script lang="ts">
  import { sessions, type CloseRequest } from '../stores/sessions.svelte';
  import SessionDialog from './SessionDialog.svelte';

  /**
   * "Close this session?": asked before a tab or a pane with a live session
   * goes away (the tab's close button, Close session, the shortcuts). The
   * request lives in the session store; this only shows it.
   */
  const request = $derived(sessions.closeRequest);

  // The dialog plays its exit after the request is gone: it keeps showing
  // the words it had.
  let last = $state.raw<CloseRequest | null>(null);
  $effect(() => {
    if (request) last = request;
  });
  const shown = $derived(request ?? last);

  const tab = $derived(shown ? sessions.tabs.find((entry) => entry.id === shown.tabId) : undefined);
  const pane = $derived(
    shown?.paneId ? tab?.panes.find((entry) => entry.id === shown.paneId) : undefined,
  );

  const copy = $derived.by(() => {
    if (!shown) return { title: '', body: '', confirm: 'Close' };
    if (shown.kind === 'pane') {
      return {
        title: 'Close this pane?',
        body: `This ends its SSH session${pane?.host ? ` to ${pane.host.name}` : ''}. The other panes keep running.`,
        confirm: 'Close pane',
      };
    }
    const name = tab?.title ?? 'this tab';
    if (shown.live > 1) {
      return {
        title: `Close “${name}”?`,
        body: `This ends its ${shown.live} SSH sessions. Programs running in them may stop.`,
        confirm: 'Close sessions',
      };
    }
    return {
      title: `Close “${name}”?`,
      body: 'This ends the SSH session. Programs running in it may stop.',
      confirm: 'Close session',
    };
  });
</script>

<SessionDialog
  open={request !== null}
  title={copy.title}
  description={copy.body}
  onclose={() => sessions.cancelClose()}
>
  {#snippet actions()}
    <button type="button" class="sx-press" data-autofocus onclick={() => sessions.cancelClose()}>
      Cancel
    </button>
    <button type="button" class="sx-press danger-fill" onclick={() => void sessions.confirmClose()}>
      {copy.confirm}
    </button>
  {/snippet}
</SessionDialog>

<style>
  button.danger-fill {
    border-color: var(--termru-err-text);
    background: var(--termru-err-text);
    color: var(--termru-on-err);
  }
</style>
