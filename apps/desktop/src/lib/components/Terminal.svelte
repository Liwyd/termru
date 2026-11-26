<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import { matchBinding } from '../keybindings';
  import { connectFlow } from '../stores/connect.svelte';
  import { prefs } from '../stores/prefs.svelte';
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
  import { attachTerminal, detachTerminal, getTerminal } from '../terminal/registry';
  import type { TerminalHandle } from '../terminal/registry';
  import type { Pane } from '../stores/sessions.svelte';

  /**
   * Hosts the terminal of a pane.
   *
   * The xterm instance lives in the terminal registry, not in this
   * component: mounting re-parents its element into the container, and
   * unmounting detaches it without disposing it, so the scrollback and the
   * session survive a route change, a split or an unsplit. The terminal is
   * disposed by the session store when the pane is really removed.
   */
  let { pane }: { pane: Pane } = $props();

  /** Where the terminal's element is shown (the padded wrapper). */
  let container: HTMLDivElement | undefined = $state();
  /** The terminal of the pane, once this component mounted. */
  let handle = $state.raw<TerminalHandle | null>(null);
  let fitTimer: ReturnType<typeof setTimeout> | undefined;

  onMount(() => {
    const host = container as HTMLDivElement;
    const paneId = pane.id;
    const existing = getTerminal(paneId);
    const instance =
      existing ??
      createTerminal(paneId, host, appearanceFromPrefs(), {
        onData: (data) => sendInput(paneId, data),
        isAppKey: (event) => matchBinding(event, prefs.prefs.keybindings) !== null,
      });
    attachTerminal(instance, host);
    // The theme or the font may have changed while the terminal was away.
    applyAppearance(instance, appearanceFromPrefs());
    handle = instance;

    const observer = new ResizeObserver(() => scheduleFit());
    observer.observe(host);
    scheduleFit();

    // A web font that finishes loading changes the cell size: measure again
    // and fit to the new grid.
    const onFontsLoaded = (): void => {
      remeasure(instance);
      scheduleFit();
    };
    document.fonts?.addEventListener?.('loadingdone', onFontsLoaded);

    return () => {
      document.fonts?.removeEventListener?.('loadingdone', onFontsLoaded);
      observer.disconnect();
      if (fitTimer) clearTimeout(fitTimer);
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
    if (applyAppearance(instance, appearance)) scheduleFit();
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

  function scheduleFit(): void {
    if (fitTimer) clearTimeout(fitTimer);
    fitTimer = setTimeout(() => void refit(), 120);
  }

  async function refit(): Promise<void> {
    const instance = handle;
    if (!instance) return;
    if (!isVisible(instance)) {
      // A hidden pane keeps no GPU context and computes no 0x0 grid.
      setAccelerated(instance, false);
      return;
    }
    await whenFontsReady(instance);
    if (handle !== instance || !isVisible(instance)) return;
    setAccelerated(instance, true);
    const changed = fitTerminal(instance);
    const sessionId = pane.sessionId;
    if (changed && sessionId) pushSize(instance, sessionId);
  }
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
      <span>Connection lost — retrying…</span>
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
      <button type="button" onclick={() => void connectFlow.reconnect(pane)}>Reconnect</button>
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
     surface inside it, which has none. */
  .terminal {
    height: 100%;
    padding: 12px 16px;
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
    gap: 0.75rem;
    background: color-mix(in srgb, var(--termru-terminal) 88%, transparent);
    color: var(--termru-muted);
    font-size: 0.9rem;
  }

  .overlay.dead {
    color: var(--termru-fg);
  }

  .dead-title {
    margin: 0;
    font-weight: 600;
  }

  .dead-detail {
    margin: 0;
    color: var(--termru-danger);
    max-width: 40rem;
    text-align: center;
    font-size: 0.85rem;
  }

  .dead-detail.muted {
    color: var(--termru-muted);
  }

  .spinner {
    width: 1.25rem;
    height: 1.25rem;
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
