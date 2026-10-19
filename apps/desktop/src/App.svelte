<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';

  interface AppInfo {
    name: string;
    version: string;
  }

  let info = $state<AppInfo | null>(null);
  let ipcError = $state<string | null>(null);

  onMount(async () => {
    try {
      info = await invoke<AppInfo>('app_info');
    } catch (err) {
      ipcError = String(err);
    }
  });
</script>

<main class="shell">
  {#if ipcError}
    <p class="error" role="alert">IPC error: {ipcError}</p>
  {:else if info}
    <h1>{info.name}</h1>
    <p class="muted">v{info.version} — connection manager coming in Phase 5.</p>
  {:else}
    <p class="muted">Starting…</p>
  {/if}
</main>

<style>
  .shell {
    display: flex;
    flex-direction: column;
    justify-content: center;
    align-items: center;
    height: 100%;
    gap: 0.5rem;
  }

  h1 {
    margin: 0;
    font-size: 1.5rem;
    font-weight: 600;
    letter-spacing: 0.02em;
  }

  .muted {
    margin: 0;
    color: var(--termru-muted);
    font-size: 0.875rem;
  }

  .error {
    margin: 0;
    color: #f87171;
    font-size: 0.875rem;
  }
</style>
