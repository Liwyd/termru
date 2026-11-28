// @vitest-environment jsdom
/**
 * The command palette as a user meets it: sections when the field is empty, a
 * ranked list with each entry's section when it is not, the keyboard doing all
 * of it, the hint under the list naming what Enter does.
 */

import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { flushSync, mount, tick, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import CommandPalette from './CommandPalette.svelte';
import { palette, type PaletteEntry } from '../stores/palette.svelte';
import { newTarget, press, stubBrowserApis, type } from '../test-support/dom';

let target: HTMLElement;
let component: ReturnType<typeof mount> | null = null;
const ran: string[] = [];

function entry(
  id: string,
  title: string,
  group: string,
  extra: Partial<PaletteEntry> = {},
): PaletteEntry {
  return { id, title, group, run: () => ran.push(id), ...extra };
}

async function open(entries: PaletteEntry[]): Promise<HTMLInputElement> {
  palette.show(entries);
  flushSync();
  await tick();
  return document.querySelector('input[aria-label="Command"]') as HTMLInputElement;
}

beforeEach(() => {
  stubBrowserApis();
  ran.length = 0;
  target = newTarget();
  component = mount(CommandPalette, { target });
});

afterEach(() => {
  palette.hide();
  if (component) unmount(component);
  component = null;
  target.remove();
  document.body.replaceChildren();
});

describe('CommandPalette', () => {
  it('is a dialog with a combobox over a listbox', async () => {
    const input = await open([entry('a', 'Alpha', 'Hosts')]);
    expect(document.querySelector('[role="dialog"][aria-label="Command palette"]')).not.toBeNull();
    expect(input.getAttribute('role')).toBe('combobox');
    const list = document.querySelector('[role="listbox"]') as HTMLElement;
    expect(input.getAttribute('aria-controls')).toBe(list.id);
    expect(list.querySelectorAll('[role="option"]')).toHaveLength(1);
    expect(input.getAttribute('aria-activedescendant')).toBe('palette-option-0');
  });

  it('shows a heading for each section while the field is empty', async () => {
    await open([
      entry('l', 'Go to atlas-prod', 'Live', { detail: '2 panes · connected' }),
      entry('h', 'Connect to archive', 'Hosts', { detail: 'ops@203.0.113.30' }),
      entry('g', 'Go to Hosts', 'Go to', { accelerator: 'Ctrl ⇧ H' }),
    ]);
    const headings = Array.from(document.querySelectorAll('.heading')).map((h) => h.textContent);
    expect(headings).toEqual(['Live', 'Hosts', 'Go to']);
    // An address is data: it sits beside the title; a shortcut is a key cap.
    expect(document.querySelector('.detail')?.textContent).toBe('2 panes · connected');
    expect(document.querySelector('kbd')).not.toBeNull();
  });

  it('shows the section of each entry instead of headings once the user types', async () => {
    const input = await open([
      entry('h', 'Connect to archive', 'Hosts'),
      entry('p', 'Preferences: About', 'Preferences'),
    ]);
    type(input, 'pref');
    await tick();
    expect(document.querySelectorAll('.heading')).toHaveLength(0);
    const options = Array.from(document.querySelectorAll('[role="option"]'));
    expect(options).toHaveLength(1);
    expect(options[0].querySelector('.section')?.textContent).toBe('Preferences');
  });

  it('moves with the arrows and runs with Enter, and Shift+Enter runs the second action', async () => {
    const input = await open([
      entry('a', 'Connect to a', 'Hosts', {
        verb: 'Connect',
        alt: { verb: 'Edit', run: () => ran.push('a:edit') },
      }),
      entry('b', 'Connect to b', 'Hosts', { verb: 'Connect' }),
    ]);
    // The hint under the list names what the keys do for the highlighted entry.
    expect(document.querySelector('.foot')?.textContent).toContain('Connect');
    expect(document.querySelector('.foot')?.textContent).toContain('Edit');

    press(input, 'ArrowDown');
    expect(palette.selected).toBe(1);
    expect(document.querySelector('.foot')?.textContent).not.toContain('Edit');
    press(input, 'ArrowUp');
    press(input, 'Enter', { shiftKey: true });
    expect(ran).toEqual(['a:edit']);
    expect(palette.open).toBe(false);
  });

  it('runs an entry on click', async () => {
    await open([entry('a', 'Alpha', 'Hosts'), entry('b', 'Beta', 'Hosts')]);
    (document.querySelectorAll('[role="option"]')[1] as HTMLElement).click();
    expect(ran).toEqual(['b']);
    expect(palette.open).toBe(false);
  });

  it('says so when nothing matches', async () => {
    const input = await open([entry('a', 'Alpha', 'Hosts')]);
    type(input, 'zzzz');
    await tick();
    expect(document.querySelector('.empty')?.textContent).toContain('Nothing matches “zzzz”');
  });

  it('closes on Escape and on the backdrop, and opens without any motion', async () => {
    const input = await open([entry('a', 'Alpha', 'Hosts')]);
    press(input.closest('[role="dialog"]') as HTMLElement, 'Escape');
    expect(palette.open).toBe(false);

    await open([entry('a', 'Alpha', 'Hosts')]);
    (document.querySelector('.backdrop') as HTMLElement).click();
    expect(palette.open).toBe(false);

    // A key opens it: nothing animates.
    const source = readStyle();
    expect(source).not.toMatch(/transition|animation|@keyframes/);
  });
});

function readStyle(): string {
  // The component's source is the contract: no transition or animation is declared.
  const source = readFileSync(resolve(import.meta.dirname, 'CommandPalette.svelte'), 'utf8');
  return source.slice(source.indexOf('<style>'));
}
