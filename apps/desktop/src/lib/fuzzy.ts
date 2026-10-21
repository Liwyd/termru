/** Fuzzy matching for the command palette and host search. */

export interface FuzzyMatch {
  /** Higher is better. */
  score: number;
  /** Indices of the matched characters in the haystack. */
  indices: number[];
}

/**
 * Subsequence match with a bonus for consecutive characters and for matches
 * at word boundaries. `null` means "no match".
 */
export function fuzzyMatch(needle: string, haystack: string): FuzzyMatch | null {
  if (needle.length === 0) return { score: 0, indices: [] };
  const lowerNeedle = needle.toLowerCase();
  const lowerHaystack = haystack.toLowerCase();

  let score = 0;
  let haystackIndex = 0;
  let previousIndex = -1;
  const indices: number[] = [];

  for (const char of lowerNeedle) {
    const found = lowerHaystack.indexOf(char, haystackIndex);
    if (found === -1) return null;
    indices.push(found);
    // Consecutive characters are a stronger signal.
    score += 10;
    if (found === previousIndex + 1) score += 15;
    // Matches at the start or after a separator are stronger.
    if (found === 0 || ' -_.:/'.includes(lowerHaystack[found - 1] ?? '')) score += 8;
    if (lowerHaystack[found] === char) score += 2;
    previousIndex = found;
    haystackIndex = found + 1;
  }

  // Shorter haystacks that match the whole needle rank higher.
  score -= Math.max(0, haystack.length - needle.length) / 10;
  return { score, indices };
}

/** Ranks items by fuzzy match score (descending), keeping the input order
 * for ties. */
export function fuzzyRank<T>(needle: string, items: T[], toText: (item: T) => string): T[] {
  if (needle.trim().length === 0) return [...items];
  return items
    .map((item, index) => {
      const match = fuzzyMatch(needle, toText(item));
      return match ? { item, index, score: match.score } : null;
    })
    .filter((entry): entry is { item: T; index: number; score: number } => entry !== null)
    .sort((a, b) => (b.score === a.score ? a.index - b.index : b.score - a.score))
    .map((entry) => entry.item);
}
