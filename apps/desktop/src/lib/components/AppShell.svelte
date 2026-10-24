<script lang="ts">
  import AppNav from './AppNav.svelte';
  import StatusBar from './StatusBar.svelte';
  import WindowControls from './WindowControls.svelte';
  import { uiPlatform } from '../platform';

  let { children } = $props();

  // macOS uses the native traffic lights through the overlay titlebar, so
  // the in-app bar is inset to the right of them (Doc A.01).
  const platform = uiPlatform();
</script>

<div class="shell">
  <header class="titlebar" class:macos={platform === 'macos'} data-tauri-drag-region>
    <span class="crumb" data-tauri-drag-region>termru / desktop</span>
    <WindowControls />
  </header>

  <AppNav />

  <div class="outlet">
    {@render children()}
  </div>

  <StatusBar />
</div>

<style>
  .shell {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
  }

  .titlebar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    height: var(--termru-titlebar-h);
    padding: 0 var(--termru-space-2) 0 var(--termru-space-4);
    border-bottom: 1px solid var(--termru-border);
    background: var(--termru-surface);
    user-select: none;
  }

  .titlebar.macos {
    padding-left: 78px;
  }

  .crumb {
    font-size: var(--termru-text-meta-size);
    line-height: var(--termru-text-meta-lh);
    color: var(--termru-muted);
  }

  .outlet {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    background: var(--termru-canvas);
  }
</style>
