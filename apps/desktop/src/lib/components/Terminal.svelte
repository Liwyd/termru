<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import { matchBinding } from '../keybindings';
  import { connectFlow } from '../stores/connect.svelte';
  import { prefs } from '../stores/prefs.svelte';
  import { sessions } from '../stores/sessions.svelte';
  import { terminalUi } from '../stores/terminal-ui.svelte';
  import { activateLink } from '../terminal/actions';
  import { appearanceFromPrefs } from '../terminal/appearance';
  import { pushSize, sendInput, startConnection } from '../terminal/connect';
  import {
    applyAppearance,
    createTerminal,
    fitTerminal,
    isVisible,
    remeasure,
    setAccelerated,
    whenFontsReady,
  } from '../terminal/create';
  import { focusIsFree } from '../terminal/focus';
  import { pasteGate } from '../terminal/paste.svelte';
  import { attachTerminal, detachTerminal, getTerminal } from '../terminal/registry';
  import type { TerminalHandle } from '../terminal/registry';
  import { claimResizeOwner, type ResizeOwner } from '../terminal/resize';
  import type { Pane } from '../stores/sessions.svelte';

  /**
   * Hosts the terminal of a pane.
   *
   * The xterm instance lives in the terminal registry, not in this
   * component: mounting re-parents its element into the container, and
   * unmounting detaches it without disposing it, so the scrollback and the
   * session survive a route change, a split or an unsplit. The terminal is
   * disposed by the session store when the pane is really removed.
   *
   * This component is the one owner of the pane's size: a single
   * ResizeObserver (see `terminal/resize.ts`) fits the grid once per frame
   * and tells the backend after the size settled.
   */
  let { pane, focused = false }: { pane: Pane; focused?: boolean } = $props();

  /** Where the terminal's element is shown (the padded wrapper). */
  let container: HTMLDivElement | undefined = $state();
  /** The terminal of the pane, once this component mounted. */
  let handle = $state.raw<TerminalHandle | null>(null);
  let resizer: ResizeOwner | null = null;

  /** True while this pane's tab is the one on screen. */
  const tabActive = $derived(
    sessions.activeId !== null && sessions.tabOfPane(pane.id)?.id === sessions.activeId,
  );

  onMount(() => {
    const host = container as HTMLDivElement;
    const paneId = pane.id;
    const existing = getTerminal(paneId);
    const instance =
      existing ??
      createTerminal(paneId, host, appearanceFromPrefs(), {
        onData: (data) => sendInput(paneId, data),
        isAppKey: (event) => matchBinding(event, prefs.prefs.keybindings) !== null,
        onPaste: (text) => void pasteGate.request(paneId, text),
        onFind: () => terminalUi.openSearch(paneId),
        onTitle: (title) => sessions.setPaneTitle(paneId, title),
        onSize: (cols, rows) => sessions.setPaneSize(paneId, cols, rows),
        onLink: (event, uri) => void activateLink(event, uri),
      });
    attachTerminal(instance, host);
    // The theme or the font may have changed while the terminal was away.
    applyAppearance(instance, appearanceFromPrefs());
    sessions.setPaneSize(paneId, instance.term.cols, instance.term.rows);
    handle = instance;

    // The context menu: right click, the Menu key and Shift+F10 all end up
    // here. The browser's own menu has nothing for a terminal.
    const onContextMenu = (event: MouseEvent): void => {
      event.preventDefault();
      const box = host.getBoundingClientRect();
      const fromKeyboard = event.clientX === 0 && event.clientY === 0;
      terminalUi.openMenu({
        paneId,
        x: fromKeyboard ? box.left + 24 : event.clientX,
        y: fromKeyboard ? box.top + 24 : event.clientY,
        link: instance.hoverLink,
        animate: event.button === 2,
      });
    };
    host.addEventListener('contextmenu', onContextMenu);

    // The pane's one size owner. A fit that finds the terminal hidden
    // gives its GPU context back and computes no 0x0 grid.
    resizer = claimResizeOwner(paneId, {
      fit: async () => {
        if (!isVisible(instance)) {
          setAccelerated(instance, false);
          return false;
        }
        await whenFontsReady(instance);
        if (handle !== instance || !isVisible(instance)) return false;
        setAccelerated(instance, true);
        return fitTerminal(instance);
      },
      push: () => {
        const sessionId = pane.sessionId;
        if (sessionId && handle === instance) pushSize(instance, sessionId);
      },
    });
    resizer.start(host);

    // A web font that finishes loading changes the cell size: measure again
    // and fit to the new grid.
    const onFontsLoaded = (): void => {
      remeasure(instance);
      resizer?.schedule();
    };
    document.fonts?.addEventListener?.('loadingdone', onFontsLoaded);

    return () => {
      document.fonts?.removeEventListener?.('loadingdone', onFontsLoaded);
      host.removeEventListener('contextmenu', onContextMenu);
      resizer?.stop();
      resizer = null;
      // A hidden terminal gives its GPU context back; the buffer stays.
      setAccelerated(instance, false);
      detachTerminal(instance, host);
      handle = null;
    };
  });

  // Appearance changes (font, theme, scrollback, cursor) apply to the live
  // terminal; a change of the cell size needs a new fit.
  $effect(() => {
    const instance = handle;
    if (!instance) return;
    const appearance = appearanceFromPrefs();
    if (applyAppearance(instance, appearance)) resizer?.schedule();
  });

  // (Re)connect whenever the pane asks for a new connection attempt.
  // `untrack` keeps the effect bound to `generation` and the terminal only:
  // reading `pane.sessionId` while connecting must not re-trigger it. A
  // remount finds the terminal already connected for this generation.
  $effect(() => {
    const instance = handle;
    const generation = pane.generation;
    if (!instance) return;
    untrack(() => {
      void startConnection(pane, instance, generation);
    });
  });

  // A pane that becomes the active pane of the visible tab takes the
  // keyboard, unless the user is typing somewhere else (a field, a dialog,
  // the menu) or is on another page of the app.
  $effect(() => {
    const instance = handle;
    if (!instance || !focused || !tabActive) return;
    untrack(() => {
      if (focusIsFree() && isVisible(instance)) instance.term.focus();
    });
  });
</script>

<div class="terminal-wrap">
  <div class="terminal" bind:this={container}></div>

  {#if pane.status.state === 'connecting' || pane.status.state === 'authenticating'}
    <div class="overlay" role="status">
      <span class="spinner" aria-hidden="true"></span>
      <span>Connecting to {pane.host?.address ?? 'the host'}…</span>
    </div>
  {:else if pane.status.state === 'reconnecting'}
    <div class="overlay" role="status">
      <span class="spinner" aria-hidden="true"></span>
      <span>
        Reconnecting (attempt {pane.status.attempt}) in {Math.round(pane.status.delay_ms / 1000)}s…
      </span>
    </div>
  {:else if pane.awaitingReconnect}
    <div class="overlay" role="status">
      <span class="spinner" aria-hidden="true"></span>
      <span>Connection lost. Retrying…</span>
    </div>
  {:else if pane.dead}
    <div class="overlay dead" role="alert">
      <p class="dead-title">
        {#if pane.restored}
          Not connected
        {:else if pane.exitCode !== null}
          Process exited with code {pane.exitCode}
        {:else}
          Connection closed
        {/if}
      </p>
      {#if pane.restored}
        <p class="dead-detail muted">Reconnect to {pane.host?.name ?? 'the host'} to continue.</p>
      {:else if pane.errorMessage}
        <p class="dead-detail">{pane.errorMessage}</p>
      {/if}
      <button type="button" class="sx-press" onclick={() => void connectFlow.reconnect(pane)}>
        Reconnect
      </button>
    </div>
  {/if}
</div>

<style>
  .terminal-wrap {
    position: relative;
    height: 100%;
    min-height: 0;
    background: var(--termru-terminal);
  }

  /* The padding sits on this wrapper: the fit addon measures the terminal
     surface inside it, which has none. The pane sets the side padding (18px). */
  .terminal {
    height: 100%;
    padding: var(--termru-space-6) var(--pane-pad-x, 18px);
    background: var(--termru-terminal);
    overflow: hidden;
  }

  .overlay {
    position: absolute;
    inset: 0;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: var(--termru-space-12);
    padding: var(--termru-space-16);
    background: color-mix(in srgb, var(--termru-terminal) 88%, transparent);
    color: var(--termru-text-2);
    font-size: var(--termru-text-body-size);
    line-height: var(--termru-text-body-lh);
  }

  .overlay.dead {
    color: var(--termru-text);
  }

  .dead-title {
    margin: 0;
    font-weight: var(--termru-text-control-weight);
  }

  .dead-detail {
    margin: 0;
    color: var(--termru-err-text);
    max-width: 40rem;
    text-align: center;
    font-size: var(--termru-text-secondary-size);
    line-height: var(--termru-text-secondary-lh);
  }

  .dead-detail.muted {
    color: var(--termru-text-2);
  }

  .spinner {
    width: 18px;
    height: 18px;
    border-radius: 50%;
    border: 2px solid var(--termru-border);
    border-top-color: var(--termru-accent);
    animation: spin 0.8s linear infinite;
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .spinner {
      animation-duration: 3s;
    }
  }
</style>
