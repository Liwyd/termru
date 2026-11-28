// @vitest-environment jsdom
/**
 * The section index of a long page (the host editor, Preferences): the sections
 * in order, the one being read marked as current, a way to any of them from the
 * keyboard, and no hash links (the app routes by the URL hash).
 */

import { flushSync, mount, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import SectionIndex from './SectionIndex.svelte';
import { SECTION_GAP } from '../scroll-section';
import { click, newTarget, press } from '../test-support/dom';

const ITEMS = [
  { id: 'sec-a', label: 'Alpha' },
  { id: 'sec-b', label: 'Beta' },
  { id: 'sec-c', label: 'Gamma' },
];

let target: HTMLElement;
let scroller: HTMLElement;
let component: ReturnType<typeof mount> | null = null;
/** Where each heading is, relative to the top of the scroller. */
const tops: Record<string, number> = {};

function frame(): Promise<void> {
  return new Promise((resolve) => requestAnimationFrame(() => resolve()));
}

function page(): void {
  scroller = document.createElement('div');
  for (const item of ITEMS) {
    const section = document.createElement('section');
    const heading = document.createElement('h2');
    heading.id = item.id;
    heading.tabIndex = -1;
    heading.textContent = item.label;
    heading.getBoundingClientRect = () => ({ top: tops[item.id] ?? 0 }) as DOMRect;
    section.append(heading);
    section.getBoundingClientRect = () => ({ top: tops[item.id] ?? 0 }) as DOMRect;
    section.scrollIntoView = vi.fn();
    scroller.append(section);
  }
  scroller.getBoundingClientRect = () => ({ top: 0 }) as DOMRect;
  Object.defineProperty(scroller, 'scrollHeight', { value: 3000, configurable: true });
  Object.defineProperty(scroller, 'clientHeight', { value: 800, configurable: true });
  Object.defineProperty(scroller, 'scrollTop', { value: 0, writable: true, configurable: true });
  document.body.append(scroller);
}

function open(): void {
  component = mount(SectionIndex, {
    target,
    props: { items: ITEMS, label: 'Sections of this page', scroller },
  });
  flushSync();
}

const buttons = () => Array.from(target.querySelectorAll<HTMLButtonElement>('nav button'));
const current = () =>
  buttons()
    .filter((button) => button.getAttribute('aria-current') === 'location')
    .map((button) => button.textContent?.trim());

beforeEach(() => {
  Object.assign(tops, { 'sec-a': 0, 'sec-b': 900, 'sec-c': 1800 });
  target = newTarget();
  page();
});

afterEach(() => {
  if (component) unmount(component);
  component = null;
  target.remove();
  scroller.remove();
});

describe('the section index', () => {
  it('is a labelled navigation of buttons, in order, with the first section current', () => {
    open();
    const nav = target.querySelector('nav');
    expect(nav?.getAttribute('aria-label')).toBe('Sections of this page');
    expect(buttons().map((button) => button.textContent?.trim())).toEqual([
      'Alpha',
      'Beta',
      'Gamma',
    ]);
    expect(current()).toEqual(['Alpha']);
    // Buttons, not links: a `#section` link would be taken for a page change.
    expect(target.querySelector('a')).toBeNull();
  });

  it('takes you to a section: it scrolls it to the top, focuses its heading and marks it', () => {
    open();
    click(buttons()[2]);
    const section = document.getElementById('sec-c')?.closest('section') as HTMLElement;
    // The scroller moves (a little air over the title), and nothing else does:
    // `scrollIntoView` would slide the header of the app out of view.
    expect(scroller.scrollTop).toBe(1800 - SECTION_GAP);
    expect(section.scrollIntoView).not.toHaveBeenCalled();
    expect(document.activeElement).toBe(document.getElementById('sec-c'));
    expect(current()).toEqual(['Gamma']);
  });

  it('is reachable and usable from the keyboard (Enter and Space click a button)', () => {
    open();
    const beta = buttons()[1];
    beta.focus();
    expect(document.activeElement).toBe(beta);
    // A key press on a button is a click with no pointer: it jumps like a click does.
    beta.dispatchEvent(new MouseEvent('click', { bubbles: true, detail: 0 }));
    flushSync();
    expect(document.activeElement).toBe(document.getElementById('sec-b'));
    expect(current()).toEqual(['Beta']);
    press(beta, 'Tab');
  });

  it('follows the scroll: the last heading that reached the top is the one being read', async () => {
    open();
    await frame();
    Object.assign(tops, { 'sec-a': -950, 'sec-b': -50, 'sec-c': 850 });
    scroller.dispatchEvent(new Event('scroll'));
    await frame();
    flushSync();
    expect(current()).toEqual(['Beta']);
  });

  it('marks the last section at the end of the page, though it never reaches the top', async () => {
    open();
    await frame();
    Object.assign(tops, { 'sec-a': -2000, 'sec-b': -1100, 'sec-c': 300 });
    scroller.scrollTop = 2200;
    scroller.dispatchEvent(new Event('scroll'));
    await frame();
    flushSync();
    expect(current()).toEqual(['Gamma']);
  });

  it('stops listening when it is gone', async () => {
    const remove = vi.spyOn(scroller, 'removeEventListener');
    open();
    await frame();
    unmount(component as ReturnType<typeof mount>);
    component = null;
    expect(remove).toHaveBeenCalledWith('scroll', expect.any(Function));
  });
});
