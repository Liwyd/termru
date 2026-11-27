/**
 * The live count line of the hosts header (frame 1): small numbers are
 * spelled the way the frame spells them ("Five saved connections. Two
 * sessions running."), larger ones stay digits.
 */

const SMALL = ['No', 'One', 'Two', 'Three', 'Four', 'Five', 'Six', 'Seven', 'Eight', 'Nine', 'Ten'];

const NUMBER = new Intl.NumberFormat('en-US');

/** `10005` → `10,005`. Every count on screen goes through this. */
export function formatCount(count: number): string {
  return NUMBER.format(count);
}

/** `3` → `Three` (up to ten), `11` → `11`, `10005` → `10,005`. */
export function spellCount(count: number): string {
  const word = SMALL[count];
  return word ?? formatCount(count);
}

/**
 * Joins host names the way the banner does: `a`, `a and b`, `a, b and c`.
 * Past `max` names it says how many more there are (`a, b and 4 more`): a
 * sentence about thirty sessions is not a list of thirty names.
 */
export function joinNames(names: string[], max = 3): string {
  if (names.length <= 1) return names.join('');
  if (names.length > max) {
    const shown = names.slice(0, max - 1);
    return `${shown.join(', ')} and ${formatCount(names.length - shown.length)} more`;
  }
  if (names.length === 2) return `${names[0]} and ${names[1]}`;
  return `${names.slice(0, -1).join(', ')} and ${names[names.length - 1]}`;
}

/** `Five saved connections. Two sessions running.` */
export function countLine(total: number, running: number): string {
  const hosts = `${spellCount(total)} saved connection${total === 1 ? '' : 's'}.`;
  const sessions =
    running === 0
      ? 'No sessions running.'
      : `${spellCount(running)} session${running === 1 ? '' : 's'} running.`;
  return `${hosts} ${sessions}`;
}
