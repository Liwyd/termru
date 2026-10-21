<script lang="ts">
  import { onMount } from 'svelte';
  import { api } from './lib/ipc';
  import { errorMessage, hosts } from './lib/stores/hosts.svelte';
  import { prefs } from './lib/stores/prefs.svelte';
  import { sessions } from './lib/stores/sessions.svelte';
  import { toasts } from './lib/stores/toasts.svelte';
  import ConnectDialog from './lib/components/ConnectDialog.svelte';
  import HostEditor from './lib/components/HostEditor.svelte';
  import HostKeyDialog from './lib/components/HostKeyDialog.svelte';
  import Sidebar from './lib/components/Sidebar.svelte';
  import TabBar from './lib/components/TabBar.svelte';
  import Terminal from './lib/components/Terminal.svelte';
  import Toasts from './lib/components/Toasts.svelte';
  import type { Host, SessionSecrets } from './lib/types';

  let pendingHost = $state<Host | null>(null);

  // Apply the preferred theme to the document (design tokens follow it).
  $effect(() => {
    const theme = prefs.prefs.theme;
    document.documentElement.dataset.theme = theme === 'system' ? '' : theme;
  });

  onMount(() => {
    void (async () => {
      await hosts.load();
      // A window opened for a moved session attaches to it instead of
      // restoring the saved layout.
      const attachId = new URLSearchParams(window.location.search).get('attach');
      if (attachId) {
        await attachToSession(attachId);
        return;
      }
      // Restored tabs reference hosts, so the layout is restored after them.
      await sessions.restoreLayout();
    })();
    void prefs.load();

    const onKeyDown = (event: KeyboardEvent) => {
      if (event.ctrlKey && event.key === 'Tab') {
        event.preventDefault();
        sessions.cycleActive(event.shiftKey ? -1 : 1);
      }
      if (event.ctrlKey && event.key === 'w') {
        const active = sessions.active;
        if (active) {
          event.preventDefault();
          void sessions.close(active);
        }
      }
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  });

  /** Attaches this window to a session that another window moved here. */
  async function attachToSession(sessionId: string): Promise<void> {
    try {
      const sessions_ = await api.sessionList();
      const info = sessions_.find((entry) => entry.id === sessionId);
      if (!info) {
        toasts.error('The session is no longer available.');
        return;
      }
      const host = hosts.hosts.find((entry) => entry.id === info.host_id);
      if (!host) {
        toasts.error('The host of that session no longer exists.');
        return;
      }
      sessions.attachTab(sessionId, host);
    } catch (err) {
      toasts.error(errorMessage(err));
    }
  }

  /** Hosts that need a secret prompt before the connection can start. */
  function requestConnect(host: Host): void {
    if (host.auth.type === 'agent') {
      openTab(host, {});
      return;
    }
    pendingHost = host;
  }

  function openTab(host: Host, secrets: SessionSecrets): void {
    sessions.open(host, secrets);
    pendingHost = null;
  }
</script>

<div class="app">
  <Sidebar onconnect={requestConnect} />

  <main>
    <div class="tabbar" role="tablist" aria-label="Open sessions">
      {#each sessions.tabs as tab (tab.id)}
        <TabBar {tab} active={tab.id === sessions.activeId} />
      {/each}
    </div>

    <div class="content">
      {#if sessions.tabs.length === 0}
        <div class="empty">
          <h1>Termru</h1>
          <p>Select a host on the left to start a terminal session.</p>
        </div>
      {:else}
        {#each sessions.tabs as tab (tab.id)}
          <div class="pane" class:hidden={tab.id !== sessions.activeId}>
            <Terminal {tab} />
          </div>
        {/each}
      {/if}
    </div>
  </main>
</div>

{#if pendingHost}
  <ConnectDialog
    host={pendingHost}
    oncancel={() => (pendingHost = null)}
    onsubmit={(secrets) => {
      if (pendingHost) openTab(pendingHost, secrets);
    }}
  />
{/if}

<HostEditor />
<HostKeyDialog />
<Toasts />

{#if hosts.error}
  <p class="sr-only" role="status">{hosts.error}</p>
{/if}

<style>
  .app {
    display: flex;
    height: 100%;
    min-height: 0;
  }

  main {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
  }

  .tabbar {
    display: flex;
    align-items: flex-end;
    gap: 0.2rem;
    min-height: 2rem;
    padding: 0.2rem 0.4rem 0;
    border-bottom: 1px solid var(--termru-border);
    background: var(--termru-surface);
    overflow-x: auto;
  }

  .content {
    flex: 1;
    min-height: 0;
    position: relative;
  }

  .pane {
    position: absolute;
    inset: 0;
  }

  .pane.hidden {
    display: none;
  }

  .empty {
    height: 100%;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 0.4rem;
    color: var(--termru-muted);
  }

  .empty h1 {
    margin: 0;
    font-size: 1.3rem;
    color: var(--termru-fg);
  }

  .empty p {
    margin: 0;
    font-size: 0.85rem;
  }

  .sr-only {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip: rect(0 0 0 0);
  }
</style>
