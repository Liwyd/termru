/**
 * Decides whether an app shortcut may run for a key event.
 *
 * The window handler sees every key, including the ones typed into a field
 * or into a dialog. A shortcut must not steal those: Ctrl+Shift+N in a text
 * field belongs to the field, and a dialog that is open owns the keyboard.
 */

import type { CommandId } from './keybindings';

/** The command that opens the palette: the one chord that works in fields. */
const FIELD_SAFE: ReadonlySet<CommandId> = new Set<CommandId>(['palette.open']);

/** True when the event's target is a place the user types text. */
export function isEditableTarget(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  // The terminal types into a hidden textarea: it is the terminal, not a
  // form field, and its shortcuts are the app's.
  if (target.closest('.xterm')) return false;
  if (target.isContentEditable) return true;
  const tag = target.tagName;
  return tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT';
}

/** True while a modal dialog is open (it handles its own keys). */
export function modalOpen(root: ParentNode = document): boolean {
  return root.querySelector('[aria-modal="true"]') !== null;
}

/** Whether the shortcut of `commandId` may run for this event. */
export function shortcutAllowed(event: KeyboardEvent, commandId: CommandId): boolean {
  if (modalOpen()) return false;
  if (isEditableTarget(event.target) && !FIELD_SAFE.has(commandId)) return false;
  return true;
}
