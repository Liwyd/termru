import type { ITheme } from '@xterm/xterm';

/**
 * Terminal colour slots (Doc A.06).
 *
 * The light set is the specification exactly: ANSI 0–7 / 8–15 as listed,
 * background `#FDFEFB`, selection `#D7E9E0`, cursor `#17665E`. Bright
 * colours deliberately keep dark hues on a light terminal, so a `ls` in
 * bright white stays readable instead of vanishing into the background.
 *
 * The dark set is the same slot mapping with the hues opened up for a dark
 * background (a light-terminal red is unreadable on `#101413`).
 */
export const LIGHT_TERMINAL_THEME = {
  background: '#FDFEFB',
  foreground: '#202925',
  cursor: '#17665E',
  cursorAccent: '#FDFEFB',
  selectionBackground: '#D7E9E0',
  black: '#202925',
  red: '#A3333D',
  green: '#276440',
  yellow: '#825713',
  blue: '#315D99',
  magenta: '#78518C',
  cyan: '#17665E',
  white: '#626D67',
  brightBlack: '#626D67',
  brightRed: '#A3333D',
  brightGreen: '#276440',
  brightYellow: '#825713',
  brightBlue: '#315D99',
  brightMagenta: '#78518C',
  brightCyan: '#17665E',
  brightWhite: '#202925',
} as const satisfies ITheme;

export const DARK_TERMINAL_THEME = {
  background: '#101413',
  foreground: '#EEF1EE',
  cursor: '#63C2B1',
  cursorAccent: '#101413',
  selectionBackground: '#26443C',
  black: '#242A27',
  red: '#F08F8A',
  green: '#5FBC8D',
  yellow: '#D9A552',
  blue: '#7FA4DD',
  magenta: '#B394D1',
  cyan: '#63C2B1',
  white: '#A7B0AB',
  brightBlack: '#A7B0AB',
  brightRed: '#F08F8A',
  brightGreen: '#5FBC8D',
  brightYellow: '#D9A552',
  brightBlue: '#7FA4DD',
  brightMagenta: '#B394D1',
  brightCyan: '#63C2B1',
  brightWhite: '#EEF1EE',
} as const satisfies ITheme;

/** xterm colour theme for the current app theme. */
export function terminalTheme(dark: boolean): ITheme {
  return dark ? DARK_TERMINAL_THEME : LIGHT_TERMINAL_THEME;
}
