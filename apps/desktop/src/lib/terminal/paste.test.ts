import { beforeEach, describe, expect, it, vi } from 'vitest';
import {
  PASTE_CONFIRM_MIN_LINES,
  PASTE_PREVIEW_LINES,
  countLines,
  needsPasteConfirm,
  pasteGate,
  pasteLines,
  sanitizePaste,
} from './paste.svelte';
import { notices } from '../notify.svelte';
import { defaultPrefs, prefs } from '../stores/prefs.svelte';
import { answers, callsOf, failures, resetTauriMock } from '../test-support/tauri-core-mock';
import { registerTerminal, pruneTerminals, type TerminalHandle } from './registry';

vi.mock('@tauri-apps/api/core', () => import('../test-support/tauri-core-mock'));

function fakeHandle(paneId: string) {
  const pasted: string[] = [];
  const handle = {
    paneId,
    disposed: false,
    dispose: () => {},
    term: {
      paste: (text: string) => pasted.push(text),
      focus: () => {},
    },
  } as unknown as TerminalHandle;
  registerTerminal(handle);
  return { handle, pasted };
}

beforeEach(() => {
  pruneTerminals(new Set());
  pasteGate.pending = null;
  resetTauriMock();
  notices.reset();
  prefs.prefs = defaultPrefs();
});

describe('paste thresholds', () => {
  it('counts lines: a final line break does not start another one', () => {
    expect(countLines('')).toBe(0);
    expect(countLines('ls')).toBe(1);
    expect(countLines('ls\n')).toBe(1);
    expect(countLines('ls\npwd')).toBe(2);
    expect(countLines('ls\npwd\n')).toBe(2);
    expect(countLines('a\r\nb\r\nc')).toBe(3);
    expect(countLines('a\rb')).toBe(2);
    expect(countLines('\n\n')).toBe(2);
  });

  it('asks from two lines on, and not for one', () => {
    expect(PASTE_CONFIRM_MIN_LINES).toBe(2);
    expect(needsPasteConfirm('ls -la')).toBe(false);
    // One command with its newline is what a copied command looks like.
    expect(needsPasteConfirm('ls -la\n')).toBe(false);
    expect(needsPasteConfirm('ls\npwd')).toBe(true);
    expect(needsPasteConfirm('a\nb\nc\nd')).toBe(true);
  });

  it('splits lines on every kind of line break', () => {
    expect(pasteLines('a\r\nb\nc\rd')).toEqual(['a', 'b', 'c', 'd']);
  });

  it('takes control characters out, and keeps tabs and line breaks', () => {
    expect(sanitizePaste('echo \u001b[201~ hi\u0007')).toBe('echo [201~ hi');
    expect(sanitizePaste('a\tb\nc\r\nd')).toBe('a\tb\nc\r\nd');
    expect(sanitizePaste('x\u007fy\u0000z')).toBe('xyz');
  });
});

describe('the paste gate', () => {
  it('sends one line at once', () => {
    const { pasted } = fakeHandle('pane-1');
    expect(pasteGate.request('pane-1', 'ls -la')).toBe('sent');
    expect(pasted).toEqual(['ls -la']);
    expect(pasteGate.pending).toBeNull();
  });

  it('ignores an empty paste, and one that is only control characters', () => {
    const { pasted } = fakeHandle('pane-1');
    expect(pasteGate.request('pane-1', '')).toBe('empty');
    expect(pasteGate.request('pane-1', '\u001b\u0007')).toBe('empty');
    expect(pasted).toEqual([]);
  });

  it('holds several lines until the user answers, with the line count', () => {
    const { pasted } = fakeHandle('pane-1');
    const text = Array.from({ length: 10 }, (_, i) => `line ${i + 1}`).join('\n');
    expect(pasteGate.request('pane-1', text)).toBe('asked');
    expect(pasted).toEqual([]);
    expect(pasteGate.pending?.lines).toBe(10);
    expect(pasteGate.pending?.preview).toHaveLength(PASTE_PREVIEW_LINES);
    expect(pasteGate.pending?.preview[0]).toBe('line 1');

    pasteGate.confirm();
    expect(pasted).toEqual([text]);
    expect(pasteGate.pending).toBeNull();
    // Without the checkbox the next one asks again.
    expect(prefs.prefs.terminal.confirm_multiline_paste).toBe(true);
    expect(callsOf('prefs_set')).toHaveLength(0);
  });

  it('sends nothing when the user cancels', () => {
    const { pasted } = fakeHandle('pane-1');
    pasteGate.request('pane-1', 'a\nb');
    pasteGate.cancel();
    expect(pasted).toEqual([]);
    expect(pasteGate.pending).toBeNull();
    // Confirming nothing is harmless.
    pasteGate.confirm();
    expect(pasted).toEqual([]);
  });

  it('asks by default: the preference is on', () => {
    expect(prefs.prefs.terminal.confirm_multiline_paste).toBe(true);
    const { pasted } = fakeHandle('pane-1');
    expect(pasteGate.request('pane-1', 'a\nb')).toBe('asked');
    expect(pasted).toEqual([]);
  });

  it("turns the question off in the preferences with 'don't ask again'", async () => {
    const { pasted } = fakeHandle('pane-1');
    pasteGate.request('pane-1', 'a\nb');
    pasteGate.confirm(true);
    // The paste went through, and the next several-line paste does not ask.
    expect(prefs.prefs.terminal.confirm_multiline_paste).toBe(false);
    expect(pasteGate.request('pane-1', 'c\nd\ne')).toBe('sent');
    expect(pasted).toEqual(['a\nb', 'c\nd\ne']);
    expect(pasteGate.pending).toBeNull();
    // It is saved, not remembered for the session only.
    await vi.waitFor(() => expect(callsOf('prefs_set')).toHaveLength(1));
    const saved = (callsOf('prefs_set')[0].args as { prefs: typeof prefs.prefs }).prefs;
    expect(saved.terminal.confirm_multiline_paste).toBe(false);
    // Nothing else in the preferences changed with it.
    expect(saved.theme).toBe(defaultPrefs().theme);
    expect(saved.terminal.font_size).toBe(defaultPrefs().terminal.font_size);
  });

  it('does not ask when the preference is off from the start, and asks again when it is on', () => {
    const { pasted } = fakeHandle('pane-1');
    prefs.prefs = {
      ...defaultPrefs(),
      terminal: { ...defaultPrefs().terminal, confirm_multiline_paste: false },
    };
    expect(pasteGate.request('pane-1', 'a\nb')).toBe('sent');
    expect(pasted).toEqual(['a\nb']);
    prefs.prefs = defaultPrefs();
    expect(pasteGate.request('pane-1', 'c\nd')).toBe('asked');
  });

  it('keeps asking, and says why, when the preference could not be saved', async () => {
    failures.set('prefs_set', 'disk full');
    answers.set('prefs_get', defaultPrefs());
    const { pasted } = fakeHandle('pane-1');
    pasteGate.request('pane-1', 'a\nb');
    pasteGate.confirm(true);
    // This paste was confirmed, so it goes through.
    expect(pasted).toEqual(['a\nb']);
    await vi.waitFor(() => expect(notices.visible).toHaveLength(1));
    expect(notices.visible[0].kind).toBe('error');
    expect(notices.visible[0].description).toContain('disk full');
    expect(prefs.prefs.terminal.confirm_multiline_paste).toBe(true);
    expect(pasteGate.request('pane-1', 'c\nd')).toBe('asked');
  });

  it('drops a paste whose pane went away while it waited', () => {
    const { pasted } = fakeHandle('pane-1');
    pasteGate.request('pane-1', 'a\nb');
    pruneTerminals(new Set());
    pasteGate.confirm();
    expect(pasted).toEqual([]);
  });
});
