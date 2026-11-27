/**
 * What the terminal context menu (and the More menu) do to a pane's
 * terminal. The entries and the actions live together so the menu can only
 * offer what the action can do.
 */

import { acceleratorFor, acceleratorLabel, isMac } from '../keybindings';
import type { MenuEntry } from '../menu';
import { prefs } from '../stores/prefs.svelte';
import { sessions } from '../stores/sessions.svelte';
import { terminalUi } from '../stores/terminal-ui.svelte';
import { notify } from '../notify.svelte';
import { copyText, isOpenGesture, openLink, type OpenResult } from './links';
import { pasteGate } from './paste.svelte';
import { getTerminal } from './registry';

export type TerminalActionId =
  | 'copy'
  | 'paste'
  | 'select-all'
  | 'clear'
  | 'find'
  | 'split-right'
  | 'split-down'
  | 'open-link'
  | 'copy-link';

/** The shortcut text of copy, paste and find on this platform. */
function chord(pc: string, mac: string): string {
  return isMac() ? mac : pc;
}

/** The entries of the context menu of a pane's terminal. */
export function terminalMenuEntries(paneId: string, link: string | null): MenuEntry[] {
  const handle = getTerminal(paneId);
  const hasSelection = handle ? handle.term.hasSelection() : false;
  const overrides = prefs.prefs.keybindings;
  const entries: MenuEntry[] = [];
  if (link) {
    entries.push(
      { id: 'open-link', label: 'Open link' },
      { id: 'copy-link', label: 'Copy link address' },
    );
  }
  entries.push(
    {
      id: 'copy',
      label: 'Copy',
      hint: chord('Ctrl+Shift+C', '⌘C'),
      disabled: !hasSelection,
      reason: hasSelection ? undefined : 'Select some text first',
      separatorBefore: link !== null,
    },
    { id: 'paste', label: 'Paste', hint: chord('Ctrl+Shift+V', '⌘V') },
    { id: 'select-all', label: 'Select all' },
    { id: 'clear', label: 'Clear scrollback', separatorBefore: true },
    { id: 'find', label: 'Find…', hint: chord('Ctrl+Shift+F', '⌘F') },
    {
      id: 'split-right',
      label: 'Split right',
      hint: acceleratorLabel(acceleratorFor('pane.splitRight', overrides)),
      separatorBefore: true,
    },
    {
      id: 'split-down',
      label: 'Split down',
      hint: acceleratorLabel(acceleratorFor('pane.splitDown', overrides)),
    },
  );
  return entries;
}

/** Tells the user what a link action did (nothing for a plain open). */
export function reportOpen(result: OpenResult): void {
  switch (result) {
    case 'opened':
      return;
    case 'copied':
      notify({ key: 'link.copied-no-browser' });
      return;
    case 'refused':
      notify({ key: 'link.refused' });
      return;
    case 'failed':
      notify({ key: 'link.failed' });
  }
}

/** A click on a link in the output: Ctrl or Cmd opens it, a plain click selects. */
export async function activateLink(event: MouseEvent, uri: string): Promise<void> {
  if (!isOpenGesture(event)) return;
  reportOpen(await openLink(uri));
}

/** Runs a menu action against the terminal of a pane. */
export async function runTerminalAction(
  id: TerminalActionId,
  paneId: string,
  link: string | null = null,
): Promise<void> {
  const handle = getTerminal(paneId);
  const pane = sessions.paneById(paneId);
  switch (id) {
    case 'copy': {
      const text = handle?.term.getSelection() ?? '';
      if (text.length === 0) return;
      if (!(await copyText(text))) notify({ key: 'clipboard.copy-failed' });
      handle?.term.focus();
      return;
    }
    case 'paste': {
      try {
        const text = await navigator.clipboard.readText();
        pasteGate.request(paneId, text);
      } catch {
        notify({ key: 'clipboard.read-failed' });
      }
      return;
    }
    case 'select-all':
      handle?.term.selectAll();
      handle?.term.focus();
      return;
    case 'clear':
      handle?.term.clear();
      handle?.term.focus();
      return;
    case 'find':
      terminalUi.openSearch(paneId);
      return;
    case 'split-right':
    case 'split-down':
      if (!pane) return;
      sessions.focusPane(pane);
      sessions.splitActive(id === 'split-right' ? 'columns' : 'rows');
      return;
    case 'open-link':
      if (link) reportOpen(await openLink(link));
      return;
    case 'copy-link':
      if (link) {
        if (await copyText(link)) notify({ key: 'link.copied' });
        else notify({ key: 'clipboard.copy-failed' });
      }
      return;
  }
}
