<script lang="ts">
  import { hosts } from '../stores/hosts.svelte';
  import { authLabel } from '../auth-label';
  import { formatLastSession } from '../last-session';
  import ContextMenu, { type MenuItem } from './ContextMenu.svelte';
  import type { Host } from '../types';

  let {
    rows,
    live,
    recentAt,
    onconnect,
    onopen,
    onedit,
    ondelete,
    ontogglefavorite,
  }: {
    rows: Host[];
    /** The host ids that have a session running right now. */
    live: Set<string>;
    /** Last connection time per host id (ms since the epoch). */
    recentAt: Record<string, number>;
    onconnect: (host: Host) => void;
    onopen: (host: Host) => void;
    onedit: (host: Host) => void;
    ondelete: (host: Host) => void;
    ontogglefavorite: (host: Host) => void;
  } = $props();

  /** Fixed row height: the windowing is a calculation, not a measurement. */
  const ROW_HEIGHT = 66;
  const OVERSCAN = 6;
  /** Used before the first layout (and where the height is unknown). */
  const FALLBACK_HEIGHT = 600;

  let body: HTMLDivElement | null = $state(null);
  let scrollTop = $state(0);
  let viewportHeight = $state(FALLBACK_HEIGHT);
  let selectedId = $state<string | null>(null);

  let menuOpen = $state(false);
  let menuX = $state(0);
  let menuY = $state(0);
  let menuHost = $state<Host | null>(null);

  $effect(() => {
    const node = body;
    if (!node) return;
    const measure = () => {
      viewportHeight = node.clientHeight || FALLBACK_HEIGHT;
    };
    measure();
    if (typeof ResizeObserver === 'undefined') return;
    const observer = new ResizeObserver(measure);
    observer.observe(node);
    return () => observer.disconnect();
  });

  /** The rows inside the viewport, plus a small overscan above and below. */
  const visibleRows = $derived.by(() => {
    const start = Math.max(0, Math.floor(scrollTop / ROW_HEIGHT) - OVERSCAN);
    const end = Math.min(
      rows.length,
      Math.ceil((scrollTop + viewportHeight) / ROW_HEIGHT) + OVERSCAN,
    );
    return { start, end, slice: rows.slice(start, end) };
  });
  const padTop = $derived(visibleRows.start * ROW_HEIGHT);
  const padBottom = $derived(Math.max(0, rows.length - visibleRows.end) * ROW_HEIGHT);

  // The rows on screen ask the credential store about their password once
  // each; the answer is cached on the store.
  $effect(() => {
    for (const host of visibleRows.slice) {
      if (host.auth.type === 'password' && hosts.passwordStored[host.id] === undefined) {
        void hosts.ensurePasswordState(host);
      }
    }
  });

  function onScroll(event: Event): void {
    scrollTop = (event.currentTarget as HTMLDivElement).scrollTop;
  }

  function activate(host: Host): void {
    if (live.has(host.id)) onopen(host);
    else onconnect(host);
  }

  function menuItems(host: Host): MenuItem[] {
    return [
      { id: 'connect', label: 'Connect' },
      { id: 'edit', label: 'Edit…' },
      {
        id: 'favorite',
        label: host.favorite ? 'Remove from favorites' : 'Add to favorites',
      },
      { id: 'delete', label: 'Delete', danger: true },
    ];
  }

  function choose(id: string): void {
    const host = menuHost;
    if (!host) return;
    switch (id) {
      case 'connect':
        onconnect(host);
        break;
      case 'edit':
        onedit(host);
        break;
      case 'favorite':
        ontogglefavorite(host);
        break;
      case 'delete':
        ondelete(host);
        break;
    }
  }

  function openMenuAtPointer(host: Host, event: MouseEvent): void {
    event.preventDefault();
    event.stopPropagation();
    menuHost = host;
    menuX = event.clientX;
    menuY = event.clientY;
    menuOpen = true;
  }

  function openMenuFromButton(host: Host, event: MouseEvent): void {
    event.stopPropagation();
    const rect = (event.currentTarget as HTMLElement).getBoundingClientRect();
    menuHost = host;
    menuX = rect.left;
    menuY = rect.bottom;
    menuOpen = true;
  }

  function folderLabel(host: Host): string {
    if (!host.group_id) return 'Unfiled';
    const option = hosts.folderOptions.find((entry) => entry.id === host.group_id);
    return option?.label ?? 'Unfiled';
  }

  function cellAuth(host: Host): string {
    return authLabel(host, hosts.identities, hosts.passwordStored[host.id] === true);
  }

  function lastSession(host: Host): { live: boolean; text: string } {
    if (live.has(host.id)) return { live: true, text: 'Connected' };
    return { live: false, text: formatLastSession(recentAt[host.id] ?? null) };
  }

  function select(host: Host): void {
    selectedId = host.id;
  }
</script>

<div class="table" role="table" aria-label="Hosts" aria-rowcount={rows.length + 1}>
  <div class="head" role="row" aria-rowindex={1}>
    <span class="star-cell" role="columnheader">STAR</span>
    <span role="columnheader">HOST / ADDRESS</span>
    <span role="columnheader">FOLDER</span>
    <span role="columnheader">AUTHENTICATION</span>
    <span role="columnheader">LAST SESSION</span>
    <span class="actions-cell" role="columnheader">ACTIONS</span>
  </div>
  <div class="body" bind:this={body} onscroll={onScroll}>
    {#if padTop > 0}<div class="pad" style:height="{padTop}px"></div>{/if}
    {#each visibleRows.slice as host, i (host.id)}
      {@const session = lastSession(host)}
      <div
        class="row"
        class:selected={selectedId === host.id}
        role="row"
        tabindex="-1"
        aria-rowindex={visibleRows.start + i + 2}
        onclick={() => select(host)}
        ondblclick={() => activate(host)}
        onkeydown={(event) => {
          if (event.key !== 'Enter' || event.target !== event.currentTarget) return;
          event.preventDefault();
          activate(host);
        }}
        oncontextmenu={(event) => openMenuAtPointer(host, event)}
      >
        <span class="star-cell" role="cell">
          <button
            type="button"
            class="star"
            class:favorite={host.favorite}
            aria-label={host.favorite
              ? `Remove ${host.name} from favorites`
              : `Add ${host.name} to favorites`}
            aria-pressed={host.favorite}
            onclick={(event) => {
              event.stopPropagation();
              ontogglefavorite(host);
            }}
          >
            {host.favorite ? '★' : '☆'}
          </button>
        </span>
        <span class="host-cell" role="cell">
          <span class="name">{host.name}</span>
          <span class="target">{host.username}@{host.address}:{host.port}</span>
        </span>
        <span class="folder-cell" role="cell" class:unfiled={!host.group_id}>
          {folderLabel(host)}
        </span>
        <span class="auth-cell" role="cell">{cellAuth(host)}</span>
        <span class="session-cell" role="cell">
          {#if session.live}
            <span class="pill"><span class="dot" aria-hidden="true"></span>{session.text}</span>
          {:else}
            <span class="date">{session.text}</span>
          {/if}
        </span>
        <span class="actions actions-cell" role="cell">
          <button
            type="button"
            class="primary"
            onclick={(event) => {
              event.stopPropagation();
              activate(host);
            }}>{live.has(host.id) ? 'Open' : 'Connect'}</button
          >
          <button
            type="button"
            onclick={(event) => {
              event.stopPropagation();
              select(host);
              onedit(host);
            }}>Edit</button
          >
          <button
            type="button"
            class="more"
            aria-label="More actions for {host.name}"
            aria-haspopup="menu"
            aria-expanded={menuOpen && menuHost?.id === host.id}
            onclick={(event) => openMenuFromButton(host, event)}
          >
            <span aria-hidden="true">⋯</span>
            <span class="sr-only">More actions</span>
          </button>
        </span>
      </div>
    {/each}
    {#if padBottom > 0}<div class="pad" style:height="{padBottom}px"></div>{/if}
  </div>
</div>

{#if menuHost}
  <ContextMenu
    open={menuOpen}
    x={menuX}
    y={menuY}
    items={menuItems(menuHost)}
    onchoose={choose}
    onclose={() => (menuOpen = false)}
  />
{/if}

<style>
  .table {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-height: 0;
    min-width: 0;
  }

  .head,
  .row {
    display: grid;
    grid-template-columns:
      3.2rem minmax(11rem, 2.2fr) minmax(7rem, 1.2fr) minmax(9rem, 1.4fr)
      minmax(7rem, 1fr) auto;
    align-items: center;
    gap: var(--termru-space-3);
    padding: 0 var(--termru-space-3);
  }

  .head {
    height: 2.4rem;
    border-bottom: 1px solid var(--termru-border);
    font-size: var(--termru-text-meta-size);
    line-height: var(--termru-text-meta-lh);
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--termru-muted);
  }

  .body {
    flex: 1 1 0;
    overflow-y: auto;
    min-height: 0;
  }

  .row {
    height: 66px;
    border-bottom: 1px solid var(--termru-border);
    cursor: default;
  }

  .row:hover,
  .row.selected {
    background: var(--termru-accent-soft);
  }

  .pad {
    flex: none;
  }

  .star-cell {
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .star {
    background: none;
    border: none;
    padding: 0.2rem;
    font-size: 1rem;
    line-height: 1;
    color: var(--termru-muted);
    cursor: pointer;
  }

  .star.favorite {
    color: var(--termru-accent);
  }

  .host-cell {
    display: flex;
    flex-direction: column;
    gap: 0.15rem;
    min-width: 0;
  }

  .name {
    font-size: var(--termru-text-body-size);
    line-height: var(--termru-text-body-lh);
    font-weight: 600;
    color: var(--termru-fg);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .target {
    font-family: var(--termru-font-mono);
    font-size: var(--termru-text-terminal-size);
    line-height: var(--termru-text-terminal-lh);
    color: var(--termru-muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .folder-cell,
  .auth-cell {
    font-size: var(--termru-text-secondary-size);
    line-height: var(--termru-text-secondary-lh);
    color: var(--termru-fg);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .folder-cell.unfiled {
    color: var(--termru-muted);
  }

  .session-cell {
    display: flex;
    align-items: center;
    min-width: 0;
  }

  .date {
    font-size: var(--termru-text-secondary-size);
    line-height: var(--termru-text-secondary-lh);
    color: var(--termru-muted);
  }

  .pill {
    display: inline-flex;
    align-items: center;
    gap: 0.35rem;
    padding: 0.15rem 0.5rem;
    border-radius: var(--termru-radius);
    background: var(--termru-accent-soft);
    color: var(--termru-accent);
    font-size: var(--termru-text-secondary-size);
    line-height: var(--termru-text-secondary-lh);
    font-weight: 600;
  }

  .pill .dot {
    width: 0.45rem;
    height: 0.45rem;
    background: var(--termru-accent);
  }

  .actions {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: var(--termru-space-3);
  }

  .actions-cell {
    justify-self: end;
  }

  .actions button {
    background: none;
    border: none;
    padding: 0.25rem 0.35rem;
    border-radius: var(--termru-radius);
    color: var(--termru-fg);
    font-size: var(--termru-text-secondary-size);
    line-height: var(--termru-text-secondary-lh);
    cursor: pointer;
  }

  .actions button:hover {
    background: var(--termru-surface);
  }

  .actions .primary {
    font-weight: var(--termru-text-control-weight);
  }

  .actions .more {
    padding: 0.25rem 0.4rem;
    color: var(--termru-muted);
  }

  .sr-only {
    position: absolute;
    width: 1px;
    height: 1px;
    padding: 0;
    margin: -1px;
    overflow: hidden;
    clip: rect(0, 0, 0, 0);
    white-space: nowrap;
    border: 0;
  }
</style>
