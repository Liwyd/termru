// @vitest-environment jsdom
/**
 * The Preferences page (frame 8): every field has a control, a change applies
 * and is saved at once, a typed value is checked when it is left, the shortcut
 * table records and unbinds per row, and "Restore defaults" asks first.
 */

import { flushSync, mount, tick, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import PreferencesPage from './PreferencesPage.svelte';
import { notices } from '../notify.svelte';
import { defaultPrefs, prefs } from '../stores/prefs.svelte';
import { shortcuts } from '../stores/shortcuts.svelte';
import { click, findButton, newTarget, stubBrowserApis, type } from '../test-support/dom';
import { answers, callsOf, failures, resetTauriMock } from '../test-support/tauri-core-mock';
import type { Preferences } from '../types';

vi.mock('@tauri-apps/api/core', () => import('../test-support/tauri-core-mock'));
vi.mock('@tauri-apps/api/event', () => ({ listen: async () => () => {} }));

let target: HTMLElement;
let component: ReturnType<typeof mount> | null = null;

const field = (id: string) => document.getElementById(id) as HTMLInputElement;
const text = () => target.textContent ?? '';
const saved = (): Preferences[] =>
  callsOf('prefs_set').map((call) => (call.args as { prefs: Preferences }).prefs);
const lastSaved = (): Preferences => saved().at(-1) as Preferences;

function toggle(name: string): HTMLButtonElement {
  const found = Array.from(target.querySelectorAll<HTMLButtonElement>('[role="switch"]')).find(
    (button) =>
      document.getElementById(button.getAttribute('aria-labelledby') ?? '')?.textContent === name,
  );
  if (!found) throw new Error(`no switch "${name}"`);
  return found;
}

function select(label: string): HTMLSelectElement {
  const found = Array.from(target.querySelectorAll<HTMLSelectElement>('select')).find(
    (element) =>
      document.getElementById(element.getAttribute('aria-labelledby') ?? '')?.textContent ===
        label || element.labels?.[0]?.textContent?.trim() === label,
  );
  if (!found) throw new Error(`no select "${label}"`);
  return found;
}

function choose(element: HTMLSelectElement, value: string): void {
  element.value = value;
  element.dispatchEvent(new Event('change', { bubbles: true }));
  flushSync();
}

async function mountPage(): Promise<void> {
  component = mount(PreferencesPage, { target });
  flushSync();
  await tick();
}

function key(init: KeyboardEventInit): void {
  window.dispatchEvent(new KeyboardEvent('keydown', { bubbles: true, cancelable: true, ...init }));
  flushSync();
}

beforeEach(() => {
  resetTauriMock();
  stubBrowserApis();
  notices.reset();
  shortcuts.cancel();
  prefs.prefs = defaultPrefs();
  answers.set('app_info', { name: 'termru-desktop', version: '0.2.0' });
  target = newTarget();
});

afterEach(() => {
  if (component) unmount(component);
  component = null;
  shortcuts.cancel();
  target.remove();
  document.body.replaceChildren();
});

describe('the page', () => {
  it('has the anatomy of a page: breadcrumb, title, one line, Restore defaults', async () => {
    await mountPage();
    expect(target.querySelector('.crumbs')?.textContent).toMatch(/Workspace\s*\/\s*This device/);
    expect(target.querySelector('h1')?.textContent).toBe('Preferences');
    expect(target.querySelector('.description')?.textContent).toContain('accessibility');
    expect(findButton(target, 'Restore defaults')).toBeTruthy();
    expect(target.querySelector('.workpage')).not.toBeNull();
  });

  it('has a control for every preference', async () => {
    await mountPage();
    // Appearance
    expect(field('pref-font').tagName).toBe('SELECT');
    expect(field('pref-size').type).toBe('number');
    expect(field('pref-line').type).toBe('number');
    expect(field('pref-theme').tagName).toBe('SELECT');
    // Terminal behavior
    expect(select('Cursor')).toBeTruthy();
    expect(toggle('Cursor blink')).toBeTruthy();
    expect(field('pref-scrollback')).not.toBeNull();
    expect(toggle('Copy on select')).toBeTruthy();
    expect(toggle('Confirm multi-line paste')).toBeTruthy();
    // Accessibility and notices
    expect(select('Reduce motion')).toBeTruthy();
    expect(toggle('High-visibility focus')).toBeTruthy();
    expect(toggle('Routine notices')).toBeTruthy();
  });

  it('shows the stored values, and the state of a switch in words', async () => {
    prefs.prefs = {
      ...defaultPrefs(),
      terminal: { ...defaultPrefs().terminal, font_size: 16, line_height: 1.5, scrollback: 20_000 },
    };
    await mountPage();
    expect(field('pref-size').value).toBe('16');
    expect(field('pref-line').value).toBe('1.5');
    expect(field('pref-scrollback').value).toBe('20,000');
    expect(toggle('Routine notices').textContent?.trim()).toBe('On');
    expect(toggle('Routine notices').getAttribute('aria-checked')).toBe('true');
    expect(toggle('Copy on select').textContent?.trim()).toBe('Off');
  });

  it('describes a switch to a screen reader', async () => {
    await mountPage();
    const button = toggle('Confirm multi-line paste');
    expect(button.getAttribute('role')).toBe('switch');
    const described = document.getElementById(button.getAttribute('aria-describedby') ?? '');
    expect(described?.textContent).toContain('Ask before a paste');
  });
});

describe('saving', () => {
  it('saves a changed size at once, with the rest unchanged', async () => {
    await mountPage();
    type(field('pref-size'), '18');
    await vi.waitFor(() => expect(saved()).toHaveLength(1));
    expect(lastSaved().terminal.font_size).toBe(18);
    expect(lastSaved().terminal.line_height).toBe(defaultPrefs().terminal.line_height);
    expect(prefs.prefs.terminal.font_size).toBe(18);
  });

  it('saves the line height, rounded to two decimals', async () => {
    await mountPage();
    type(field('pref-line'), '1.456');
    await vi.waitFor(() => expect(saved()).toHaveLength(1));
    expect(lastSaved().terminal.line_height).toBe(1.46);
  });

  it('saves the theme, the cursor and the motion choice', async () => {
    await mountPage();
    choose(field('pref-theme') as unknown as HTMLSelectElement, 'dark');
    await vi.waitFor(() => expect(lastSaved()?.theme).toBe('dark'));
    choose(select('Cursor'), 'bar');
    await vi.waitFor(() => expect(lastSaved().terminal.cursor_style).toBe('bar'));
    choose(select('Reduce motion'), 'on');
    await vi.waitFor(() => expect(lastSaved().reduced_motion).toBe('on'));
    // Each save keeps the earlier changes.
    expect(lastSaved().theme).toBe('dark');
    expect(lastSaved().terminal.cursor_style).toBe('bar');
  });

  it('saves every switch to its own field', async () => {
    await mountPage();
    click(toggle('Cursor blink'));
    await vi.waitFor(() => expect(lastSaved().terminal.cursor_blink).toBe(false));
    click(toggle('Copy on select'));
    await vi.waitFor(() => expect(lastSaved().terminal.copy_on_select).toBe(true));
    click(toggle('Confirm multi-line paste'));
    await vi.waitFor(() => expect(lastSaved().terminal.confirm_multiline_paste).toBe(false));
    click(toggle('High-visibility focus'));
    await vi.waitFor(() => expect(lastSaved().focus_visibility).toBe(true));
    click(toggle('Routine notices'));
    await vi.waitFor(() => expect(lastSaved().routine_notices).toBe(false));
    // All five stuck: nothing overwrote an earlier change.
    const final = lastSaved();
    expect(final.terminal.cursor_blink).toBe(false);
    expect(final.terminal.copy_on_select).toBe(true);
    expect(final.terminal.confirm_multiline_paste).toBe(false);
    expect(final.focus_visibility).toBe(true);
  });

  it('saves a font from the list, and a custom one', async () => {
    await mountPage();
    choose(field('pref-font') as unknown as HTMLSelectElement, 'Fira Code, monospace');
    await vi.waitFor(() => expect(lastSaved().terminal.font_family).toBe('Fira Code, monospace'));

    choose(field('pref-font') as unknown as HTMLSelectElement, '__custom__');
    const custom = field('pref-font-custom');
    expect(custom).not.toBeNull();
    type(custom, 'Iosevka, monospace');
    await vi.waitFor(() => expect(lastSaved().terminal.font_family).toBe('Iosevka, monospace'));
  });

  it('reads a stored font that is not in the list as a custom one', async () => {
    prefs.prefs = {
      ...defaultPrefs(),
      terminal: { ...defaultPrefs().terminal, font_family: 'Iosevka, monospace' },
    };
    await mountPage();
    expect((field('pref-font') as unknown as HTMLSelectElement).value).toBe('__custom__');
    expect(field('pref-font-custom').value).toBe('Iosevka, monospace');
  });

  it('shows the live preview with the chosen font, size and cursor', async () => {
    await mountPage();
    const screen = () => target.querySelector<HTMLElement>('[data-testid="preview-screen"]');
    expect(screen()?.style.fontSize).toBe('14px');
    type(field('pref-size'), '20');
    await vi.waitFor(() => expect(screen()?.style.fontSize).toBe('20px'));
    choose(select('Cursor'), 'underline');
    await vi.waitFor(() => expect(target.querySelector('.cursor.underline')).not.toBeNull());
    // The row height follows the line height (1.3 of the glyph row).
    expect(screen()?.style.lineHeight).toBe(`${Math.round(20 * 1.3 * 1.3 * 10) / 10}px`);
  });

  it('shows what is really saved when the store refuses a change, and says why', async () => {
    failures.set('prefs_set', 'disk full');
    answers.set('prefs_get', defaultPrefs());
    await mountPage();
    type(field('pref-size'), '20');
    await vi.waitFor(() => expect(notices.visible.length).toBeGreaterThan(0));
    expect(notices.visible[0].kind).toBe('error');
    expect(notices.visible[0].description).toContain('disk full');
    await vi.waitFor(() => expect(prefs.prefs.terminal.font_size).toBe(14));
  });
});

describe('typed values', () => {
  it('refuses a size out of range, next to the field, and saves nothing', async () => {
    await mountPage();
    type(field('pref-size'), '90');
    expect(field('pref-size').getAttribute('aria-invalid')).toBe('true');
    const message = document.getElementById(
      field('pref-size').getAttribute('aria-describedby') ?? '',
    );
    expect(message?.textContent).toContain('6 to 72');
    expect(saved()).toHaveLength(0);
  });

  it('refuses a line height out of range', async () => {
    await mountPage();
    type(field('pref-line'), '5');
    expect(field('pref-line').getAttribute('aria-invalid')).toBe('true');
    expect(saved()).toHaveLength(0);
  });

  it('refuses a scrollback that is not a number of lines', async () => {
    await mountPage();
    type(field('pref-scrollback'), 'lots');
    expect(text()).toContain('whole number from 1,000 to 1,000,000');
    expect(saved()).toHaveLength(0);
    type(field('pref-scrollback'), '50,000');
    await vi.waitFor(() => expect(lastSaved().terminal.scrollback).toBe(50_000));
    expect(field('pref-scrollback').value).toBe('50,000');
  });

  it('clears the message once the value is right', async () => {
    await mountPage();
    type(field('pref-size'), '90');
    type(field('pref-size'), '12');
    expect(field('pref-size').getAttribute('aria-invalid')).toBeNull();
    await vi.waitFor(() => expect(lastSaved().terminal.font_size).toBe(12));
  });
});

describe('shortcuts', () => {
  const record = (title: string) =>
    click(target.querySelector(`[aria-label="Record a shortcut for ${title}"]`) as Element);

  it('lists every command with both platforms, and the built-in keys as fixed', async () => {
    await mountPage();
    const rows = target.querySelectorAll('.shortcuts tbody tr:not(.group)');
    // Every command of the registry plus the three terminal keys.
    expect(rows.length).toBeGreaterThanOrEqual(24 + 3);
    expect(text()).toContain('Command palette');
    expect(text()).toContain('macOS');
    expect(text()).toContain('Windows / Linux');
    expect(text()).toContain('Copy the terminal selection');
    expect(target.querySelectorAll('.fixed').length).toBe(3);
  });

  it('records a new combination and saves it', async () => {
    await mountPage();
    record('Command palette');
    expect(shortcuts.recording).toBe('palette.open');
    expect(text()).toContain('Press the new keys');
    key({ key: 'F8', ctrlKey: true, shiftKey: true });
    await vi.waitFor(() => expect(lastSaved().keybindings['palette.open']).toBe('Ctrl+Shift+F8'));
    expect(shortcuts.recording).toBeNull();
  });

  it('refuses a combination another command uses, and saves nothing', async () => {
    await mountPage();
    record('Command palette');
    key({ key: 'T', ctrlKey: true, shiftKey: true });
    expect(text()).toContain('Already used by “New tab”');
    expect(shortcuts.recording).toBe('palette.open');
    expect(saved()).toHaveLength(0);
  });

  it('stops recording on Escape without changing anything', async () => {
    await mountPage();
    record('Command palette');
    key({ key: 'Escape' });
    expect(shortcuts.recording).toBeNull();
    expect(saved()).toHaveLength(0);
  });

  it('stops recording when the page is left', async () => {
    await mountPage();
    record('Command palette');
    unmount(component as ReturnType<typeof mount>);
    component = null;
    expect(shortcuts.recording).toBeNull();
  });

  it('unbinds a command: an empty override, shown as Unbound', async () => {
    await mountPage();
    click(target.querySelector('[aria-label="Unbind Command palette"]') as Element);
    await vi.waitFor(() => expect(lastSaved().keybindings['palette.open']).toBe(''));
    flushSync();
    const row = Array.from(target.querySelectorAll('.shortcuts tr')).find((tr) =>
      tr.textContent?.includes('Command palette'),
    ) as HTMLElement;
    expect(row.textContent).toContain('Unbound');
    // An unbound command has nothing left to unbind, and offers a way back.
    expect(row.querySelector('[aria-label="Unbind Command palette"]')).toBeNull();
    expect(row.querySelector('[aria-label="Reset Command palette to its default"]')).not.toBeNull();
  });

  it('resets one command to its default', async () => {
    prefs.prefs = { ...defaultPrefs(), keybindings: { 'tab.close': 'Ctrl+Shift+F9' } };
    await mountPage();
    click(target.querySelector('[aria-label="Reset Close tab to its default"]') as Element);
    await vi.waitFor(() => expect(saved()).toHaveLength(1));
    expect('tab.close' in lastSaved().keybindings).toBe(false);
  });
});

describe('Restore defaults', () => {
  it('asks first, and changes nothing when cancelled', async () => {
    prefs.prefs = {
      ...defaultPrefs(),
      theme: 'dark',
      terminal: { ...defaultPrefs().terminal, font_size: 20 },
    };
    await mountPage();
    click(findButton(target, 'Restore defaults'));
    await vi.waitFor(() => expect(document.querySelector('[role="alertdialog"]')).not.toBeNull());
    const dialog = document.querySelector('[role="alertdialog"]') as HTMLElement;
    expect(dialog.textContent).toContain('Restore default preferences?');
    expect(dialog.textContent).toContain('Hosts, identities and credentials are not touched');
    click(findButton(dialog, 'Cancel'));
    expect(saved()).toHaveLength(0);
    expect(prefs.prefs.terminal.font_size).toBe(20);
  });

  it('puts every preference back, shortcuts included, and says so', async () => {
    prefs.prefs = {
      ...defaultPrefs(),
      theme: 'dark',
      keybindings: { 'tab.close': 'Ctrl+Shift+F9' },
      routine_notices: false,
      terminal: { ...defaultPrefs().terminal, font_size: 20, confirm_multiline_paste: false },
    };
    await mountPage();
    click(findButton(target, 'Restore defaults'));
    await vi.waitFor(() => expect(document.querySelector('[role="alertdialog"]')).not.toBeNull());
    click(
      findButton(document.querySelector('[role="alertdialog"]') as HTMLElement, 'Restore defaults'),
    );
    await vi.waitFor(() => expect(saved()).toHaveLength(1));
    expect(lastSaved()).toEqual(defaultPrefs());
    expect(lastSaved().keybindings).toEqual({});
    expect(lastSaved().terminal.confirm_multiline_paste).toBe(true);
    await vi.waitFor(() =>
      expect(notices.visible.map((notice) => notice.key)).toContain('prefs.restored'),
    );
  });

  it('puts the focus on Cancel, the safe answer', async () => {
    await mountPage();
    click(findButton(target, 'Restore defaults'));
    await vi.waitFor(() => expect(document.querySelector('[role="alertdialog"]')).not.toBeNull());
    await tick();
    expect(document.activeElement).toBe(
      findButton(document.querySelector('[role="alertdialog"]') as HTMLElement, 'Cancel'),
    );
  });
});

describe('notices', () => {
  it('previews a notice even when routine notices are off', async () => {
    prefs.prefs = { ...defaultPrefs(), routine_notices: false };
    await mountPage();
    click(findButton(target, 'Preview notice'));
    expect(notices.visible.map((notice) => notice.key)).toEqual(['prefs.preview']);
  });
});

describe('about', () => {
  it('shows the version the app reports', async () => {
    await mountPage();
    await vi.waitFor(() => expect(text()).toContain('Version 0.2.0'));
    expect(text()).toContain('termru-desktop');
    expect(callsOf('app_info')).toHaveLength(1);
  });

  it('says so when the version cannot be read', async () => {
    failures.set('app_info', 'no such command');
    await mountPage();
    await vi.waitFor(() => expect(text()).toContain('The version could not be read'));
  });
});
