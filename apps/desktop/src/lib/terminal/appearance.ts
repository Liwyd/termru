import { prefs } from '../stores/prefs.svelte';
import type { TerminalAppearance } from './create';

/** The look of the terminals, from the current preferences. */
export function appearanceFromPrefs(): TerminalAppearance {
  const terminal = prefs.prefs.terminal;
  return {
    fontFamily: terminal.font_family,
    fontSize: terminal.font_size,
    lineHeight: terminal.line_height,
    scrollback: terminal.scrollback,
    cursorBlink: terminal.cursor_blink,
    cursorStyle: terminal.cursor_style,
    copyOnSelect: terminal.copy_on_select,
    dark: prefs.isDark,
  };
}
