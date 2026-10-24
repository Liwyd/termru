/**
 * Source guard: destructive confirmations use the in-app ContextConfirm
 * dialog (Doc A.05), never the browser's `window.confirm` / `alert`.
 */

import { readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';

function sourceFiles(dir: string): string[] {
  const out: string[] = [];
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const full = join(dir, entry.name);
    if (entry.isDirectory()) {
      if (entry.name === 'node_modules') continue;
      out.push(...sourceFiles(full));
    } else if (/\.(ts|svelte)$/.test(entry.name) && !entry.name.endsWith('.test.ts')) {
      out.push(full);
    }
  }
  return out;
}

describe('no native dialogs', () => {
  it('never calls window.confirm or alert in app source', () => {
    const offenders = sourceFiles(join(__dirname, '..'))
      .map((file) => ({ file, text: readFileSync(file, 'utf8') }))
      .filter(({ text }) => /window\.confirm\s*\(|(^|[^.\w])alert\s*\(/m.test(text))
      .map(({ file }) => file);
    expect(offenders).toEqual([]);
  });
});
