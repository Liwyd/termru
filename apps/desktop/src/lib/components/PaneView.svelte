<script lang="ts">
  import { untrack } from 'svelte';
  import { sessions } from '../stores/sessions.svelte';
  import { connectFlow } from '../stores/connect.svelte';
  import { hosts } from '../stores/hosts.svelte';
  import { route } from '../stores/route.svelte';
  import { terminalUi } from '../stores/terminal-ui.svelte';
  import { focusIsFree } from '../terminal/focus';
  import { inputLine, shellLabel, sizeLabel, statusTone } from '../terminal/session-view';
  import SessionIcon from './SessionIcon.svelte';
  import Terminal from './Terminal.svelte';
  import TerminalSearch from './TerminalSearch.svelte';
  import type { Pane } from '../stores/sessions.svelte';

  /**
   * One pane of a tab (frames 2 and 3).
   *
   * Header: `host / shell / active` in mono, accent while the pane has the
   * focus. Alone in its tab the header carries `ACTIVE PANE` and the grid;
   * in a split it carries Expand and Close, and the grid moves to the
   * footer (`INPUT READY · UTF-8 · LF` on the left). The focused pane has a
   * 2px accent rule on top, the others a 2px rule of the border colour.
   */
  let {
    pane,
    focused,
    single = false,
    zoomed = false,
  }: {
    pane: Pane;
    focused: boolean;
    /** The only pane of its tab: no split controls, no footer. */
    single?: boolean;
    /** This pane is expanded over its siblings. */
    zoomed?: boolean;
  } = $props();

  /** What the user typed to find a host for an empty pane. */
  let query = $state('');
  let filter = $state<HTMLInputElement | null>(null);

  /** Every host that matches the search (not only the first few). */
  const candidates = $derived.by(() => {
    const needle = query.trim().toLowerCase();
    const all = hosts.sortedAll;
    if (!needle) return all;
    return all.filter((host) =>
      [host.name, host.address, host.username].some((field) =>
        field.toLowerCase().includes(needle),
      ),
    );
  });

  function pick(host: (typeof candidates)[number]): void {
    void connectFlow.pick(pane, host);
  }

  const path = $derived(
    pane.host
      ? `${pane.title} / ${shellLabel(pane)}${focused && !single ? ' / active' : ''}`
      : 'empty pane',
  );
  const size = $derived(sizeLabel(pane.cols ?? 0, pane.rows ?? 0));
  const footer = $derived(inputLine(pane));
  const tone = $derived(statusTone(pane.status));

  // A new empty pane waits for a host: the search takes the keyboard.
  $effect(() => {
    if (!focused || !filter) return;
    untrack(() => {
      if (focusIsFree()) filter?.focus();
    });
  });
</script>

<section
  class="pane"
  class:focused
  class:single
  aria-label={pane.title}
  data-pane-id={pane.id}
  onpointerdowncapture={() => sessions.focusPane(pane)}
  onfocusin={() => {
    if (!focused) sessions.focusPane(pane);
  }}
>
  <header class="head">
    <span class="path" title={pane.host ? `${pane.host.address}` : 'empty pane'}>{path}</span>

    {#if single && pane.host}
      <span class="flag">Active pane</span>
      {#if size}<span class="size">{size}</span>{/if}
    {:else}
      {#if pane.host}
        <button
          type="button"
          class="tool sx-press"
          aria-label={zoomed ? 'Restore the panes' : 'Expand the pane'}
          title={zoomed ? 'Restore the panes' : 'Expand the pane'}
          onclick={() => sessions.toggleZoom(pane)}
        >
          <SessionIcon name={zoomed ? 'collapse' : 'expand'} />
        </button>
      {/if}
      <button
        type="button"
        class="tool sx-press"
        aria-label="Close the pane"
        title="Close the pane"
        onclick={() => sessions.requestClosePane(pane)}
      >
        <SessionIcon name="close" />
      </button>
    {/if}
  </header>

  <div class="body">
    {#if pane.host}
      {#key pane.id}
        <Terminal {pane} {focused} />
      {/key}
      {#if terminalUi.isSearchOpen(pane.id)}
        <TerminalSearch paneId={pane.id} />
      {/if}
    {:else}
      <div class="empty">
        <p>No session in this pane.</p>
        {#if hosts.hosts.length > 0}
          <input
            type="search"
            class="filter"
            placeholder="Search hosts"
            aria-label="Search hosts for this pane"
            bind:this={filter}
            bind:value={query}
            onkeydown={(event) => {
              if (event.key === 'Enter' && candidates[0]) {
                event.preventDefault();
                pick(candidates[0]);
              }
            }}
          />
          {#if candidates.length > 0}
            <ul>
              {#each candidates as host (host.id)}
                <li>
                  <button type="button" class="sx-press" onclick={() => pick(host)}>
                    {host.name}
                    <span class="address">{host.username}@{host.address}</span>
                  </button>
                </li>
              {/each}
            </ul>
          {:else}
            <p class="hint">No host matches “{query.trim()}”.</p>
          {/if}
        {:else}
          <p class="hint">No saved hosts yet.</p>
          <button type="button" class="primary new sx-press" onclick={() => route.edit(null)}>
            New host
          </button>
        {/if}
      </div>
    {/if}
  </div>

  {#if !single}
    <footer class="foot {tone}">
      <span class="state">{footer}</span>
      {#if size}<span class="size">{size}</span>{/if}
    </footer>
  {/if}
</section>

<style>
  .pane {
    --pane-pad-x: 24px;
    display: flex;
    flex-direction: column;
    height: 100%;
    min-width: 0;
    min-height: 0;
    background: var(--termru-terminal);
    border-top: var(--termru-pane-rule-w) solid var(--termru-border);
  }

  .pane.single {
    --pane-pad-x: 28px;
  }

  .pane.focused {
    border-top-color: var(--termru-accent);
  }

  .head {
    display: flex;
    align-items: center;
    gap: var(--termru-space-12);
    height: 36px;
    flex: none;
    padding: 0 calc(var(--pane-pad-x) - 8px) 0 var(--pane-pad-x);
    font-family: var(--termru-font-mono);
    font-size: var(--termru-text-meta-size);
    line-height: var(--termru-text-meta-lh);
    color: var(--termru-text-2);
  }

  .pane.single .head {
    padding-right: var(--pane-pad-x);
  }

  .path {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .pane.focused:not(.single) .path,
  .flag,
  .pane.single .size {
    color: var(--termru-accent-text);
  }

  .flag {
    flex: none;
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }

  .size {
    flex: none;
    font-variant-numeric: tabular-nums;
  }

  .tool {
    flex: none;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    min-height: 0;
    width: 28px;
    height: 28px;
    padding: 0;
    border: none;
    border-radius: var(--termru-radius-button);
    background: none;
    color: var(--termru-text-2);
  }

  @media (hover: hover) and (pointer: fine) {
    .tool:hover {
      background: color-mix(in srgb, var(--termru-text) 7%, transparent);
      color: var(--termru-text);
      border-color: transparent;
    }
  }

  .body {
    flex: 1;
    min-height: 0;
    position: relative;
  }

  .foot {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--termru-space-16);
    flex: none;
    height: 30px;
    padding: 0 var(--pane-pad-x);
    background: var(--termru-side);
    font-family: var(--termru-font-mono);
    font-size: var(--termru-text-meta-size);
    line-height: var(--termru-text-meta-lh);
    letter-spacing: 0.02em;
    color: var(--termru-text-2);
  }

  .foot.busy .state {
    color: var(--termru-accent-text);
  }

  .foot.failed .state {
    color: var(--termru-err-text);
  }

  .foot.closed .state {
    color: var(--termru-warn-text);
  }

  .foot .state {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .empty {
    height: 100%;
    min-height: 0;
    padding: var(--termru-space-16) 0;
    overflow: hidden;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: var(--termru-space-12);
    color: var(--termru-text-2);
    font-size: var(--termru-text-control-size);
  }

  .empty p {
    margin: 0;
  }

  .empty ul {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: var(--termru-space-4);
    width: min(24rem, 90%);
    min-height: 0;
    overflow-y: auto;
  }

  .empty .filter {
    width: min(24rem, 90%);
    padding: 0 var(--termru-space-12);
    border: var(--termru-border-w) solid var(--termru-border);
    background: var(--termru-base);
    color: var(--termru-text);
  }

  .empty .new {
    width: auto;
    text-align: center;
  }

  .empty li button {
    width: 100%;
    min-height: 36px;
    text-align: left;
    background: var(--termru-base);
  }

  .address {
    color: var(--termru-text-2);
    font-family: var(--termru-font-mono);
    font-size: var(--termru-text-meta-size);
    margin-left: var(--termru-space-8);
  }

  .hint {
    font-size: var(--termru-text-secondary-size);
  }
</style>
