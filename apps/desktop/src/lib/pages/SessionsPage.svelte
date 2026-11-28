<script lang="ts">
  import CloseConfirm from '../components/CloseConfirm.svelte';
  import PasteConfirm from '../components/PasteConfirm.svelte';
  import PopMenu from '../components/PopMenu.svelte';
  import SessionIcon from '../components/SessionIcon.svelte';
  import SessionToolbar from '../components/SessionToolbar.svelte';
  import SplitView from '../components/SplitView.svelte';
  import TabBar from '../components/TabBar.svelte';
  import { acceleratorFor, acceleratorLabel, compactChord } from '../keybindings';
  import { prefs } from '../stores/prefs.svelte';
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
   * The Sessions page: one 46 px row with the tabs and, at the right, the
   * tools of the session on screen; under it the panes, 12 px cards with a
   * 6 px gap. Reconnect, Close session and the rest of the session's actions
   * are in the pane footer and the More menu. Every tab's panes stay mounted
   * (the inactive ones hidden) so a switch of tab is instant; the terminals
   * themselves live in the registry.
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

  const bindings = $derived(prefs.prefs.keybindings);
  const paletteChord = $derived(acceleratorFor('palette.open', bindings));
</script>

<div class="page" data-sx-page>
  <div class="topbar">
    <nav class="strip-nav" aria-label="Open sessions">
      <ul class="strip">
        {#each sessions.tabs as tab (tab.id)}
          <TabBar {tab} active={tab.id === sessions.activeId} />
        {/each}
      </ul>
      <button
        type="button"
        class="new sx-press"
        aria-label="New session"
        title="New session"
        onclick={startNewSession}
      >
        <SessionIcon name="plus" />
      </button>
    </nav>

    {#if active}
      <SessionToolbar tab={active} />
    {/if}
  </div>

  <div class="content">
    {#if sessions.tabs.length === 0}
      <div class="empty">
        <h1>No open sessions</h1>
        <p>
          Start one from your hosts, or press
          <kbd title={acceleratorLabel(paletteChord)}>{compactChord(paletteChord)}</kbd>
          to search and run.
        </p>
        <div class="empty-actions">
          <button type="button" class="primary sx-press" onclick={startNewSession}>
            <SessionIcon name="plus" />
            New session
          </button>
          <button type="button" class="ghost sx-press" onclick={() => route.go('hosts')}>
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

  /* One row: the tabs on the left (they scroll when they do not fit), the
     tools of the open session on the right. */
  .topbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--termru-space-12);
    flex: none;
    height: var(--termru-tabstrip-h);
    padding: 0 var(--termru-space-12) 0 var(--termru-space-16);
  }

  .strip-nav {
    display: flex;
    align-items: center;
    gap: 2px;
    min-width: 0;
  }

  .strip {
    display: flex;
    align-items: center;
    gap: 2px;
    min-width: 0;
    margin: 0;
    padding: 0;
    list-style: none;
    overflow-x: auto;
    overflow-y: hidden;
    scrollbar-width: none;
  }

  .strip::-webkit-scrollbar {
    display: none;
  }

  .new {
    flex: none;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: var(--termru-tool-size);
    height: var(--termru-tool-size);
    min-height: 0;
    padding: 0;
    border-radius: var(--termru-radius-input);
    background: transparent;
    color: var(--termru-text-2);
  }

  @media (hover: hover) and (pointer: fine) {
    button.new:hover:not(:disabled) {
      background: var(--termru-raised);
      color: var(--termru-text);
    }
  }

  .content {
    flex: 1;
    min-height: 0;
    position: relative;
  }

  /* The panes sit on the page colour, 10 px from the window's edge. */
  .stage {
    position: absolute;
    inset: 0;
    padding: 0 var(--termru-space-10) var(--termru-space-10);
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
    gap: var(--termru-space-8);
    padding: var(--termru-space-24);
    padding-bottom: var(--termru-space-56);
    text-align: center;
    color: var(--termru-text-2);
  }

  .empty h1 {
    margin: 0;
    font-size: var(--termru-text-title-size);
    line-height: var(--termru-text-title-lh);
    font-weight: var(--termru-text-title-weight);
    letter-spacing: -0.01em;
    color: var(--termru-text);
  }

  .empty p {
    margin: 0;
    font-size: var(--termru-text-body-size);
    line-height: var(--termru-text-body-lh);
  }

  .empty-actions {
    display: flex;
    align-items: center;
    gap: var(--termru-space-8);
    margin-top: var(--termru-space-12);
  }

  .empty-actions button {
    display: inline-flex;
    align-items: center;
    gap: var(--termru-space-8);
  }
</style>
