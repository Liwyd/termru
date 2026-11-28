// @vitest-environment jsdom
import { flushSync, mount, unmount } from 'svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';
import ContextConfirm from './components/ContextConfirm.svelte';
import Toasts from './components/Toasts.svelte';
import { notices, notify } from './notify.svelte';
import { lastInputWasKeyboard } from './input-modality';

/**
 * Nothing a key triggers animates. A dialog that a key opened (or that a key
 * closes) is simply there; one that a pointer opened plays its short entrance.
 */

let component: ReturnType<typeof mount> | null = null;

afterEach(() => {
  if (component) unmount(component);
  component = null;
  document.body.replaceChildren();
  window.dispatchEvent(new Event('pointerdown'));
});

function open(): void {
  component = mount(ContextConfirm, {
    target: document.body,
    props: { open: true, title: 'Delete?', body: 'Gone.', onconfirm: () => {} },
  });
  flushSync();
}

describe('the way the last input came', () => {
  it('is the keyboard after a key and the pointer after a press', () => {
    window.dispatchEvent(new KeyboardEvent('keydown', { key: 'w', ctrlKey: true }));
    expect(lastInputWasKeyboard()).toBe(true);
    window.dispatchEvent(new Event('pointerdown'));
    expect(lastInputWasKeyboard()).toBe(false);
  });

  it('ignores a modifier held for a click', () => {
    window.dispatchEvent(new Event('pointerdown'));
    window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Control' }));
    expect(lastInputWasKeyboard()).toBe(false);
  });
});

describe('a dialog and the way it was opened', () => {
  it('is open at once when a key opened it: no frame at the closed state', () => {
    window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter' }));
    open();
    expect(document.querySelector('.backdrop')?.classList.contains('shown')).toBe(true);
  });

  it('starts closed and opens a frame later when the pointer opened it', async () => {
    window.dispatchEvent(new Event('pointerdown'));
    open();
    expect(document.querySelector('.backdrop')?.classList.contains('shown')).toBe(false);
    await vi.waitFor(() =>
      expect(document.querySelector('.backdrop')?.classList.contains('shown')).toBe(true),
    );
  });

  it('leaves at once on Escape, and plays its exit when a click closes it', async () => {
    window.dispatchEvent(new Event('pointerdown'));
    open();
    await vi.waitFor(() =>
      expect(document.querySelector('.backdrop')?.classList.contains('shown')).toBe(true),
    );
    const cancel = Array.from(document.querySelectorAll('button')).find(
      (button) => button.textContent?.trim() === 'Cancel',
    ) as HTMLButtonElement;
    window.dispatchEvent(new Event('pointerdown'));
    cancel.click();
    flushSync();
    // Still there, on its way out.
    expect(document.querySelector('.backdrop')).not.toBeNull();
    expect(document.querySelector('.backdrop')?.classList.contains('shown')).toBe(false);
    await vi.waitFor(() => expect(document.querySelector('.backdrop')).toBeNull());
  });
});

describe('a notice and the way it was caused', () => {
  const noticeEl = () => document.querySelector<HTMLElement>('[data-notice-key]');

  afterEach(() => notices.reset());

  it('is there at once after a key, with no entrance', () => {
    component = mount(Toasts, { target: document.body });
    window.dispatchEvent(new KeyboardEvent('keydown', { key: 'c', ctrlKey: true }));
    notify({ key: 'ssh.command-copied' });
    flushSync();
    expect(noticeEl()?.classList.contains('shown')).toBe(true);
    expect(noticeEl()?.classList.contains('still')).toBe(true);
  });

  it('comes in a frame later after a click', async () => {
    component = mount(Toasts, { target: document.body });
    window.dispatchEvent(new Event('pointerdown'));
    notify({ key: 'ssh.command-copied' });
    flushSync();
    expect(noticeEl()?.classList.contains('shown')).toBe(false);
    expect(noticeEl()?.classList.contains('still')).toBe(false);
    await vi.waitFor(() => expect(noticeEl()?.classList.contains('shown')).toBe(true));
  });
});
