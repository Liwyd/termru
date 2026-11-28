<script lang="ts">
  import { untrack } from 'svelte';
  import { sessions } from '../stores/sessions.svelte';
  import { connectFlow } from '../stores/connect.svelte';
  import { hosts } from '../stores/hosts.svelte';
  import { route } from '../stores/route.svelte';
  import { terminalUi } from '../stores/terminal-ui.svelte';
  import { focusIsFree } from '../terminal/focus';
  import { forwardFootLine, forwardRows } from '../forwards';
  import {
    inputLine,
    reconnectState,
    shellLabel,
    sizeLabel,
    sshTarget,
    statusTone,
  } from '../terminal/session-view';
  import SessionIcon from './SessionIcon.svelte';
  import Terminal from './Terminal.svelte';
  import TerminalSearch from './TerminalSearch.svelte';
  import type { Pane } from '../stores/sessions.svelte';

  /**
   * One pane of a tab: a 12 px card on the terminal colour.
   *
   * Header (34 px, mono 11): `host · title`, the grid, and in a split Expand
   * and Close. Footer (28 px): the state of the session on the left (with
   * Reconnect when it is over), the forwards on the right. The focused pane
   * of a split has a 1 px champagne line inside its edge and an accent
   * header; a pane alone in its tab has neither (there is nothing to tell it
   * from). A change of focus does not animate: a key moves it as often as a
   * click does.
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

  const path = $derived(pane.host ? `${pane.title} · ${shellLabel(pane)}` : 'empty pane');
  /** Where the session is: the tooltip of the header (`user@host:22 · Folder`). */
  const where = $derived.by(() => {
    const host = pane.host;
    if (!host) return 'empty pane';
    const folder = host.group_id
      ? hosts.folderOptions.find((option) => option.id === host.group_id)?.label
      : null;
    return folder ? `${sshTarget(host)} · ${folder}` : sshTarget(host);
  });
  const size = $derived(sizeLabel(pane.cols ?? 0, pane.rows ?? 0));
  const footer = $derived(inputLine(pane));
  /** An empty pane is waiting for a host, not in trouble: no tone. */
  const tone = $derived(pane.host ? statusTone(pane.status) : 'empty');
  const reconnect = $derived(reconnectState(pane));
  const forwards = $derived(
    forwardFootLine(forwardRows(pane.host, pane.forwards, pane.execPending === true)),
  );

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
  class:focused={focused && !single}
  class:single
  aria-label={pane.title}
  aria-current={focused && !single ? 'true' : undefined}
  data-pane-id={pane.id}
  onpointerdowncapture={() => sessions.focusPane(pane)}
  onfocusin={() => {
    if (!focused) sessions.focusPane(pane);
  }}
>
  <header class="head">
    <span class="path" title={where}>{path}</span>
    {#if size}<span class="size">{size}</span>{/if}
    {#if !single}
      {#if pane.host}
        <button
          type="button"
          class="tool sx-press"
          aria-label={zoomed ? 'Restore the panes' : 'Expand the pane'}
          title={zoomed ? 'Restore the panes' : 'Expand the pane'}
          onclick={() => sessions.toggleZoom(pane)}
        >
          <SessionIcon name={zoomed ? 'collapse' : 'expand'} size={14} />
        </button>
      {/if}
      <button
        type="button"
        class="tool sx-press"
        aria-label="Close the pane"
        title="Close the pane"
        onclick={() => sessions.requestClosePane(pane)}
      >
        <SessionIcon name="close" size={14} />
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
        <p class="lead">No session in this pane</p>
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
                  <button type="button" class="pick sx-press" onclick={() => pick(host)}>
                    <span class="name">{host.name}</span>
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

  <footer class="foot {tone}">
    <span class="state">{footer}</span>
    {#if reconnect.can}
      <button
        type="button"
        class="again sx-press"
        title={reconnect.reason}
        onclick={() => void connectFlow.reconnect(pane)}
      >
        Reconnect
      </button>
    {/if}
    {#if forwards}<span class="fwd" title="Port forwards of this session">{forwards}</span>{/if}
  </footer>
</section>

<style>
  .pane {
    --pane-pad-x: 18px;
    position: relative;
    display: flex;
    flex-direction: column;
    height: 100%;
    min-width: 0;
    min-height: 0;
    overflow: hidden;
    border-radius: var(--termru-radius-pane);
    background: var(--termru-terminal);
  }

  /* The focused pane of a split: a 1 px champagne line inside its edge. It is
     drawn over the pane and takes no clicks. */
  .pane.focused::before {
    content: '';
    position: absolute;
    inset: 0;
    z-index: 6;
    border-radius: inherit;
    box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--termru-accent) 45%, transparent);
    pointer-events: none;
  }

  .head {
    display: flex;
    align-items: center;
    gap: var(--termru-space-8);
    height: var(--termru-pane-head-h);
    flex: none;
    padding: 0 var(--termru-space-6) 0 var(--termru-space-16);
    font-family: var(--termru-font-mono);
    font-size: var(--termru-text-meta-size);
    line-height: var(--termru-text-meta-lh);
    font-weight: var(--termru-text-meta-weight);
    color: var(--termru-text-3);
  }

  .pane.single .head {
    padding-right: var(--termru-space-16);
  }

  .path {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .pane.focused .path {
    color: var(--termru-accent-text);
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
    width: 26px;
    height: 26px;
    padding: 0;
    border: none;
    border-radius: var(--termru-radius-sm);
    background: none;
    color: var(--termru-text-3);
  }

  @media (hover: hover) and (pointer: fine) {
    button.tool:hover:not(:disabled) {
      background: color-mix(in srgb, var(--termru-text) 7%, transparent);
      color: var(--termru-text);
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
    gap: var(--termru-space-12);
    flex: none;
    height: var(--termru-pane-foot-h);
    padding: 0 var(--termru-space-16);
    font-family: var(--termru-font-mono);
    font-size: var(--termru-text-meta-size);
    line-height: var(--termru-text-meta-lh);
    font-weight: var(--termru-text-meta-weight);
    color: var(--termru-text-3);
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

  .state,
  .fwd {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  /* The state keeps its words; the forwards give way first, at the right. */
  .state {
    flex: 0 1 auto;
  }

  .fwd {
    flex: 1 1 0;
    text-align: right;
  }

  /* Reconnect, in the footer of a session that is over. */
  .again {
    flex: none;
    min-height: 0;
    height: 20px;
    padding: 0 var(--termru-space-8);
    border-radius: var(--termru-radius-sm);
    background: var(--termru-accent-soft);
    color: var(--termru-accent-text);
    font: inherit;
  }

  @media (hover: hover) and (pointer: fine) {
    button.again:hover:not(:disabled) {
      background-color: color-mix(in srgb, var(--termru-accent) 20%, transparent);
    }
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

  .empty .lead {
    color: var(--termru-text);
    font-weight: var(--termru-text-control-weight);
  }

  .empty ul {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
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
    font-family: var(--termru-font-ui);
  }

  .empty .new {
    width: auto;
    text-align: center;
  }

  /* A host to put in the pane: a borderless row, raised under the pointer. */
  .pick {
    display: flex;
    align-items: baseline;
    gap: var(--termru-space-8);
    width: 100%;
    min-height: 36px;
    padding: 0 var(--termru-space-12);
    border-radius: var(--termru-radius-row);
    background: transparent;
    text-align: left;
  }

  @media (hover: hover) and (pointer: fine) {
    button.pick:hover:not(:disabled) {
      background: var(--termru-raised);
    }
  }

  .pick .name {
    flex: none;
  }

  .address {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--termru-text-3);
    font-family: var(--termru-font-mono);
    font-size: var(--termru-text-meta-size);
  }

  .hint {
    font-size: var(--termru-text-secondary-size);
  }
</style>
