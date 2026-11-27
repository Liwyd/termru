<script lang="ts">
  import { connectFlow } from '../stores/connect.svelte';
  import { sessions, type Tab } from '../stores/sessions.svelte';
  import { inputLine } from '../terminal/session-view';

  /**
   * The bar at the foot of the page (frames 2 and 3): what the terminal is
   * doing on the left, the actions on the session on the right. Reconnect
   * works when the session is not connected; Close session always does.
   */
  let { tab }: { tab: Tab } = $props();

  const pane = $derived(sessions.activePaneOf(tab));
  const split = $derived(tab.panes.length > 1);

  const state = $derived(pane?.status.state);
  const canReconnect = $derived(
    pane?.host != null && (pane.dead || state === 'failed' || state === 'closed'),
  );
  const reconnectReason = $derived(
    !pane?.host
      ? 'There is no session to reconnect'
      : state === 'ready'
        ? 'The session is connected'
        : 'The session is still connecting',
  );
</script>

<div class="bar" class:split>
  {#if split}
    <p class="hint">
      Active pane indicated by label and rule. Closing a pane leaves the other running.
    </p>
  {:else if pane}
    <p class="meta">{inputLine(pane)}</p>
  {/if}

  <div class="actions" role="group" aria-label="Session actions">
    <span class="group-label">Session actions</span>
    <button
      type="button"
      class="action sx-press"
      aria-disabled={canReconnect ? undefined : 'true'}
      title={canReconnect ? 'Connect this session again' : reconnectReason}
      onclick={() => {
        if (canReconnect && pane) void connectFlow.reconnect(pane);
      }}
    >
      Reconnect
    </button>
    <button type="button" class="action danger sx-press" onclick={() => sessions.requestClose(tab)}>
      Close session
    </button>
  </div>
</div>

<style>
  .bar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--termru-space-4);
    min-height: 42px;
    padding: 0 var(--termru-space-3) 0 28px;
    background: var(--termru-surface);
    border-top: var(--termru-border-w) solid var(--termru-border);
  }

  /* With a footer on every pane the bar needs no rule of its own. */
  .bar.split {
    padding-left: 24px;
    border-top-color: transparent;
  }

  p {
    margin: 0;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .meta {
    font-family: var(--termru-font-mono);
    font-size: var(--termru-text-meta-size);
    line-height: var(--termru-text-meta-lh);
    letter-spacing: 0.02em;
    color: var(--termru-muted);
  }

  .hint {
    font-size: var(--termru-text-secondary-size);
    line-height: var(--termru-text-secondary-lh);
    color: var(--termru-muted);
  }

  .actions {
    display: flex;
    align-items: center;
    gap: var(--termru-space-1);
    margin-left: auto;
    flex: none;
  }

  .group-label {
    margin-right: var(--termru-space-2);
    font-size: var(--termru-text-meta-size);
    line-height: var(--termru-text-meta-lh);
    color: var(--termru-muted);
  }

  .action {
    min-height: var(--termru-control-h);
    padding: 0 var(--termru-space-3);
    border: none;
    border-radius: var(--termru-radius-button);
    background: transparent;
    color: var(--termru-fg);
    font-weight: 500;
  }

  .action.danger {
    color: var(--termru-danger);
  }

  .action[aria-disabled='true'] {
    color: var(--termru-faint);
    cursor: default;
  }

  @media (hover: hover) and (pointer: fine) {
    .action:hover:not([aria-disabled='true']) {
      background: var(--termru-subtle);
      border-color: transparent;
    }
  }

  @media (max-width: 900px) {
    .group-label {
      display: none;
    }

    .bar {
      padding-left: var(--termru-space-4);
    }
  }
</style>
