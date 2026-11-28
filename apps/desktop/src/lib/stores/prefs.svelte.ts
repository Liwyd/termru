import { api } from '../ipc';
import type { Preferences } from '../types';

/**
 * The line height of the terminal is a multiplier of xterm's cell height
 * (the glyph row of the font, about 1.3 times the font size). 1.3 gives
 * the 23 px rows of the design at 14 px.
 */
export const DEFAULT_LINE_HEIGHT = 1.3;

/**
 * The previous default (1.65). It was meant relative to the font size, but
 * xterm applies it to the cell height, which made the rows about 29 px.
 */
const LEGACY_LINE_HEIGHT = 1.65;

const DEFAULT_PREFS: Preferences = {
  // Dark is the default theme; "System" follows the operating system.
  theme: 'dark',
  terminal: {
    font_family: 'Geist Mono, monospace',
    font_size: 13,
    scrollback: 10_000,
    cursor_blink: true,
    cursor_style: 'block',
    copy_on_select: false,
    line_height: DEFAULT_LINE_HEIGHT,
    confirm_multiline_paste: true,
  },
  keybindings: {},
  reduced_motion: 'system',
  focus_visibility: false,
  routine_notices: true,
};

/** A fresh copy of the defaults ("Restore defaults" writes it). */
export function defaultPrefs(): Preferences {
  return structuredClone(DEFAULT_PREFS);
}

/**
 * Fills what an older record lacks and replaces the retired default line
 * height; any other value is the user's. The backend already fills the
 * missing fields (serde defaults); this keeps the frontend safe against a
 * store that answers with less.
 */
export function migratePrefs(loaded: Preferences): Preferences {
  const terminal = { ...DEFAULT_PREFS.terminal, ...loaded.terminal };
  const lineHeight = terminal.line_height;
  if (typeof lineHeight === 'number' && Math.abs(lineHeight - LEGACY_LINE_HEIGHT) < 1e-6) {
    terminal.line_height = DEFAULT_LINE_HEIGHT;
  }
  return { ...DEFAULT_PREFS, ...loaded, terminal };
}

/** Portable preferences (theme, terminal appearance). */
class PrefsStore {
  prefs = $state<Preferences>(structuredClone(DEFAULT_PREFS));

  /** The operating system prefers a dark theme (a plain mirror of the query). */
  #systemDark = false;
  /** Bumped when the system theme changes, so readers of `isDark` update. */
  #systemVersion = $state(0);
  #watching = false;

  async load(): Promise<void> {
    try {
      this.prefs = migratePrefs(await api.prefsGet());
    } catch {
      // Keep defaults when the store is unavailable; the UI still works.
    }
  }

  /**
   * Applies and persists the preferences. The new values are applied right
   * away; when the store refuses them the error is thrown, so the caller
   * can tell the user the settings were not saved.
   */
  async update(next: Preferences): Promise<void> {
    this.prefs = next;
    await api.prefsSet(next);
  }

  /** Starts following the system theme (once; needs `matchMedia`). */
  #watchSystemTheme(): void {
    if (this.#watching) return;
    if (typeof window === 'undefined' || typeof window.matchMedia !== 'function') return;
    this.#watching = true;
    const query = window.matchMedia('(prefers-color-scheme: dark)');
    this.#systemDark = query.matches === true;
    const onChange = (event: { matches: boolean }): void => {
      this.#systemDark = event.matches;
      this.#systemVersion += 1;
    };
    if (typeof query.addEventListener === 'function') query.addEventListener('change', onChange);
    else if (typeof query.addListener === 'function') query.addListener(onChange);
  }

  /** True when the effective theme is dark (reactive to the OS theme). */
  get isDark(): boolean {
    if (this.prefs.theme === 'dark') return true;
    if (this.prefs.theme === 'light') return false;
    this.#watchSystemTheme();
    // Read for the subscription: a change of the system theme re-runs readers.
    void this.#systemVersion;
    return this.#systemDark;
  }
}

export const prefs = new PrefsStore();
