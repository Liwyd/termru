/**
 * A tiny focus trap for dialogs (`use:focusTrap`).
 *
 * * Tab and Shift+Tab cycle inside the dialog;
 * * the first field is focused when the dialog opens (an element marked
 *   `data-autofocus`, else `initialFocus`, else the first tabbable one);
 * * Escape calls `onescape`;
 * * when the dialog goes away the focus returns to the element that had it
 *   before — unless something else took the focus meanwhile (a dialog that
 *   opened from this one's action must keep it).
 */

export interface FocusTrapOptions {
  /** Called when Escape is pressed inside the dialog. */
  onescape?: () => void;
  /** A selector for the element that takes the focus first. */
  initialFocus?: string;
  /** Turns the trap off (a dialog shown in page mode). Default: on. */
  enabled?: boolean;
}

const TABBABLE = [
  'a[href]',
  'button:not([disabled])',
  'input:not([disabled]):not([type="hidden"])',
  'select:not([disabled])',
  'textarea:not([disabled])',
  '[tabindex]:not([tabindex="-1"])',
].join(',');

/** The elements Tab can reach inside `root`, in DOM order. */
export function tabbables(root: HTMLElement): HTMLElement[] {
  return Array.from(root.querySelectorAll<HTMLElement>(TABBABLE)).filter((element) => {
    if (element.hidden || element.closest('[hidden], [inert]')) return false;
    const style = getComputedStyle(element);
    return style.display !== 'none' && style.visibility !== 'hidden';
  });
}

export function focusTrap(node: HTMLElement, initial: FocusTrapOptions = {}) {
  let options = initial;
  const opener = document.activeElement instanceof HTMLElement ? document.activeElement : null;

  function focusFirst(): void {
    if (options.enabled === false) return;
    const marked = node.querySelector<HTMLElement>('[data-autofocus]');
    const chosen =
      marked ??
      (options.initialFocus ? node.querySelector<HTMLElement>(options.initialFocus) : null) ??
      tabbables(node)[0] ??
      null;
    if (chosen) {
      chosen.focus();
    } else {
      // A dialog without a field: the dialog itself holds the focus.
      if (!node.hasAttribute('tabindex')) node.setAttribute('tabindex', '-1');
      node.focus();
    }
  }

  function onKeyDown(event: KeyboardEvent): void {
    if (options.enabled === false) return;
    if (event.key === 'Escape' && options.onescape) {
      event.preventDefault();
      event.stopPropagation();
      options.onescape();
      return;
    }
    if (event.key !== 'Tab') return;
    const items = tabbables(node);
    if (items.length === 0) {
      event.preventDefault();
      node.focus();
      return;
    }
    const first = items[0];
    const last = items[items.length - 1];
    const active = document.activeElement;
    if (event.shiftKey && (active === first || !node.contains(active))) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && (active === last || !node.contains(active))) {
      event.preventDefault();
      first.focus();
    }
  }

  node.addEventListener('keydown', onKeyDown);
  // The element is in the document, but a child may bind after the action.
  queueMicrotask(focusFirst);

  return {
    update(next: FocusTrapOptions = {}) {
      options = next;
    },
    destroy() {
      node.removeEventListener('keydown', onKeyDown);
      if (options.enabled === false || !opener || !opener.isConnected) return;
      const active = document.activeElement;
      // Restore only when the focus was lost or is still in the dialog.
      if (!active || active === document.body || node.contains(active)) opener.focus();
    },
  };
}
