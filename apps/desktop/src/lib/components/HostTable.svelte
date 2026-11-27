<script lang="ts">
  import { tick } from 'svelte';
  import { hosts } from '../stores/hosts.svelte';
  import { authLabel } from '../auth-label';
  import { shortenPath } from '../folders';
  import { formatLastSession } from '../last-session';
  import PopMenu from './PopMenu.svelte';
  import type { MenuEntry } from '../menu';
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

  /** Fixed row height (frame 1): the windowing is a calculation, not a measurement. */
  const ROW_HEIGHT = 70;
  const OVERSCAN = 6;
  /** Used before the first layout (and where the height is unknown). */
  const FALLBACK_HEIGHT = 600;

  let body: HTMLDivElement | null = $state(null);
  let scrollTop = $state(0);
  let viewportHeight = $state(FALLBACK_HEIGHT);
  /** Width of the body's scrollbar, so the header band lines up with the rows. */
  let scrollbarWidth = $state(0);
  let selectedId = $state<string | null>(null);
  /** The row that owns the table's single tab stop (roving tabindex). */
  let activeId = $state<string | null>(null);

  let menuOpen = $state(false);
  let menuX = $state(0);
  let menuY = $state(0);
  let menuHost = $state<Host | null>(null);
  /** False when the keyboard opened the menu: it appears without motion. */
  let menuAnimate = $state(true);

  $effect(() => {
    const node = body;
    if (!node) return;
    const measure = () => {
      viewportHeight = node.clientHeight || FALLBACK_HEIGHT;
      scrollbarWidth = Math.max(0, node.offsetWidth - node.clientWidth);
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
  /**
   * The row that is the tab stop: the last one focused, or the first one on
   * screen. It is always a rendered row, so the table never loses its stop
   * when the active row is scrolled out of the window.
   */
  const rovingId = $derived(
    visibleRows.slice.some((host) => host.id === activeId)
      ? activeId
      : (visibleRows.slice[0]?.id ?? null),
  );
  /**
   * The row drawn as the current one (frame 1 tints the first): the row the
   * user clicked, else the first row. It is a fixed row, not the roving
   * tab stop, so the tint does not jump while the list scrolls.
   */
  const currentId = $derived(selectedId ?? rows[0]?.id ?? null);
  /** Where the rendered rows start (px), and how tall the whole list is. */
  const offsetTop = $derived(visibleRows.start * ROW_HEIGHT);
  const totalHeight = $derived(rows.length * ROW_HEIGHT);

  // The rows on screen ask the credential store about their password once
  // each; the answer is cached on the store. A scroll that is still moving
  // asks nothing: a fast flick through 10 000 hosts would otherwise put
  // thousands of keyring lookups in the queue, for rows nobody looked at.
  /** When the list last scrolled (performance.now). */
  let lastScrollAt = -Infinity;
  const SETTLE_MS = 150;
  $effect(() => {
    const unknown = visibleRows.slice.filter(
      (host) => host.auth.type === 'password' && hosts.passwordStored[host.id] === undefined,
    );
    if (unknown.length === 0) return;
    const moving = performance.now() - lastScrollAt < SETTLE_MS;
    const timer = setTimeout(
      () => {
        for (const host of unknown) void hosts.ensurePasswordState(host);
      },
      moving ? SETTLE_MS : 0,
    );
    return () => clearTimeout(timer);
  });

  function onScroll(event: Event): void {
    lastScrollAt = performance.now();
    scrollTop = (event.currentTarget as HTMLDivElement).scrollTop;
  }

  function activate(host: Host): void {
    if (live.has(host.id)) onopen(host);
    else onconnect(host);
  }

  function menuItems(host: Host): MenuEntry[] {
    return [
      { id: 'connect', label: 'Connect' },
      { id: 'edit', label: 'Edit…' },
      {
        id: 'favorite',
        label: host.favorite ? 'Remove from favorites' : 'Add to favorites',
      },
      { id: 'delete', label: 'Delete', danger: true, separatorBefore: true },
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
    // A right-click is button 2; the menu key and Shift+F10 report button 0.
    menuAnimate = event.button === 2;
    menuOpen = true;
  }

  function openMenuFromButton(host: Host, event: MouseEvent): void {
    event.stopPropagation();
    const rect = (event.currentTarget as HTMLElement).getBoundingClientRect();
    menuHost = host;
    menuX = rect.right;
    menuY = rect.bottom + 4;
    // Enter or Space on the button is a click without a pointer (detail 0).
    menuAnimate = event.detail > 0;
    menuOpen = true;
  }

  /** The path of every folder, built once (the getter sorts every folder on each read). */
  const folderLabels = $derived(
    new Map(hosts.folderOptions.map((option) => [option.id, option.label])),
  );

  function folderLabel(host: Host): string {
    if (!host.group_id) return 'Unfiled';
    return folderLabels.get(host.group_id) ?? 'Unfiled';
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

  /** Moves the focus to the row at `index` (scrolling it into the window). */
  async function focusRow(index: number): Promise<void> {
    const target = rows[Math.max(0, Math.min(rows.length - 1, index))];
    if (!target || !body) return;
    const top = rows.indexOf(target) * ROW_HEIGHT;
    if (top < body.scrollTop) body.scrollTop = top;
    else if (top + ROW_HEIGHT > body.scrollTop + viewportHeight) {
      body.scrollTop = top + ROW_HEIGHT - viewportHeight;
    }
    scrollTop = body.scrollTop;
    activeId = target.id;
    await tick();
    Array.from(body.querySelectorAll<HTMLElement>('[data-host-id]'))
      .find((element) => element.dataset.hostId === target.id)
      ?.focus();
  }

  /** Keyboard on a focused row: arrows move, Enter connects, E / F2 edits. */
  function onRowKeyDown(event: KeyboardEvent, host: Host): void {
    if (event.target !== event.currentTarget) return;
    const index = rows.indexOf(host);
    switch (event.key) {
      case 'Enter':
        event.preventDefault();
        activate(host);
        break;
      case 'ArrowDown':
        event.preventDefault();
        void focusRow(index + 1);
        break;
      case 'ArrowUp':
        event.preventDefault();
        void focusRow(index - 1);
        break;
      case 'Home':
        event.preventDefault();
        void focusRow(0);
        break;
      case 'End':
        event.preventDefault();
        void focusRow(rows.length - 1);
        break;
      case 'PageDown':
        event.preventDefault();
        void focusRow(index + Math.max(1, Math.floor(viewportHeight / ROW_HEIGHT) - 1));
        break;
      case 'PageUp':
        event.preventDefault();
        void focusRow(index - Math.max(1, Math.floor(viewportHeight / ROW_HEIGHT) - 1));
        break;
      case 'F2':
      case 'e':
      case 'E':
        if (event.ctrlKey || event.metaKey || event.altKey) return;
        event.preventDefault();
        select(host);
        onedit(host);
        break;
    }
  }
</script>

<div class="table" role="table" aria-label="Hosts" aria-rowcount={rows.length + 1}>
  <div class="head" role="row" aria-rowindex={1} style:--sbw="{scrollbarWidth}px">
    <span class="star-cell" role="columnheader">Star</span>
    <span role="columnheader">Host / address</span>
    <span class="col-folder" role="columnheader">Folder</span>
    <span class="col-auth" role="columnheader">Authentication</span>
    <span role="columnheader">Last session</span>
    <span class="actions-cell" role="columnheader">Actions</span>
  </div>
  <div class="body" bind:this={body} onscroll={onScroll}>
    <!-- The sizer is as tall as every row; the rows on screen sit in a window that
         is moved with a transform, so a scroll does not lay the rows out again. -->
    <div class="sizer" style:height="{totalHeight}px">
      <div class="window" style:transform="translateY({offsetTop}px)">
        {#each visibleRows.slice as host, i (host.id)}
          {@const session = lastSession(host)}
          <div
            class="row"
            class:selected={currentId === host.id}
            role="row"
            tabindex={rovingId === host.id ? 0 : -1}
            data-host-id={host.id}
            aria-rowindex={visibleRows.start + i + 2}
            onclick={() => select(host)}
            onfocus={() => (activeId = host.id)}
            ondblclick={() => activate(host)}
            onkeydown={(event) => onRowKeyDown(event, host)}
            oncontextmenu={(event) => openMenuAtPointer(host, event)}
          >
            <span class="star-cell" role="cell">
              <button
                type="button"
                class="star"
                tabindex={rovingId === host.id ? 0 : -1}
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
                <svg
                  viewBox="0 0 20 20"
                  width="18"
                  height="18"
                  aria-hidden="true"
                  focusable="false"
                >
                  <path
                    d="M10 2.2l2.36 4.97 5.44.7-4 3.76 1.02 5.4L10 14.38 5.18 17.03l1.02-5.4-4-3.76 5.44-.7z"
                    fill={host.favorite ? 'currentColor' : 'none'}
                    stroke="currentColor"
                    stroke-width="1.4"
                    stroke-linejoin="round"
                  />
                </svg>
              </button>
            </span>
            <span class="host-cell" role="cell">
              <span class="name" title={host.name}>{host.name}</span>
              <span class="target" title="{host.username}@{host.address}:{host.port}"
                >{host.username}@{host.address}:{host.port}</span
              >
            </span>
            <span class="folder-cell col-folder" role="cell" title={folderLabel(host)}>
              {shortenPath(folderLabel(host))}
            </span>
            <span class="auth-cell col-auth" role="cell" title={cellAuth(host)}
              >{cellAuth(host)}</span
            >
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
                class="act primary-act"
                tabindex={rovingId === host.id ? 0 : -1}
                onclick={(event) => {
                  event.stopPropagation();
                  activate(host);
                }}>{live.has(host.id) ? 'Open' : 'Connect'}</button
              >
              <button
                type="button"
                class="act"
                tabindex={rovingId === host.id ? 0 : -1}
                onclick={(event) => {
                  event.stopPropagation();
                  select(host);
                  onedit(host);
                }}>Edit</button
              >
              <button
                type="button"
                class="act more"
                tabindex={rovingId === host.id ? 0 : -1}
                aria-label="More actions for {host.name}"
                aria-haspopup="menu"
                aria-expanded={menuOpen && menuHost?.id === host.id}
                onclick={(event) => openMenuFromButton(host, event)}
              >
                <svg
                  viewBox="0 0 16 16"
                  width="16"
                  height="16"
                  aria-hidden="true"
                  focusable="false"
                >
                  <circle cx="3" cy="8" r="1.2" />
                  <circle cx="8" cy="8" r="1.2" />
                  <circle cx="13" cy="8" r="1.2" />
                </svg>
              </button>
            </span>
          </div>
        {/each}
      </div>
    </div>
  </div>
</div>

<PopMenu
  open={menuOpen && menuHost !== null}
  x={menuX}
  y={menuY}
  align="right"
  animate={menuAnimate}
  label={menuHost ? `Actions for ${menuHost.name}` : 'Host actions'}
  items={menuHost ? menuItems(menuHost) : []}
  onchoose={choose}
  onclose={() => (menuOpen = false)}
/>

<style>
  .table {
    display: flex;
    flex-direction: column;
    /* Frame 1: the table is as tall as its rows, and shrinks to what the
       page leaves it (the banner and the hint sit under it, not over it). */
    flex: 0 1 auto;
    /* Never less than five rows: with the banner and the hint under it, a
       short window would leave a long list two. The page scrolls instead. */
    min-height: calc(34px + 5 * 70px);
    min-width: 0;
  }

  .head,
  .row {
    display: grid;
    /* Frame 1: 26 | host | folder | authentication | last session | actions.
       The actions column is a fixed width, so the columns of every row line up. */
    grid-template-columns:
      26px minmax(11rem, 1.4fr) minmax(7rem, 1fr) minmax(9rem, 1fr)
      minmax(7rem, 1.2fr) 152px;
    align-items: center;
    gap: var(--termru-space-4);
    padding: 0 var(--termru-space-4);
  }

  .head {
    flex: none;
    height: 34px;
    padding-right: calc(var(--termru-space-4) + var(--sbw, 0px));
    background: var(--termru-subtle);
    font-size: var(--termru-text-meta-size);
    line-height: var(--termru-text-meta-lh);
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--termru-muted);
  }

  .head > span {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  /* "Star" is wider than its 26 px column; it may run into the gap. */
  .head > .star-cell {
    overflow: visible;
  }

  .body {
    flex: 0 1 auto;
    overflow-y: auto;
    min-height: 0;
  }

  .row {
    height: 70px;
    border-bottom: 1px solid var(--termru-border);
    cursor: default;
    /* A row is laid out and painted on its own: one row coming into the window
       does not touch the others. */
    contain: layout style;
  }

  .row.selected {
    background: var(--termru-accent-soft);
  }

  /* The ring sits inside the row: the scrolling body would clip it outside. */
  .row:focus-visible {
    outline-offset: calc(var(--termru-focus-w) * -1);
  }

  @media (hover: hover) and (pointer: fine) {
    .row {
      transition: background-color var(--motion-fast) ease;
    }

    .row:hover {
      background: var(--termru-accent-soft);
    }
  }

  .sizer {
    position: relative;
  }

  .window {
    position: absolute;
    inset: 0 0 auto 0;
  }

  .star-cell {
    display: flex;
    align-items: center;
    justify-content: flex-start;
  }

  .star {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    min-height: 0;
    width: 26px;
    height: 26px;
    padding: 0;
    border: none;
    border-radius: var(--termru-radius);
    background: none;
    color: var(--termru-muted);
    cursor: pointer;
    transition:
      transform var(--motion-fast) var(--ease-out),
      color var(--motion-fast) ease;
  }

  .star.favorite {
    color: var(--termru-accent);
  }

  .star:active {
    transform: scale(var(--press-scale));
  }

  .host-cell {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }

  .name {
    font-size: var(--termru-text-body-size);
    line-height: var(--termru-text-body-lh);
    font-weight: var(--termru-text-control-weight);
    color: var(--termru-fg);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .target {
    font-family: var(--termru-font-mono);
    font-size: var(--termru-text-secondary-size);
    line-height: var(--termru-text-secondary-lh);
    color: var(--termru-muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .folder-cell,
  .auth-cell,
  .date {
    font-size: var(--termru-text-control-size);
    line-height: var(--termru-text-control-lh);
    color: var(--termru-muted);
  }

  .folder-cell,
  .auth-cell {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .session-cell {
    display: flex;
    align-items: center;
    min-width: 0;
  }

  .date {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .pill {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    height: 23px;
    padding: 0 8px;
    border-radius: 4px;
    background: var(--termru-accent-soft);
    color: var(--termru-accent);
    font-size: var(--termru-text-meta-size);
    line-height: var(--termru-text-meta-lh);
    font-weight: var(--termru-text-control-weight);
  }

  .pill .dot {
    width: 5px;
    height: 5px;
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

  .act {
    min-height: 32px;
    padding: 0 6px;
    border: none;
    background: none;
    color: var(--termru-fg);
    font-size: var(--termru-text-control-size);
    line-height: var(--termru-text-control-lh);
    font-weight: var(--termru-text-control-weight);
    cursor: pointer;
    transition:
      transform var(--motion-fast) var(--ease-out),
      background-color var(--motion-fast) ease,
      color var(--motion-fast) ease;
  }

  .act:active {
    transform: scale(var(--press-scale));
  }

  @media (hover: hover) and (pointer: fine) {
    .act:hover:not(:disabled) {
      background: var(--termru-subtle);
      color: var(--termru-accent);
    }

    .star:hover {
      color: var(--termru-accent);
    }
  }

  .more {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    min-width: 28px;
    padding: 0;
    color: var(--termru-muted);
  }

  .more svg {
    fill: currentColor;
  }

  /* Narrow windows: the folder and the authentication move to the editor. */
  @media (max-width: 900px) {
    .head,
    .row {
      grid-template-columns: 26px minmax(9rem, 1fr) minmax(6rem, 0.6fr) 152px;
    }

    .col-folder,
    .col-auth {
      display: none;
    }
  }
</style>
