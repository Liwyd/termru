<script lang="ts">
  import HostList from '../components/HostList.svelte';
  import { route } from '../stores/route.svelte';
  import { hosts } from '../stores/hosts.svelte';
  import { sessions } from '../stores/sessions.svelte';
  import type { Host, SessionSecrets } from '../types';

  let {
    onconnect,
  }: {
    onconnect: (host: Host, secrets?: SessionSecrets, remember?: boolean) => void;
  } = $props();

  const countLine = $derived.by(() => {
    const total = hosts.hosts.length;
    if (total === 0) return 'No saved connections yet.';
    const running = sessions.tabs.length;
    const hostWord = `${total} saved connection${total === 1 ? '' : 's'}`;
    const sessionWord =
      running === 0
        ? 'No sessions running.'
        : `${running} session${running === 1 ? '' : 's'} running.`;
    return `${hostWord} ${sessionWord}`;
  });
</script>

<div class="page">
  <header class="header">
    <p class="breadcrumb">WORKSPACE / CONNECTION LIBRARY</p>
    <div class="title-row">
      <div>
        <h1>Hosts</h1>
        <p class="count">{countLine}</p>
      </div>
      <button type="button" class="primary" onclick={() => route.edit(null)}>+ New host</button>
    </div>
  </header>

  <div class="body">
    <HostList onconnect={(host) => onconnect(host)} />
  </div>
</div>

<style>
  .page {
    display: flex;
    flex-direction: column;
    min-height: 0;
    height: 100%;
    overflow: auto;
    padding: var(--termru-page-inset-top) var(--termru-page-inset-x);
    gap: var(--termru-space-5);
  }

  .breadcrumb {
    margin: 0;
    font-size: var(--termru-text-meta-size);
    line-height: var(--termru-text-meta-lh);
    letter-spacing: 0.08em;
    color: var(--termru-muted);
  }

  .title-row {
    display: flex;
    align-items: flex-end;
    justify-content: space-between;
    gap: var(--termru-space-4);
  }

  h1 {
    margin: 0;
    font-size: var(--termru-text-title-size);
    line-height: var(--termru-text-title-lh);
    font-weight: var(--termru-text-title-weight);
    color: var(--termru-fg);
  }

  .count {
    margin: 0;
    font-size: var(--termru-text-body-size);
    line-height: var(--termru-text-body-lh);
    color: var(--termru-muted);
  }

  .body {
    flex: 1;
    min-height: 0;
  }
</style>
