// @vitest-environment jsdom
import { flushSync, mount, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { DEFAULT_LINE_HEIGHT, defaultPrefs, migratePrefs, prefs } from './prefs.svelte';
import IsDarkProbe from '../test-support/IsDarkProbe.svelte';
import { answers, callsOf, failures, resetTauriMock } from '../test-support/tauri-core-mock';
import type { Preferences } from '../types';

vi.mock('@tauri-apps/api/core', () => import('../test-support/tauri-core-mock'));

function stored(lineHeight: number, theme: Preferences['theme'] = 'system'): Preferences {
  return {
    theme,
    terminal: {
      font_family: 'IBM Plex Mono, monospace',
      font_size: 14,
      scrollback: 10_000,
      cursor_blink: true,
      cursor_style: 'block',
      copy_on_select: false,
      confirm_multiline_paste: true,
      line_height: lineHeight,
    },
    keybindings: {},
    reduced_motion: 'system',
    focus_visibility: false,
    routine_notices: true,
  };
}

beforeEach(() => {
  resetTauriMock();
});

afterEach(() => {
  vi.unstubAllGlobals();
});

describe('terminal line height', () => {
  it('migrates the retired 1.65 default (rows of 29 px) to the new multiplier', () => {
    const migrated = migratePrefs(stored(1.65));
    expect(migrated.terminal.line_height).toBe(DEFAULT_LINE_HEIGHT);
    // The default gives the 23 px rows of the design at 14 px (cell ~18 px).
    expect(DEFAULT_LINE_HEIGHT).toBeGreaterThan(1.2);
    expect(DEFAULT_LINE_HEIGHT).toBeLessThan(1.4);
  });

  it('keeps a value the user chose', () => {
    expect(migratePrefs(stored(1.1)).terminal.line_height).toBe(1.1);
    expect(migratePrefs(stored(2)).terminal.line_height).toBe(2);
  });

  it('applies the migration when the preferences load', async () => {
    answers.set('prefs_get', stored(1.65));
    await prefs.load();
    expect(prefs.prefs.terminal.line_height).toBe(DEFAULT_LINE_HEIGHT);
  });
});

describe('the paste question', () => {
  it('is on by default', () => {
    expect(defaultPrefs().terminal.confirm_multiline_paste).toBe(true);
    expect(prefs.prefs.terminal.confirm_multiline_paste).toBe(true);
  });

  it('fills it in for a record from before it existed', () => {
    const old = stored(1.3) as unknown as { terminal: Record<string, unknown> };
    delete old.terminal.confirm_multiline_paste;
    expect(migratePrefs(old as unknown as Preferences).terminal.confirm_multiline_paste).toBe(true);
  });

  it('keeps the value the user chose, through a load and a save', async () => {
    const off = stored(1.3);
    off.terminal.confirm_multiline_paste = false;
    answers.set('prefs_get', off);
    await prefs.load();
    expect(prefs.prefs.terminal.confirm_multiline_paste).toBe(false);
    await prefs.update({ ...prefs.prefs, theme: 'dark' });
    const sent = callsOf('prefs_set').at(-1)?.args as { prefs: Preferences };
    expect(sent.prefs.terminal.confirm_multiline_paste).toBe(false);
  });

  it('round-trips every preference field, the new ones included', async () => {
    const full: Preferences = {
      theme: 'dark',
      terminal: {
        font_family: 'Fira Code, monospace',
        font_size: 16,
        scrollback: 50_000,
        cursor_blink: false,
        cursor_style: 'underline',
        copy_on_select: true,
        line_height: 1.5,
        confirm_multiline_paste: false,
      },
      keybindings: { 'tab.close': '', 'palette.open': 'Ctrl+Shift+F8' },
      reduced_motion: 'on',
      focus_visibility: true,
      routine_notices: false,
    };
    answers.set('prefs_get', structuredClone(full));
    await prefs.load();
    expect(prefs.prefs).toEqual(full);
    await prefs.update(prefs.prefs);
    expect((callsOf('prefs_set').at(-1)?.args as { prefs: Preferences }).prefs).toEqual(full);
  });
});

describe('saving preferences', () => {
  it('applies them at once and persists them', async () => {
    await prefs.update(stored(1.3, 'dark'));
    expect(prefs.prefs.theme).toBe('dark');
    expect(callsOf('prefs_set')).toHaveLength(1);
  });

  it('reports a store that refuses them, instead of swallowing the error', async () => {
    failures.set('prefs_set', 'disk full');
    await expect(prefs.update(stored(1.3, 'light'))).rejects.toThrow('disk full');
  });
});

describe('the system theme', () => {
  /** A fake `matchMedia` whose answer a test can change. */
  function fakeMedia(initial: boolean) {
    let listener: ((event: { matches: boolean }) => void) | null = null;
    const query = {
      matches: initial,
      addEventListener: (_type: string, handler: (event: { matches: boolean }) => void) => {
        listener = handler;
      },
      removeEventListener: () => {},
    };
    vi.stubGlobal('matchMedia', () => query);
    return {
      change(matches: boolean) {
        query.matches = matches;
        listener?.({ matches });
      },
    };
  }

  it('follows the operating system while the theme is "system"', async () => {
    const media = fakeMedia(false);
    await prefs.update(stored(1.3, 'system'));
    // A component reads `isDark` like the terminals do.
    const target = document.createElement('div');
    document.body.appendChild(target);
    const probe = mount(IsDarkProbe, { target });
    flushSync();
    expect(target.textContent).toBe('light');

    media.change(true);
    flushSync();
    expect(target.textContent).toBe('dark');

    media.change(false);
    flushSync();
    expect(target.textContent).toBe('light');
    unmount(probe);
    target.remove();
  });

  it('ignores the system for an explicit theme', async () => {
    fakeMedia(true);
    await prefs.update(stored(1.3, 'light'));
    expect(prefs.isDark).toBe(false);
    await prefs.update(stored(1.3, 'dark'));
    expect(prefs.isDark).toBe(true);
  });
});
