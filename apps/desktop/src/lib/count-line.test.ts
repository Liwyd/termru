import { describe, expect, it } from 'vitest';
import { countLine, formatCount, joinNames, spellCount } from './count-line';

describe('spellCount', () => {
  it('spells the small numbers the way the frame does', () => {
    expect(spellCount(0)).toBe('No');
    expect(spellCount(1)).toBe('One');
    expect(spellCount(5)).toBe('Five');
    expect(spellCount(10)).toBe('Ten');
  });

  it('keeps digits for the larger ones', () => {
    expect(spellCount(11)).toBe('11');
    expect(spellCount(1000)).toBe('1,000');
    expect(spellCount(10005)).toBe('10,005');
  });
});

describe('formatCount', () => {
  it('puts a separator in every count over 999', () => {
    expect(formatCount(0)).toBe('0');
    expect(formatCount(999)).toBe('999');
    expect(formatCount(1284)).toBe('1,284');
    expect(formatCount(10005)).toBe('10,005');
    expect(formatCount(1_000_000)).toBe('1,000,000');
  });
});

describe('countLine', () => {
  it('reads like the frame: hosts and sessions', () => {
    expect(countLine(5, 2)).toBe('Five saved connections. Two sessions running.');
    expect(countLine(5, 0)).toBe('Five saved connections. No sessions running.');
    expect(countLine(1, 1)).toBe('One saved connection. One session running.');
    expect(countLine(11, 12)).toBe('11 saved connections. 12 sessions running.');
    expect(countLine(10005, 2)).toBe('10,005 saved connections. Two sessions running.');
  });
});

describe('joinNames', () => {
  it('joins one, two and many host names', () => {
    expect(joinNames([])).toBe('');
    expect(joinNames(['atlas-prod'])).toBe('atlas-prod');
    expect(joinNames(['atlas-prod', 'relay-edge'])).toBe('atlas-prod and relay-edge');
    expect(joinNames(['a', 'b', 'c'])).toBe('a, b and c');
  });

  it('says how many more instead of listing every name', () => {
    expect(joinNames(['a', 'b', 'c', 'd'])).toBe('a, b and 2 more');
    const many = Array.from({ length: 30 }, (_, i) => `host-${i}`);
    expect(joinNames(many)).toBe('host-0, host-1 and 28 more');
    expect(joinNames(Array.from({ length: 1_003 }, (_, i) => `h${i}`))).toBe(
      'h0, h1 and 1,001 more',
    );
    // The limit is the caller's.
    expect(joinNames(['a', 'b', 'c', 'd'], 5)).toBe('a, b, c and d');
  });
});
