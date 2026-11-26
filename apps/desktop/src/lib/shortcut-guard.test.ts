// @vitest-environment jsdom
import { afterEach, describe, expect, it } from 'vitest';
import { isEditableTarget, modalOpen, shortcutAllowed } from './shortcut-guard';

function eventOn(target: EventTarget): KeyboardEvent {
  const event = new KeyboardEvent('keydown', { key: 'N', ctrlKey: true, shiftKey: true });
  Object.defineProperty(event, 'target', { value: target });
  return event;
}

afterEach(() => {
  document.body.replaceChildren();
});

describe('shortcut guard', () => {
  it('treats text fields, selects and editable regions as places the user types', () => {
    for (const tag of ['input', 'textarea', 'select']) {
      expect(isEditableTarget(document.createElement(tag))).toBe(true);
    }
    const editable = document.createElement('div');
    Object.defineProperty(editable, 'isContentEditable', { value: true });
    expect(isEditableTarget(editable)).toBe(true);
    expect(isEditableTarget(document.createElement('button'))).toBe(false);
    expect(isEditableTarget(window)).toBe(false);
  });

  it('does not take the terminal for a form field', () => {
    const xterm = document.createElement('div');
    xterm.className = 'xterm';
    const helper = document.createElement('textarea');
    xterm.appendChild(helper);
    document.body.appendChild(xterm);
    expect(isEditableTarget(helper)).toBe(false);
    expect(shortcutAllowed(eventOn(helper), 'host.new')).toBe(true);
  });

  it('keeps chords out of fields, except the palette chord', () => {
    const input = document.createElement('input');
    document.body.appendChild(input);
    expect(shortcutAllowed(eventOn(input), 'host.new')).toBe(false);
    expect(shortcutAllowed(eventOn(input), 'tab.close')).toBe(false);
    expect(shortcutAllowed(eventOn(input), 'palette.open')).toBe(true);
  });

  it('keeps every chord out while a modal dialog is open', () => {
    const dialog = document.createElement('div');
    dialog.setAttribute('role', 'dialog');
    dialog.setAttribute('aria-modal', 'true');
    document.body.appendChild(dialog);
    expect(modalOpen()).toBe(true);
    expect(shortcutAllowed(eventOn(document.body), 'host.new')).toBe(false);
    expect(shortcutAllowed(eventOn(document.body), 'palette.open')).toBe(false);
    dialog.remove();
    expect(modalOpen()).toBe(false);
    expect(shortcutAllowed(eventOn(document.body), 'host.new')).toBe(true);
  });
});
