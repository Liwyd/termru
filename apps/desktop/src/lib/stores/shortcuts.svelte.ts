import {
  commandTitle,
  conflictFor,
  effectiveBindings,
  eventToAccelerator,
  type CommandId,
} from '../keybindings';

/**
 * The recorder of a new keybinding.
 *
 * While a command is being recorded, the keyboard belongs to the recorder:
 * the global shortcuts must not fire, or the command the user is rebinding
 * would run instead of being captured.
 */
class ShortcutStore {
  /** The command being rebound, or null. */
  recording = $state<CommandId | null>(null);
  /** Why the last attempt was refused, or null. */
  conflict = $state<string | null>(null);

  /** Starts recording a new accelerator for a command. */
  start(commandId: CommandId): void {
    this.recording = commandId;
    this.conflict = null;
  }

  /** Stops the recording. */
  cancel(): void {
    this.recording = null;
    this.conflict = null;
  }

  /**
   * Feeds a key event into the recorder.
   *
   * Returns true when the event was consumed, so the caller can stop it
   * from reaching the application. A combination that another command
   * already uses is refused, with the reason in `conflict`.
   */
  record(event: KeyboardEvent, draft: Record<string, string>): boolean {
    if (this.recording === null) return false;

    // Escape leaves the recorder without changing anything.
    if (event.key === 'Escape') {
      this.cancel();
      return true;
    }

    const accelerator = eventToAccelerator(event);
    // A modifier pressed on its own: wait for the key.
    if (accelerator === null) return true;

    const other = conflictFor(accelerator, effectiveBindings(draft), this.recording);
    if (other !== null) {
      this.conflict = `Already used by “${commandTitle(other)}”.`;
      return true;
    }

    this.conflict = null;
    // The draft is updated in place: it is the reactive state of the dialog.
    draft[this.recording] = accelerator;
    this.recording = null;
    return true;
  }
}

export const shortcuts = new ShortcutStore();
