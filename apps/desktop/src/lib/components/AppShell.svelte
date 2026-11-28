<script lang="ts">
  import type { Snippet } from 'svelte';
  import Sidebar from './Sidebar.svelte';
  import StatusBar from './StatusBar.svelte';
  import WindowControls from './WindowControls.svelte';
  import { uiPlatform } from '../platform';
  import { chrome } from '../stores/chrome.svelte';

  let { children, onpalette = () => {} }: { children: Snippet; onpalette?: () => void } = $props();

  // Linux keeps the native window decorations, so the app draws no title bar
  // there. Windows draws its own controls; macOS uses the native traffic lights
  // through the overlay titlebar, so the strip is inset to the right of them.
  const platform = uiPlatform();
  const titlebar = platform !== 'linux';

  $effect(() => {
    chrome.watchViewport();
  });

  // Fixed layers (notices) start below whatever the window chrome takes.
  $effect(() => {
    document.documentElement.style.setProperty(
      '--termru-chrome-top',
      titlebar ? 'var(--termru-titlebar-h)' : '0px',
    );
  });
</script>

<div class="shell">
  {#if titlebar}
    <header class="titlebar" class:macos={platform === 'macos'} data-tauri-drag-region>
      <WindowControls />
    </header>
  {/if}

  <div class="frame" class:rail={chrome.railed}>
    <Sidebar {onpalette} />

    <main class="outlet">
      {@render children()}
    </main>
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
    justify-content: flex-end;
    flex: none;
    height: var(--termru-titlebar-h);
    padding: 0 var(--termru-space-8) 0 var(--termru-space-16);
    /* Window chrome is the sidebar's colour. */
    background: var(--termru-side);
    user-select: none;
  }

  .titlebar.macos {
    padding-left: 78px;
  }

  .frame {
    display: grid;
    grid-template-columns: var(--termru-sidebar-w) minmax(0, 1fr);
    flex: 1;
    min-height: 0;
  }

  .frame.rail {
    grid-template-columns: var(--termru-rail-w) minmax(0, 1fr);
  }

  .outlet {
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
    background: var(--termru-base);
  }
</style>
