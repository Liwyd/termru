<script lang="ts">
  import type { Pane } from '../stores/sessions.svelte';
  import { statusLabel, statusTone } from '../terminal/session-view';

  /**
   * The status of a session: a word, a glyph and a colour (never the colour
   * alone). ■ connected, □ working, ◆ failed, ▬ not connected.
   */
  let { pane }: { pane: Pick<Pane, 'status' | 'restored' | 'host'> } = $props();

  const tone = $derived(statusTone(pane.status));
  const label = $derived(statusLabel(pane));
</script>

<span class="badge {tone} {pane.status.state}" title={label}>
  <span class="glyph" aria-hidden="true"></span>
  {label}
</span>

<style>
  .badge {
    display: inline-flex;
    align-items: center;
    gap: var(--termru-space-8);
    height: 28px;
    padding: 0 10px;
    border-radius: var(--termru-radius-input);
    background: var(--termru-accent-soft);
    color: var(--termru-accent-text);
    font-size: var(--termru-text-secondary-size);
    line-height: var(--termru-text-secondary-lh);
    font-weight: var(--termru-text-control-weight);
    white-space: nowrap;
  }

  .glyph {
    flex: none;
    width: 7px;
    height: 7px;
    background: currentColor;
  }

  .busy .glyph {
    background: transparent;
    border: 1.5px solid currentColor;
  }

  .failed .glyph {
    transform: rotate(45deg) scale(0.9);
  }

  .closed .glyph {
    width: 8px;
    height: 2px;
  }

  .reconnecting {
    background: var(--termru-warn-soft);
    color: var(--termru-warn-text);
  }

  .failed {
    background: var(--termru-err-soft);
    color: var(--termru-err-text);
  }

  .closed {
    background: var(--termru-side);
    color: var(--termru-text-2);
  }
</style>
