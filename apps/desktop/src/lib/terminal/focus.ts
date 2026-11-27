/**
 * Moving the keyboard to a pane without taking it from the user.
 *
 * A pane that becomes the active one of the visible tab should be ready to
 * type into. But the user may be in a field, a dialog or a menu, or on a
 * different page of the app: then the focus stays where it is.
 */

/** What keeps the focus it has. */
const KEEPS_FOCUS =
  'input, select, textarea:not(.xterm-helper-textarea), [contenteditable="true"], [role="dialog"], [role="alertdialog"], [role="menu"], [aria-modal="true"]';

/** True when the keyboard is nowhere in particular, or on the Sessions page. */
export function focusIsFree(active: Element | null = document.activeElement): boolean {
  if (!active || active === document.body) return true;
  return active.closest('[data-sx-page]') !== null && active.closest(KEEPS_FOCUS) === null;
}
