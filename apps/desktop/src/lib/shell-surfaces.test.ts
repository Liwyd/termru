import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

/**
 * The surfaces of the shell ("Obsidian"): the page is the base, the sidebar,
 * the inspector, the window chrome and the status bar are the side surface, and
 * what is raised (a selected row, a field, a menu) is one step above.
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

  it('puts the page on the base surface, so every page sits on the same ground', () => {
    expect(rule(shell, '.outlet')).toMatch(/background:\s*var\(--termru-base\)/);
  });

  it('keeps the window chrome on the sidebar surface', () => {
    expect(rule(shell, '.titlebar')).toMatch(/background:\s*var\(--termru-side\)/);
  });

  it('draws the sidebar and the status bar on the side surface', () => {
    expect(rule(read('components/Sidebar.svelte'), '.side')).toMatch(
      /background:\s*var\(--termru-side\)/,
    );
    expect(rule(read('components/StatusBar.svelte'), '.status')).toMatch(
      /background:\s*var\(--termru-side\)/,
    );
  });

  it('raises the selected row, and sets the inspector on the side surface', () => {
    expect(rule(read('components/HostTable.svelte'), '.row.selected')).toMatch(
      /background:\s*var\(--termru-raised\)/,
    );
    expect(rule(read('components/HostInspector.svelte'), '.insp')).toMatch(
      /background:\s*var\(--termru-side\)/,
    );
  });

  it('gives every field of a page and of a dialog its own fill', () => {
    const css = read('../app.css');
    // A page: a well one step above the base. A dialog is raised itself, so
    // its fields sit one step down.
    expect(css).toMatch(/--termru-field:\s*var\(--termru-raised\)/);
    expect(css).toMatch(/\.sx-dialog\s*\{\s*--termru-field:\s*var\(--termru-base\)/);
  });
});
