/**
 * The unsaved-changes guard.
 *
 * A page that holds work the user has not saved (the host editor) registers
 * a check with `hold`. Anything that would throw that work away — a page
 * change, the window closing — first goes through `ask`: while something is
 * unsaved it opens a question instead of proceeding, and the page shows it
 * as an in-app dialog (never `window.confirm`). "Keep editing" closes the
 * question, "Discard" runs what the user wanted in the first place.
 */

/** What the user was about to do. */
export type LeaveKind = 'leave' | 'close';

export interface LeaveQuestion {
  kind: LeaveKind;
  /** What the user wanted; runs when they agree to lose the changes. */
  proceed: () => void;
}

export class UnsavedGuard {
  #check = $state.raw<(() => boolean) | null>(null);

  /** The question on screen, if any. */
  question = $state.raw<LeaveQuestion | null>(null);

  /** Whether a page holds changes that are not saved. */
  get dirty(): boolean {
    return this.#check?.() ?? false;
  }

  /** A page declares its unsaved state; the returned function withdraws it. */
  hold(check: () => boolean): () => void {
    this.#check = check;
    return () => {
      if (this.#check !== check) return;
      this.#check = null;
      this.question = null;
    };
  }

  /**
   * Asks before work is lost. Returns true when a question is now on screen
   * (the caller stops and waits for the answer); false when nothing is
   * unsaved and the caller simply goes on.
   */
  ask(proceed: () => void, kind: LeaveKind = 'leave'): boolean {
    if (!this.dirty) return false;
    this.question = { kind, proceed };
    return true;
  }

  /** "Discard changes": the work is dropped and the user's action happens. */
  discard(): void {
    const question = this.question;
    this.question = null;
    question?.proceed();
  }

  /** "Keep editing". */
  keep(): void {
    this.question = null;
  }
}

export const unsaved = new UnsavedGuard();
