import { api } from '../ipc';
import type { Preferences } from '../types';

const DEFAULT_PREFS: Preferences = {
  theme: 'system',
  terminal: {
    font_family: 'monospace',
    font_size: 14,
    scrollback: 10_000,
    cursor_blink: true,
    cursor_style: 'block',
    copy_on_select: false,
  },
  keybindings: {},
};

/** Portable preferences (theme, terminal appearance). */
class PrefsStore {
  prefs = $state<Preferences>(structuredClone(DEFAULT_PREFS));

  async load(): Promise<void> {
    try {
      this.prefs = await api.prefsGet();
    } catch {
      // Keep defaults when the store is unavailable; the UI still works.
    }
  }

  async update(next: Preferences): Promise<void> {
    this.prefs = next;
    try {
      await api.prefsSet(next);
    } catch {
      // Persisting preferences is best-effort for now.
    }
  }

  get isDark(): boolean {
    if (this.prefs.theme === 'dark') return true;
    if (this.prefs.theme === 'light') return false;
    return window.matchMedia('(prefers-color-scheme: dark)').matches;
  }
}

export const prefs = new PrefsStore();
