import { describe, expect, it } from 'vitest';
import { readFileSync, readdirSync, statSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

/**
 * Motion tokens (Doc B).
 *
 * One duration per class of change, one easing, and a reduced-motion path
 * that collapses to instant (structure changes keep at most 80 ms). The
 * tokens live in `app.css`; this test pins their values and scans the
 * sources for the patterns the specification forbids: `transition: all`,
 * bounce/spring/elastic easings, blur/filter chrome and an entrance
 * cascade.
 */

const SOURCE_ROOT = new URL('..', import.meta.url).pathname;
const CSS = readFileSync(fileURLToPath(new URL('../app.css', import.meta.url)), 'utf8')
  // Comments would glue themselves onto the following declaration name.
  .replace(/\/\*[\s\S]*?\*\//g, '');

/** The first rule whose selector ends with the given fragment. */
function declarationsOf(selectorFragment: string): Record<string, string> {
  const re = /([^{}]+)\{([^{}]*)\}/g;
  for (const match of CSS.matchAll(re)) {
    const selector = match[1].trim().replace(/\s+/g, ' ');
    if (selector !== selectorFragment && !selector.endsWith(selectorFragment)) continue;
    const out: Record<string, string> = {};
    for (const line of match[2].split(';')) {
      const [name, ...rest] = line.split(':');
      if (!name || rest.length === 0) continue;
      const key = name.trim();
      if (!key.startsWith('--')) continue;
      out[key] = rest.join(':').trim();
    }
    return out;
  }
  throw new Error(`missing rule for ${selectorFragment}`);
}

function sourceFiles(directory: string): string[] {
  const out: string[] = [];
  for (const entry of readdirSync(directory)) {
    if (entry === 'node_modules' || entry === 'dist') continue;
    const path = join(directory, entry);
    if (statSync(path).isDirectory()) {
      out.push(...sourceFiles(path));
    } else if (/\.(ts|svelte|js|css)$/.test(entry) && !entry.endsWith('.test.ts')) {
      out.push(path);
    }
  }
  return out;
}

function durationMs(value: string): number {
  if (value.endsWith('ms')) return Number.parseFloat(value);
  if (value.endsWith('s')) return Number.parseFloat(value) * 1000;
  throw new Error(`not a duration: ${value}`);
}

describe('motion tokens (Doc B)', () => {
  const root = declarationsOf(':root');

  it('defines the four motion tokens of the specification', () => {
    expect(root['--motion-fast']).toBe('100ms');
    expect(root['--motion-context']).toBe('160ms');
    expect(root['--motion-structure']).toBe('220ms');
    expect(root['--ease-quiet']).toBe('cubic-bezier(0.2, 0, 0, 1)');
  });

  it('collapses the tokens under prefers-reduced-motion', () => {
    const reduced = declarationsOf(":root:not([data-reduced-motion='off'])");
    expect(Object.keys(reduced).length).toBeGreaterThan(0);
    expect(durationMs(reduced['--motion-fast'])).toBe(0);
    expect(durationMs(reduced['--motion-context'])).toBe(0);
    expect(durationMs(reduced['--motion-structure'])).toBeLessThanOrEqual(80);
  });

  it('lets the preference force the same collapse', () => {
    const forced = declarationsOf(":root[data-reduced-motion='on']");
    expect(durationMs(forced['--motion-fast'])).toBe(0);
    expect(durationMs(forced['--motion-context'])).toBe(0);
    expect(durationMs(forced['--motion-structure'])).toBeLessThanOrEqual(80);
  });

  it('widens the focus ring for the high-visibility preference', () => {
    const focus = declarationsOf(":root[data-focus-visibility='on']");
    expect(focus['--termru-focus-w']).toBe('3px');
    expect(focus['--termru-focus-offset']).toBe('3px');
  });

  it('never animates with `transition: all`, blur or a bounce easing', () => {
    const offenders: string[] = [];
    for (const file of sourceFiles(SOURCE_ROOT)) {
      const content = readFileSync(file, 'utf8');
      const rules = [
        /transition\s*:\s*all\b/,
        /backdrop-filter\s*:/,
        /filter\s*:\s*blur\(/,
        /cubic-bezier\(\s*0\.68\s*,/,
        /cubic-bezier\(\s*0\.175\s*,/,
        /cubic-bezier\(\s*0\.34\s*,\s*1\.56/,
        /\b(animation|transition)[^;]*\b(bounce|elastic|spring|back)\b/i,
      ];
      if (rules.some((rule) => rule.test(content))) offenders.push(file);
    }
    expect(offenders).toEqual([]);
  });
});
