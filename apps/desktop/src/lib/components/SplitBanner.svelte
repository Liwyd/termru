<script lang="ts">
  import { acceleratorFor, acceleratorLabel } from '../keybindings';
  import { prefs } from '../stores/prefs.svelte';
  import type { Tab } from '../stores/sessions.svelte';
  import { terminalUi } from '../stores/terminal-ui.svelte';
  import { paneMoveHint, splitKind } from '../terminal/session-view';
  import SessionIcon from './SessionIcon.svelte';

  /**
   * The hint above a split tab (frame 3): what kind of split this is and how
   * to move between the panes. It can be put away for good.
   */
  let { tab }: { tab: Tab } = $props();

  const bindings = $derived(prefs.prefs.keybindings);
  const move = $derived(
    paneMoveHint(
      acceleratorLabel(acceleratorFor('pane.next', bindings)),
      acceleratorLabel(acceleratorFor('pane.previous', bindings)),
    ),
  );
</script>

<section class="banner" aria-label="Split view hint">
  <div class="text">
    <p class="title">Inspect a release. Keep the logs in view.</p>
    <p class="detail">
      {splitKind(tab.root)} · Click a pane to focus · {move} to move between panes
    </p>
  </div>
  <button
    type="button"
    class="dismiss sx-press"
    aria-label="Dismiss this hint"
    title="Dismiss this hint"
    onclick={() => terminalUi.dismissHint()}
  >
    <SessionIcon name="close" />
  </button>
</section>

<style>
  .banner {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: var(--termru-space-16);
    padding: var(--termru-space-24) var(--termru-space-12) var(--termru-space-24) 28px;
    background: var(--termru-side);
  }

  .text {
    min-width: 0;
  }

  p {
    margin: 0;
  }

  .title {
    font-size: var(--termru-text-body-size);
    line-height: var(--termru-text-body-lh);
    font-weight: var(--termru-text-control-weight);
    color: var(--termru-text);
  }

  .detail {
    margin-top: var(--termru-space-4);
    font-size: var(--termru-text-secondary-size);
    line-height: var(--termru-text-secondary-lh);
    color: var(--termru-text-2);
  }

  .dismiss {
    flex: none;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    min-height: 0;
    width: 32px;
    height: 32px;
    padding: 0;
    border: none;
    border-radius: var(--termru-radius-button);
    background: transparent;
    color: var(--termru-text-3);
  }

  @media (hover: hover) and (pointer: fine) {
    .dismiss:hover {
      background: color-mix(in srgb, var(--termru-text) 7%, transparent);
      color: var(--termru-text);
      border-color: transparent;
    }
  }
</style>
