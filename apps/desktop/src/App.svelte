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
  import { route } from './lib/stores/route.svelte';
  import { sessions } from './lib/stores/sessions.svelte';
  import { shortcuts } from './lib/stores/shortcuts.svelte';
  import { sync } from './lib/stores/sync.svelte';
  import { toasts } from './lib/stores/toasts.svelte';
  import AppShell from './lib/components/AppShell.svelte';
  import CommandPalette from './lib/components/CommandPalette.svelte';
  import ConnectDialog from './lib/components/ConnectDialog.svelte';
  import HostKeyDialog from './lib/components/HostKeyDialog.svelte';
  import Toasts from './lib/components/Toasts.svelte';
  import FoldersPage from './lib/pages/FoldersPage.svelte';
  import HostEditPage from './lib/pages/HostEditPage.svelte';
  import HostsPage from './lib/pages/HostsPage.svelte';
  import IdentitiesPage from './lib/pages/IdentitiesPage.svelte';
  import PreferencesPage from './lib/pages/PreferencesPage.svelte';
  import SessionsPage from './lib/pages/SessionsPage.svelte';
  import SyncPage from './lib/pages/SyncPage.svelte';
  import type { Host, SessionSecrets } from './lib/types';

  let pendingHost = $state<Host | null>(null);
  let pendingSecretStored = $state(false);

  // Apply the preferred theme to the document (design tokens follow it).
  // The motion and focus preferences drive the token overrides of Doc B.05
  // and the high-visibility focus ring.
  $effect(() => {
    const theme = prefs.prefs.theme;
    document.documentElement.dataset.theme = theme === 'system' ? '' : theme;
    document.documentElement.dataset.reducedMotion = prefs.prefs.reduced_motion;
    document.documentElement.dataset.focusVisibility = prefs.prefs.focus_visibility ? 'on' : '';
  });

  // Closing the host editor (save or cancel) leaves the host-edit route.
  $effect(() => {
    if (route.route === 'host-edit' && !hosts.editorOpen) route.go('hosts');
  });

  onMount(() => {
    void (async () => {
      // The preferences come first: the settings page copies them when it
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
        route.edit(null);
        break;
      case 'host.search':
        if (route.route !== 'hosts') route.go('hosts');
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
        route.go('sync');
        break;
      case 'settings.open':
        route.go('preferences');
        break;
      case 'route.hosts':
        route.go('hosts');
        break;
      case 'route.sessions':
        route.go('sessions');
        break;
      case 'route.folders':
        route.go('folders');
        break;
      case 'route.identities':
        route.go('identities');
        break;
      case 'route.sync':
        route.go('sync');
        break;
      case 'route.preferences':
        route.go('preferences');
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
      route.go('sessions');
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
    // The hosts table shows the same fact in its Authentication column.
    if (host.auth.type === 'password') hosts.markPasswordStored(host.id, pendingSecretStored);
    pendingHost = host;
  }

  /** The "Open" row action: jumps to the running session of a host. */
  function openHost(host: Host): void {
    for (const tab of sessions.tabs) {
      const pane = tab.panes.find((entry) => entry.host?.id === host.id);
      if (pane) {
        sessions.focusPane(pane);
        break;
      }
    }
    route.go('sessions');
  }

  function openTab(host: Host, secrets: SessionSecrets, remember = false): void {
    sessions.open(host, secrets, remember);
    pendingHost = null;
    // The session is the point of the app: show it right away.
    route.go('sessions');
  }
</script>

<AppShell>
  {#if route.route === 'hosts'}
    <HostsPage onconnect={(host) => void requestConnect(host)} onopen={openHost} />
  {:else if route.route === 'sessions'}
    <SessionsPage />
  {:else if route.route === 'folders'}
    <FoldersPage />
  {:else if route.route === 'identities'}
    <IdentitiesPage />
  {:else if route.route === 'sync'}
    <SyncPage />
  {:else if route.route === 'preferences'}
    <PreferencesPage />
  {:else if route.route === 'host-edit'}
    <HostEditPage />
  {/if}
</AppShell>

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

<HostKeyDialog />
<CommandPalette />
<Toasts />

{#if hosts.error}
  <p class="sr-only" role="status">{hosts.error}</p>
{/if}

<style>
  .sr-only {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip: rect(0 0 0 0);
  }
</style>
