// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { focusTrap, tabbables } from './focus-trap';

let opener: HTMLButtonElement;
let dialog: HTMLElement;

function tab(shift = false): KeyboardEvent {
  const event = new KeyboardEvent('keydown', {
    key: 'Tab',
    shiftKey: shift,
    cancelable: true,
    bubbles: true,
  });
  dialog.dispatchEvent(event);
  return event;
}

beforeEach(() => {
  opener = document.createElement('button');
  opener.textContent = 'open';
  document.body.appendChild(opener);
  opener.focus();

  dialog = document.createElement('div');
  for (const name of ['first', 'second', 'third']) {
    const input = document.createElement('input');
    input.name = name;
    dialog.appendChild(input);
  }
  const hidden = document.createElement('input');
  hidden.name = 'hidden';
  hidden.style.display = 'none';
  dialog.appendChild(hidden);
  const disabled = document.createElement('button');
  disabled.disabled = true;
  dialog.appendChild(disabled);
  document.body.appendChild(dialog);
});

afterEach(() => {
  document.body.replaceChildren();
});

describe('focusTrap', () => {
  it('lists only what Tab can reach', () => {
    expect(tabbables(dialog).map((element) => (element as HTMLInputElement).name)).toEqual([
      'first',
      'second',
      'third',
    ]);
  });

  it('focuses the first field when the dialog opens', async () => {
    focusTrap(dialog);
    await Promise.resolve();
    expect((document.activeElement as HTMLInputElement).name).toBe('first');
  });

  it('prefers the element marked data-autofocus', async () => {
    dialog.querySelector<HTMLInputElement>('[name="third"]')?.setAttribute('data-autofocus', '');
    focusTrap(dialog);
    await Promise.resolve();
    expect((document.activeElement as HTMLInputElement).name).toBe('third');
  });

  it('wraps Tab from the last field to the first, and Shift+Tab the other way', async () => {
    focusTrap(dialog);
    await Promise.resolve();
    dialog.querySelector<HTMLInputElement>('[name="third"]')?.focus();
    expect(tab().defaultPrevented).toBe(true);
    expect((document.activeElement as HTMLInputElement).name).toBe('first');

    expect(tab(true).defaultPrevented).toBe(true);
    expect((document.activeElement as HTMLInputElement).name).toBe('third');

    // In the middle Tab is left to the browser.
    dialog.querySelector<HTMLInputElement>('[name="second"]')?.focus();
    expect(tab().defaultPrevented).toBe(false);
  });

  it('calls onescape on Escape', async () => {
    const onescape = vi.fn();
    focusTrap(dialog, { onescape });
    dialog.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    expect(onescape).toHaveBeenCalledTimes(1);
  });

  it('returns the focus to the opener when the dialog goes away', async () => {
    const action = focusTrap(dialog);
    await Promise.resolve();
    expect(document.activeElement).not.toBe(opener);
    dialog.remove();
    action.destroy();
    expect(document.activeElement).toBe(opener);
  });

  it('leaves the focus alone when something else took it meanwhile', async () => {
    const action = focusTrap(dialog);
    await Promise.resolve();
    const next = document.createElement('input');
    document.body.appendChild(next);
    next.focus();
    dialog.remove();
    action.destroy();
    expect(document.activeElement).toBe(next);
  });

  it('does nothing while disabled (a dialog shown as a page)', async () => {
    const onescape = vi.fn();
    const action = focusTrap(dialog, { enabled: false, onescape });
    await Promise.resolve();
    expect(document.activeElement).toBe(opener);
    dialog.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    expect(onescape).not.toHaveBeenCalled();
    action.destroy();
    expect(document.activeElement).toBe(opener);
  });
});
