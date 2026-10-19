import { describe, expect, it } from 'vitest';
import { computeFit } from './fit';

describe('computeFit', () => {
  it('divides container by cell metrics', () => {
    expect(computeFit(800, 600, 10, 20)).toEqual({ cols: 80, rows: 30 });
  });

  it('floors fractional cells', () => {
    expect(computeFit(85, 45, 10, 20)).toEqual({ cols: 8, rows: 2 });
  });

  it('enforces minimums', () => {
    expect(computeFit(5, 5, 10, 20)).toEqual({ cols: 2, rows: 1 });
  });

  it('returns zeros for non-positive inputs', () => {
    expect(computeFit(0, 600, 10, 20)).toEqual({ cols: 0, rows: 0 });
    expect(computeFit(800, -1, 10, 20)).toEqual({ cols: 0, rows: 0 });
  });
});
