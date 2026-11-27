/**
 * Paste, with a question for the risky kind.
 *
 * Every way text reaches a terminal from the clipboard (the keyboard, the
 * browser's paste event, the context menu) goes through `requestPaste`. A
 * paste of several lines runs every line as soon as the shell sees it, so it
 * waits for a yes in a dialog; one line is sent at once.
 *
 * The question is a preference (`terminal.confirm_multiline_paste`, on by
 * default; Preferences has the switch). "Don't ask again" in the dialog turns
 * it off in the same place, so there is one setting and one way back, and the
 * dialog says where. A refused save is reported, and the question stays on.
 */

import { notify } from '../notify.svelte';
import { errorMessage } from '../stores/hosts.svelte';
import { prefs } from '../stores/prefs.svelte';
import { getTerminal } from './registry';

/** A paste of at least this many lines asks first. */
export const PASTE_CONFIRM_MIN_LINES = 2;

/** How many lines the dialog shows of what is about to be pasted. */
export const PASTE_PREVIEW_LINES = 6;

/**
 * Control characters other than tab and the line breaks. An escape sequence
 * in a paste could end the shell's bracketed-paste mode early and run what
 * follows as typed input.
 */
const CONTROL_CHARACTERS = /[\u0000-\u0008\u000b\u000c\u000e-\u001f\u007f]/g;

/** The paste without control characters. */
export function sanitizePaste(text: string): string {
  return text.replace(CONTROL_CHARACTERS, '');
}

/** The lines of a paste (a final line break does not start another line). */
export function pasteLines(text: string): string[] {
  const lines = text.replace(/\r\n?/g, '\n').split('\n');
  if (lines.length > 1 && lines[lines.length - 1] === '') lines.pop();
  return lines;
}

export function countLines(text: string): number {
  return text.length === 0 ? 0 : pasteLines(text).length;
}

/** True when a paste has to be confirmed. */
export function needsPasteConfirm(text: string): boolean {
  return countLines(text) >= PASTE_CONFIRM_MIN_LINES;
}

export interface PendingPaste {
  paneId: string;
  text: string;
  lines: number;
  /** The first lines, for the dialog. */
  preview: string[];
}

class PasteGate {
  /** The paste waiting for the user's answer. */
  pending = $state.raw<PendingPaste | null>(null);

  /** Sends a paste to a pane's terminal, or asks first. Returns what happened. */
  request(paneId: string, raw: string): 'sent' | 'asked' | 'empty' {
    const text = sanitizePaste(raw);
    if (text.length === 0) return 'empty';
    if (!prefs.prefs.terminal.confirm_multiline_paste || !needsPasteConfirm(text)) {
      this.#send(paneId, text);
      return 'sent';
    }
    const lines = pasteLines(text);
    this.pending = {
      paneId,
      text,
      lines: lines.length,
      preview: lines.slice(0, PASTE_PREVIEW_LINES),
    };
    return 'asked';
  }

  /** The user chose Paste. */
  confirm(dontAskAgain = false): void {
    const pending = this.pending;
    this.pending = null;
    if (!pending) return;
    if (dontAskAgain) void this.#stopAsking();
    this.#send(pending.paneId, pending.text);
  }

  /** Turns the question off in the preferences. */
  async #stopAsking(): Promise<void> {
    const before = prefs.prefs;
    try {
      await prefs.update({
        ...prefs.prefs,
        terminal: { ...prefs.prefs.terminal, confirm_multiline_paste: false },
      });
    } catch (err) {
      // Not saved: the question stays on, and the user is told why.
      prefs.prefs = before;
      notify({ key: 'prefs.save-failed', values: { detail: errorMessage(err) } });
    }
  }

  /** The user chose Cancel (or Escape): nothing is sent. */
  cancel(): void {
    this.pending = null;
  }

  #send(paneId: string, text: string): void {
    const handle = getTerminal(paneId);
    if (!handle || handle.disposed) return;
    handle.term.paste(text);
    handle.term.focus();
  }
}

export const pasteGate = new PasteGate();
