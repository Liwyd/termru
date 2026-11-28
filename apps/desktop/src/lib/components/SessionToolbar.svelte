<script lang="ts">
  import { acceleratorFor, acceleratorLabel } from '../keybindings';
  import type { MenuEntry } from '../menu';
  import { connectFlow } from '../stores/connect.svelte';
  import { prefs } from '../stores/prefs.svelte';
  import { sessions, type Tab } from '../stores/sessions.svelte';
  import { terminalUi } from '../stores/terminal-ui.svelte';
  import { notify } from '../notify.svelte';
  import { copyText } from '../terminal/links';
  import { getTerminal } from '../terminal/registry';
  import { reconnectState, sshCommand } from '../terminal/session-view';
  import { execApproval } from '../stores/exec-approval.svelte';
  import ForwardsIndicator from './ForwardsIndicator.svelte';
  import PopMenu from './PopMenu.svelte';
  import SessionIcon from './SessionIcon.svelte';

  /**
   * The tools at the right of the tab strip, for the session on screen: the
   * forwards chip, the notice that synced settings are off, Split right,
   * Split down, Duplicate, Detach (move to a window of its own) and More,
   * which holds every other action. They are icon buttons with names and
   * tooltips; the strip and the tools share one 46 px row (`SessionsPage`).
   */
  let { tab }: { tab: Tab } = $props();

  const pane = $derived(sessions.activePaneOf(tab));
  const host = $derived(pane?.host ?? null);
  /** Duplicate and Detach work from a session that is connected. */
  const connected = $derived(pane?.status.state === 'ready' && pane.sessionId !== null);
  const full = $derived(tab.panes.length >= 16);

  const bindings = $derived(prefs.prefs.keybindings);
  const rightChord = $derived(acceleratorLabel(acceleratorFor('pane.splitRight', bindings)));
  const downChord = $derived(acceleratorLabel(acceleratorFor('pane.splitDown', bindings)));

  const splitReason = 'A tab holds at most 16 panes';
  const needsSession = (what: string) => `${what} needs a connected session`;

  function splitDown(): void {
    if (!pane || full) return;
    sessions.focusPane(pane);
    sessions.splitActive('rows');
  }

  function splitRight(): void {
    if (!pane || full) return;
    sessions.focusPane(pane);
    sessions.splitActive('columns');
  }

  // --- More menu -------------------------------------------------------

  let more = $state<{ x: number; y: number; animate: boolean } | null>(null);

  function toggleMore(event: MouseEvent): void {
    if (more) {
      more = null;
      return;
    }
    const box = (event.currentTarget as HTMLElement).getBoundingClientRect();
    // `detail` is 0 for a click made with the keyboard: that opens without motion.
    more = { x: box.right, y: box.bottom + 4, animate: event.detail > 0 };
  }

  const entries = $derived.by((): MenuEntry[] => {
    const again = reconnectState(pane);
    const list: MenuEntry[] = [
      { id: 'reconnect', label: 'Reconnect', disabled: !again.can, reason: again.reason },
      { id: 'find', label: 'Find in terminal', disabled: !pane?.host, reason: 'No terminal here' },
      { id: 'clear', label: 'Clear scrollback', disabled: !pane?.host, reason: 'No terminal here' },
      {
        id: 'copy-ssh',
        label: 'Copy SSH command',
        disabled: !host,
        reason: 'No host in this pane',
      },
    ];
    if (pane?.execPending && host) {
      list.push({ id: 'review-settings', label: 'Review synced settings…', separatorBefore: true });
    }
    if (tab.panes.length > 1) {
      list.push({ id: 'close-pane', label: 'Close pane', separatorBefore: true });
    }
    list.push({
      id: 'close-session',
      label: 'Close session',
      danger: true,
      separatorBefore: tab.panes.length <= 1,
    });
    return list;
  });

  function choose(id: string): void {
    if (!pane) return;
    switch (id) {
      case 'reconnect':
        void connectFlow.reconnect(pane);
        break;
      case 'find':
        terminalUi.openSearch(pane.id);
        break;
      case 'clear': {
        const handle = getTerminal(pane.id);
        handle?.term.clear();
        handle?.term.focus();
        break;
      }
      case 'copy-ssh':
        if (host) {
          void copyText(sshCommand(host)).then((ok) =>
            ok ? notify({ key: 'ssh.command-copied' }) : notify({ key: 'clipboard.copy-failed' }),
          );
        }
        break;
      case 'review-settings':
        if (host) void execApproval.show(host.id);
        break;
      case 'close-pane':
        sessions.requestClosePane(pane);
        break;
      case 'close-session':
        sessions.requestClose(tab);
        break;
    }
  }
</script>

<div class="tools" role="toolbar" aria-label="Session">
  {#if pane?.execPending && host}
    <button
      type="button"
      class="approval sx-press"
      title="Synced settings of this host are not approved on this device, so this session runs without them. Review them."
      onclick={() => void execApproval.show(host.id)}
    >
      <SessionIcon name="alert" size={13} />
      Synced settings off
      <span class="review">Review</span>
    </button>
  {/if}
  <ForwardsIndicator {pane} />

  <button
    type="button"
    class="tool sx-press"
    aria-label="Split right, new pane to the right"
    aria-disabled={!pane || full ? 'true' : undefined}
    title={full ? splitReason : `Split right (${rightChord})`}
    onclick={splitRight}
  >
    <SessionIcon name="split-v" />
  </button>
  <button
    type="button"
    class="tool sx-press"
    aria-label="Split down, new pane below"
    aria-disabled={!pane || full ? 'true' : undefined}
    title={full ? splitReason : `Split down (${downChord})`}
    onclick={splitDown}
  >
    <SessionIcon name="split-h" />
  </button>
  <button
    type="button"
    class="tool sx-press"
    aria-label="Duplicate"
    aria-disabled={connected ? undefined : 'true'}
    title={connected
      ? 'Open a second session to the same host in a new tab'
      : needsSession('Duplicate')}
    onclick={() => {
      if (connected) sessions.duplicate(tab);
    }}
  >
    <SessionIcon name="duplicate" />
  </button>
  <button
    type="button"
    class="tool sx-press"
    aria-label="Detach"
    aria-disabled={connected ? undefined : 'true'}
    title={connected ? 'Move this session to a window of its own' : needsSession('Detach')}
    onclick={() => {
      if (connected) void sessions.moveToNewWindow(tab);
    }}
  >
    <SessionIcon name="detach" />
  </button>
  <button
    type="button"
    class="tool sx-press"
    aria-label="More"
    aria-haspopup="menu"
    aria-expanded={more !== null}
    title="More session actions"
    onclick={toggleMore}
  >
    <SessionIcon name="more" />
  </button>
</div>

<PopMenu
  open={more !== null}
  x={more?.x ?? 0}
  y={more?.y ?? 0}
  align="right"
  animate={more?.animate ?? true}
  label="More session actions"
  items={entries}
  onchoose={choose}
  onclose={() => (more = null)}
/>

<style>
  .tools {
    display: flex;
    align-items: center;
    gap: 2px;
    flex: none;
  }

  /* Synced settings are off: warn-tinted, like the forwards chip, with the way
     to review them in the same button. */
  .approval {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    min-height: 26px;
    margin-right: var(--termru-space-6);
    padding: 0 9px;
    border: none;
    border-radius: var(--termru-radius-sm);
    background: var(--termru-warn-soft);
    color: var(--termru-warn-text);
    font-family: var(--termru-font-mono);
    font-size: var(--termru-text-meta-size);
    line-height: var(--termru-text-meta-lh);
    font-weight: var(--termru-text-meta-weight);
    white-space: nowrap;
  }

  .approval .review {
    text-decoration: underline;
    text-underline-offset: 2px;
  }

  @media (hover: hover) and (pointer: fine) {
    button.approval:hover:not(:disabled) {
      background-color: color-mix(in srgb, var(--termru-warn) 20%, transparent);
    }
  }

  .tool {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: var(--termru-tool-size);
    height: var(--termru-tool-size);
    min-height: 0;
    padding: 0;
    border: none;
    border-radius: var(--termru-radius-input);
    background: transparent;
    color: var(--termru-text-2);
  }

  @media (hover: hover) and (pointer: fine) {
    button.tool:hover:not(:disabled):not([aria-disabled='true']) {
      background: var(--termru-raised);
      color: var(--termru-text);
    }

    /* A disabled tool is read, not lit. */
    button.tool[aria-disabled='true']:hover {
      background: transparent;
    }
  }

  .tool[aria-expanded='true'] {
    background: var(--termru-raised);
    color: var(--termru-text);
  }

  .tool[aria-disabled='true'] {
    color: var(--termru-text-3);
    cursor: default;
  }
</style>
