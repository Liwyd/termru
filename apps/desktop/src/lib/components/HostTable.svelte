<script lang="ts">
  import { tick } from 'svelte';
  import { hosts } from '../stores/hosts.svelte';
  import { authLabel } from '../auth-label';
  import { shortenPath } from '../folders';
  import { hostMenuItems, type HostActionId } from '../host-actions';
  import { formatLastSession } from '../last-session';
  import Icon from './Icon.svelte';
  import PopMenu from './PopMenu.svelte';
  import type { Host } from '../types';

  /**
   * The hosts list: borderless 52 px rows on fixed columns (star, name and
   * address, folder, sign-in, last session, actions). The row you have
   * selected is raised; the arrows move the selection and Enter opens it, the
   * inspector beside the list shows it. The rows are windowed: 10 000 hosts
   * are a few dozen elements.
   */
  let {
    rows,
    live,
    recentAt,
    selectedId = $bindable(null),
    onselect,
    onconnect,
    onopen,
    onedit,
    ondelete,
    ontogglefavorite,
  }: {
    rows: Host[];
    /** The hosts with a session running right now, and how many panes each has. */
    live: Map<string, number>;
    /** Last connection time per host id (ms since the epoch). */
    recentAt: Record<string, number>;
    /** The selected host; null selects the first row. Bindable, so the table also works on its own. */
    selectedId?: string | null;
    /** The selection changed; `via` is how (a key never animates the inspector). */
    onselect: (host: Host, via: 'pointer' | 'keyboard') => void;
    onconnect: (host: Host) => void;
    onopen: (host: Host) => void;
    onedit: (host: Host) => void;
    ondelete: (host: Host) => void;
    ontogglefavorite: (host: Host) => void;
  } = $props();

  /** Fixed row height: the windowing is a calculation, not a measurement. */
  const ROW_HEIGHT = 52;
  const OVERSCAN = 6;
  /** Used before the first layout (and where the height is unknown). */
  const FALLBACK_HEIGHT = 600;

  let body: HTMLDivElement | null = $state(null);
  let scrollTop = $state(0);
  let viewportHeight = $state(FALLBACK_HEIGHT);
  /** Width of the body's scrollbar, so the header band lines up with the rows. */
  let scrollbarWidth = $state(0);

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
   * The row drawn as the selected one: the host that was chosen, else the
   * first row. It is a fixed row, not the focus, so the raised row does not
   * jump while the list scrolls.
   */
  const currentId = $derived(
    selectedId !== null && rows.some((host) => host.id === selectedId)
      ? selectedId
      : (rows[0]?.id ?? null),
  );

  /**
   * The row that owns the table's single tab stop (roving tabindex): the
   * selected one, or the first one on screen when it is scrolled out of the
   * window. It is always a rendered row, so the table never loses its stop.
   */
  const rovingId = $derived(
    visibleRows.slice.some((host) => host.id === currentId)
      ? currentId
      : (visibleRows.slice[0]?.id ?? null),
  );

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

  /** Selects a host (the inspector shows it) and says how. */
  function pick(host: Host, via: 'pointer' | 'keyboard'): void {
    selectedId = host.id;
    onselect(host, via);
  }

  function activate(host: Host): void {
    if (live.has(host.id)) onopen(host);
    else onconnect(host);
  }

  function choose(id: string): void {
    const host = menuHost;
    if (!host) return;
    switch (id as HostActionId) {
      case 'open':
        onopen(host);
        break;
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
    pick(host, 'pointer');
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

  function lastSession(host: Host): { count: number; text: string } {
    const panes = live.get(host.id);
    if (panes !== undefined) return { count: Math.max(1, panes), text: 'Live' };
    return { count: 0, text: formatLastSession(recentAt[host.id] ?? null) };
  }

  /** Moves the selection to the row at `index` (scrolling it into the window) and focuses it. */
  async function selectRow(index: number): Promise<void> {
    const target = rows[Math.max(0, Math.min(rows.length - 1, index))];
    if (!target || !body) return;
    const top = rows.indexOf(target) * ROW_HEIGHT;
    if (top < body.scrollTop) body.scrollTop = top;
    else if (top + ROW_HEIGHT > body.scrollTop + viewportHeight) {
      body.scrollTop = top + ROW_HEIGHT - viewportHeight;
    }
    scrollTop = body.scrollTop;
    pick(target, 'keyboard');
    await tick();
    Array.from(body.querySelectorAll<HTMLElement>('[data-host-id]'))
      .find((element) => element.dataset.hostId === target.id)
      ?.focus();
  }

  /** Keyboard on a focused row: arrows select, Enter opens, E / F2 edits. */
  function onRowKeyDown(event: KeyboardEvent, host: Host): void {
    if (event.target !== event.currentTarget) return;
    const index = rows.indexOf(host);
    const page = Math.max(1, Math.floor(viewportHeight / ROW_HEIGHT) - 1);
    switch (event.key) {
      case 'Enter':
        event.preventDefault();
        activate(host);
        break;
      case 'ArrowDown':
        event.preventDefault();
        void selectRow(index + 1);
        break;
      case 'ArrowUp':
        event.preventDefault();
        void selectRow(index - 1);
        break;
      case 'Home':
        event.preventDefault();
        void selectRow(0);
        break;
      case 'End':
        event.preventDefault();
        void selectRow(rows.length - 1);
        break;
      case 'PageDown':
        event.preventDefault();
        void selectRow(index + page);
        break;
      case 'PageUp':
        event.preventDefault();
        void selectRow(index - page);
        break;
      case 'F2':
      case 'e':
      case 'E':
        if (event.ctrlKey || event.metaKey || event.altKey) return;
        event.preventDefault();
        pick(host, 'keyboard');
        onedit(host);
        break;
    }
  }
</script>

<div class="table" role="table" aria-label="Hosts" aria-rowcount={rows.length + 1}>
  <div class="head" role="row" aria-rowindex={1} style:--sbw="{scrollbarWidth}px">
    <span role="columnheader"><span class="sr-only">Favorite</span></span>
    <span role="columnheader">Host</span>
    <span class="col-folder" role="columnheader">Folder</span>
    <span class="col-auth" role="columnheader">Sign-in</span>
    <span role="columnheader">Last session</span>
    <span role="columnheader"><span class="sr-only">Actions</span></span>
  </div>
  <div class="body" bind:this={body} onscroll={onScroll}>
    <!-- The sizer is as tall as every row; the rows on screen sit in a window that
         is moved with a transform, so a scroll does not lay the rows out again. -->
    <div class="sizer" style:height="{totalHeight}px">
      <div class="window" style:transform="translateY({offsetTop}px)">
        {#each visibleRows.slice as host, i (host.id)}
          {@const session = lastSession(host)}
          {@const selected = currentId === host.id}
          <div
            class="row"
            class:selected
            role="row"
            tabindex={rovingId === host.id ? 0 : -1}
            data-host-id={host.id}
            aria-rowindex={visibleRows.start + i + 2}
            aria-current={selected ? 'true' : undefined}
            onclick={() => pick(host, 'pointer')}
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
                <Icon name="star" size={15} filled={host.favorite} />
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
            <span class="auth-cell col-auth" role="cell" title={cellAuth(host)}>
              {#if host.auth.type === 'private_key'}<Icon name="key" size={14} />{/if}
              <span class="auth-text"
                >{host.auth.type === 'private_key'
                  ? cellAuth(host).replace(/^Identity · /, '')
                  : cellAuth(host)}</span
              >
            </span>
            <span class="session-cell" role="cell">
              {#if session.count > 0}
                <span class="live"
                  ><span class="dot" aria-hidden="true"></span>{session.text}{session.count > 1
                    ? ` · ${session.count}`
                    : ''}</span
                >
              {:else}
                <span class="date">{session.text}</span>
              {/if}
            </span>
            <span class="actions" role="cell">
              <button
                type="button"
                class="go"
                tabindex={rovingId === host.id ? 0 : -1}
                onclick={(event) => {
                  event.stopPropagation();
                  pick(host, 'pointer');
                  activate(host);
                }}
                >{live.has(host.id) ? 'Open' : 'Connect'}{#if selected}<kbd aria-hidden="true"
                    >↵</kbd
                  >{/if}</button
              >
              <button
                type="button"
                class="more"
                tabindex={rovingId === host.id ? 0 : -1}
                aria-label="More actions for {host.name}"
                aria-haspopup="menu"
                aria-expanded={menuOpen && menuHost?.id === host.id}
                onclick={(event) => openMenuFromButton(host, event)}
              >
                <Icon name="more" size={16} />
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
  items={menuHost ? hostMenuItems(menuHost, live.has(menuHost.id)) : []}
  onchoose={choose}
  onclose={() => (menuOpen = false)}
/>

<style>
  .table {
    display: flex;
    flex-direction: column;
    /* The list is as tall as its rows and shrinks to what the page leaves it
       (the running-sessions strip sits right under the rows). */
    flex: 0 1 auto;
    min-height: 0;
    min-width: 0;
    container: list / inline-size;
  }

  .head,
  .row {
    display: grid;
    /* star | host | folder | sign-in | last session | actions. The actions
       column is a fixed width, so the columns of every row line up. */
    grid-template-columns:
      24px minmax(11rem, 1.6fr) minmax(0, 1fr) minmax(0, 1.1fr)
      104px 122px;
    align-items: center;
    column-gap: var(--termru-space-12);
    padding: 0 var(--termru-space-12);
  }

  .head {
    flex: none;
    height: 30px;
    padding-right: calc(var(--termru-space-12) + var(--sbw, 0px));
    font-size: var(--termru-text-meta-size);
    line-height: var(--termru-text-meta-lh);
    font-weight: var(--termru-text-meta-weight);
    color: var(--termru-text-3);
  }

  .head > span {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .sr-only {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip: rect(0 0 0 0);
    white-space: nowrap;
  }

  .body {
    flex: 0 1 auto;
    overflow-y: auto;
    min-height: 0;
  }

  .row {
    height: var(--termru-row-h);
    border-radius: var(--termru-radius-row);
    cursor: default;
    /* A row is laid out and painted on its own: one row coming into the window
       does not touch the others. */
    contain: layout style;
  }

  .row.selected {
    background: var(--termru-raised);
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
      background: var(--termru-raised);
    }
  }

  /* The actions are there for the row you point at, the one you selected, and
     the one the keyboard is in. */
  .actions {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 2px;
    opacity: 0;
  }

  .row.selected .actions,
  .row:focus-within .actions {
    opacity: 1;
  }

  @media (hover: hover) and (pointer: fine) {
    .row:hover .actions {
      opacity: 1;
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
  }

  .star {
    display: inline-grid;
    place-items: center;
    min-height: 0;
    width: 24px;
    height: 24px;
    padding: 0;
    border-radius: var(--termru-radius-sm);
    background: none;
    color: var(--termru-text-3);
  }

  .star.favorite {
    color: var(--termru-accent-text);
  }

  @media (hover: hover) and (pointer: fine) {
    .star:hover {
      background: none;
      color: var(--termru-accent-text);
    }
  }

  .host-cell {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }

  .name {
    font-size: 13.5px;
    line-height: 20px;
    font-weight: 500;
    letter-spacing: -0.005em;
    color: var(--termru-text);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .target {
    font-family: var(--termru-font-mono);
    font-size: var(--termru-text-mono-size);
    line-height: 16px;
    letter-spacing: -0.01em;
    color: var(--termru-text-3);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .folder-cell,
  .auth-cell {
    min-width: 0;
    color: var(--termru-text-2);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .auth-cell {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .auth-cell :global(svg) {
    flex: none;
  }

  .auth-text {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .session-cell {
    display: flex;
    align-items: center;
    min-width: 0;
  }

  .date {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font: 500 12px var(--termru-font-mono);
    color: var(--termru-text-2);
  }

  .live {
    display: inline-flex;
    align-items: center;
    gap: 7px;
    font-weight: 500;
    color: var(--termru-live-text);
    white-space: nowrap;
  }

  .dot {
    flex: none;
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--termru-live);
    box-shadow: 0 0 0 3px color-mix(in srgb, var(--termru-live) 16%, transparent);
  }

  .go,
  .more {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 7px;
    height: 28px;
    min-height: 0;
    border-radius: var(--termru-radius-button);
    background: transparent;
    color: var(--termru-text-2);
  }

  .go {
    padding: 0 10px;
    color: var(--termru-text);
  }

  .row.selected .go,
  .go:focus-visible {
    background: var(--termru-raised-2);
  }

  .row.selected .go kbd {
    background: var(--termru-raised);
  }

  .more {
    width: 28px;
    padding: 0;
  }

  @media (hover: hover) and (pointer: fine) {
    .go:hover,
    .more:hover {
      background: var(--termru-raised-2);
      color: var(--termru-text);
    }
  }

  /* Room is by the width of the list, not of the window: the sidebar and the
     inspector take their share. The folder goes first, then the sign-in. */
  @container list (max-width: 760px) {
    .head,
    .row {
      grid-template-columns: 24px minmax(9rem, 1.6fr) minmax(0, 1.1fr) 96px 122px;
    }

    .col-folder {
      display: none;
    }
  }

  @container list (max-width: 560px) {
    .head,
    .row {
      grid-template-columns: 24px minmax(8rem, 1fr) 90px 122px;
    }

    .col-auth {
      display: none;
    }
  }
</style>
