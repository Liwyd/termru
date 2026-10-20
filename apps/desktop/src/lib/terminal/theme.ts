import type { ITheme } from '@xterm/xterm';

/** xterm colour themes matching the app's design tokens. */
export function terminalTheme(dark: boolean): ITheme {
  if (dark) {
    return {
      background: '#0f172a',
      foreground: '#e2e8f0',
      cursor: '#38bdf8',
      cursorAccent: '#0f172a',
      selectionBackground: '#33415580',
      black: '#1e293b',
      red: '#f87171',
      green: '#4ade80',
      yellow: '#fbbf24',
      blue: '#60a5fa',
      magenta: '#c084fc',
      cyan: '#22d3ee',
      white: '#cbd5e1',
      brightBlack: '#64748b',
    };
  }
  return {
    background: '#f8fafc',
    foreground: '#0f172a',
    cursor: '#0284c7',
    cursorAccent: '#f8fafc',
    selectionBackground: '#cbd5e180',
    black: '#334155',
    red: '#dc2626',
    green: '#16a34a',
    yellow: '#ca8a04',
    blue: '#2563eb',
    magenta: '#9333ea',
    cyan: '#0891b2',
    white: '#e2e8f0',
    brightBlack: '#94a3b8',
  };
}
