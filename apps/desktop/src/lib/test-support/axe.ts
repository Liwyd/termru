/**
 * axe-core for the component tests (jsdom).
 *
 * jsdom has no layout engine and no canvas, so the rules that measure pixels
 * cannot be evaluated here and are switched off, not skipped silently:
 *
 *  - `color-contrast`, `color-contrast-enhanced`: need the rendered colours.
 *    Contrast is covered another way: `design-tokens.test.ts` computes the
 *    WCAG ratio of every text/background pair of both palettes.
 *  - `target-size`: needs element rectangles (jsdom reports all zeros).
 *  - `scrollable-region-focusable`: needs scroll sizes.
 *  - `link-in-text-block`: needs computed colours.
 *
 * Everything else of WCAG 2.0 / 2.1 / 2.2 A and AA, and axe's best-practice
 * rules, runs.
 */

import axe, { type AxeResults, type Result, type RunOptions } from 'axe-core';

export const RULES_JSDOM_CANNOT_EVALUATE = [
  'color-contrast',
  'color-contrast-enhanced',
  'target-size',
  'scrollable-region-focusable',
  'link-in-text-block',
] as const;

const OPTIONS: RunOptions = {
  runOnly: {
    type: 'tag',
    values: ['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa', 'wcag22aa', 'best-practice'],
  },
  rules: Object.fromEntries(
    RULES_JSDOM_CANNOT_EVALUATE.map((id) => [id, { enabled: false }] as const),
  ),
};

export interface AxeFailure {
  rule: string;
  impact: string;
  help: string;
  nodes: string[];
}

function describe(result: Result): AxeFailure {
  return {
    rule: result.id,
    impact: result.impact ?? 'unknown',
    help: result.help,
    nodes: result.nodes.map((node) => `${node.target.join(' ')}  ${node.html.slice(0, 140)}`),
  };
}

/** Runs axe on the whole document (the page as a screen reader meets it). */
export async function axeViolations(
  context: Element | Document = document,
  options: RunOptions = OPTIONS,
): Promise<AxeFailure[]> {
  const results: AxeResults = await axe.run(context, options);
  return results.violations.map(describe);
}

/** A readable message for `expect(...).toEqual([])`. */
export function formatViolations(failures: AxeFailure[]): string {
  return failures
    .map(
      (failure) =>
        `${failure.rule} (${failure.impact}): ${failure.help}\n  ${failure.nodes.join('\n  ')}`,
    )
    .join('\n');
}
