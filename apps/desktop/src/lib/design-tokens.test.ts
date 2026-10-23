import { describe, expect, it } from 'vitest';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { DARK_TERMINAL_THEME, LIGHT_TERMINAL_THEME } from './terminal/theme';

/**
 * Design tokens (Doc A.02 colour, A.03 geometry, A.06 terminal).
 *
 * `app.css` is the only place UI colour lives, so the test reads it: a
 * drift in a hex value, a token that disappears from one theme or a
 * foreground/background pair that falls under the 4.5:1 floor of the
 * specification fails here instead of shipping.
 */

const CSS = readFileSync(fileURLToPath(new URL('../app.css', import.meta.url)), 'utf8')
  // Comments would glue themselves onto the following declaration name.
  .replace(/\/\*[\s\S]*?\*\//g, '');

/** The `:root`/`[data-…]` blocks, keyed by their selector. */
function blocks(): Map<string, Record<string, string>> {
  const out = new Map<string, Record<string, string>>();
  const re = /([^{}]+)\{([^{}]*)\}/g;
  for (const match of CSS.matchAll(re)) {
    const selector = match[1].trim().replace(/\s+/g, ' ');
    if (!selector.startsWith(':root') && !selector.includes(':root')) continue;
    const declarations: Record<string, string> = {};
    for (const line of match[2].split(';')) {
      const [name, ...rest] = line.split(':');
      if (!name || rest.length === 0) continue;
      const key = name.trim();
      if (!key.startsWith('--')) continue;
      declarations[key] = rest.join(':').trim();
    }
    if (Object.keys(declarations).length > 0) out.set(selector, declarations);
  }
  return out;
}

const ALL_BLOCKS = blocks();

function block(selector: string): Record<string, string> {
  // A rule inside a media query keeps the `@media … {` prefix in the key.
  for (const [key, value] of ALL_BLOCKS) {
    if (key === selector || key.endsWith(selector)) return value;
  }
  throw new Error(`missing token block: ${selector}`);
}

/** Light tokens (`:root`), with `var(--x)` references resolved. */
const LIGHT = block(':root');
const DARK = block(":root[data-theme='dark']");
const DARK_MEDIA = block(":root:not([data-theme='light'])");

function resolve(name: string, scope: Record<string, string>): string {
  const value = scope[name] ?? LIGHT[name];
  if (value === undefined) throw new Error(`unknown token ${name}`);
  const ref = /^var\((--[a-z0-9-]+)\)$/i.exec(value);
  return ref ? resolve(ref[1], scope) : value;
}

function channel(value: number): number {
  const c = value / 255;
  return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
}

function luminance(hex: string): number {
  const clean = hex.replace('#', '');
  const full = clean.length === 3 ? [...clean].map((c) => c + c).join('') : clean;
  const r = Number.parseInt(full.slice(0, 2), 16);
  const g = Number.parseInt(full.slice(2, 4), 16);
  const b = Number.parseInt(full.slice(4, 6), 16);
  return 0.2126 * channel(r) + 0.7152 * channel(g) + 0.0722 * channel(b);
}

function contrast(a: string, b: string): number {
  const [first, second] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return (first + 0.05) / (second + 0.05);
}

/** Foreground/background pairs that carry normal text (Doc A.02/A.06). */
const TEXT_PAIRS: [string, string][] = [
  ['--termru-fg', '--termru-surface'],
  ['--termru-fg', '--termru-canvas'],
  ['--termru-fg', '--termru-subtle'],
  ['--termru-fg', '--termru-terminal'],
  ['--termru-fg', '--termru-selection'],
  ['--termru-muted', '--termru-surface'],
  ['--termru-muted', '--termru-canvas'],
  ['--termru-muted', '--termru-subtle'],
  ['--termru-muted', '--termru-terminal'],
  ['--termru-accent', '--termru-surface'],
  ['--termru-accent', '--termru-canvas'],
  ['--termru-accent', '--termru-accent-soft'],
  ['--termru-success', '--termru-surface'],
  ['--termru-success', '--termru-canvas'],
  ['--termru-danger', '--termru-surface'],
  ['--termru-danger', '--termru-danger-soft'],
  ['--termru-danger', '--termru-canvas'],
  ['--termru-warning', '--termru-surface'],
  ['--termru-warning', '--termru-canvas'],
  ['--termru-warning', '--termru-warning-soft'],
  ['--termru-on-accent', '--termru-accent'],
  ['--termru-on-success', '--termru-success'],
  ['--termru-on-danger', '--termru-danger'],
];

/** The palette of Doc A.02, exactly as specified. */
const SPEC_LIGHT: Record<string, string> = {
  '--termru-surface': '#ffffff',
  '--termru-canvas': '#f4f5f2',
  '--termru-subtle': '#fafbf8',
  '--termru-terminal': '#fdfefb',
  '--termru-fg': '#202925',
  '--termru-muted': '#626d67',
  '--termru-faint': '#87928b',
  '--termru-border': '#dbe0da',
  '--termru-accent': '#17665e',
  '--termru-accent-soft': '#eaf3ef',
  '--termru-success': '#276440',
  '--termru-danger': '#a3333d',
  '--termru-danger-soft': '#fbf0ef',
  '--termru-warning': '#825713',
  '--termru-warning-soft': '#faf3e4',
  '--termru-selection': '#d7e9e0',
};

describe('design tokens', () => {
  it('keeps the light palette of the specification', () => {
    for (const [name, hex] of Object.entries(SPEC_LIGHT)) {
      expect(resolve(name, LIGHT).toLowerCase()).toBe(hex);
    }
  });

  it('defines the same palette slots in the dark theme', () => {
    const slots = Object.keys(SPEC_LIGHT);
    for (const name of slots) {
      expect(DARK[name], `dark token ${name}`).toBeDefined();
    }
    // The media-query set and the explicit override must not drift apart.
    expect(DARK_MEDIA).toEqual(DARK);
  });

  it('keeps every normal-text pair at 4.5:1 or better (light + dark)', () => {
    for (const scope of [LIGHT, DARK]) {
      for (const [fg, bg] of TEXT_PAIRS) {
        const ratio = contrast(resolve(fg, scope), resolve(bg, scope));
        expect(
          ratio,
          `${fg} on ${bg} = ${ratio.toFixed(2)}:1 in ${scope === LIGHT ? 'light' : 'dark'}`,
        ).toBeGreaterThanOrEqual(4.5);
      }
    }
  });

  it('keeps the non-essential faint ink visible', () => {
    // Doc A.02: `faint` is for non-essential text only, so it is held to a
    // lower floor than normal text — but it may never disappear.
    for (const scope of [LIGHT, DARK]) {
      const ratio = contrast(resolve('--termru-faint', scope), resolve('--termru-canvas', scope));
      expect(
        ratio,
        `faint on canvas in ${scope === LIGHT ? 'light' : 'dark'}`,
      ).toBeGreaterThanOrEqual(2.5);
    }
  });

  it('carries the geometry and type scale of the specification', () => {
    expect(LIGHT['--termru-space-1']).toBe('4px');
    expect(LIGHT['--termru-space-9']).toBe('56px');
    expect(LIGHT['--termru-control-h']).toBe('40px');
    expect(LIGHT['--termru-input-h']).toBe('42px');
    expect(LIGHT['--termru-radius-input']).toBe('4px');
    expect(LIGHT['--termru-radius-button']).toBe('6px');
    expect(LIGHT['--termru-radius-pane']).toBe('0px');
    expect(LIGHT['--termru-radius-lg']).toBe('16px');
    expect(LIGHT['--termru-border-w']).toBe('1px');
    expect(LIGHT['--termru-focus-w']).toBe('2px');
    expect(LIGHT['--termru-focus-offset']).toBe('2px');
    expect(LIGHT['--termru-titlebar-h']).toBe('36px');
    expect(LIGHT['--termru-nav-h']).toBe('64px');
    expect(LIGHT['--termru-text-title-size']).toBe('28px');
    expect(LIGHT['--termru-text-title-lh']).toBe('42px');
    expect(LIGHT['--termru-text-body-size']).toBe('14px');
    expect(LIGHT['--termru-text-body-lh']).toBe('21px');
    expect(LIGHT['--termru-text-terminal-size']).toBe('14px');
    expect(LIGHT['--termru-text-terminal-lh']).toBe('23.1px');
  });
});

describe('terminal theme slots (Doc A.06)', () => {
  it('matches the specification exactly on a light terminal', () => {
    expect(LIGHT_TERMINAL_THEME).toEqual({
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
    });
  });

  it('maps the same slots in the dark set', () => {
    expect(Object.keys(DARK_TERMINAL_THEME).sort()).toEqual(
      Object.keys(LIGHT_TERMINAL_THEME).sort(),
    );
  });

  it('keeps text readable over background and selection in both sets', () => {
    for (const theme of [LIGHT_TERMINAL_THEME, DARK_TERMINAL_THEME]) {
      expect(contrast(theme.foreground, theme.background)).toBeGreaterThanOrEqual(4.5);
      expect(contrast(theme.foreground, theme.selectionBackground)).toBeGreaterThanOrEqual(4.5);
      expect(contrast(theme.cursor, theme.background)).toBeGreaterThanOrEqual(3);
    }
    // The dark set is the same slots on a dark background.
    expect(luminance(DARK_TERMINAL_THEME.background)).toBeLessThan(
      luminance(LIGHT_TERMINAL_THEME.background),
    );
  });
});
