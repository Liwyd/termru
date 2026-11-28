<script lang="ts">
  import { acceleratorFor, compactChord } from '../keybindings';
  import { route } from '../stores/route.svelte';
  import { hosts } from '../stores/hosts.svelte';
  import { network } from '../stores/network.svelte';
  import { prefs } from '../stores/prefs.svelte';
  import { sessions } from '../stores/sessions.svelte';
  import { sync } from '../stores/sync.svelte';
  import { unsaved } from '../stores/unsaved.svelte';
  import { relativeAge, sessionsContext } from '../terminal/session-view';

  // "Synced 2 min ago" and "Connected 12 min ago" need a clock.
  let now = $state(Date.now());
  $effect(() => {
    now = Date.now();
    const timer = setInterval(() => (now = Date.now()), 30_000);
    return () => clearInterval(timer);
  });

  /** Tabs with a connected pane: the same count as the live list of the sidebar. */
  const liveCount = $derived(
    sessions.tabs.filter((tab) => tab.panes.some((pane) => pane.status.state === 'ready')).length,
  );

  /** The real state of the sync, not a constant. */
  const syncLine = $derived.by((): { text: string; tone: 'ok' | 'warn' | 'err' | null } => {
    if (!sync.signedIn) return { text: 'Local workspace', tone: null };
    if (!network.online) return { text: 'Sync · offline', tone: 'warn' };
    if (sync.lastError) return { text: 'Sync failed', tone: 'err' };
    if (sync.conflictCount > 0) {
      const n = sync.conflictCount;
      return { text: `${n} sync conflict${n === 1 ? '' : 's'}`, tone: 'warn' };
    }
    if (sync.pending > 0) return { text: `Sync · ${sync.pending} pending`, tone: null };
    const at = sync.status?.last_sync_at ? Date.parse(sync.status.last_sync_at) : Number.NaN;
    return {
      text: Number.isNaN(at) ? 'Sync · signed in' : `Synced ${relativeAge(at, now)}`,
      tone: null,
    };
  });

  /** The palette chord, as the user has it bound. */
  const paletteHint = $derived(
    compactChord(acceleratorFor('palette.open', prefs.prefs.keybindings)),
  );

  /**
   * One honest line about the page you are on, where the page has something to
   * say: what the session is, what has not been saved.
   */
  const context = $derived.by(() => {
    switch (route.route) {
      case 'hosts':
        // "No saved hosts" would be false: they could not be read.
        return hosts.error ? 'Could not read the hosts' : '';
      case 'sessions':
        return sessionsContext(sessions.tabs, sessions.active, now, hosts.identities);
      case 'host-edit':
        // The editor says honestly that nothing is applied until it is saved.
        if (unsaved.dirty) return 'Unsaved changes · No connection settings have been applied';
        return route.editHost ? `Editing ${route.editHost.name}` : 'New host';
      default:
        return '';
    }
  });

  /** What the keys do on this page (the hosts list is the one with a cursor). */
  const hints = $derived(
    route.route === 'hosts' && !hosts.error && hosts.rows.length > 0
      ? ['↑↓ select', '↵ open', 'E edit']
      : [],
  );
</script>

<footer class="status" aria-label="Status">
  <span class="live" class:on={liveCount > 0}>
    <span class="dot" aria-hidden="true"></span>
    {liveCount > 0 ? `${liveCount} live` : 'No live sessions'}
  </span>
  <span class="sync {syncLine.tone ?? ''}">{syncLine.text}</span>
  {#if context}<span class="context">{context}</span>{/if}
  <span class="grow"></span>
  {#each hints as hint (hint)}<span class="hint">{hint}</span>{/each}
  <span class="hint">{paletteHint} command</span>
</footer>

<style>
  .status {
    display: flex;
    align-items: center;
    gap: 18px;
    flex: none;
    height: var(--termru-status-h);
    padding: 0 var(--termru-space-16);
    background: var(--termru-side);
    font-family: var(--termru-font-mono);
    font-size: var(--termru-text-meta-size);
    font-weight: var(--termru-text-meta-weight);
    line-height: var(--termru-text-meta-lh);
    color: var(--termru-text-3);
    white-space: nowrap;
  }

  .grow {
    flex: 1;
    min-width: 0;
  }

  .live {
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }

  .dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--termru-text-3);
  }

  .live.on {
    color: var(--termru-live-text);
  }

  .live.on .dot {
    background: var(--termru-live);
  }

  .sync.warn {
    color: var(--termru-warn-text);
  }

  .sync.err {
    color: var(--termru-err-text);
  }

  .context {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .hint {
    flex: none;
  }

  /* A narrow window keeps the state and drops the hints that are also in the menus. */
  @media (max-width: 760px) {
    .hint:not(:last-child) {
      display: none;
    }
  }
</style>
