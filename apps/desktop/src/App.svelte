<script lang="ts">
  import { onMount } from 'svelte';
  import { api } from './lib/ipc';
  import { commandEntries, palette } from './lib/stores/palette.svelte';
  import {
    effectiveBindings,
    eventMatches,
    expandAccelerator,
    type CommandId,
  } from './lib/keybindings';
  import { errorMessage, hosts } from './lib/stores/hosts.svelte';
  import { prefs } from './lib/stores/prefs.svelte';
  import { sessions } from './lib/stores/sessions.svelte';
  import { shortcuts } from './lib/stores/shortcuts.svelte';
  import { sync } from './lib/stores/sync.svelte';
  import { toasts } from './lib/stores/toasts.svelte';
  import CommandPalette from './lib/components/CommandPalette.svelte';
  import ConnectDialog from './lib/components/ConnectDialog.svelte';
  import HostEditor from './lib/components/HostEditor.svelte';
  import HostKeyDialog from './lib/components/HostKeyDialog.svelte';
  import IdentityEditor from './lib/components/IdentityEditor.svelte';
  import SettingsPanel from './lib/components/SettingsPanel.svelte';
  import Sidebar from './lib/components/Sidebar.svelte';
  import SyncPanel from './lib/components/SyncPanel.svelte';
  import TabBar from './lib/components/TabBar.svelte';
  import SplitView from './lib/components/SplitView.svelte';
  import Toasts from './lib/components/Toasts.svelte';
  import type { Host, SessionSecrets } from './lib/types';

  let pendingHost = $state<Host | null>(null);
  let pendingSecretStored = $state(false);
  let syncOpen = $state(false);
  let settingsOpen = $state(false);
  let identitiesOpen = $state(false);

  // Apply the preferred theme to the document (design tokens follow it).
  $effect(() => {
    const theme = prefs.prefs.theme;
    document.documentElement.dataset.theme = theme === 'system' ? '' : theme;
  });

  onMount(() => {
    void (async () => {
      // The preferences come first: the settings dialog copies them when it
      // opens, and a slow load would show the defaults.
      await prefs.load();
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
    void sync.load();

    const onKeyDown = (event: KeyboardEvent) => {
      // The palette owns the keyboard while it is open.
      if (palette.open) return;
      // So does the recorder of a new keybinding.
      if (shortcuts.recording !== null) return;
      // The user's overrides are applied on top of the defaults.
      for (const binding of effectiveBindings(prefs.prefs.keybindings)) {
        if (!eventMatches(event, binding.accelerator)) continue;
        event.preventDefault();
        runCommand(binding.commandId);
        return;
      }
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  });

  /** Runs a command from the registry (keyboard or palette). */
  function runCommand(id: CommandId): void {
    const active = sessions.active;
    switch (id) {
      case 'palette.open':
        openPalette();
        break;
      case 'tab.new':
        openPalette('hosts');
        break;
      case 'tab.close':
        if (active) void sessions.close(active);
        break;
      case 'tab.next':
        sessions.cycleActive(1);
        break;
      case 'tab.previous':
        sessions.cycleActive(-1);
        break;
      case 'tab.duplicate':
        if (active) sessions.duplicate(active);
        break;
      case 'host.new':
        hosts.editorOpen = true;
        hosts.editing = null;
        break;
      case 'host.search':
        document.querySelector<HTMLInputElement>('input[type="search"]')?.focus();
        break;
      case 'view.toggleTheme': {
        const next = prefs.prefs.theme === 'dark' ? 'light' : 'dark';
        void prefs.update({ ...prefs.prefs, theme: next });
        break;
      }
      case 'view.increaseFont': {
        const size = Math.min(72, prefs.prefs.terminal.font_size + 1);
        void prefs.update({
          ...prefs.prefs,
          terminal: { ...prefs.prefs.terminal, font_size: size },
        });
        break;
      }
      case 'view.decreaseFont': {
        const size = Math.max(6, prefs.prefs.terminal.font_size - 1);
        void prefs.update({
          ...prefs.prefs,
          terminal: { ...prefs.prefs.terminal, font_size: size },
        });
        break;
      }
      case 'sync.open':
        syncOpen = true;
        void sync.load();
        break;
      case 'settings.open':
        settingsOpen = true;
        break;
      case 'pane.splitRight':
        sessions.splitActive('columns');
        break;
      case 'pane.splitDown':
        sessions.splitActive('rows');
        break;
      case 'pane.close': {
        const pane = sessions.activePane;
        if (pane) void sessions.closePane(pane);
        break;
      }
      case 'pane.next':
        sessions.cyclePane(1);
        break;
      case 'pane.previous':
        sessions.cyclePane(-1);
        break;
    }
  }

  /** Opens the command palette with the current commands and hosts. */
  function openPalette(scope: 'all' | 'hosts' = 'all'): void {
    const overrides = prefs.prefs.keybindings;
    const entries =
      scope === 'hosts'
        ? []
        : commandEntries(runCommand, (command) =>
            expandAccelerator(overrides[command.id] ?? command.accelerator),
          );
    for (const host of hosts.visible) {
      entries.push({
        id: `host:${host.id}`,
        title: `Connect to ${host.name}`,
        group: 'Hosts',
        run: () => void requestConnect(host),
      });
    }
    palette.show(entries);
  }

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
  async function requestConnect(host: Host): Promise<void> {
    if (host.auth.type === 'agent') {
      openTab(host, {});
      return;
    }
    const kind = host.auth.type === 'password' ? 'password' : 'passphrase';
    try {
      pendingSecretStored = await api.secretHas(host.id, kind);
    } catch {
      pendingSecretStored = false;
    }
    pendingHost = host;
  }

  function openTab(host: Host, secrets: SessionSecrets, remember = false): void {
    sessions.open(host, secrets, remember);
    pendingHost = null;
  }
</script>

<div class="app">
  <Sidebar
    onconnect={requestConnect}
    onopensync={() => (syncOpen = true)}
    onopenidentities={() => (identitiesOpen = true)}
  />

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
            <SplitView node={tab.root} {tab} />
          </div>
        {/each}
      {/if}
    </div>
  </main>
</div>

{#if pendingHost}
  <ConnectDialog
    host={pendingHost}
    secretStored={pendingSecretStored}
    oncancel={() => (pendingHost = null)}
    onsubmit={(secrets, remember) => {
      if (pendingHost) openTab(pendingHost, secrets, remember);
    }}
  />
{/if}

<HostEditor onmanageidentities={() => (identitiesOpen = true)} />
<HostKeyDialog />
<IdentityEditor bind:open={identitiesOpen} />
<SyncPanel bind:open={syncOpen} />
<SettingsPanel bind:open={settingsOpen} />
<CommandPalette />
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
