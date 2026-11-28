<script lang="ts">
  import { appear } from '../actions/appear';
  import { formatCount, joinNames } from '../count-line';
  import { formatLastSession } from '../last-session';
  import { acceleratorFor, compactChord } from '../keybindings';
  import ContextConfirm from '../components/ContextConfirm.svelte';
  import HostInspector from '../components/HostInspector.svelte';
  import HostTable from '../components/HostTable.svelte';
  import Icon from '../components/Icon.svelte';
  import PageHeader from '../components/PageHeader.svelte';
  import { chrome } from '../stores/chrome.svelte';
  import { hosts, type HostSort } from '../stores/hosts.svelte';
  import { prefs } from '../stores/prefs.svelte';
  import { route } from '../stores/route.svelte';
  import { sshImport } from '../stores/ssh-import.svelte';
  import { sessions } from '../stores/sessions.svelte';
  import { sync } from '../stores/sync.svelte';
  import { notify } from '../notify.svelte';
  import { afterFirstPaint } from '../settled.svelte';
  import type { Host, SessionSecrets } from '../types';

  /**
   * The Hosts page: a one-line header (the view, how many, how it is ordered,
   * search, import, New host), the list, and beside it the inspector of the
   * selected host. The sidebar chooses the view (All hosts, Favorites, Recent,
   * a folder); the header chips choose the order.
   */
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
  /** How the selection last changed: a key never animates the inspector. */
  let via = $state<'pointer' | 'keyboard'>('pointer');
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
  /** The hosts with a session, and how many panes each has. */
  const liveCounts = $derived.by(() => {
    const counts = new Map<string, number>();
    for (const pane of livePanes) {
      const id = pane.host?.id;
      if (id !== undefined) counts.set(id, (counts.get(id) ?? 0) + 1);
    }
    return counts;
  });
  const runningNames = $derived([
    ...new Set(
      livePanes.map((pane) => pane.host?.name).filter((name): name is string => name !== undefined),
    ),
  ]);

  // --- the view ------------------------------------------------------------
  const viewTitle = $derived.by(() => {
    if (hosts.folderFilter) {
      return (
        hosts.folderOptions.find((option) => option.id === hosts.folderFilter)?.label ?? 'Folder'
      );
    }
    switch (hosts.filterTab) {
      case 'favorites':
        return 'Favorites';
      case 'recent':
        return 'Recent';
      case 'unfiled':
        return 'Unfiled';
      default:
        return 'All hosts';
    }
  });

  const SORTS: { id: HostSort; label: string; short: string }[] = [
    { id: 'recent', label: 'Recent activity', short: 'Recent' },
    { id: 'name', label: 'Name', short: 'Name' },
    { id: 'folder', label: 'Folder', short: 'Folder' },
  ];

  const stripTitle = $derived(
    `${runningNames.length} ${runningNames.length === 1 ? 'session' : 'sessions'} running`,
  );
  const bannerSentence = $derived(
    `${joinNames(runningNames)} ${runningNames.length === 1 ? 'stays' : 'stay'} connected while you browse.`,
  );
  const lastSync = $derived.by(() => {
    const at = sync.status?.last_sync_at;
    if (!at) return 'never';
    const ms = Date.parse(at);
    return Number.isNaN(ms) ? 'never' : formatLastSession(ms);
  });
  const searchHint = $derived(compactChord(acceleratorFor('host.search', prefs.prefs.keybindings)));

  // --- the selection -------------------------------------------------------
  /** The selected host: the one chosen, else the first row. */
  const selected = $derived(
    hosts.rows.find((host) => host.id === hosts.selectedId) ?? hosts.rows[0] ?? null,
  );
  const showInspector = $derived(
    chrome.inspectorOpen && selected !== null && !hosts.loading && hosts.error === null,
  );
  const selectedPane = $derived(
    selected ? (livePanes.find((pane) => pane.host?.id === selected.id) ?? null) : null,
  );

  function select(host: Host, how: 'pointer' | 'keyboard'): void {
    hosts.selectedId = host.id;
    via = how;
  }

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
  <PageHeader title={viewTitle} count={hosts.error ? null : formatCount(hosts.rows.length)}>
    {#snippet chips()}
      {#if !hosts.error && hosts.hosts.length > 0}
        <div class="sorts" role="group" aria-label="Order the hosts">
          {#each SORTS as sort (sort.id)}
            <button
              type="button"
              class="chip"
              class:on={hosts.sort === sort.id}
              aria-pressed={hosts.sort === sort.id}
              aria-label={sort.label}
              onclick={() => hosts.setSort(sort.id)}
            >
              <span class="long" aria-hidden="true">{sort.label}</span><span
                class="short"
                aria-hidden="true">{sort.short}</span
              >
            </button>
          {/each}
        </div>
      {/if}
    {/snippet}
    {#snippet actions()}
      {#if !hosts.error}
        <label class="search">
          <Icon name="search" size={15} />
          <input
            type="search"
            placeholder="Search hosts"
            aria-label="Search hosts"
            aria-keyshortcuts={acceleratorFor('host.search', prefs.prefs.keybindings)}
            bind:value={hosts.query}
            bind:this={searchInput}
          />
          {#if searchHint}<kbd>{searchHint}</kbd>{/if}
        </label>
      {/if}
      <button
        type="button"
        class="ghost import"
        aria-label="Import ~/.ssh/config"
        title="Import hosts from ~/.ssh/config"
        onclick={() => void sshImport.show()}
      >
        <Icon name="import" size={15} />
        <span class="import-label">Import ~/.ssh/config</span>
      </button>
      {#if !hosts.error && hosts.hosts.length > 0}
        <button
          type="button"
          class="ghost inspector-toggle"
          aria-label={chrome.inspectorOpen ? 'Hide the inspector' : 'Show the inspector'}
          aria-pressed={chrome.inspectorOpen}
          title={chrome.inspectorOpen ? 'Hide the inspector' : 'Show the inspector'}
          onclick={() => chrome.toggleInspector()}
        >
          <Icon name="panel-right" size={16} />
        </button>
      {/if}
      <button type="button" class="primary new" onclick={() => route.edit(null)}>
        <Icon name="plus" size={15} />
        New host
      </button>
    {/snippet}
  </PageHeader>

  <div class="body-wrap">
    <div class="body" class:with-inspector={showInspector}>
      <div class="list">
        {#if !online}
          <p class="offline">Offline · Changes are safe locally. Last sync {lastSync}.</p>
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
              <p class="state-title">The saved hosts could not be read</p>
              <p class="state-text error">{hosts.error}</p>
              <button type="button" onclick={() => void hosts.load()}>Retry</button>
            </div>
          {:else if hosts.hosts.length === 0}
            <div class="state">
              <p class="state-title">No hosts yet</p>
              <p class="state-text">Add your first host, or bring in the ones you already use.</p>
              <div class="state-actions">
                <button type="button" class="primary" onclick={() => route.edit(null)}>
                  <Icon name="plus" size={15} />
                  New host
                </button>
                <button type="button" onclick={() => void sshImport.show()}>
                  <Icon name="import" size={15} />
                  Import ~/.ssh/config
                </button>
              </div>
            </div>
          {:else if hosts.rows.length === 0}
            <div class="state">
              <p class="state-text">No hosts match your filters.</p>
              <button type="button" onclick={() => hosts.clearFilters()}>Clear filters</button>
            </div>
          {:else}
            <HostTable
              rows={hosts.rows}
              live={liveCounts}
              recentAt={hosts.recentAt}
              selectedId={selected?.id ?? null}
              onselect={select}
              {onconnect}
              {onopen}
              onedit={edit}
              ondelete={remove}
              ontogglefavorite={(target) => hosts.toggleFavorite(target)}
            />
          {/if}
        </div>

        {#if runningNames.length > 0}
          <section class="strip" aria-label="Running sessions" use:appear={{ enabled: page.value }}>
            <Icon name="terminal" size={16} />
            <p class="strip-text">
              <b>{stripTitle}</b>
              — {bannerSentence}
            </p>
            <button
              type="button"
              class="ghost strip-return"
              title="Return to {runningNames[0]}"
              onclick={returnToFirstSession}
            >
              <span class="return-label">Return to {runningNames[0]}</span>
              <Icon name="arrow-right" size={15} />
            </button>
          </section>
        {/if}
      </div>

      {#if showInspector && selected}
        <div class="insp-slot">
          <HostInspector
            host={selected}
            live={liveCounts.get(selected.id) ?? 0}
            pane={selectedPane}
            animate={via === 'pointer'}
            {onconnect}
            {onopen}
            onedit={edit}
            ondelete={remove}
            ontogglefavorite={(target) => hosts.toggleFavorite(target)}
          />
        </div>
      {/if}
    </div>
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
    flex: 1;
    min-width: 0;
    min-height: 0;
  }

  /* --- the header: chips, search, actions --------------------------------- */
  .sorts {
    display: flex;
    gap: 2px;
  }

  .chip {
    display: inline-flex;
    align-items: center;
    height: 28px;
    min-height: 0;
    padding: 0 10px;
    border-radius: var(--termru-radius-sm);
    background: transparent;
    color: var(--termru-text-2);
    font-weight: 400;
    white-space: nowrap;
    /* Only the press moves. The order changes at once: a chip is often
       pressed from the keyboard, and that is never animated. */
    transition: transform var(--motion-fast) var(--ease-out);
  }

  /* The hover fade belongs to the chips that are not on. */
  @media (hover: hover) and (pointer: fine) {
    .chip:not(.on) {
      transition:
        transform var(--motion-fast) var(--ease-out),
        background-color var(--motion-fast) ease,
        color var(--motion-fast) ease;
    }

    .chip:not(.on):hover {
      color: var(--termru-text);
    }
  }

  .chip.on {
    background: var(--termru-raised);
    color: var(--termru-text);
  }

  .chip .short {
    display: none;
  }

  .search {
    display: flex;
    align-items: center;
    gap: var(--termru-space-8);
    width: 220px;
    min-width: 0;
    height: 30px;
    padding: 0 var(--termru-space-10);
    border-radius: var(--termru-radius-button);
    background: var(--termru-raised);
    color: var(--termru-text-3);
  }

  .search:focus-within {
    outline: var(--termru-focus-w) solid var(--termru-accent-text);
    outline-offset: var(--termru-focus-offset);
  }

  .search input {
    flex: 1;
    min-width: 0;
    min-height: 0;
    height: 100%;
    padding: 0;
    border: none;
    background: none;
    color: var(--termru-text);
    font: inherit;
    outline: none;
  }

  .search input::placeholder {
    color: var(--termru-text-3);
  }

  .search input::-webkit-search-cancel-button {
    cursor: pointer;
  }

  .search kbd {
    flex: none;
  }

  .import,
  .new {
    display: inline-flex;
    align-items: center;
    gap: 7px;
  }

  .inspector-toggle {
    width: 30px;
    padding: 0;
    justify-content: center;
  }

  .inspector-toggle[aria-pressed='true'] {
    background: var(--termru-raised);
    color: var(--termru-text);
  }

  /* The header is one line at any width: the long labels give way first. */
  @container head (max-width: 1060px) {
    .import-label,
    .search kbd {
      display: none;
    }

    .import {
      width: 30px;
      padding: 0;
      justify-content: center;
    }

    .chip .long {
      display: none;
    }

    .chip .short {
      display: inline;
    }
  }

  @container head (max-width: 760px) {
    .search {
      width: 130px;
    }

    .inspector-toggle {
      display: none;
    }
  }

  /* --- the body: the list, the inspector ---------------------------------- */
  /* The room the list and the inspector share: the sidebar and the window take
     theirs first, so the inspector goes by the width of this box. */
  .body-wrap {
    display: flex;
    flex: 1;
    min-width: 0;
    min-height: 0;
    container: body / inline-size;
  }

  .body {
    display: grid;
    grid-template-columns: minmax(0, 1fr);
    flex: 1;
    min-width: 0;
    min-height: 0;
  }

  .body.with-inspector {
    grid-template-columns: minmax(0, 1fr) var(--termru-inspector-w);
  }

  .insp-slot {
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
  }

  .insp-slot > :global(.insp) {
    flex: 1;
  }

  /* No room for the inspector: the list keeps the width, every action of the
     inspector is in the row menu. */
  @container body (max-width: 820px) {
    .body.with-inspector {
      grid-template-columns: minmax(0, 1fr);
    }

    .insp-slot {
      display: none;
    }
  }

  .list {
    display: flex;
    flex-direction: column;
    gap: var(--termru-space-16);
    min-width: 0;
    min-height: 0;
    padding: var(--termru-space-4) var(--termru-space-16) var(--termru-space-16);
    overflow: auto;
  }

  .offline {
    margin: 0 var(--termru-space-12);
    font-size: var(--termru-text-secondary-size);
    line-height: var(--termru-text-secondary-lh);
    color: var(--termru-warn-text);
  }

  .content {
    display: flex;
    flex-direction: column;
    flex: 0 1 auto;
    min-height: 0;
    min-width: 0;
  }

  .state {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: var(--termru-space-8);
    flex: 1;
    padding: var(--termru-space-48) var(--termru-space-16);
    text-align: center;
  }

  .state-title {
    margin: 0;
    font-size: var(--termru-text-heading-size);
    line-height: var(--termru-text-heading-lh);
    font-weight: var(--termru-text-heading-weight);
    letter-spacing: -0.015em;
  }

  .state-text {
    max-width: 34rem;
    margin: 0;
    color: var(--termru-text-2);
    overflow-wrap: anywhere;
  }

  .state-text.error {
    color: var(--termru-err-text);
  }

  .state button {
    display: inline-flex;
    align-items: center;
    gap: 7px;
  }

  .state-actions {
    display: flex;
    flex-wrap: wrap;
    justify-content: center;
    gap: var(--termru-space-8);
    margin-top: var(--termru-space-8);
  }

  .placeholders {
    display: flex;
    flex-direction: column;
    width: 100%;
    max-width: 48rem;
  }

  .placeholder {
    height: var(--termru-row-h);
    margin-bottom: 2px;
    border-radius: var(--termru-radius-row);
    background: var(--termru-side);
  }

  /* The running sessions: a soft accent strip under the rows. */
  .strip {
    display: flex;
    flex: none;
    align-items: center;
    gap: 14px;
    margin: 0 var(--termru-space-12);
    padding: 14px var(--termru-space-16);
    border-radius: var(--termru-radius-pane);
    background: var(--termru-accent-soft);
    color: var(--termru-accent-text);
  }

  .strip-text {
    flex: 1;
    min-width: 0;
    margin: 0;
    color: var(--termru-text-2);
    overflow-wrap: anywhere;
  }

  .strip-text b {
    font-weight: 500;
    color: var(--termru-text);
  }

  .strip-return {
    display: inline-flex;
    flex: none;
    align-items: center;
    gap: 7px;
    max-width: 22rem;
    color: var(--termru-text);
  }

  .return-label {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  @container body (max-width: 640px) {
    .strip {
      flex-wrap: wrap;
    }
  }
</style>
