<script lang="ts">
  import { lastInputWasKeyboard } from '../input-modality';
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

  /**
   * Adds the open state one frame after the notice appeared, so it transitions
   * in. A notice that follows a key press (a copy by shortcut, say) is simply
   * there: nothing a key triggers moves.
   */
  function enter(node: HTMLElement): { destroy(): void } {
    if (lastInputWasKeyboard()) {
      node.classList.add('shown', 'still');
      return { destroy() {} };
    }
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
      <span class="glyph" aria-hidden="true"><Icon name={ICONS[notice.kind]} size={16} /></span>
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
        <Icon name="x" size={13} />
      </button>
    </div>
  {/each}
</section>

<style>
  /* A notice is compact: raised, a 12 px card, one line of title and at most a
     short description. top: the window chrome (a title bar on Windows and
     macOS) + the page header (56) + 8, so a notice never covers the primary
     action; right: 24. */
  .viewport {
    position: fixed;
    top: calc(var(--termru-chrome-top, 0px) + var(--termru-header-h) + var(--termru-space-8));
    right: var(--termru-space-24);
    z-index: 90;
    display: flex;
    flex-direction: column;
    gap: var(--termru-space-8);
    width: min(20rem, calc(100vw - 2 * var(--termru-space-24)));
    /* The strip never blocks the page under it; only a notice takes clicks. */
    pointer-events: none;
  }

  .notice {
    display: flex;
    align-items: flex-start;
    gap: var(--termru-space-10);
    padding: var(--termru-space-12);
    border: var(--termru-border-w) solid var(--termru-border);
    border-radius: var(--termru-radius-pane);
    background: var(--termru-raised);
    color: var(--termru-text);
    box-shadow: var(--termru-shadow-float);
    pointer-events: auto;
    /* Enter: down from above, 140 ms. Opacity and transform only. */
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

  /* No entrance for a notice that a key caused (its exit still plays). */
  .notice:global(.still) {
    transition: none;
  }

  /* Exit: faster than the enter (120 ms), and it no longer takes clicks. */
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
    margin-top: 2px;
  }

  .success .glyph {
    color: var(--termru-accent-text);
  }

  .info .glyph {
    color: var(--termru-text-2);
  }

  .warning .glyph {
    color: var(--termru-warn-text);
  }

  .error .glyph {
    color: var(--termru-err-text);
  }

  .error {
    border-color: color-mix(in srgb, var(--termru-err-text) 45%, var(--termru-border));
  }

  .warning {
    border-color: color-mix(in srgb, var(--termru-warn-text) 45%, var(--termru-border));
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
    font-weight: var(--termru-text-control-weight);
  }

  .description {
    margin-top: 2px;
    font-size: var(--termru-text-secondary-size);
    line-height: var(--termru-text-secondary-lh);
    color: var(--termru-text-2);
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
    min-height: 28px;
    margin-top: var(--termru-space-8);
    padding: 0 var(--termru-space-10);
    border-radius: var(--termru-radius-sm);
    background: var(--termru-raised-2);
    font-size: var(--termru-text-secondary-size);
  }

  /* A 28 px target for a 13 px glyph. */
  .dismiss {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    flex: none;
    width: 28px;
    min-height: 28px;
    margin: -4px -4px 0 0;
    padding: 0;
    border: none;
    background: none;
    color: var(--termru-text-2);
  }

  @media (hover: hover) and (pointer: fine) {
    button.dismiss:hover:not(:disabled) {
      background: var(--termru-raised-2);
      color: var(--termru-text);
    }

    button.action:hover:not(:disabled) {
      background: color-mix(in srgb, var(--termru-text) 12%, var(--termru-raised-2));
    }
  }
</style>
