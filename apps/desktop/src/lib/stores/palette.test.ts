import { beforeEach, describe, expect, it } from 'vitest';
import { MAX_RESULTS, palette, type PaletteEntry } from './palette.svelte';

function entry(id: string, title: string, group = 'Hosts'): PaletteEntry {
  return { id, title, group, run: () => {} };
}

beforeEach(() => palette.hide());

describe('command palette', () => {
  it('starts the choice over at the best match when the query changes', () => {
    palette.show([entry('a', 'Alpha'), entry('b', 'Beta'), entry('c', 'Gamma')]);
    palette.move(1);
    palette.move(1);
    expect(palette.selected).toBe(2);

    palette.query = 'be';
    expect(palette.selected).toBe(0);
    expect(palette.results[0]?.id).toBe('b');
  });

  it('runs the highlighted entry of the filtered list', () => {
    const ran: string[] = [];
    palette.show([
      { ...entry('a', 'Alpha'), run: () => ran.push('a') },
      { ...entry('b', 'Beta'), run: () => ran.push('b') },
    ]);
    palette.query = 'beta';
    palette.choose();
    expect(ran).toEqual(['b']);
    expect(palette.open).toBe(false);
  });

  it('resets on show and on hide', () => {
    palette.show([entry('a', 'Alpha'), entry('b', 'Beta')]);
    palette.move(1);
    palette.query = 'a';
    palette.hide();
    expect(palette.query).toBe('');
    expect(palette.selected).toBe(0);
    palette.show([entry('a', 'Alpha')]);
    expect(palette.query).toBe('');
    expect(palette.selected).toBe(0);
  });

  it('ranks a match in the title above one that only matches letter by letter', () => {
    palette.show([
      entry('prev', 'Print every file', 'Tabs'),
      entry('prefs', 'Preferences: About', 'Preferences'),
      entry('go', 'Go to Preferences', 'Go to'),
    ]);
    palette.query = 'pref';
    expect(palette.results.map((e) => e.id)).toEqual(['prefs', 'go', 'prev']);
  });

  it('leaves the long tail out of the start list, and caps the list', () => {
    const many = Array.from({ length: 300 }, (_, i) => ({
      ...entry(`h${i}`, `Host ${i}`),
      searchOnly: i >= 5,
    }));
    palette.show(many);
    expect(palette.results).toHaveLength(5);
    palette.query = 'host';
    expect(palette.results).toHaveLength(MAX_RESULTS);
  });

  it('wraps the choice around the ends of the list', () => {
    palette.show([entry('a', 'Alpha'), entry('b', 'Beta')]);
    palette.move(-1);
    expect(palette.selected).toBe(1);
    palette.move(1);
    expect(palette.selected).toBe(0);
  });
});
