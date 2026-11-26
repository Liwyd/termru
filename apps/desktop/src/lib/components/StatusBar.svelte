<script lang="ts">
  import { acceleratorFor, acceleratorLabel } from '../keybindings';
  import { route } from '../stores/route.svelte';
  import { hosts } from '../stores/hosts.svelte';
  import { prefs } from '../stores/prefs.svelte';
  import { sessions } from '../stores/sessions.svelte';
  import { sync } from '../stores/sync.svelte';

  // The network state is read from the window's own events: `navigator.onLine`
  // alone is not reactive.
  let online = $state(true);
  $effect(() => {
    online = navigator.onLine;
    const apply = (event: Event) => {
      online = event.type !== 'offline';
    };
    window.addEventListener('online', apply);
    window.addEventListener('offline', apply);
    return () => {
      window.removeEventListener('online', apply);
      window.removeEventListener('offline', apply);
    };
  });

  /** The real state of the sync, not a constant. */
  const syncState = $derived(
    !sync.signedIn ? 'Local workspace' : online ? 'Sync · signed in' : 'Sync · offline',
  );

  /** The palette chord, as the user has it bound. */
  const paletteHint = $derived(
    acceleratorLabel(acceleratorFor('palette.open', prefs.prefs.keybindings)),
  );

  /**
   * The left line is contextual: one honest sentence about the current
   * page (Doc A.03 / status bar).
   */
  const context = $derived.by(() => {
    switch (route.route) {
      case 'hosts': {
        const total = hosts.hosts.length;
        const running = sessions.tabs.length;
        const hostLine = total === 0 ? 'No saved hosts' : `${total} host${total === 1 ? '' : 's'}`;
        const sessionLine =
          running === 0 ? 'no sessions' : `${running} session${running === 1 ? '' : 's'} running`;
        return `${hostLine} · ${sessionLine}`;
      }
      case 'sessions': {
        const tab = sessions.active;
        if (!tab) return 'No open sessions';
        const panes = tab.panes.length;
        const paneLine = panes > 1 ? ` · ${panes} panes` : '';
        return `${tab.title}${paneLine}`;
      }
      case 'folders':
        return 'One folder per host';
      case 'identities':
        return 'Identities hold key references, never key material';
      case 'sync':
        return sync.signedIn
          ? `Signed in${sync.pending > 0 ? ` · ${sync.pending} pending` : ''}`
          : 'Local workspace';
      case 'preferences':
        return 'Preferences apply to this device';
      case 'host-edit':
        return route.editHost ? `Editing ${route.editHost.name}` : 'New host';
    }
  });
</script>

<footer class="statusbar" role="status">
  <span class="context">{context}</span>
  <span class="right">
    <span>SSH · {syncState}</span>
    <span class="hint">{paletteHint} Quick switch</span>
  </span>
</footer>

<style>
  .statusbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--termru-space-4);
    height: 28px;
    padding: 0 var(--termru-page-inset-x);
    border-top: 1px solid var(--termru-border);
    background: var(--termru-surface);
    font-size: var(--termru-text-meta-size);
    line-height: var(--termru-text-meta-lh);
    color: var(--termru-muted);
  }

  .context {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .right {
    display: flex;
    gap: var(--termru-space-4);
    flex: none;
  }

  .hint {
    color: var(--termru-faint);
  }
</style>
