<script lang="ts">
  import { formatCount } from '../count-line';
  import { acceleratorFor, acceleratorLabel, compactChord } from '../keybindings';
  import { chrome } from '../stores/chrome.svelte';
  import { hosts, type HostFilterTab } from '../stores/hosts.svelte';
  import { intents } from '../stores/intents.svelte';
  import { network } from '../stores/network.svelte';
  import { prefs } from '../stores/prefs.svelte';
  import { route } from '../stores/route.svelte';
  import { sessions, type Tab } from '../stores/sessions.svelte';
  import { sync } from '../stores/sync.svelte';
  import Icon, { type IconName } from './Icon.svelte';

  /**
   * The left sidebar: the way around the app. The brand and the palette, the
   * running sessions (one item per tab, click to look at it), the library
   * (All hosts, Favorites, Recent), the folders, and at the bottom the
   * identities, the sync and the preferences. It collapses to an icon rail.
   * Every item is a button of one `nav`; the page you are on carries
   * `aria-current`.
   */
  let { onpalette }: { onpalette: () => void } = $props();

  const rail = $derived(chrome.railed);

  const bindings = $derived(prefs.prefs.keybindings);
  const paletteChord = $derived(acceleratorFor('palette.open', bindings));
  const paletteKeys = $derived(compactChord(paletteChord));
  const sidebarChord = $derived(acceleratorLabel(acceleratorFor('view.toggleSidebar', bindings)));

  type LiveState = 'live' | 'pending' | 'dead';

  /** The state of a tab: connected when any pane is, working on it, or over. */
  function tabState(tab: Tab): LiveState {
    const states = tab.panes.map((pane) => pane.status.state);
    if (states.includes('ready')) return 'live';
    if (states.some((s) => s === 'connecting' || s === 'authenticating' || s === 'reconnecting')) {
      return 'pending';
    }
    return 'dead';
  }

  const STATE_WORD: Record<LiveState, string> = {
    live: 'connected',
    pending: 'connecting',
    dead: 'not connected',
  };

  function paneWord(count: number): string {
    return `${count} ${count === 1 ? 'pane' : 'panes'}`;
  }

  function focusTab(tab: Tab): void {
    sessions.setActive(tab.id);
    route.go('sessions');
  }

  /** Shows the hosts page with a Library entry or a folder applied. */
  function showHosts(apply: () => void): void {
    apply();
    route.go('hosts');
  }

  const onHosts = $derived(route.route === 'hosts');
  const libraryCurrent = $derived(
    onHosts && hosts.folderFilter === '' ? hosts.filterTab : (null as HostFilterTab | null),
  );

  const library: { id: HostFilterTab; label: string; icon: IconName }[] = [
    { id: 'all', label: 'All hosts', icon: 'server' },
    { id: 'favorites', label: 'Favorites', icon: 'star' },
    { id: 'recent', label: 'Recent', icon: 'clock' },
  ];

  const counts = $derived(hosts.libraryCounts);
  const folders = $derived(hosts.folderTree);

  /** The indent of a folder (px): small steps, the name needs the room. */
  function indent(depth: number): number {
    return Math.min(depth, 4) * 12;
  }

  const syncTone = $derived.by((): 'live' | 'warn' | 'err' | null => {
    if (!sync.signedIn) return null;
    if (sync.lastError || sync.conflictCount > 0) return 'err';
    return network.online ? 'live' : 'warn';
  });
  const syncWord = $derived(
    syncTone === 'live'
      ? 'signed in'
      : syncTone === 'warn'
        ? 'offline'
        : syncTone === 'err'
          ? 'needs attention'
          : 'not signed in',
  );

  const liveCount = $derived(sessions.tabs.length);
</script>

<aside class="side" class:rail aria-label="Sidebar">
  <div class="brand">
    <span class="mark" aria-hidden="true">&gt;_</span>
    <span class="wordmark">termru</span>
    {#if !rail}
      <button
        type="button"
        class="tool sx-press"
        aria-label="Collapse the sidebar"
        title="Collapse the sidebar ({sidebarChord})"
        onclick={() => chrome.toggleSidebar()}
      >
        <Icon name="panel-left" size={15} />
      </button>
    {/if}
  </div>

  <button
    type="button"
    class="palette-pill"
    aria-label="Search or run a command"
    aria-keyshortcuts={paletteChord}
    title="Search or run a command ({acceleratorLabel(paletteChord)})"
    onclick={onpalette}
  >
    <Icon name="search" size={15} />
    <span class="txt grow">Search or run…</span>
    <kbd class="txt">{paletteKeys}</kbd>
  </button>

  <nav class="scroll" aria-label="Primary">
    <div class="group" role="group" aria-label="Live sessions">
      <div class="label txt">
        <button
          type="button"
          class="label-link"
          aria-label="Live sessions"
          aria-current={route.route === 'sessions' && sessions.tabs.length === 0
            ? 'page'
            : undefined}
          onclick={() => route.go('sessions')}>Live</button
        >
        {#if liveCount > 0}<span class="n">{liveCount}</span>{/if}
      </div>
      {#if rail}
        <button
          type="button"
          class="item"
          class:on={route.route === 'sessions'}
          aria-label="Live sessions, {liveCount}"
          aria-current={route.route === 'sessions' ? 'page' : undefined}
          title="Live sessions"
          onclick={() => route.go('sessions')}
        >
          <Icon name="terminal" size={15} />
          {#if liveCount > 0}<span class="badge" aria-hidden="true"></span>{/if}
        </button>
      {:else if sessions.tabs.length === 0}
        <p class="none">No live sessions</p>
      {:else}
        <ul>
          {#each sessions.tabs as tab (tab.id)}
            {@const state = tabState(tab)}
            <li>
              <button
                type="button"
                class="item"
                class:on={route.route === 'sessions' && sessions.activeId === tab.id}
                aria-current={route.route === 'sessions' && sessions.activeId === tab.id
                  ? 'page'
                  : undefined}
                aria-label="{tab.title}, {paneWord(tab.panes.length)}, {STATE_WORD[state]}"
                title={tab.title}
                onclick={() => focusTab(tab)}
              >
                <span class="dot {state}" aria-hidden="true"></span>
                <span class="txt grow ellipsis">{tab.title}</span>
                <span class="n txt" aria-hidden="true">{tab.panes.length}</span>
              </button>
            </li>
          {/each}
        </ul>
      {/if}
    </div>

    <div class="group" role="group" aria-label="Library">
      <div class="label txt">Library</div>
      <ul>
        {#each library as entry (entry.id)}
          <li>
            <button
              type="button"
              class="item"
              class:on={libraryCurrent === entry.id ||
                (entry.id === 'all' && route.route === 'host-edit')}
              aria-current={libraryCurrent === entry.id ? 'page' : undefined}
              aria-label={rail ? `${entry.label}, ${formatCount(counts[entry.id])}` : undefined}
              title={rail ? entry.label : undefined}
              onclick={() => showHosts(() => hosts.showLibrary(entry.id))}
            >
              <Icon name={entry.icon} size={15} />
              <span class="txt grow ellipsis">{entry.label}</span>
              <span class="n txt">{formatCount(counts[entry.id])}</span>
            </button>
          </li>
        {/each}
      </ul>
    </div>

    {#if rail}
      <div class="group" role="group" aria-label="Folders">
        <button
          type="button"
          class="item"
          class:on={route.route === 'folders'}
          aria-label="Folders"
          aria-current={route.route === 'folders' ? 'page' : undefined}
          title="Folders"
          onclick={() => route.go('folders')}
        >
          <Icon name="folder" size={15} />
        </button>
      </div>
    {:else}
      <div class="group" role="group" aria-label="Folders">
        <div class="label">
          <button
            type="button"
            class="label-link"
            aria-label="Manage folders"
            aria-current={route.route === 'folders' ? 'page' : undefined}
            title="Manage folders"
            onclick={() => route.go('folders')}>Folders</button
          >
          <button
            type="button"
            class="mini sx-press"
            aria-label="New folder"
            title="New folder"
            onclick={() => {
              intents.request({ kind: 'new-folder' });
              route.go('folders');
            }}
          >
            <Icon name="plus" size={13} />
          </button>
        </div>
        <ul>
          {#each folders.nodes as node (node.group.id)}
            <li>
              <button
                type="button"
                class="item folder"
                class:on={onHosts && hosts.folderFilter === node.group.id}
                aria-current={onHosts && hosts.folderFilter === node.group.id ? 'page' : undefined}
                aria-label="{node.path}, {formatCount(node.count)}"
                title={node.path}
                style:padding-left="{8 + indent(node.depth)}px"
                onclick={() => showHosts(() => hosts.showFolder(node.group.id))}
              >
                {#if node.depth === 0}
                  <Icon name="folder" size={15} />
                {:else}
                  <span class="leaf" aria-hidden="true"></span>
                {/if}
                <span class="txt grow ellipsis">{node.group.name}</span>
                <span class="n txt" aria-hidden="true">{formatCount(node.count)}</span>
              </button>
            </li>
          {/each}
          {#if folders.unfiled > 0 && folders.nodes.length > 0}
            <li>
              <button
                type="button"
                class="item folder unfiled"
                class:on={libraryCurrent === 'unfiled'}
                aria-current={libraryCurrent === 'unfiled' ? 'page' : undefined}
                aria-label="Unfiled, {formatCount(folders.unfiled)}"
                onclick={() => showHosts(() => hosts.showLibrary('unfiled'))}
              >
                <span class="leaf" aria-hidden="true"></span>
                <span class="txt grow ellipsis">Unfiled</span>
                <span class="n txt" aria-hidden="true">{formatCount(folders.unfiled)}</span>
              </button>
            </li>
          {/if}
        </ul>
      </div>
    {/if}
  </nav>

  <div class="bottom" role="group" aria-label="Workspace">
    <button
      type="button"
      class="item"
      class:on={route.route === 'identities'}
      aria-current={route.route === 'identities' ? 'page' : undefined}
      aria-label={rail ? `Identities, ${hosts.identities.length}` : undefined}
      title={rail ? 'Identities' : undefined}
      onclick={() => route.go('identities')}
    >
      <Icon name="key" size={15} />
      <span class="txt grow">Identities</span>
      <span class="n txt">{formatCount(hosts.identities.length)}</span>
    </button>
    <button
      type="button"
      class="item"
      class:on={route.route === 'sync'}
      aria-current={route.route === 'sync' ? 'page' : undefined}
      aria-label={rail ? `Sync, ${syncWord}` : undefined}
      title={rail ? 'Sync' : undefined}
      onclick={() => route.go('sync')}
    >
      <Icon name="refresh" size={15} />
      <span class="txt grow">Sync</span>
      {#if syncTone}
        <span class="dot sync {syncTone}" aria-hidden="true"></span>
        <span class="sr-only">{syncWord}</span>
      {/if}
    </button>
    <button
      type="button"
      class="item"
      class:on={route.route === 'preferences'}
      aria-current={route.route === 'preferences' ? 'page' : undefined}
      aria-label={rail ? 'Preferences' : undefined}
      title={rail ? 'Preferences' : undefined}
      onclick={() => route.go('preferences')}
    >
      <Icon name="gear" size={15} />
      <span class="txt grow">Preferences</span>
    </button>
    {#if rail}
      <button
        type="button"
        class="item"
        aria-label="Expand the sidebar"
        title="Expand the sidebar ({sidebarChord})"
        disabled={chrome.narrow}
        onclick={() => chrome.toggleSidebar()}
      >
        <Icon name="panel-left" size={15} />
      </button>
    {/if}
  </div>
</aside>

<style>
  .side {
    display: flex;
    flex-direction: column;
    gap: var(--termru-space-16);
    min-width: 0;
    min-height: 0;
    padding: 14px var(--termru-space-10) var(--termru-space-10);
    background: var(--termru-side);
    color: var(--termru-text-2);
  }

  .brand {
    display: flex;
    align-items: center;
    gap: 9px;
    min-height: 28px;
    padding: 2px 2px 0 var(--termru-space-8);
    color: var(--termru-text);
    font-size: 14px;
    font-weight: 600;
    letter-spacing: -0.01em;
  }

  .mark {
    display: grid;
    place-items: center;
    flex: none;
    width: 20px;
    height: 20px;
    border-radius: 6px;
    background: var(--termru-accent);
    color: var(--termru-accent-ink);
    font: 700 11px var(--termru-font-mono);
  }

  .wordmark {
    flex: 1;
    min-width: 0;
  }

  .tool,
  .mini {
    display: inline-grid;
    place-items: center;
    min-height: 0;
    padding: 0;
    border-radius: var(--termru-radius-sm);
    background: transparent;
    color: var(--termru-text-3);
  }

  .tool {
    width: 28px;
    height: 28px;
  }

  .mini {
    width: 22px;
    height: 22px;
  }

  @media (hover: hover) and (pointer: fine) {
    .tool:hover:not(:disabled),
    .mini:hover:not(:disabled) {
      background: var(--termru-raised);
      color: var(--termru-text);
    }
  }

  .palette-pill {
    display: flex;
    align-items: center;
    gap: var(--termru-space-8);
    height: 32px;
    min-height: 0;
    padding: 0 var(--termru-space-10);
    border-radius: var(--termru-radius-button);
    background: var(--termru-raised);
    color: var(--termru-text-3);
    font-weight: 400;
    text-align: left;
  }

  @media (hover: hover) and (pointer: fine) {
    .palette-pill:hover {
      color: var(--termru-text-2);
    }
  }

  .grow {
    flex: 1;
    min-width: 0;
  }

  .ellipsis {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  /* The groups scroll when the folders are many; the bottom stays put. */
  .scroll {
    display: flex;
    flex-direction: column;
    gap: 18px;
    flex: 1;
    min-height: 0;
    margin: 0 calc(var(--termru-space-10) * -1);
    padding: 0 var(--termru-space-10);
    overflow-y: auto;
  }

  .group {
    display: flex;
    flex-direction: column;
    gap: 1px;
  }

  ul {
    display: flex;
    flex-direction: column;
    gap: 1px;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .label {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--termru-space-8);
    padding: 0 var(--termru-space-8) var(--termru-space-6);
    font-size: var(--termru-text-meta-size);
    line-height: var(--termru-text-meta-lh);
    font-weight: var(--termru-text-meta-weight);
    color: var(--termru-text-3);
  }

  .label-link {
    min-height: 0;
    padding: 0;
    border-radius: 4px;
    background: none;
    color: inherit;
    font: inherit;
    text-align: left;
  }

  @media (hover: hover) and (pointer: fine) {
    .label-link:hover {
      background: none;
      color: var(--termru-text);
    }
  }

  .label .n {
    margin-left: auto;
  }

  .none {
    margin: 0;
    padding: 0 var(--termru-space-8);
    font-size: var(--termru-text-secondary-size);
    line-height: 30px;
    color: var(--termru-text-3);
  }

  .item {
    display: flex;
    align-items: center;
    gap: 9px;
    width: 100%;
    height: 30px;
    min-height: 0;
    padding: 0 var(--termru-space-8);
    border-radius: var(--termru-radius-sm);
    background: transparent;
    color: var(--termru-text-2);
    font-weight: 400;
    text-align: left;
    /* Only the press moves. The page changes at once: a route is often
       switched from the keyboard, and that is never animated. */
    transition: transform var(--motion-fast) var(--ease-out);
  }

  /* The hover fade belongs to the items that are not on, so becoming current
     (a click, or a shortcut) does not fade. */
  @media (hover: hover) and (pointer: fine) {
    .item:not(.on) {
      transition:
        transform var(--motion-fast) var(--ease-out),
        background-color var(--motion-fast) ease,
        color var(--motion-fast) ease;
    }

    .item:not(.on):hover:not(:disabled) {
      background: color-mix(in srgb, var(--termru-raised) 60%, transparent);
      color: var(--termru-text);
    }
  }

  .item.on {
    background: var(--termru-raised);
    color: var(--termru-text);
  }

  .item :global(svg) {
    flex: none;
  }

  .n {
    margin-left: auto;
    flex: none;
    font: 500 11px var(--termru-font-mono);
    color: var(--termru-text-3);
  }

  .leaf {
    flex: none;
    width: 15px;
    height: 15px;
    position: relative;
  }

  .leaf::after {
    content: '';
    position: absolute;
    left: 6px;
    top: 6px;
    width: 3px;
    height: 3px;
    border-radius: 50%;
    background: var(--termru-text-3);
  }

  .unfiled .txt {
    color: var(--termru-text-2);
  }

  .dot {
    flex: none;
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--termru-live);
    box-shadow: 0 0 0 3px color-mix(in srgb, var(--termru-live) 16%, transparent);
  }

  .dot.pending {
    background: var(--termru-warn);
    box-shadow: 0 0 0 3px color-mix(in srgb, var(--termru-warn) 16%, transparent);
  }

  .dot.dead {
    background: var(--termru-text-3);
    box-shadow: none;
  }

  /* The sync state: a dot without the halo, in the colour of the state. */
  .dot.sync {
    margin-left: auto;
    box-shadow: none;
  }

  .dot.sync.warn {
    background: var(--termru-warn);
  }

  .dot.sync.err {
    background: var(--termru-err);
  }

  .bottom {
    display: flex;
    flex-direction: column;
    gap: 1px;
  }

  .sr-only {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip: rect(0 0 0 0);
    white-space: nowrap;
  }

  /* The icon rail. */
  .rail {
    align-items: stretch;
    padding-left: var(--termru-space-8);
    padding-right: var(--termru-space-8);
  }

  .rail .brand {
    justify-content: center;
    padding: 2px 0 0;
  }

  .rail .txt,
  .rail .wordmark,
  .rail .label {
    display: none;
  }

  .rail .palette-pill,
  .rail .item {
    justify-content: center;
    padding: 0;
  }

  .rail .scroll {
    gap: var(--termru-space-12);
    margin: 0;
    padding: 0;
  }

  .rail .item {
    position: relative;
  }

  .rail .badge {
    position: absolute;
    top: 6px;
    right: 9px;
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--termru-live);
  }

  .rail .dot.sync {
    position: absolute;
    top: 6px;
    right: 9px;
    margin: 0;
  }
</style>
