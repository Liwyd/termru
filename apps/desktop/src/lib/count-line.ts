/**
 * The live count line of the hosts header (frame 1): small numbers are
 * spelled the way the frame spells them ("Five saved connections. Two
 * sessions running."), larger ones stay digits.
 */

const SMALL = ['No', 'One', 'Two', 'Three', 'Four', 'Five', 'Six', 'Seven', 'Eight', 'Nine', 'Ten'];

/** `3` → `Three` (up to ten), `11` → `11`. */
export function spellCount(count: number): string {
  const word = SMALL[count];
  return word ?? String(count);
}

/** Joins host names the way the banner does: `a`, `a and b`, `a, b and c`. */
export function joinNames(names: string[]): string {
  if (names.length <= 1) return names.join('');
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
