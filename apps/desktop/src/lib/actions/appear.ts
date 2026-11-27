/**
 * A block that arrives while the user is looking at the page (the
 * running-sessions banner, the edit form of a rail) fades in and rises 4 px:
 * `opacity` and `transform` over `--appear-duration` (160 ms) with
 * `--ease-out`. It is an enter only: removing the block is instant.
 *
 * It does not move when
 *  - the block is part of the page's first paint (`enabled: false`: use
 *    `afterFirstPaint()` below), because a page load does not animate;
 *  - the last thing the user did was press a key, because nothing a shortcut
 *    or the keyboard starts is animated (the palette, tab and route switches
 *    of the same rule).
 *
 * The motion is a CSS transition (`.appear` / `.appear-in` in `app.css`), so
 * it needs no library, only moves compositor properties and is interruptible.
 * Under reduced motion the shift is 0 and the fade is 80 ms.
 */

type Modality = 'keyboard' | 'pointer';

let modality: Modality = 'pointer';

if (typeof window !== 'undefined') {
  window.addEventListener('keydown', () => (modality = 'keyboard'), { capture: true });
  window.addEventListener('pointerdown', () => (modality = 'pointer'), { capture: true });
}

/** What the user last used: a key, or a pointer. (Exported for the tests.) */
export function lastModality(): Modality {
  return modality;
}

export interface AppearOptions {
  /** False plays nothing (the block belongs to the first paint of its page). */
  enabled?: boolean;
}

export function appear(node: HTMLElement, options: AppearOptions = {}): { destroy(): void } | void {
  if (options.enabled === false || modality === 'keyboard') return;
  node.classList.add('appear');
  // One frame at the hidden state, then the transition has somewhere to go from.
  const frame = requestAnimationFrame(() => node.classList.add('appear-in'));
  return { destroy: () => cancelAnimationFrame(frame) };
}
