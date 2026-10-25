import { describe, expect, it } from 'vitest';
import { formatLastSession } from './last-session';

// A fixed "now": 24 Oct 2026, 15:00 local time.
const NOW = new Date(2026, 9, 24, 15, 0, 0);

describe('formatLastSession', () => {
  it('is the em dash when the host was never used', () => {
    expect(formatLastSession(null, NOW)).toBe('—');
  });

  it('is the em dash for an unreadable timestamp', () => {
    expect(formatLastSession(Number.NaN, NOW)).toBe('—');
  });

  it('shows the time for today', () => {
    expect(formatLastSession(new Date(2026, 9, 24, 9, 18).getTime(), NOW)).toBe('Today, 09:18');
    expect(formatLastSession(new Date(2026, 9, 24, 0, 5).getTime(), NOW)).toBe('Today, 00:05');
  });

  it('shows the day name for yesterday', () => {
    expect(formatLastSession(new Date(2026, 9, 23, 23, 59).getTime(), NOW)).toBe('Yesterday');
  });

  it('shows the date within the year', () => {
    expect(formatLastSession(new Date(2026, 9, 18, 8, 0).getTime(), NOW)).toBe('18 Oct');
    expect(formatLastSession(new Date(2026, 0, 3, 8, 0).getTime(), NOW)).toBe('3 Jan');
  });

  it('adds the year outside of it', () => {
    expect(formatLastSession(new Date(2025, 10, 5, 8, 0).getTime(), NOW)).toBe('5 Nov 2025');
  });
});
