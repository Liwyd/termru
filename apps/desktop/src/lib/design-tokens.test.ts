import { describe, expect, it } from 'vitest';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { DARK_TERMINAL_THEME, LIGHT_TERMINAL_THEME } from './terminal/theme';

/**
 * Design tokens ("Obsidian": colour, geometry, type, terminal).
 *
 * `app.css` is the only place UI colour lives, so the test reads it: a
 * drift in a hex value, a token that disappears from one theme or a
 * foreground/background pair that falls under the 4.5:1 floor fails here
 * instead of shipping. Dark is the default (`:root`); the light set is
 * `:root[data-theme='light']` and, for "System" on a light operating
 * system, the same slots under `prefers-color-scheme: light`.
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
    if (!selector.includes(':root')) continue;
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
  for (const [key, value] of ALL_BLOCKS) {
    if (key === selector || key.endsWith(selector)) return value;
  }
  throw new Error(`missing token block: ${selector}`);
}

/** Dark is the default theme: the first `:root` block. */
const DARK = block(':root');
const LIGHT = block(":root[data-theme='light']");
const LIGHT_MEDIA = block(":root:not([data-theme='dark'])");

function resolve(name: string, scope: Record<string, string>): string {
  const value = scope[name] ?? DARK[name];
  if (value === undefined) throw new Error(`unknown token ${name}`);
  const ref = /^var\((--[a-z0-9-]+)\)$/i.exec(value);
  return ref ? resolve(ref[1], scope) : value;
}

function channel(value: number): number {
  const c = value / 255;
  return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
}

function rgb(hex: string): [number, number, number] {
  const clean = hex.replace('#', '');
  const full = clean.length === 3 ? [...clean].map((c) => c + c).join('') : clean;
  return [
    Number.parseInt(full.slice(0, 2), 16),
    Number.parseInt(full.slice(2, 4), 16),
    Number.parseInt(full.slice(4, 6), 16),
  ];
}

function luminance(hex: string): number {
  const [r, g, b] = rgb(hex);
  return 0.2126 * channel(r) + 0.7152 * channel(g) + 0.0722 * channel(b);
}

function contrast(a: string, b: string): number {
  const [first, second] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return (first + 0.05) / (second + 0.05);
}

/** A token as a hex colour; an `rgba()` fill is composited over `under`. */
function solid(name: string, scope: Record<string, string>, under?: string): string {
  const value = resolve(name, scope);
  const match = /^rgba\(\s*(\d+),\s*(\d+),\s*(\d+),\s*([\d.]+)\s*\)$/.exec(value);
  if (!match) return value;
  if (!under) throw new Error(`${name} is translucent: say what it sits on`);
  const alpha = Number(match[4]);
  const base = rgb(under);
  const mixed = [Number(match[1]), Number(match[2]), Number(match[3])].map((part, i) =>
    Math.round(part * alpha + base[i] * (1 - alpha)),
  );
  return `#${mixed.map((part) => part.toString(16).padStart(2, '0')).join('')}`;
}

/** Ink that carries text, and the surfaces it sits on. */
const INK = [
  '--termru-text',
  '--termru-text-2',
  '--termru-text-3',
  '--termru-accent-text',
  '--termru-live-text',
  '--termru-warn-text',
  '--termru-err-text',
];
const SURFACES = ['--termru-base', '--termru-side', '--termru-raised'];

/** Soft fills and the ink that is set on them (the fill sits on a surface). */
const SOFT: [string, string][] = [
  ['--termru-accent-text', '--termru-accent-soft'],
  ['--termru-live-text', '--termru-live-soft'],
  ['--termru-warn-text', '--termru-warn-soft'],
  ['--termru-err-text', '--termru-err-soft'],
];

describe('design tokens', () => {
  it('keeps the approved dark palette (the default theme)', () => {
    const spec: Record<string, string> = {
      '--termru-base': '#0e0f11',
      '--termru-side': '#121316',
      '--termru-raised': '#1a1b1f',
      '--termru-raised-2': '#212227',
      '--termru-terminal': '#0b0c0d',
      '--termru-text': '#ecebe7',
      '--termru-text-2': '#a3a199',
      '--termru-accent': '#cbb68c',
      '--termru-accent-ink': '#17140e',
      '--termru-live': '#74cfa2',
      '--termru-warn': '#e2a85c',
      '--termru-err': '#e7796f',
    };
    for (const [name, hex] of Object.entries(spec)) {
      expect(resolve(name, DARK).toLowerCase(), name).toBe(hex);
    }
    expect(DARK['--termru-line']).toBe('rgba(255, 255, 255, 0.055)');
    expect(DARK['--termru-accent-soft']).toBe('rgba(203, 182, 140, 0.11)');
  });

  it('keeps the approved light palette', () => {
    const spec: Record<string, string> = {
      '--termru-base': '#f8f7f4',
      '--termru-side': '#f0eee9',
      '--termru-raised': '#e8e5de',
      '--termru-raised-2': '#dfdbd2',
      '--termru-terminal': '#fcfbf8',
      '--termru-text': '#17161a',
      '--termru-text-2': '#5f5d57',
      '--termru-accent': '#8a7142',
      '--termru-live': '#2f9a67',
      '--termru-warn': '#b9741f',
      '--termru-err': '#c4473d',
    };
    for (const [name, hex] of Object.entries(spec)) {
      expect(resolve(name, LIGHT).toLowerCase(), name).toBe(hex);
    }
    expect(LIGHT['--termru-line']).toBe('rgba(20, 18, 12, 0.07)');
    expect(LIGHT['--termru-accent-soft']).toBe('rgba(138, 113, 66, 0.1)');
  });

  it('defines every colour slot in both themes, and the same set for "System"', () => {
    const colourSlots = Object.keys(DARK).filter((name) =>
      /^--termru-(base|side|raised|raised-2|terminal|text|text-2|text-3|line|border|accent|accent-text|accent-ink|accent-soft|live|live-text|live-soft|warn|warn-text|warn-soft|err|err-text|err-soft|on-err|shadow-float|scrim)$/.test(
        name,
      ),
    );
    expect(colourSlots.length).toBe(26);
    for (const name of colourSlots) {
      expect(LIGHT[name], `light token ${name}`).toBeDefined();
    }
    // The media-query set and the explicit override must not drift apart.
    expect(LIGHT_MEDIA).toEqual(LIGHT);
  });

  it('keeps every text pair at 4.5:1 or better, on every surface (dark + light)', () => {
    for (const [label, scope] of [
      ['dark', DARK],
      ['light', LIGHT],
    ] as const) {
      for (const ink of INK) {
        for (const surface of SURFACES) {
          const ratio = contrast(solid(ink, scope), solid(surface, scope));
          expect(
            ratio,
            `${ink} on ${surface} = ${ratio.toFixed(2)}:1 in ${label}`,
          ).toBeGreaterThanOrEqual(4.5);
        }
      }
      // The key caps and the terminal: text and secondary text.
      for (const ink of ['--termru-text', '--termru-text-2']) {
        for (const surface of ['--termru-raised-2', '--termru-terminal']) {
          const ratio = contrast(solid(ink, scope), solid(surface, scope));
          expect(
            ratio,
            `${ink} on ${surface} = ${ratio.toFixed(2)}:1 in ${label}`,
          ).toBeGreaterThanOrEqual(4.5);
        }
      }
    }
  });

  it('keeps the ink on every soft fill, over every surface, at 4.5:1 or better', () => {
    for (const [label, scope] of [
      ['dark', DARK],
      ['light', LIGHT],
    ] as const) {
      for (const [ink, fill] of SOFT) {
        for (const surface of SURFACES) {
          const under = solid(surface, scope);
          const ratio = contrast(solid(ink, scope), solid(fill, scope, under));
          expect(
            ratio,
            `${ink} on ${fill} over ${surface} = ${ratio.toFixed(2)}:1 in ${label}`,
          ).toBeGreaterThanOrEqual(4.5);
        }
      }
    }
  });

  it('keeps the text on a filled control at 4.5:1 or better', () => {
    for (const [label, scope] of [
      ['dark', DARK],
      ['light', LIGHT],
    ] as const) {
      for (const [ink, fill] of [
        ['--termru-accent-ink', '--termru-accent'],
        ['--termru-on-err', '--termru-err'],
      ]) {
        const ratio = contrast(solid(ink, scope), solid(fill, scope));
        expect(
          ratio,
          `${ink} on ${fill} = ${ratio.toFixed(2)}:1 in ${label}`,
        ).toBeGreaterThanOrEqual(4.5);
      }
    }
  });

  it('makes the control edge visible and the hairline faint', () => {
    for (const [label, scope] of [
      ['dark', DARK],
      ['light', LIGHT],
    ] as const) {
      const under = solid('--termru-base', scope);
      const edge = contrast(solid('--termru-border', scope, under), under);
      const hairline = contrast(solid('--termru-line', scope, under), under);
      expect(edge, `border in ${label}`).toBeGreaterThanOrEqual(1.4);
      expect(hairline, `line in ${label}`).toBeLessThan(edge);
    }
  });

  it('carries the geometry and type scale of the brief', () => {
    // Radii: controls 7-8, rows 10, panes and cards 12, panels 14.
    expect(DARK['--termru-radius-sm']).toBe('7px');
    expect(DARK['--termru-radius-button']).toBe('8px');
    expect(DARK['--termru-radius-input']).toBe('8px');
    expect(DARK['--termru-radius-row']).toBe('10px');
    expect(DARK['--termru-radius-pane']).toBe('12px');
    expect(DARK['--termru-radius-lg']).toBe('14px');
    // Spacing: 4 / 6 / 8 / 10 / 12 / 16 / 20 / 24 / 28.
    for (const step of [4, 6, 8, 10, 12, 16, 20, 24, 28]) {
      expect(DARK[`--termru-space-${step}`], `space ${step}`).toBe(`${step}px`);
    }
    // The frame of the app.
    expect(DARK['--termru-sidebar-w']).toBe('236px');
    expect(DARK['--termru-inspector-w']).toBe('340px');
    expect(DARK['--termru-header-h']).toBe('56px');
    expect(DARK['--termru-row-h']).toBe('52px');
    expect(DARK['--termru-status-h']).toBe('28px');
    expect(DARK['--termru-border-w']).toBe('1px');
    expect(DARK['--termru-focus-w']).toBe('2px');
    // Type: Geist 13/20, 12 secondary, 11 labels, page title 15/600,
    // inspector title 17/600, Geist Mono 12, terminal 13/20.
    expect(DARK['--termru-font-ui']).toMatch(/^'Geist',/);
    expect(DARK['--termru-font-mono']).toMatch(/^'Geist Mono',/);
    expect(DARK['--termru-text-body-size']).toBe('13px');
    expect(DARK['--termru-text-body-lh']).toBe('20px');
    expect(DARK['--termru-text-secondary-size']).toBe('12px');
    expect(DARK['--termru-text-meta-size']).toBe('11px');
    expect(DARK['--termru-text-meta-weight']).toBe('500');
    expect(DARK['--termru-text-title-size']).toBe('15px');
    expect(DARK['--termru-text-title-weight']).toBe('600');
    expect(DARK['--termru-text-heading-size']).toBe('17px');
    expect(DARK['--termru-text-heading-weight']).toBe('600');
    expect(DARK['--termru-text-mono-size']).toBe('12px');
    expect(DARK['--termru-text-terminal-size']).toBe('13px');
    expect(DARK['--termru-text-terminal-lh']).toBe('20px');
  });
});

describe('terminal themes', () => {
  const background = (scope: Record<string, string>) => resolve('--termru-terminal', scope);

  it('builds on the interface colours: surface, ink and the champagne cursor', () => {
    for (const [theme, scope] of [
      [DARK_TERMINAL_THEME, DARK],
      [LIGHT_TERMINAL_THEME, LIGHT],
    ] as const) {
      expect(theme.background.toLowerCase()).toBe(background(scope).toLowerCase());
      expect(theme.foreground.toLowerCase()).toBe(resolve('--termru-text', scope).toLowerCase());
      expect(theme.cursor.toLowerCase()).toBe(resolve('--termru-accent', scope).toLowerCase());
      expect(theme.cursorAccent.toLowerCase()).toBe(theme.background.toLowerCase());
    }
  });

  it('fills the 16 ANSI slots in both sets', () => {
    const slots = [
      'black',
      'red',
      'green',
      'yellow',
      'blue',
      'magenta',
      'cyan',
      'white',
      'brightBlack',
      'brightRed',
      'brightGreen',
      'brightYellow',
      'brightBlue',
      'brightMagenta',
      'brightCyan',
      'brightWhite',
    ];
    for (const theme of [LIGHT_TERMINAL_THEME, DARK_TERMINAL_THEME]) {
      for (const slot of slots) {
        expect((theme as Record<string, string>)[slot], slot).toMatch(/^#[0-9A-F]{6}$/);
      }
    }
    expect(Object.keys(DARK_TERMINAL_THEME).sort()).toEqual(
      Object.keys(LIGHT_TERMINAL_THEME).sort(),
    );
  });

  it('keeps every colour that is read at 4.5:1 on the terminal surface', () => {
    for (const theme of [LIGHT_TERMINAL_THEME, DARK_TERMINAL_THEME]) {
      for (const [slot, colour] of Object.entries(theme)) {
        if (['background', 'cursorAccent', 'selectionBackground'].includes(slot)) continue;
        // The dark set's black is a background colour (reverse video), not ink.
        if (theme === DARK_TERMINAL_THEME && slot === 'black') continue;
        if (slot === 'cursor') {
          expect(contrast(colour, theme.background), slot).toBeGreaterThanOrEqual(3);
          continue;
        }
        expect(contrast(colour, theme.background), `${slot} ${colour}`).toBeGreaterThanOrEqual(4.5);
      }
    }
  });

  it('keeps text readable over the selection in both sets', () => {
    for (const theme of [LIGHT_TERMINAL_THEME, DARK_TERMINAL_THEME]) {
      expect(contrast(theme.foreground, theme.selectionBackground)).toBeGreaterThanOrEqual(4.5);
    }
    // The dark set is the same slots on a dark background.
    expect(luminance(DARK_TERMINAL_THEME.background)).toBeLessThan(
      luminance(LIGHT_TERMINAL_THEME.background),
    );
  });
});
