<script lang="ts">
  import { appear } from '../actions/appear';
  import { countLine, formatCount, joinNames } from '../count-line';
  import { formatLastSession } from '../last-session';
  import { acceleratorFor, acceleratorLabel } from '../keybindings';
  import ContextConfirm from '../components/ContextConfirm.svelte';
  import HostTable from '../components/HostTable.svelte';
  import Icon from '../components/Icon.svelte';
  import PageHeader from '../components/PageHeader.svelte';
  import { hosts } from '../stores/hosts.svelte';
  import { prefs } from '../stores/prefs.svelte';
  import { route } from '../stores/route.svelte';
  import { sshImport } from '../stores/ssh-import.svelte';
  import { sessions } from '../stores/sessions.svelte';
  import { sync } from '../stores/sync.svelte';
  import { notify } from '../notify.svelte';
  import { afterFirstPaint } from '../settled.svelte';
  import type { Host, SessionSecrets } from '../types';

  let {
    onconnect,
    onopen,
  }: {
    onconnect: (host: Host, secrets?: SessionSecrets, remember?: boolean) => void;
    onopen: (host: Host) => void;
  } = $props();

  let confirmOpen = $state(false);
  let pendingDelete = $state<Host | null>(null);
  let online = $state(true);
  let searchInput = $state<HTMLInputElement | null>(null);
  /** The banner that comes while the page is open moves in; one that is there at the start does not. */
  const page = afterFirstPaint();

  // "Search hosts" from another page asks for the focus before this page
  // exists: the request waits in the store and is served once the field is
  // there (or at once, when the page is already open).
  $effect(() => {
    if (hosts.searchFocus && searchInput) {
      searchInput.focus();
      searchInput.select();
      hosts.searchFocus = false;
    }
  });

  // The offline note reacts to the events themselves: `navigator.onLine`
  // is not reliable (and not writable) in every environment.
  $effect(() => {
    online = navigator.onLine;
    const apply = (event: Event) => {
      online = event.type !== 'offline';
    };
    window.addEventListener('online', apply);
    window.addEventListener('offline', apply);
    return () => {
      window.removeEventListener('online', apply);
      window.removeEventListener('offline', apply);
    };
  });

  // The "Last session" column shows live data: refresh it when sessions
  // come and go, and when one becomes ready (that is when the backend
  // records the connection).
  const readyCount = $derived(
    sessions.tabs.flatMap((tab) => tab.panes).filter((pane) => pane.status.state === 'ready')
      .length,
  );
  $effect(() => {
    void sessions.tabs.length;
    void readyCount;
    void hosts.refreshRecents();
  });

  /** Panes that hold a session right now (connected, connecting, retrying). */
  const livePanes = $derived(
    sessions.tabs
      .flatMap((tab) => tab.panes)
      .filter(
        (pane) =>
          pane.host !== null && pane.status.state !== 'closed' && pane.status.state !== 'failed',
      ),
  );
  const liveHostIds = $derived(
    new Set(livePanes.map((pane) => pane.host?.id).filter((id): id is string => id !== undefined)),
  );
  const runningNames = $derived([
    ...new Set(
      livePanes.map((pane) => pane.host?.name).filter((name): name is string => name !== undefined),
    ),
  ]);

  const headerCount = $derived(countLine(hosts.hosts.length, sessions.tabs.length));
  const sortLine = $derived(
    `${formatCount(hosts.rows.length)} ${hosts.rows.length === 1 ? 'host' : 'hosts'} · Recent activity`,
  );
  const bannerSentence = $derived(
    `${joinNames(runningNames)} ${runningNames.length === 1 ? 'stays' : 'stay'} connected while you organize your workspace.`,
  );
  const lastSync = $derived.by(() => {
    const at = sync.status?.last_sync_at;
    if (!at) return 'never';
    const ms = Date.parse(at);
    return Number.isNaN(ms) ? 'never' : formatLastSession(ms);
  });
  const searchHint = $derived(
    acceleratorLabel(acceleratorFor('host.search', prefs.prefs.keybindings)),
  );

  function edit(host: Host | null): void {
    route.edit(host);
  }

  function remove(host: Host): void {
    pendingDelete = host;
    confirmOpen = true;
  }

  async function confirmedDelete(): Promise<void> {
    const host = pendingDelete;
    pendingDelete = null;
    if (!host) return;
    // A failed delete is reported by the store; only success is announced.
    if (await hosts.remove(host.id)) {
      notify({ key: 'host.deleted', entityId: host.id, values: { name: host.name } });
    }
  }

  function returnToFirstSession(): void {
    const pane = livePanes[0];
    if (pane?.host) onopen(pane.host);
  }
</script>

<div class="page">
  <PageHeader
    crumbs={[{ label: 'Workspace' }, { label: 'Connection library' }]}
    title="Hosts"
    description={hosts.error ? 'The saved connections could not be read.' : headerCount}
  >
    {#snippet actions()}
      <button type="button" class="import" onclick={() => void sshImport.show()}>
        Import from SSH config
      </button>
      <button type="button" class="primary new" onclick={() => route.edit(null)}>
        <Icon name="plus" size={17} />
        New host
      </button>
    {/snippet}
  </PageHeader>

  {#if !online}
    <p class="offline">Offline · Changes are safe locally. Last sync {lastSync}.</p>
  {/if}

  {#if !hosts.error}
    <div class="filters">
      <div class="search">
        <Icon name="search" size={16} />
        <input
          type="search"
          placeholder="Search name, address or username"
          aria-label="Search hosts"
          bind:value={hosts.query}
          bind:this={searchInput}
        />
        {#if searchHint}<kbd>{searchHint}</kbd>{/if}
      </div>
      <div class="folder">
        <Icon name="folder" size={16} />
        <select aria-label="Folder" bind:value={hosts.folderFilter}>
          <option value="">All folders</option>
          {#each hosts.folderOptions as option (option.id)}
            <option value={option.id}>{option.label}</option>
          {/each}
        </select>
        <Icon name="chevron-down" size={12} />
      </div>
      <p class="sortline">
        {sortLine}
        <Icon name="arrow-down" size={14} />
      </p>
    </div>

    <div class="tabs" role="group" aria-label="Filter hosts">
      <button
        type="button"
        class:active={hosts.filterTab === 'all'}
        aria-pressed={hosts.filterTab === 'all'}
        onclick={() => (hosts.filterTab = 'all')}
      >
        All hosts <span class="n">{formatCount(hosts.counts.all)}</span>
      </button>
      <button
        type="button"
        class:active={hosts.filterTab === 'favorites'}
        aria-pressed={hosts.filterTab === 'favorites'}
        onclick={() => (hosts.filterTab = 'favorites')}
      >
        <svg
          class="fav"
          viewBox="0 0 20 20"
          width="14"
          height="14"
          aria-hidden="true"
          focusable="false"
        >
          <path
            d="M10 2.2l2.36 4.97 5.44.7-4 3.76 1.02 5.4L10 14.38 5.18 17.03l1.02-5.4-4-3.76 5.44-.7z"
            fill="none"
            stroke="currentColor"
            stroke-width="1.4"
            stroke-linejoin="round"
          />
        </svg>
        Favorites <span class="n">{formatCount(hosts.counts.favorites)}</span>
      </button>
      <button
        type="button"
        class:active={hosts.filterTab === 'recent'}
        aria-pressed={hosts.filterTab === 'recent'}
        onclick={() => (hosts.filterTab = 'recent')}
      >
        Recent
      </button>
      <span class="divider" aria-hidden="true"></span>
      <button
        type="button"
        class:active={hosts.filterTab === 'unfiled'}
        aria-pressed={hosts.filterTab === 'unfiled'}
        onclick={() => (hosts.filterTab = 'unfiled')}
      >
        Unfiled <span class="n">{formatCount(hosts.counts.unfiled)}</span>
      </button>
    </div>
  {/if}

  <div class="content">
    {#if hosts.loading}
      <div class="state" aria-busy="true">
        <div class="placeholders" aria-hidden="true">
          <div class="placeholder"></div>
          <div class="placeholder"></div>
          <div class="placeholder"></div>
        </div>
        <p class="state-text">Loading hosts…</p>
      </div>
    {:else if hosts.error}
      <div class="state" role="alert">
        <p class="state-text error">{hosts.error}</p>
        <button type="button" onclick={() => void hosts.load()}>Retry</button>
      </div>
    {:else if hosts.hosts.length === 0}
      <div class="state">
        <p class="state-text">No hosts yet. Add your first host.</p>
        <button type="button" class="primary" onclick={() => route.edit(null)}>
          <Icon name="plus" size={17} />
          New host
        </button>
      </div>
    {:else if hosts.rows.length === 0}
      <div class="state">
        <p class="state-text">No hosts match your filters.</p>
        <button type="button" onclick={() => hosts.clearFilters()}>Clear filters</button>
      </div>
    {:else}
      <HostTable
        rows={hosts.rows}
        live={liveHostIds}
        recentAt={hosts.recentAt}
        {onconnect}
        {onopen}
        onedit={edit}
        ondelete={remove}
        ontogglefavorite={(target) => hosts.toggleFavorite(target)}
      />
    {/if}

    <!-- Frame 1: the banner sits under the rows, not over the filters. -->
    {#if runningNames.length > 0}
      <section class="banner" aria-label="Running sessions" use:appear={{ enabled: page.value }}>
        <span class="prompt" aria-hidden="true">&gt;_</span>
        <div class="banner-text">
          <h2>Your sessions are still running</h2>
          <p>{bannerSentence}</p>
        </div>
        <button
          type="button"
          class="banner-return"
          title="Return to {runningNames[0]}"
          onclick={returnToFirstSession}
        >
          <Icon name="arrow-up-right" size={15} />
          <span class="return-label">Return to {runningNames[0]}</span>
        </button>
      </section>
    {/if}

    {#if !hosts.loading && !hosts.error && hosts.rows.length > 0}
      <footer class="hint">
        Double-click a row or press Enter to connect. Delete is in the row’s More menu.
      </footer>
    {/if}
  </div>
</div>

<ContextConfirm
  bind:open={confirmOpen}
  title={pendingDelete ? `Delete host “${pendingDelete.name}”?` : 'Delete host?'}
  body="The host profile is removed from this device. Running sessions are not closed."
  onconfirm={() => void confirmedDelete()}
/>

<style>
  .page {
    display: flex;
    flex-direction: column;
    min-height: 0;
    height: 100%;
    overflow: auto;
    padding: var(--termru-page-inset-top) var(--termru-page-inset-x);
    gap: var(--termru-space-5);
  }

  .new,
  .import {
    display: inline-flex;
    align-items: center;
    gap: var(--termru-space-2);
    padding: 0 var(--termru-space-4);
  }

  .offline {
    margin: calc(var(--termru-space-2) * -1) 0 0;
    font-size: var(--termru-text-secondary-size);
    line-height: var(--termru-text-secondary-lh);
    color: var(--termru-warning);
  }

  .filters {
    display: flex;
    align-items: center;
    gap: var(--termru-space-4);
    min-width: 0;
  }

  .search {
    display: flex;
    align-items: center;
    gap: var(--termru-space-2);
    flex: 0 1 440px;
    min-width: 12rem;
    height: var(--termru-input-h);
    padding: 0 var(--termru-space-4);
    border: 1px solid var(--termru-border);
    border-radius: var(--termru-radius-button);
    background: var(--wp-field, var(--termru-surface));
    color: var(--termru-muted);
  }

  .search:focus-within {
    outline: var(--termru-focus-w) solid var(--termru-accent);
    outline-offset: var(--termru-focus-offset);
  }

  .search input {
    flex: 1;
    min-width: 0;
    min-height: 0;
    padding: 0;
    border: none;
    background: none;
    color: var(--termru-fg);
    font: inherit;
    font-size: var(--termru-text-control-size);
    outline: none;
  }

  .search input::placeholder {
    color: var(--termru-muted);
  }

  .search input::-webkit-search-cancel-button {
    cursor: pointer;
  }

  .search kbd {
    flex: none;
    font-family: var(--termru-font-mono);
    font-size: var(--termru-text-meta-size);
    color: var(--termru-muted);
  }

  /* The native select stays (list, keyboard, screen reader); the icon and the
     caret are drawn around it. */
  .folder {
    position: relative;
    display: flex;
    align-items: center;
    flex: none;
    height: var(--termru-control-h);
    border: 1px solid var(--termru-border);
    border-radius: var(--termru-radius-button);
    background: var(--wp-field, var(--termru-surface));
    color: var(--termru-muted);
  }

  .folder:focus-within {
    outline: var(--termru-focus-w) solid var(--termru-accent);
    outline-offset: var(--termru-focus-offset);
  }

  .folder :global(svg) {
    position: absolute;
    pointer-events: none;
  }

  .folder :global(svg:first-child) {
    left: var(--termru-space-3);
  }

  .folder :global(svg:last-child) {
    right: var(--termru-space-3);
  }

  .folder select {
    min-height: 0;
    height: 100%;
    width: 11rem;
    padding: 0 calc(var(--termru-space-3) + 20px) 0 calc(var(--termru-space-3) + 24px);
    border: none;
    border-radius: inherit;
    background: none;
    color: var(--termru-fg);
    font-size: var(--termru-text-control-size);
    font-weight: var(--termru-text-control-weight);
    appearance: none;
    outline: none;
    cursor: pointer;
    text-overflow: ellipsis;
  }

  .sortline {
    display: inline-flex;
    align-items: center;
    gap: var(--termru-space-1);
    margin: 0 0 0 auto;
    font-size: var(--termru-text-secondary-size);
    line-height: var(--termru-text-secondary-lh);
    color: var(--termru-muted);
    white-space: nowrap;
  }

  .tabs {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: var(--termru-space-5);
  }

  .tabs button {
    min-height: 0;
    padding: 0;
    border: none;
    border-radius: 2px;
    background: none;
    color: var(--termru-muted);
    font-size: var(--termru-text-body-size);
    line-height: var(--termru-text-body-lh);
    font-weight: 400;
    cursor: pointer;
    /* Only the press moves; the active tab changes at once (it is a key press
       as often as a click). The hover fade is below, for the others. */
    transition: transform var(--motion-fast) var(--ease-out);
  }

  .tabs button.active {
    color: var(--termru-accent);
    font-weight: var(--termru-text-control-weight);
  }

  @media (hover: hover) and (pointer: fine) {
    .tabs button:not(.active) {
      transition:
        transform var(--motion-fast) var(--ease-out),
        color var(--motion-fast) ease;
    }

    .tabs button:hover:not(.active) {
      color: var(--termru-fg);
    }
  }

  .tabs .fav {
    margin-right: var(--termru-space-1);
    vertical-align: -1px;
  }

  .tabs .n {
    margin-left: var(--termru-space-1);
    font-weight: 400;
  }

  .tabs .divider {
    width: 1px;
    height: 16px;
    background: var(--termru-border);
  }

  .content {
    display: flex;
    flex-direction: column;
    gap: var(--termru-space-5);
    flex: 1;
    min-height: 10rem;
    min-width: 0;
  }

  .state {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: var(--termru-space-3);
    flex: 1;
    padding: var(--termru-space-7) var(--termru-space-4);
    border-top: 1px solid var(--termru-border);
    text-align: center;
  }

  .state-text {
    margin: 0;
    color: var(--termru-muted);
    font-size: var(--termru-text-body-size);
    line-height: var(--termru-text-body-lh);
    overflow-wrap: anywhere;
  }

  .state-text.error {
    color: var(--termru-danger);
  }

  .state button {
    display: inline-flex;
    align-items: center;
    gap: var(--termru-space-2);
  }

  .placeholders {
    display: flex;
    flex-direction: column;
    gap: 0;
    width: 100%;
    max-width: 48rem;
  }

  .placeholder {
    height: 70px;
    border-bottom: 1px solid var(--termru-border);
    background: var(--termru-subtle);
  }

  .banner {
    display: flex;
    flex: none;
    align-items: center;
    gap: var(--termru-space-5);
    min-height: 73px;
    padding: var(--termru-space-4) 22px;
    border: 1px solid var(--termru-border);
    border-radius: var(--termru-radius-pane);
    background: var(--termru-subtle);
  }

  .prompt {
    flex: none;
    width: 2.2ch;
    font-family: var(--termru-font-mono);
    font-size: var(--termru-text-body-size);
    color: var(--termru-muted);
  }

  .banner-text {
    flex: 1;
    min-width: 0;
  }

  .banner-text h2 {
    margin: 0;
    font-size: var(--termru-text-control-size);
    line-height: var(--termru-text-control-lh);
    font-weight: var(--termru-text-control-weight);
    color: var(--termru-fg);
  }

  .banner-text p {
    margin: 0;
    font-size: var(--termru-text-control-size);
    line-height: var(--termru-text-control-lh);
    color: var(--termru-muted);
    overflow-wrap: anywhere;
  }

  .banner-return {
    display: inline-flex;
    flex: none;
    align-items: center;
    gap: var(--termru-space-2);
    max-width: 22rem;
    padding: 0 var(--termru-space-4);
  }

  .return-label {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .hint {
    flex: none;
    margin: 0;
    font-size: var(--termru-text-secondary-size);
    line-height: var(--termru-text-secondary-lh);
    color: var(--termru-muted);
  }

  @media (max-width: 900px) {
    .filters {
      flex-wrap: wrap;
    }

    .sortline {
      margin-left: 0;
    }

    .banner {
      flex-wrap: wrap;
    }
  }
</style>
