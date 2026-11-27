// @vitest-environment jsdom
/**
 * The arrival of a block on a page: it moves in only when the user was
 * looking (not in the first paint of the page) and did not use the keyboard.
 */

import { afterEach, describe, expect, it, vi } from 'vitest';
import { appear, lastModality } from './appear';

function press(): void {
  window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
}

function point(): void {
  window.dispatchEvent(new Event('pointerdown', { bubbles: true }));
}

function block(): HTMLElement {
  const node = document.createElement('div');
  document.body.appendChild(node);
  return node;
}

afterEach(() => {
  document.body.replaceChildren();
  point();
});

describe('appear', () => {
  it('starts hidden and moves in one frame later', async () => {
    point();
    const node = block();
    appear(node);
    expect(node.classList.contains('appear')).toBe(true);
    expect(node.classList.contains('appear-in')).toBe(false);
    await vi.waitFor(() => expect(node.classList.contains('appear-in')).toBe(true));
  });

  it('plays nothing in the first paint of a page', () => {
    point();
    const node = block();
    expect(appear(node, { enabled: false })).toBeUndefined();
    expect(node.className).toBe('');
  });

  it('plays nothing when the last thing the user did was press a key', () => {
    press();
    expect(lastModality()).toBe('keyboard');
    const node = block();
    expect(appear(node)).toBeUndefined();
    expect(node.className).toBe('');
  });

  it('moves again once a pointer is used', () => {
    press();
    point();
    expect(lastModality()).toBe('pointer');
    const node = block();
    appear(node);
    expect(node.classList.contains('appear')).toBe(true);
  });

  it('does not touch a block that was taken away before its frame', async () => {
    point();
    const node = block();
    const handle = appear(node) as { destroy(): void };
    handle.destroy();
    await new Promise((resolve) => setTimeout(resolve, 50));
    expect(node.classList.contains('appear-in')).toBe(false);
  });
});
