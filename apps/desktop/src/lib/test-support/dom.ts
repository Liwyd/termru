import { flushSync } from 'svelte';

/** A click, as a user's: it bubbles (the app uses delegated handlers). */
export function click(element: Element): void {
  element.dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true }));
  flushSync();
}

/** Types a value into a field. */
export function type(element: HTMLInputElement | HTMLSelectElement, value: string): void {
  element.value = value;
  element.dispatchEvent(new Event('input', { bubbles: true }));
  element.dispatchEvent(new Event('change', { bubbles: true }));
  flushSync();
}

/** A key press on an element (down only: that is what the app listens to). */
export function press(
  element: EventTarget,
  key: string,
  init: KeyboardEventInit = {},
): KeyboardEvent {
  const event = new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true, ...init });
  element.dispatchEvent(event);
  flushSync();
  return event;
}

/**
 * What a button says: its text without the decorations that are not part of its
 * name (a key cap such as the Enter hint, a count, text for screen readers only).
 */
function labelOf(button: HTMLButtonElement): string {
  const clone = button.cloneNode(true) as HTMLButtonElement;
  clone.querySelectorAll('kbd, .n, .sr-only').forEach((decoration) => decoration.remove());
  return clone.textContent?.trim() ?? '';
}

/** The button with exactly this text (decorations aside). */
export function findButton(root: ParentNode, label: string): HTMLButtonElement {
  const button = Array.from(root.querySelectorAll('button')).find(
    (candidate) => labelOf(candidate) === label,
  );
  if (!button) throw new Error(`button "${label}" not found`);
  return button;
}

/** The button with this accessible name (aria-label). */
export function findByLabel(root: ParentNode, label: string): HTMLElement {
  const element = root.querySelector<HTMLElement>(`[aria-label="${label}"]`);
  if (!element) throw new Error(`no element labelled "${label}"`);
  return element;
}

/** A mounted target in the document. */
export function newTarget(): HTMLElement {
  const target = document.createElement('div');
  document.body.appendChild(target);
  return target;
}

/** The layout APIs jsdom lacks, as the components expect them. */
export function stubBrowserApis(): void {
  (globalThis as { ResizeObserver?: unknown }).ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  };
  (globalThis as { matchMedia?: unknown }).matchMedia = () => ({
    matches: false,
    addEventListener() {},
    removeEventListener() {},
  });
}
