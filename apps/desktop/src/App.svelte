<script lang="ts">
  import { onMount } from 'svelte';
  import { api } from './lib/ipc';
  import { listen } from './lib/ipc-backend';
  import { commandEntries, palette } from './lib/stores/palette.svelte';
  import {
    acceleratorFor,
    expandAccelerator,
    matchBinding,
    type CommandId,
  } from './lib/keybindings';
  import { shortcutAllowed } from './lib/shortcut-guard';
  import { connectFlow } from './lib/stores/connect.svelte';
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
  import AuthPromptDialog from './lib/components/AuthPromptDialog.svelte';
  import HostKeyDialog from './lib/components/HostKeyDialog.svelte';
  import Toasts from './lib/components/Toasts.svelte';
  import FoldersPage from './lib/pages/FoldersPage.svelte';
  import HostEditPage from './lib/pages/HostEditPage.svelte';
  import HostsPage from './lib/pages/HostsPage.svelte';
  import IdentitiesPage from './lib/pages/IdentitiesPage.svelte';
  import PreferencesPage from './lib/pages/PreferencesPage.svelte';
  import SessionsPage from './lib/pages/SessionsPage.svelte';
  import SyncPage from './lib/pages/SyncPage.svelte';
  import type { Host } from './lib/types';

  // Apply the preferred theme to the document (design tokens follow it).
  // The motion and focus preferences drive the token overrides of Doc B.05
  // and the high-visibility focus ring.
  $effect(() => {
    const theme = prefs.prefs.theme;
    document.documentElement.dataset.theme = theme === 'system' ? '' : theme;
    document.documentElement.dataset.reducedMotion = prefs.prefs.reduced_motion;
    document.documentElement.dataset.focusVisibility = prefs.prefs.focus_visibility ? 'on' : '';
  });

  // The page the user is on is part of the saved layout: save it on
  // navigation too, not only when the tabs change.
  $effect(() => {
    void route.route;
    sessions.scheduleLayoutSave();
  });

  onMount(() => {
    void (async () => {
      // The preferences come first: the settings page copies them when it
      // opens, and a slow load would show the defaults.
      await prefs.load();
      await hosts.load();
      // A window opened for a moved session attaches to it instead of
      // restoring the saved layout. It never owns the saved layout.
      const attachId = new URLSearchParams(window.location.search).get('attach');
      if (attachId) {
        sessions.windowLabel = `session-${attachId}`;
        await attachToSession(attachId);
        return;
      }
      // Restored tabs reference hosts, so the layout is restored after them.
      await sessions.restoreLayout();
    })();
    void sync.load();

    // The backend applied a sync (pulled records): the local copies are
    // stale until they are read again.
    let unlistenSync: (() => void) | undefined;
    let disposed = false;
    void listen('sync-applied', () => void sync.reloadApplied()).then((fn) => {
      if (disposed) fn();
      else unlistenSync = fn;
    });

    const onKeyDown = (event: KeyboardEvent) => {
      // The palette owns the keyboard while it is open.
      if (palette.open) return;
      // So does the recorder of a new keybinding.
      if (shortcuts.recording !== null) return;
      // The user's overrides are applied on top of the defaults.
      const binding = matchBinding(event, prefs.prefs.keybindings);
      if (!binding) return;
      // A dialog or a text field owns the keys it needs.
      if (!shortcutAllowed(event, binding.commandId)) return;
      event.preventDefault();
      runCommand(binding.commandId);
    };
    window.addEventListener('keydown', onKeyDown);
    return () => {
      disposed = true;
      unlistenSync?.();
      window.removeEventListener('keydown', onKeyDown);
    };
  });

  /** Applies new preferences; a refusal of the store is shown, not swallowed. */
  async function updatePrefs(next: typeof prefs.prefs): Promise<void> {
    try {
      await prefs.update(next);
    } catch (err) {
      toasts.error(`Could not save the preferences: ${errorMessage(err)}`);
    }
  }

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
        // A tab with a live session asks before it goes.
        if (active) sessions.requestClose(active);
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
        route.edit(null);
        break;
      case 'host.search':
        // The page may not exist yet: the hosts page consumes the request
        // when it mounts (or at once when it is already there).
        hosts.searchFocus = true;
        if (route.route !== 'hosts') route.go('hosts');
        break;
      case 'view.toggleTheme': {
        // From "system" the toggle goes to the opposite of what is shown.
        const next = prefs.isDark ? 'light' : 'dark';
        void updatePrefs({ ...prefs.prefs, theme: next });
        break;
      }
      case 'view.increaseFont': {
        const size = Math.min(72, prefs.prefs.terminal.font_size + 1);
        void updatePrefs({
          ...prefs.prefs,
          terminal: { ...prefs.prefs.terminal, font_size: size },
        });
        break;
      }
      case 'view.decreaseFont': {
        const size = Math.max(6, prefs.prefs.terminal.font_size - 1);
        void updatePrefs({
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
        if (pane) sessions.requestClosePane(pane);
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
            expandAccelerator(acceleratorFor(command.id, overrides)),
          );
    // Every host, not only the ones the hosts page currently filters to.
    for (const host of hosts.sortedAll) {
      entries.push({
        id: `host:${host.id}`,
        title: `Connect to ${host.name}`,
        group: 'Hosts',
        run: () => void connectFlow.request(host),
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
</script>

<AppShell>
  {#if route.route === 'hosts'}
    <HostsPage onconnect={(host) => void connectFlow.request(host)} onopen={openHost} />
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

{#if connectFlow.pending}
  <ConnectDialog
    host={connectFlow.pending.host}
    secretStored={connectFlow.pending.secretStored}
    oncancel={() => connectFlow.cancel()}
    onsubmit={(secrets, remember) => connectFlow.submit(secrets, remember)}
  />
{/if}

<HostKeyDialog />
<AuthPromptDialog />
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
