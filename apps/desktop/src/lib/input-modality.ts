/**
 * How the user last gave input: a key or a pointer. A surface that appears
 * because of a key (a confirmation after Ctrl+W, the paste question after
 * Ctrl+V) does not animate: a shortcut is used as often as it is remembered,
 * and a fade makes it feel slow. One opened with the pointer plays its short
 * entrance.
 *
 * Capture listeners on the window, installed once when the module loads.
 */

let keyboard = false;

if (typeof window !== 'undefined') {
  window.addEventListener(
    'keydown',
    (event) => {
      // A modifier on its own is not an action (Ctrl held for a click).
      if (!['Control', 'Shift', 'Alt', 'Meta'].includes(event.key)) keyboard = true;
    },
    true,
  );
  window.addEventListener('pointerdown', () => (keyboard = false), true);
}

/** The latest input was a key press. */
export function lastInputWasKeyboard(): boolean {
  return keyboard;
}
