<script lang="ts">
  import { notices, type ActiveNotice } from '../notify.svelte';
  import Icon, { type IconName } from './Icon.svelte';

  /**
   * The one adapter that draws notices (Doc C.03): a fixed viewport at the
   * top right, below the title bar and the navigation, outside the route
   * outlet. It holds no policy; `notify.svelte.ts` decides what is shown.
   *
   * A failure or a warning is `role="alert"`, everything else
   * `role="status"`. Nothing takes the focus: the action and the dismiss
   * button are reachable with Tab, in the order the notices appear. The
   * timer of a notice waits while the pointer or the keyboard focus is on it.
   */

  const ICONS: Record<ActiveNotice['kind'], IconName> = {
    success: 'check-circle',
    info: 'info',
    warning: 'alert-circle',
    error: 'alert-circle',
  };

  /** Adds the open state one frame after the notice appeared, so it transitions in. */
  function enter(node: HTMLElement): { destroy(): void } {
    const frame = requestAnimationFrame(() => node.classList.add('shown'));
    return { destroy: () => cancelAnimationFrame(frame) };
  }
</script>

<section class="viewport" aria-label="Notices">
  {#each notices.visible as notice (notice.id)}
    <div
      class="notice {notice.kind}"
      class:leaving={notice.leaving}
      role={notice.kind === 'error' || notice.kind === 'warning' ? 'alert' : 'status'}
      aria-atomic="true"
      data-notice-key={notice.key}
      use:enter
      onpointerenter={() => notices.hold(notice.id)}
      onpointerleave={() => notices.release(notice.id)}
      onfocusin={() => notices.hold(notice.id)}
      onfocusout={() => notices.release(notice.id)}
    >
      <span class="glyph" aria-hidden="true"><Icon name={ICONS[notice.kind]} size={22} /></span>
      <div class="text">
        <p class="title">{notice.title}</p>
        {#if notice.description}<p class="description">{notice.description}</p>{/if}
        {#if notice.action}
          <button type="button" class="action" onclick={() => notices.runAction(notice.id)}>
            {notice.action.label}
          </button>
        {/if}
      </div>
      <button
        type="button"
        class="dismiss"
        aria-label="Dismiss notification"
        onclick={() => notices.dismiss(notice.id)}
      >
        <Icon name="x" size={14} />
      </button>
    </div>
  {/each}
</section>

<style>
  /* top: title bar (36) + navigation (64) + 24; right: 24 (Doc C.03). */
  .viewport {
    position: fixed;
    top: calc(var(--termru-titlebar-h) + var(--termru-nav-h) + var(--termru-space-5));
    right: var(--termru-space-5);
    z-index: 90;
    display: flex;
    flex-direction: column;
    gap: var(--termru-space-3);
    width: min(21rem, calc(100vw - 2 * var(--termru-space-5)));
    /* The strip never blocks the page under it; only a notice takes clicks. */
    pointer-events: none;
  }

  .notice {
    display: flex;
    align-items: flex-start;
    gap: var(--termru-space-3);
    padding: var(--termru-space-4);
    border: var(--termru-border-w) solid var(--termru-border);
    border-radius: var(--termru-radius-lg);
    background: var(--termru-surface);
    color: var(--termru-fg);
    box-shadow: var(--termru-shadow-float);
    pointer-events: auto;
    /* Enter: up from above, 220 ms. Opacity and transform only. */
    opacity: 0;
    transform: translateY(var(--notice-shift));
    transition:
      opacity var(--motion-structure) var(--ease-out),
      transform var(--motion-structure) var(--ease-out);
  }

  .notice:global(.shown) {
    opacity: 1;
    transform: none;
  }

  /* Exit: faster than the enter (160 ms), and it no longer takes clicks. */
  .notice.leaving {
    opacity: 0;
    transform: translateY(var(--notice-shift));
    pointer-events: none;
    transition:
      opacity var(--motion-context) var(--ease-out),
      transform var(--motion-context) var(--ease-out);
  }

  .glyph {
    display: inline-flex;
    margin-top: 1px;
  }

  .success .glyph {
    color: var(--termru-accent);
  }

  .info .glyph {
    color: var(--termru-muted);
  }

  .warning .glyph {
    color: var(--termru-warning);
  }

  .error .glyph {
    color: var(--termru-danger);
  }

  .error {
    border-color: color-mix(in srgb, var(--termru-danger) 45%, var(--termru-border));
  }

  .warning {
    border-color: color-mix(in srgb, var(--termru-warning) 45%, var(--termru-border));
  }

  .text {
    flex: 1;
    min-width: 0;
  }

  p {
    margin: 0;
    overflow-wrap: anywhere;
  }

  .title {
    font-size: var(--termru-text-body-size);
    line-height: var(--termru-text-body-lh);
    font-weight: 550;
  }

  .description {
    margin-top: 2px;
    font-size: var(--termru-text-secondary-size);
    line-height: var(--termru-text-secondary-lh);
    color: var(--termru-muted);
  }

  button {
    transition:
      transform var(--motion-fast) var(--ease-out),
      background-color var(--motion-fast) ease,
      color var(--motion-fast) ease;
  }

  button:active:not(:disabled) {
    transform: scale(var(--press-scale));
  }

  .action {
    min-height: 32px;
    margin-top: var(--termru-space-2);
    padding: 0 var(--termru-space-3);
    border-radius: var(--termru-radius-button);
    font-size: var(--termru-text-secondary-size);
  }

  /* A 32 px target for a 14 px glyph. */
  .dismiss {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    flex: none;
    width: 32px;
    min-height: 32px;
    margin: -6px -6px 0 0;
    padding: 0;
    border: none;
    background: none;
    color: var(--termru-muted);
  }

  @media (hover: hover) and (pointer: fine) {
    .dismiss:hover {
      background: var(--termru-subtle);
      color: var(--termru-fg);
    }
  }
</style>
