import { beforeEach, describe, expect, it } from 'vitest';
import { palette, type PaletteEntry } from './palette.svelte';

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
});
