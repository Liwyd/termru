import type { ITheme } from '@xterm/xterm';

/**
 * Terminal colour slots ("Obsidian" palette).
 *
 * Both sets are built on the colours of the interface: the terminal surface,
 * the ink and the champagne cursor come from `app.css` (`--termru-terminal`,
 * `--termru-text`, `--termru-accent`), and the hues are the ones the design
 * draws terminal output with (green prompts, blue directories, amber
 * warnings, violet hashes). The remaining slots are filled to a full ANSI
 * palette.
 *
 * Every colour that is meant to be read (everything but the dark set's
 * `black`, which is a background colour there) holds 4.5:1 on the terminal
 * surface; `design-tokens.test.ts` checks it. The light set keeps its bright
 * colours as dark as the normal ones, so a `ls` in bright white stays
 * readable instead of vanishing into the background.
 */
export const DARK_TERMINAL_THEME = {
  background: '#0B0C0D',
  foreground: '#ECEBE7',
  cursor: '#CBB68C',
  cursorAccent: '#0B0C0D',
  selectionBackground: '#3A3424',
  black: '#2A2B30',
  red: '#E7796F',
  green: '#8FD6A8',
  yellow: '#E2B46C',
  blue: '#8FB5E6',
  magenta: '#C3A2E6',
  cyan: '#84CFD0',
  white: '#A3A199',
  brightBlack: '#7D7C76',
  brightRed: '#F19088',
  brightGreen: '#A6E3BB',
  brightYellow: '#EFC787',
  brightBlue: '#A8C8EE',
  brightMagenta: '#D3B8EE',
  brightCyan: '#A0DFE0',
  brightWhite: '#ECEBE7',
} as const satisfies ITheme;

export const LIGHT_TERMINAL_THEME = {
  background: '#FCFBF8',
  foreground: '#17161A',
  cursor: '#8A7142',
  cursorAccent: '#FCFBF8',
  selectionBackground: '#E6DCC4',
  black: '#17161A',
  red: '#C4473D',
  green: '#2D8052',
  yellow: '#9A6A14',
  blue: '#2F63A8',
  magenta: '#7A4FB0',
  cyan: '#1C7474',
  white: '#5F5D57',
  brightBlack: '#68665F',
  brightRed: '#C4473D',
  brightGreen: '#2D8052',
  brightYellow: '#9A6A14',
  brightBlue: '#2F63A8',
  brightMagenta: '#7A4FB0',
  brightCyan: '#1C7474',
  brightWhite: '#17161A',
} as const satisfies ITheme;

/** xterm colour theme for the current app theme. */
export function terminalTheme(dark: boolean): ITheme {
  return dark ? DARK_TERMINAL_THEME : LIGHT_TERMINAL_THEME;
}

/**
 * The colours of find-in-terminal matches (`#RRGGBB`, as the search addon
 * requires). Warm yellow reads as "found" on both backgrounds without
 * borrowing the accent or the selection colour.
 */
export function searchDecorations(dark: boolean) {
  return dark
    ? {
        matchBackground: '#4D4220',
        matchBorder: '#6B5A2A',
        matchOverviewRuler: '#D9A552',
        activeMatchBackground: '#8F6F1C',
        activeMatchBorder: '#D9A552',
        activeMatchColorOverviewRuler: '#F2C94C',
      }
    : {
        matchBackground: '#EBDDAA',
        matchBorder: '#D8C27A',
        matchOverviewRuler: '#C9A64A',
        activeMatchBackground: '#E3B341',
        activeMatchBorder: '#B8892B',
        activeMatchColorOverviewRuler: '#B8892B',
      };
}
