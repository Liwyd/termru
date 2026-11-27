<script lang="ts">
  import { acceleratorFor, acceleratorLabel } from '../keybindings';
  import type { MenuEntry } from '../menu';
  import { connectFlow } from '../stores/connect.svelte';
  import { hosts } from '../stores/hosts.svelte';
  import { prefs } from '../stores/prefs.svelte';
  import { sessions, type Tab } from '../stores/sessions.svelte';
  import { terminalUi } from '../stores/terminal-ui.svelte';
  import { notify } from '../notify.svelte';
  import { copyText } from '../terminal/links';
  import { getTerminal } from '../terminal/registry';
  import { sshCommand, sshTarget } from '../terminal/session-view';
  import PopMenu from './PopMenu.svelte';
  import SessionIcon from './SessionIcon.svelte';
  import StatusBadge from './StatusBadge.svelte';

  /**
   * The bar under the tabs (frame 2): the state of the session you are
   * looking at, where it is, and what you can do with it: Split H, Split V,
   * Duplicate, Detach (move to a window of its own) and More.
   */
  let { tab }: { tab: Tab } = $props();

  const pane = $derived(sessions.activePaneOf(tab));
  const host = $derived(pane?.host ?? null);
  const folder = $derived(
    host?.group_id
      ? (hosts.folderOptions.find((option) => option.id === host.group_id)?.label ?? null)
      : null,
  );

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
    more = { x: box.right, y: box.bottom + 6, animate: event.detail > 0 };
  }

  const entries = $derived.by((): MenuEntry[] => {
    const ready = pane?.status.state === 'ready';
    const list: MenuEntry[] = [
      {
        id: 'reconnect',
        label: 'Reconnect',
        disabled: !pane?.host || ready,
        reason: ready ? 'The session is connected' : 'There is no session to reconnect',
      },
      { id: 'find', label: 'Find in terminal', disabled: !pane?.host, reason: 'No terminal here' },
      { id: 'clear', label: 'Clear scrollback', disabled: !pane?.host, reason: 'No terminal here' },
      {
        id: 'copy-ssh',
        label: 'Copy SSH command',
        disabled: !host,
        reason: 'No host in this pane',
      },
    ];
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
      case 'close-pane':
        sessions.requestClosePane(pane);
        break;
      case 'close-session':
        sessions.requestClose(tab);
        break;
    }
  }
</script>

<div class="toolbar" role="toolbar" aria-label="Session">
  <div class="where">
    {#if pane}<StatusBadge {pane} />{/if}
    {#if host}
      <span class="target" title={host.name}>{sshTarget(host)}</span>
      {#if folder}<span class="folder" title="Folder">{folder}</span>{/if}
    {:else}
      <span class="folder">No host in this pane yet</span>
    {/if}
  </div>

  <div class="actions">
    <button
      type="button"
      class="action sx-press"
      aria-label="Split H, new pane below"
      aria-disabled={!pane || full ? 'true' : undefined}
      title={full ? splitReason : `Split the pane, new pane below (${downChord})`}
      onclick={splitDown}
    >
      <SessionIcon name="split-h" />
      <span class="label">Split H</span>
    </button>
    <button
      type="button"
      class="action sx-press"
      aria-label="Split V, new pane to the right"
      aria-disabled={!pane || full ? 'true' : undefined}
      title={full ? splitReason : `Split the pane, new pane to the right (${rightChord})`}
      onclick={splitRight}
    >
      <SessionIcon name="split-v" />
      <span class="label">Split V</span>
    </button>
    <button
      type="button"
      class="action sx-press"
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
      <span class="label">Duplicate</span>
    </button>
    <button
      type="button"
      class="action sx-press"
      aria-label="Detach"
      aria-disabled={connected ? undefined : 'true'}
      title={connected ? 'Move this session to a window of its own' : needsSession('Detach')}
      onclick={() => {
        if (connected) void sessions.moveToNewWindow(tab);
      }}
    >
      <SessionIcon name="detach" />
      <span class="label">Detach</span>
    </button>
    <button
      type="button"
      class="action sx-press"
      aria-label="More"
      aria-haspopup="menu"
      aria-expanded={more !== null}
      onclick={toggleMore}
    >
      <span class="label">More</span>
      <SessionIcon name="caret" size={14} />
    </button>
  </div>
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
  .toolbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--termru-space-4);
    min-height: var(--termru-toolbar-h);
    padding: 0 var(--termru-space-3) 0 28px;
    background: var(--termru-surface);
  }

  .where {
    display: flex;
    align-items: center;
    gap: var(--termru-space-3);
    min-width: 0;
  }

  .target {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-family: var(--termru-font-mono);
    font-size: var(--termru-text-secondary-size);
    line-height: var(--termru-text-secondary-lh);
    color: var(--termru-fg);
  }

  .folder {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: var(--termru-text-secondary-size);
    line-height: var(--termru-text-secondary-lh);
    color: var(--termru-muted);
  }

  .actions {
    display: flex;
    align-items: center;
    gap: var(--termru-space-1);
    flex: none;
  }

  .action {
    display: inline-flex;
    align-items: center;
    gap: var(--termru-space-2);
    min-height: var(--termru-control-h);
    padding: 0 var(--termru-space-3);
    border: none;
    border-radius: var(--termru-radius-button);
    background: transparent;
    color: var(--termru-fg);
    font-weight: 500;
  }

  @media (hover: hover) and (pointer: fine) {
    .action:hover:not([aria-disabled='true']) {
      background: var(--termru-subtle);
      border-color: transparent;
    }
  }

  .action[aria-expanded='true'] {
    background: var(--termru-subtle);
  }

  .action[aria-disabled='true'] {
    color: var(--termru-faint);
    cursor: default;
  }

  /* Narrow windows: icons only (the buttons keep their names). */
  @media (max-width: 960px) {
    .folder {
      display: none;
    }
  }

  @media (max-width: 900px) {
    .label {
      display: none;
    }

    .action {
      padding: 0 10px;
    }

    .toolbar {
      padding-left: var(--termru-space-4);
    }
  }
</style>
