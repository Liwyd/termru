import { describe, expect, it } from 'vitest';
import { fuzzyMatch, fuzzyRank } from './fuzzy';

describe('fuzzyMatch', () => {
  it('matches a subsequence', () => {
    const match = fuzzyMatch('cmd', 'Command palette');
    expect(match).not.toBeNull();
    expect(match?.indices).toHaveLength(3);
  });

  it('returns null when the needle is not a subsequence', () => {
    expect(fuzzyMatch('zzz', 'Command palette')).toBeNull();
  });

  it('scores consecutive matches higher', () => {
    const consecutive = fuzzyMatch('tab', 'tab close');
    const scattered = fuzzyMatch('tab', 'terminal and background');
    expect(consecutive?.score).toBeGreaterThan(scattered?.score ?? 0);
  });

  it('prefers word-boundary matches', () => {
    const boundary = fuzzyMatch('cp', 'Command palette');
    const inner = fuzzyMatch('cp', 'escape profile');
    expect(boundary?.score).toBeGreaterThan(inner?.score ?? 0);
  });
});

describe('fuzzyRank', () => {
  it('keeps the input order for an empty query', () => {
    const items = ['b', 'a', 'c'];
    expect(fuzzyRank('', items, (item) => item)).toEqual(items);
  });

  it('ranks the best match first', () => {
    const items = ['Command palette', 'Close tab', 'New tab'];
    const ranked = fuzzyRank('tab', items, (item) => item);
    expect(ranked[0]).toBe('New tab');
    expect(ranked).not.toContain('Command palette');
  });
});
