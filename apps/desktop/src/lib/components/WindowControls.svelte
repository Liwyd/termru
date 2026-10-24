<script lang="ts">
  import { uiPlatform } from '../platform';

  // Linux keeps native decorations (documented deviation from the mockup):
  // the titlebar is a drag region + breadcrumb only. Windows gets custom
  // controls; macOS relies on the native traffic lights (titlebar overlay).
  const platform = uiPlatform();

  async function windowAction(action: 'minimize' | 'toggleMaximize' | 'close'): Promise<void> {
    // Loaded on demand so tests and non-Tauri builds never touch the API.
    const { getCurrentWindow } = await import('@tauri-apps/api/window');
    const win = getCurrentWindow();
    if (action === 'minimize') await win.minimize();
    else if (action === 'toggleMaximize') await win.toggleMaximize();
    else await win.close();
  }
</script>

{#if platform === 'windows'}
  <div class="controls">
    <button
      type="button"
      class="control"
      aria-label="Minimize"
      onclick={() => void windowAction('minimize')}
    >
      <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true"
        ><path d="M0 5h10" stroke="currentColor" stroke-width="1" /></svg
      >
    </button>
    <button
      type="button"
      class="control"
      aria-label="Maximize"
      onclick={() => void windowAction('toggleMaximize')}
    >
      <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true"
        ><rect
          x="0.5"
          y="0.5"
          width="9"
          height="9"
          fill="none"
          stroke="currentColor"
          stroke-width="1"
        /></svg
      >
    </button>
    <button
      type="button"
      class="control close"
      aria-label="Close window"
      onclick={() => void windowAction('close')}
    >
      <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true"
        ><path d="M0 0l10 10M10 0L0 10" stroke="currentColor" stroke-width="1" /></svg
      >
    </button>
  </div>
{/if}

<style>
  .controls {
    display: flex;
    align-items: stretch;
    height: 100%;
  }

  .control {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 44px;
    border: none;
    background: none;
    color: var(--termru-muted);
    cursor: default;
  }

  .control:hover {
    background: var(--termru-subtle);
    color: var(--termru-fg);
  }

  .control.close:hover {
    background: var(--termru-danger);
    color: var(--termru-on-danger);
  }
</style>
