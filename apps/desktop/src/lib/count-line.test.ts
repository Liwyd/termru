import { describe, expect, it } from 'vitest';
import { countLine, joinNames, spellCount } from './count-line';

describe('spellCount', () => {
  it('spells the small numbers the way the frame does', () => {
    expect(spellCount(0)).toBe('No');
    expect(spellCount(1)).toBe('One');
    expect(spellCount(5)).toBe('Five');
    expect(spellCount(10)).toBe('Ten');
  });

  it('keeps digits for the larger ones', () => {
    expect(spellCount(11)).toBe('11');
    expect(spellCount(1000)).toBe('1000');
  });
});

describe('countLine', () => {
  it('reads like the frame: hosts and sessions', () => {
    expect(countLine(5, 2)).toBe('Five saved connections. Two sessions running.');
    expect(countLine(5, 0)).toBe('Five saved connections. No sessions running.');
    expect(countLine(1, 1)).toBe('One saved connection. One session running.');
    expect(countLine(11, 12)).toBe('11 saved connections. 12 sessions running.');
  });
});

describe('joinNames', () => {
  it('joins one, two and many host names', () => {
    expect(joinNames([])).toBe('');
    expect(joinNames(['atlas-prod'])).toBe('atlas-prod');
    expect(joinNames(['atlas-prod', 'relay-edge'])).toBe('atlas-prod and relay-edge');
    expect(joinNames(['a', 'b', 'c'])).toBe('a, b and c');
  });
});
