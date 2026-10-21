import { describe, expect, it } from 'vitest';
import { readFileSync, readdirSync, statSync } from 'node:fs';
import { join } from 'node:path';

/**
 * Webview hardening: the frontend must never render untrusted markup.
 *
 * Svelte escapes every interpolation by default, so a host name that
 * contains `<script>` is shown as text. The only way to break that is a raw
 * HTML insertion (`{@html …}` in Svelte, `innerHTML` in the DOM). This test
 * reads the source of the frontend and fails when such an insertion appears.
 */
const SOURCE_ROOT = new URL('..', import.meta.url).pathname;

/** Every source file of the frontend. */
function sourceFiles(directory: string): string[] {
  const out: string[] = [];
  for (const entry of readdirSync(directory)) {
    if (entry === 'node_modules' || entry === 'dist') continue;
    const path = join(directory, entry);
    if (statSync(path).isDirectory()) {
      out.push(...sourceFiles(path));
    } else if (/\.(ts|svelte|js)$/.test(entry) && !entry.endsWith('.test.ts')) {
      // The test files themselves mention the patterns they look for.
      out.push(path);
    }
  }
  return out;
}

describe('webview hardening', () => {
  it('never inserts untrusted markup', () => {
    const offenders: string[] = [];
    for (const file of sourceFiles(SOURCE_ROOT)) {
      const content = readFileSync(file, 'utf8');
      if (content.includes('{@html') || content.includes('innerHTML')) {
        offenders.push(file);
      }
    }
    expect(offenders).toEqual([]);
  });

  it('does not load remote content into the webview', () => {
    const offenders: string[] = [];
    for (const file of sourceFiles(SOURCE_ROOT)) {
      const content = readFileSync(file, 'utf8');
      // A remote script, stylesheet or frame would defeat the CSP. A
      // placeholder in an input is harmless; a remote *resource* is not.
      if (/(?:src|href)\s*=\s*["']https?:\/\//.test(content)) {
        offenders.push(file);
      }
    }
    expect(offenders).toEqual([]);
  });
});
