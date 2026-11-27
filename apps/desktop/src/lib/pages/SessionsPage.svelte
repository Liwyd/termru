<script lang="ts">
  import CloseConfirm from '../components/CloseConfirm.svelte';
  import PasteConfirm from '../components/PasteConfirm.svelte';
  import PopMenu from '../components/PopMenu.svelte';
  import SessionBar from '../components/SessionBar.svelte';
  import SessionIcon from '../components/SessionIcon.svelte';
  import SessionToolbar from '../components/SessionToolbar.svelte';
  import SplitBanner from '../components/SplitBanner.svelte';
  import SplitView from '../components/SplitView.svelte';
  import TabBar from '../components/TabBar.svelte';
  import { route } from '../stores/route.svelte';
  import { sessions } from '../stores/sessions.svelte';
  import { startNewSession } from '../stores/new-session';
  import { terminalUi } from '../stores/terminal-ui.svelte';
  import {
    runTerminalAction,
    terminalMenuEntries,
    type TerminalActionId,
  } from '../terminal/actions';
  import { getTerminal } from '../terminal/registry';

  /**
   * The Sessions page (frames 2 and 3): the tab strip, the toolbar of the
   * session on screen, the panes and the bar with the session's actions.
   * Every tab's panes stay mounted (the inactive ones hidden) so a switch of
   * tab is instant; the terminals themselves live in the registry.
   */
  const active = $derived(sessions.active);

  // Leaving the page leaves no menu or find bar behind.
  $effect(() => () => terminalUi.resetTransient());

  const menu = $derived(terminalUi.menu);
  const menuEntries = $derived(menu ? terminalMenuEntries(menu.paneId, menu.link) : []);

  function closeMenu(): void {
    const paneId = menu?.paneId;
    terminalUi.closeMenu();
    // The terminal gets the keyboard back (the menu came from it).
    const handle = paneId ? getTerminal(paneId) : undefined;
    if (handle && !handle.disposed) handle.term.focus();
  }

  const showHint = $derived(
    active !== null && active.panes.length > 1 && !active.zoomedPaneId && !terminalUi.hintDismissed,
  );
</script>

<div class="page" data-sx-page>
  <div class="strip" role="tablist" aria-label="Open sessions">
    {#each sessions.tabs as tab (tab.id)}
      <TabBar {tab} active={tab.id === sessions.activeId} />
    {/each}
    <button type="button" class="new sx-press" onclick={startNewSession}>
      <SessionIcon name="plus" />
      New session
    </button>
  </div>

  {#if active}
    <SessionToolbar tab={active} />
    {#if showHint}
      <SplitBanner tab={active} />
    {/if}
  {/if}

  <div class="content">
    {#if sessions.tabs.length === 0}
      <div class="empty">
        <h1>No open sessions</h1>
        <p>Start one from your saved hosts. It opens in a tab here.</p>
        <div class="empty-actions">
          <button type="button" class="primary sx-press" onclick={startNewSession}>
            <SessionIcon name="plus" />
            New session
          </button>
          <button type="button" class="sx-press" onclick={() => route.go('hosts')}>
            Open Hosts
          </button>
        </div>
      </div>
    {:else}
      {#each sessions.tabs as tab (tab.id)}
        <div class="stage" class:hidden={tab.id !== sessions.activeId}>
          <SplitView node={tab.root} {tab} />
        </div>
      {/each}
    {/if}
  </div>

  {#if active}
    <SessionBar tab={active} />
  {/if}
</div>

<PopMenu
  open={menu !== null}
  x={menu?.x ?? 0}
  y={menu?.y ?? 0}
  animate={menu?.animate ?? true}
  label="Terminal"
  items={menuEntries}
  onchoose={(id) => {
    if (menu) void runTerminalAction(id as TerminalActionId, menu.paneId, menu.link);
  }}
  onclose={closeMenu}
/>

<CloseConfirm />
<PasteConfirm />

<style>
  .page {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
  }

  /* The strip is the canvas; the active tab is the toolbar's white. */
  .strip {
    display: flex;
    align-items: stretch;
    flex: none;
    height: var(--termru-tabstrip-h);
    padding: 0 var(--termru-space-5) 0 var(--termru-space-5);
    border-bottom: var(--termru-border-w) solid var(--termru-border);
    background: var(--termru-canvas);
    overflow-x: auto;
    overflow-y: hidden;
    scrollbar-width: none;
  }

  .strip::-webkit-scrollbar {
    display: none;
  }

  .new {
    flex: none;
    align-self: center;
    display: inline-flex;
    align-items: center;
    gap: var(--termru-space-2);
    margin-left: var(--termru-space-4);
    padding: 0 var(--termru-space-3);
    border: none;
    border-radius: var(--termru-radius-button);
    background: transparent;
    color: var(--termru-fg);
    font-weight: 500;
    white-space: nowrap;
  }

  @media (hover: hover) and (pointer: fine) {
    .new:hover {
      background: color-mix(in srgb, var(--termru-surface) 70%, transparent);
      border-color: transparent;
    }
  }

  .content {
    flex: 1;
    min-height: 0;
    position: relative;
  }

  .stage {
    position: absolute;
    inset: 0;
  }

  .stage.hidden {
    display: none;
  }

  .empty {
    height: 100%;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: var(--termru-space-3);
    padding: var(--termru-space-5);
    text-align: center;
    background: var(--termru-surface);
    color: var(--termru-muted);
  }

  .empty h1 {
    margin: 0;
    font-size: var(--termru-text-title-size);
    line-height: var(--termru-text-title-lh);
    font-weight: var(--termru-text-title-weight);
    color: var(--termru-fg);
  }

  .empty p {
    margin: 0;
    font-size: var(--termru-text-body-size);
    line-height: var(--termru-text-body-lh);
  }

  .empty-actions {
    display: flex;
    gap: var(--termru-space-3);
    margin-top: var(--termru-space-3);
  }

  .empty-actions button {
    display: inline-flex;
    align-items: center;
    gap: var(--termru-space-2);
  }

  @media (max-width: 900px) {
    .strip {
      padding: 0 var(--termru-space-3);
    }
  }
</style>
