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

describe('the Sessions page surfaces', () => {
  it('draws a pane as a 12 px card on the terminal colour, clipped to its corners', () => {
    const pane = rule(read('components/PaneView.svelte'), '.pane');
    expect(pane).toMatch(/background:\s*var\(--termru-terminal\)/);
    expect(pane).toMatch(/border-radius:\s*var\(--termru-radius-pane\)/);
    expect(pane).toMatch(/overflow:\s*hidden/);
  });

  it('marks the focused pane with a 1 px champagne line inside its edge, not a rule on top', () => {
    const source = read('components/PaneView.svelte');
    const ring = rule(source, '.pane.focused::before');
    expect(ring).toMatch(
      /box-shadow:\s*inset 0 0 0 1px color-mix\(in srgb, var\(--termru-accent\)/,
    );
    expect(ring).toMatch(/pointer-events:\s*none/);
    expect(rule(source, '.pane.focused .path')).toMatch(/color:\s*var\(--termru-accent-text\)/);
    expect(source).not.toMatch(/border-top/);
  });

  it('sizes the pane header and footer from the tokens (34 px and 28 px)', () => {
    const source = read('components/PaneView.svelte');
    expect(rule(source, '.head')).toMatch(/height:\s*var\(--termru-pane-head-h\)/);
    expect(rule(source, '.foot')).toMatch(/height:\s*var\(--termru-pane-foot-h\)/);
  });

  it('makes the split divider the 6 px gap between panes, with a bar that shows on demand', () => {
    const source = read('components/SplitView.svelte');
    const divider = rule(source, '.divider');
    expect(divider).toMatch(/flex:\s*0 0 var\(--termru-divider-w\)/);
    expect(divider).toMatch(/background:\s*transparent/);
    expect(rule(source, '.divider::after')).toMatch(/opacity:\s*0/);
    // Focus and the held divider show it in full (a key moves the divider too).
    expect(source).toMatch(
      /\.divider:focus-visible::after,\s*\.divider\.dragging::after \{\s*opacity:\s*1/,
    );
  });

  it('sets the panes on the page colour, 10 px from the window edge', () => {
    const page = read('pages/SessionsPage.svelte');
    expect(rule(page, '.stage')).toMatch(
      /padding:\s*0 var\(--termru-space-10\) var\(--termru-space-10\)/,
    );
    expect(rule(page, '.topbar')).toMatch(/height:\s*var\(--termru-tabstrip-h\)/);
  });

  it('raises the open tab one step, with no rule', () => {
    const tab = read('components/TabBar.svelte');
    expect(rule(tab, '.tab.active')).toMatch(/background:\s*var\(--termru-raised\)/);
    expect(tab).not.toMatch(/border-top/);
  });

  it('tints the forwards chip with the soft champagne, and trouble with warn and error', () => {
    const chip = read('components/ForwardsIndicator.svelte');
    expect(rule(chip, '.chip')).toMatch(/background:\s*var\(--termru-accent-soft\)/);
    expect(rule(chip, '.chip.bad')).toMatch(/background:\s*var\(--termru-err-soft\)/);
    expect(rule(chip, '.chip.warn')).toMatch(/background:\s*var\(--termru-warn-soft\)/);
  });
});

describe('the surfaces of the overlays', () => {
  it('raises every floating layer, with the one soft shadow', () => {
    for (const [path, selector] of [
      ['components/PopMenu.svelte', '.menu'],
      ['components/SessionDialog.svelte', '.dialog'],
      ['components/Toasts.svelte', '.notice'],
      ['components/TerminalSearch.svelte', '.find'],
    ] as const) {
      const declarations = rule(read(path), selector);
      expect(declarations, path).toMatch(/background:\s*var\(--termru-raised\)/);
      expect(declarations, path).toMatch(/box-shadow:\s*var\(--termru-shadow-float\)/);
    }
  });

  it('rounds menus, notices and the find bar to 12 px and dialogs to 14 px', () => {
    for (const [path, selector] of [
      ['components/PopMenu.svelte', '.menu'],
      ['components/Toasts.svelte', '.notice'],
      ['components/TerminalSearch.svelte', '.find'],
    ] as const) {
      expect(rule(read(path), selector), path).toMatch(
        /border-radius:\s*var\(--termru-radius-pane\)/,
      );
    }
    expect(rule(read('components/SessionDialog.svelte'), '.dialog')).toMatch(
      /border-radius:\s*var\(--termru-radius-lg\)/,
    );
  });

  it('keeps champagne for the one primary action: a danger is the error colour, filled', () => {
    const css = read('../app.css');
    expect(css).toMatch(
      /button\.danger-fill \{\s*background:\s*var\(--termru-err-text\);\s*color:\s*var\(--termru-on-err\)/,
    );
    // Every destructive confirm of the app uses it.
    for (const path of [
      'components/CloseConfirm.svelte',
      'components/ContextConfirm.svelte',
      'components/HostEditor.svelte',
      'pages/IdentitiesPage.svelte',
      'pages/FoldersPage.svelte',
      'pages/PreferencesPage.svelte',
    ]) {
      expect(read(path), path).toContain('danger-fill');
    }
    expect(read('components/HostKeyDialog.svelte')).toContain('class:danger-fill={changed}');
  });

  it('builds the confirm of the hosts page on the same dialog as every other question', () => {
    expect(read('components/ContextConfirm.svelte')).toContain(
      "import SessionDialog from './SessionDialog.svelte'",
    );
  });
});
