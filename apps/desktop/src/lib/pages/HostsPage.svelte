<script lang="ts">
  import { countLine, joinNames } from '../count-line';
  import { formatLastSession } from '../last-session';
  import { uiPlatform } from '../platform';
  import ContextConfirm from '../components/ContextConfirm.svelte';
  import HostTable from '../components/HostTable.svelte';
  import { hosts } from '../stores/hosts.svelte';
  import { route } from '../stores/route.svelte';
  import { sessions } from '../stores/sessions.svelte';
  import { sync } from '../stores/sync.svelte';
  import { toasts } from '../stores/toasts.svelte';
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
  // come and go.
  $effect(() => {
    sessions.tabs.length;
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
    `${hosts.rows.length} ${hosts.rows.length === 1 ? 'host' : 'hosts'} · Recent activity ↓`,
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
  const searchHint = $derived(uiPlatform() === 'macos' ? '⌘ F' : 'Ctrl F');

  function edit(host: Host | null): void {
    hosts.editing = host;
    hosts.editorOpen = true;
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
    await hosts.remove(host.id);
    toasts.success(`Deleted ${host.name}`);
  }

  function returnToFirstSession(): void {
    const pane = livePanes[0];
    if (pane?.host) onopen(pane.host);
  }
</script>

<div class="page">
  <header class="header">
    <p class="breadcrumb">WORKSPACE / CONNECTION LIBRARY</p>
    <div class="title-row">
      <div>
        <h1>Hosts</h1>
        <p class="count">{headerCount}</p>
        {#if !online}
          <p class="offline">
            Offline · Changes are safe locally. Last sync {lastSync}.
          </p>
        {/if}
      </div>
      <button type="button" class="primary" onclick={() => route.edit(null)}>+ New host</button>
    </div>
  </header>

  {#if runningNames.length > 0}
    <section class="banner" aria-label="Running sessions">
      <span class="prompt" aria-hidden="true">&gt;_</span>
      <div class="banner-text">
        <h2>Your sessions are still running</h2>
        <p>{bannerSentence}</p>
      </div>
      <button type="button" class="banner-return" onclick={returnToFirstSession}>
        <span aria-hidden="true">↗</span>
        Return to {runningNames[0]}
      </button>
    </section>
  {/if}

  <div class="filters">
    <div class="search">
      <svg viewBox="0 0 16 16" aria-hidden="true" focusable="false">
        <circle cx="7" cy="7" r="4.5" />
        <path d="M10.5 10.5 L14 14" />
      </svg>
      <input
        type="search"
        placeholder="Search name, address or username"
        aria-label="Search hosts"
        bind:value={hosts.query}
      />
      <kbd>{searchHint}</kbd>
    </div>
    <select class="folder" aria-label="Folder" bind:value={hosts.folderFilter}>
      <option value="">All folders</option>
      {#each hosts.folderOptions as option (option.id)}
        <option value={option.id}>{option.label}</option>
      {/each}
    </select>
    <p class="sortline">{sortLine}</p>
  </div>

  <div class="tabs" role="group" aria-label="Filter hosts">
    <button
      type="button"
      class:active={hosts.filterTab === 'all'}
      aria-pressed={hosts.filterTab === 'all'}
      onclick={() => (hosts.filterTab = 'all')}
    >
      All hosts <span class="n">{hosts.counts.all}</span>
    </button>
    <button
      type="button"
      class:active={hosts.filterTab === 'favorites'}
      aria-pressed={hosts.filterTab === 'favorites'}
      onclick={() => (hosts.filterTab = 'favorites')}
    >
      ☆ Favorites <span class="n">{hosts.counts.favorites}</span>
    </button>
    <button
      type="button"
      class:active={hosts.filterTab === 'recent'}
      aria-pressed={hosts.filterTab === 'recent'}
      onclick={() => (hosts.filterTab = 'recent')}
    >
      Recent
    </button>
    <span class="divider" aria-hidden="true">|</span>
    <button
      type="button"
      class:active={hosts.filterTab === 'unfiled'}
      aria-pressed={hosts.filterTab === 'unfiled'}
      onclick={() => (hosts.filterTab = 'unfiled')}
    >
      Unfiled <span class="n">{hosts.counts.unfiled}</span>
    </button>
  </div>

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
        <p class="state-text">No hosts yet — add your first one.</p>
        <button type="button" class="primary" onclick={() => route.edit(null)}>+ New host</button>
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
      <footer class="hint">
        Double-click a host to connect. Delete is available in the row's More menu.
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

  .breadcrumb {
    margin: 0;
    font-size: var(--termru-text-meta-size);
    line-height: var(--termru-text-meta-lh);
    letter-spacing: 0.08em;
    color: var(--termru-muted);
  }

  .title-row {
    display: flex;
    align-items: flex-end;
    justify-content: space-between;
    gap: var(--termru-space-4);
  }

  h1 {
    margin: 0;
    font-size: var(--termru-text-title-size);
    line-height: var(--termru-text-title-lh);
    font-weight: var(--termru-text-title-weight);
    color: var(--termru-fg);
  }

  .count {
    margin: 0;
    font-size: var(--termru-text-body-size);
    line-height: var(--termru-text-body-lh);
    color: var(--termru-muted);
  }

  .offline {
    margin: 0.25rem 0 0;
    font-size: var(--termru-text-meta-size);
    line-height: var(--termru-text-meta-lh);
    color: var(--termru-warning);
  }

  .banner {
    display: flex;
    align-items: center;
    gap: var(--termru-space-4);
    padding: var(--termru-space-4);
    border: 1px solid var(--termru-border);
    border-radius: var(--termru-radius-lg);
    background: var(--termru-surface);
  }

  .prompt {
    font-family: var(--termru-font-mono);
    font-size: var(--termru-text-terminal-size);
    color: var(--termru-muted);
  }

  .banner-text {
    flex: 1;
    min-width: 0;
  }

  .banner-text h2 {
    margin: 0;
    font-size: var(--termru-text-secondary-size);
    line-height: var(--termru-text-secondary-lh);
    font-weight: 600;
    color: var(--termru-fg);
  }

  .banner-text p {
    margin: 0.15rem 0 0;
    font-size: var(--termru-text-secondary-size);
    line-height: var(--termru-text-secondary-lh);
    color: var(--termru-muted);
  }

  .banner-return {
    display: inline-flex;
    align-items: center;
    gap: 0.4rem;
    padding: 0.45rem 0.8rem;
    border: 1px solid var(--termru-border);
    border-radius: var(--termru-radius-button);
    background: var(--termru-bg);
    color: var(--termru-fg);
    font-size: var(--termru-text-control-size);
    line-height: var(--termru-text-control-lh);
    font-weight: var(--termru-text-control-weight);
    cursor: pointer;
  }

  .filters {
    display: flex;
    align-items: center;
    gap: var(--termru-space-3);
    min-width: 0;
  }

  .search {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    flex: 1;
    min-width: 12rem;
    max-width: 30rem;
    height: var(--termru-input-h);
    padding: 0 0.75rem;
    border: 1px solid var(--termru-border);
    border-radius: var(--termru-radius-input);
    background: var(--termru-bg);
  }

  .search:focus-within {
    outline: var(--termru-focus-w) solid var(--termru-accent);
    outline-offset: var(--termru-focus-offset);
  }

  .search svg {
    width: 0.9rem;
    height: 0.9rem;
    flex: none;
    fill: none;
    stroke: var(--termru-muted);
    stroke-width: 1.5;
    stroke-linecap: round;
  }

  .search input {
    flex: 1;
    min-width: 0;
    border: none;
    background: none;
    color: var(--termru-fg);
    font: inherit;
    font-size: var(--termru-text-secondary-size);
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

  select.folder {
    height: var(--termru-input-h);
    padding: 0 0.6rem;
    border: 1px solid var(--termru-border);
    border-radius: var(--termru-radius-input);
    background: var(--termru-bg);
    color: var(--termru-fg);
    font-size: var(--termru-text-secondary-size);
    cursor: pointer;
  }

  .sortline {
    margin: 0 0 0 auto;
    font-size: var(--termru-text-secondary-size);
    line-height: var(--termru-text-secondary-lh);
    color: var(--termru-muted);
    white-space: nowrap;
  }

  .tabs {
    display: flex;
    align-items: center;
    gap: var(--termru-space-4);
  }

  .tabs button {
    background: none;
    border: none;
    padding: 0.2rem 0;
    color: var(--termru-muted);
    font-size: var(--termru-text-secondary-size);
    line-height: var(--termru-text-secondary-lh);
    cursor: pointer;
  }

  .tabs button.active {
    color: var(--termru-accent);
    font-weight: 600;
    border-bottom: 2px solid var(--termru-accent);
  }

  .tabs .n {
    font-weight: 600;
  }

  .tabs .divider {
    color: var(--termru-border);
  }

  .content {
    display: flex;
    flex-direction: column;
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
  }

  .state-text.error {
    color: var(--termru-danger);
  }

  .placeholders {
    display: flex;
    flex-direction: column;
    gap: var(--termru-space-3);
    width: 100%;
    max-width: 48rem;
  }

  .placeholder {
    height: 3.4rem;
    border: 1px solid var(--termru-border);
    border-radius: var(--termru-radius);
    background: var(--termru-surface);
  }

  .hint {
    margin: var(--termru-space-3) 0 0;
    font-size: var(--termru-text-meta-size);
    line-height: var(--termru-text-meta-lh);
    color: var(--termru-muted);
  }
</style>
