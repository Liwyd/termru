/**
 * Keeps an element in the DOM long enough to play its exit.
 *
 * Svelte removes an element the moment its `{#if}` turns false. For a menu
 * or a dialog that is allowed to animate, `presence` splits "should be shown"
 * into two flags the markup can use:
 *
 * - `mounted`: render the element (true from the open until the exit ended);
 * - `shown`: the element is in its open state. It turns true one frame after
 *   the element appeared (so the CSS transition has a start to leave from)
 *   and false the moment `open` turns false.
 *
 * With `animate` false (a keyboard opened it) both flags follow `open`
 * directly: nothing moves.
 */

/** How long an element lingers for its exit transition. */
export const EXIT_MS = 100;

export interface Presence {
  readonly mounted: boolean;
  readonly shown: boolean;
}

export function presence(open: () => boolean, animate: () => boolean = () => true): Presence {
  let mounted = $state(open());
  let shown = $state(open() && !animate());

  $effect(() => {
    const isOpen = open();
    const moves = animate();
    if (isOpen) {
      mounted = true;
      if (!moves) {
        shown = true;
        return;
      }
      // One frame at the closed state, then the transition runs.
      const frame = requestAnimationFrame(() => (shown = true));
      return () => cancelAnimationFrame(frame);
    }
    shown = false;
    if (!moves) {
      mounted = false;
      return;
    }
    const timer = setTimeout(() => (mounted = false), EXIT_MS);
    return () => clearTimeout(timer);
  });

  return {
    get mounted() {
      return mounted;
    },
    get shown() {
      return shown;
    },
  };
}
