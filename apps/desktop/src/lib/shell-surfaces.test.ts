import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

/**
 * The surfaces of the shell, as every frame draws them (frames 1, 4, 5, 8):
 * the window chrome (title bar) is the canvas, the navigation and the page are
 * the white surface, and the status bar is the secondary surface. The Hosts
 * page used to sit on the grey canvas while the work pages were white.
 */

const read = (path: string) => readFileSync(new URL(path, import.meta.url), 'utf8');

/** The declarations of the first rule whose selector is exactly `selector`. */
function rule(source: string, selector: string): string {
  const style = source.slice(source.indexOf('<style>'));
  const match = new RegExp(`(^|\\n)\\s*${selector.replace('.', '\\.')}\\s*\\{([^}]*)\\}`).exec(
    style,
  );
  if (!match) throw new Error(`no rule ${selector}`);
  return match[2];
}

describe('the shell surfaces', () => {
  const shell = read('components/AppShell.svelte');

  it('puts the page on the white surface, so Hosts matches the work pages', () => {
    expect(rule(shell, '.outlet')).toMatch(/background:\s*var\(--termru-surface\)/);
  });

  it('keeps the window chrome on the canvas', () => {
    expect(rule(shell, '.titlebar')).toMatch(/background:\s*var\(--termru-canvas\)/);
  });

  it('draws the status bar on the secondary surface', () => {
    const bar = read('components/StatusBar.svelte');
    expect(rule(bar, '.statusbar')).toMatch(/background:\s*var\(--termru-subtle\)/);
  });

  it('gives the Hosts page the same field fill as the work pages', () => {
    const hosts = read('pages/HostsPage.svelte');
    // No field of the page is filled with the canvas any more.
    expect(hosts).not.toMatch(/background:\s*var\(--termru-bg\)/);
    expect(hosts).toContain('var(--wp-field, var(--termru-surface))');
    const css = read('../app.css');
    expect(css).toMatch(/\.outlet\s*\{\s*--wp-field:\s*var\(--termru-surface\)/);
  });
});
