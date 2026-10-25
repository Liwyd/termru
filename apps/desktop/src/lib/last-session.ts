/**
 * Formats the "Last session" cell of a host row (frame 1).
 *
 * `null` (never connected) is the em dash the frame uses; a live session
 * is decided by the caller (the "Connected" pill), not here.
 */

const MONTHS = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec'];

const DAY_MS = 86_400_000;

/** Local midnight of a date. */
function startOfDay(date: Date): number {
  return new Date(date.getFullYear(), date.getMonth(), date.getDate()).getTime();
}

/**
 * `Today, 09:18` / `Yesterday` / `18 Oct` / `18 Oct 2025` / `—`.
 *
 * The month names are fixed English labels: the format of the frame is a
 * product decision, not the locale of the machine.
 */
export function formatLastSession(atMs: number | null, now: Date = new Date()): string {
  if (atMs === null) return '—';
  const at = new Date(atMs);
  if (Number.isNaN(at.getTime())) return '—';

  const days = Math.round((startOfDay(now) - startOfDay(at)) / DAY_MS);
  if (days <= 0) {
    const hh = String(at.getHours()).padStart(2, '0');
    const mm = String(at.getMinutes()).padStart(2, '0');
    return `Today, ${hh}:${mm}`;
  }
  if (days === 1) return 'Yesterday';

  const stamp = `${at.getDate()} ${MONTHS[at.getMonth()]}`;
  return at.getFullYear() === now.getFullYear() ? stamp : `${stamp} ${at.getFullYear()}`;
}
