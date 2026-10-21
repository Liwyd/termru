<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import { Channel } from '@tauri-apps/api/core';
  import { Terminal } from '@xterm/xterm';
  import { FitAddon } from '@xterm/addon-fit';
  import { WebglAddon } from '@xterm/addon-webgl';
  import { api } from '../ipc';
  import { errorMessage } from '../stores/hosts.svelte';
  import { prefs } from '../stores/prefs.svelte';
  import { sessions } from '../stores/sessions.svelte';
  import { decodeBase64, encodeUtf8 } from '../terminal/codec';
  import { terminalTheme } from '../terminal/theme';
  import type { Pane } from '../stores/sessions.svelte';
  import type { SessionUpdate } from '../types';

  let { pane }: { pane: Pane } = $props();

  let container: HTMLDivElement | undefined = $state();
  let term = $state<Terminal | null>(null);
  let fitAddon: FitAddon | null = null;
  let resizeTimer: ReturnType<typeof setTimeout> | undefined;

  onMount(() => {
    const instance = new Terminal({
      fontFamily: prefs.prefs.terminal.font_family,
      fontSize: prefs.prefs.terminal.font_size,
      scrollback: prefs.prefs.terminal.scrollback,
      cursorBlink: prefs.prefs.terminal.cursor_blink,
      cursorStyle: prefs.prefs.terminal.cursor_style,
      theme: terminalTheme(prefs.isDark),
      allowProposedApi: true,
    });
    const fit = new FitAddon();
    instance.loadAddon(fit);
    instance.open(container as HTMLDivElement);
    term = instance;
    fitAddon = fit;

    // WebGL renderer when available; the DOM renderer stays as fallback.
    try {
      const webgl = new WebglAddon();
      instance.loadAddon(webgl);
    } catch {
      // No WebGL context (software rendering, blocked GPU): keep the DOM
      // renderer — slower, but correct.
    }

    instance.onData((data) => {
      const sessionId = pane.sessionId;
      if (!sessionId) return;
      void api.sessionWrite(sessionId, encodeUtf8(data)).catch(() => {
        // The session ended between the keystroke and the write; the status
        // stream tells the user what happened.
      });
    });

    const observer = new ResizeObserver(() => scheduleFit());
    observer.observe(container as HTMLDivElement);
    scheduleFit();

    const onKeyDown = (event: KeyboardEvent) => {
      if (!event.ctrlKey || !event.shiftKey) return;
      if (event.key === 'C' || event.key === 'c') {
        const selection = instance.getSelection();
        if (!selection) return;
        event.preventDefault();
        void navigator.clipboard?.writeText(selection).catch(() => undefined);
      } else if (event.key === 'V' || event.key === 'v') {
        event.preventDefault();
        void navigator.clipboard
          ?.readText()
          .then((text) => {
            if (text.length > 0) instance.paste(text);
          })
          .catch(() => undefined);
      }
    };
    (container as HTMLDivElement).addEventListener('keydown', onKeyDown);

    return () => {
      observer.disconnect();
      (container as HTMLDivElement).removeEventListener('keydown', onKeyDown);
      if (resizeTimer) clearTimeout(resizeTimer);
      instance.dispose();
      term = null;
      fitAddon = null;
    };
  });

  // Appearance changes (font size, theme) apply to the live terminal.
  $effect(() => {
    const instance = term;
    if (!instance) return;
    const { font_family, font_size, cursor_blink, cursor_style } = prefs.prefs.terminal;
    instance.options.fontFamily = font_family;
    instance.options.fontSize = font_size;
    instance.options.cursorBlink = cursor_blink;
    instance.options.cursorStyle = cursor_style;
    instance.options.theme = terminalTheme(prefs.isDark);
  });

  // (Re)connect whenever the pane asks for a new connection attempt.
  // `untrack` keeps the effect bound to `generation` only: reading
  // `pane.sessionId` inside `connect` must not re-trigger it.
  $effect(() => {
    const generation = pane.generation;
    untrack(() => {
      void connect(generation);
    });
  });

  async function connect(generation: number): Promise<void> {
    // An empty pane has no host: there is nothing to connect to.
    if (!pane.host) return;

    // A previous attempt may still own a backend session: close it so the
    // backend never keeps an orphaned session.
    if (pane.sessionId) {
      const previous = pane.sessionId;
      pane.sessionId = null;
      try {
        await api.sessionClose(previous);
      } catch {
        // Already gone — nothing to clean up.
      }
    }

    const channel = new Channel<SessionUpdate>();
    channel.onmessage = (update) => {
      if (update.kind === 'data') {
        term?.write(decodeBase64(update.base64));
        return;
      }
      sessions.applyUpdate(pane, update);
    };

    const geometry = currentGeometry();
    try {
      if (pane.attachTo) {
        // The session lives in another window: attach to it (the backend
        // sends the replay buffer, then the live stream).
        await api.sessionAttach(pane.attachTo, channel);
        if (pane.generation !== generation) return;
        pane.sessionId = pane.attachTo;
        return;
      }
      const id = pane.duplicateFrom
        ? await api.sessionDuplicate(pane.duplicateFrom, geometry, channel)
        : await api.sessionOpen(
            pane.host.id,
            { cols: geometry.cols, rows: geometry.rows, secrets: pane.secrets },
            channel,
          );
      if (pane.generation !== generation) {
        // Superseded by a newer attempt: drop this session immediately.
        void api.sessionClose(id).catch(() => {});
        return;
      }
      pane.sessionId = id;
    } catch (err) {
      if (pane.generation === generation) {
        sessions.fail(pane, errorMessage(err));
      }
    }
  }

  function currentGeometry(): { cols: number; rows: number } {
    if (term) return { cols: term.cols, rows: term.rows };
    return { cols: 80, rows: 24 };
  }

  function scheduleFit(): void {
    if (resizeTimer) clearTimeout(resizeTimer);
    resizeTimer = setTimeout(() => {
      if (!fitAddon || !term) return;
      const element = container as HTMLDivElement | undefined;
      if (!element || element.offsetWidth === 0 || element.offsetHeight === 0) {
        // Hidden pane: nothing to fit (xterm would compute a 0x0 grid).
        return;
      }
      try {
        fitAddon.fit();
      } catch {
        return;
      }
      const sessionId = pane.sessionId;
      if (!sessionId) return;
      void api.sessionResize(sessionId, term.cols, term.rows).catch(() => {
        // Resize failures are transient (session closing).
      });
    }, 120);
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
        {#if pane.exitCode !== null}
          Process exited with code {pane.exitCode}
        {:else}
          Connection closed
        {/if}
      </p>
      {#if pane.errorMessage}
        <p class="dead-detail">{pane.errorMessage}</p>
      {/if}
      <button type="button" onclick={() => sessions.retry(pane)}>Reconnect</button>
    </div>
  {/if}
</div>

<style>
  .terminal-wrap {
    position: relative;
    height: 100%;
    min-height: 0;
    background: var(--termru-bg);
  }

  .terminal {
    height: 100%;
  }

  .overlay {
    position: absolute;
    inset: 0;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 0.75rem;
    background: color-mix(in srgb, var(--termru-bg) 88%, transparent);
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
