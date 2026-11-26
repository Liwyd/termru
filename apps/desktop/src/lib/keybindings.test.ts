import { describe, expect, it } from 'vitest';
import {
  COMMANDS,
  acceleratorFor,
  acceleratorLabel,
  matchBinding,
  conflictFor,
  defaultBindings,
  effectiveBindings,
  eventMatches,
  eventToAccelerator,
  expandAccelerator,
  findConflicts,
  normalizeAccelerator,
} from './keybindings';

describe('accelerator normalization', () => {
  it('expands Mod to the platform modifier', () => {
    const expanded = expandAccelerator('Mod+Shift+K');
    expect(expanded).toMatch(/^(Ctrl|Cmd)\+Shift\+K$/);
  });

  it('normalizes order and case for comparison', () => {
    expect(normalizeAccelerator('Shift+Mod+K')).toBe(normalizeAccelerator('Mod+Shift+k'));
  });
});

describe('default bindings', () => {
  it('have no conflicts', () => {
    expect(findConflicts(defaultBindings())).toEqual([]);
  });

  it('cover every command exactly once', () => {
    const bindings = defaultBindings();
    expect(bindings).toHaveLength(COMMANDS.length);
    expect(new Set(bindings.map((binding) => binding.commandId)).size).toBe(COMMANDS.length);
  });
});

describe('conflict detection', () => {
  it('reports two commands sharing an accelerator', () => {
    const conflicts = findConflicts([
      { commandId: 'tab.new', accelerator: 'Mod+T' },
      { commandId: 'tab.close', accelerator: 'Mod+T' },
    ]);
    expect(conflicts).toHaveLength(1);
    expect(conflicts[0].commands).toEqual(['tab.close', 'tab.new']);
  });

  it('ignores different modifiers', () => {
    const conflicts = findConflicts([
      { commandId: 'tab.new', accelerator: 'Mod+T' },
      { commandId: 'tab.close', accelerator: 'Mod+Shift+T' },
    ]);
    expect(conflicts).toEqual([]);
  });

  it('treats the same command twice as no conflict', () => {
    const conflicts = findConflicts([
      { commandId: 'tab.new', accelerator: 'Mod+T' },
      { commandId: 'tab.new', accelerator: 'Mod+T' },
    ]);
    expect(conflicts).toEqual([]);
  });
});

describe('event matching', () => {
  function keyEvent(init: Partial<KeyboardEvent> & { key: string }): KeyboardEvent {
    return {
      key: init.key,
      ctrlKey: init.ctrlKey ?? false,
      metaKey: init.metaKey ?? false,
      altKey: init.altKey ?? false,
      shiftKey: init.shiftKey ?? false,
    } as KeyboardEvent;
  }

  it('matches a simple accelerator', () => {
    const accelerator = expandAccelerator('Mod+K');
    const event = keyEvent({
      key: 'k',
      ctrlKey: accelerator.startsWith('Ctrl'),
      metaKey: accelerator.startsWith('Cmd'),
    });
    expect(eventMatches(event, 'Mod+K')).toBe(true);
  });

  it('does not match when a modifier differs', () => {
    const accelerator = expandAccelerator('Mod+Shift+K');
    const event = keyEvent({
      key: 'K',
      ctrlKey: accelerator.startsWith('Ctrl'),
      metaKey: accelerator.startsWith('Cmd'),
      shiftKey: false,
    });
    expect(eventMatches(event, 'Mod+Shift+K')).toBe(false);
  });

  it('does not match a bare key press', () => {
    expect(eventMatches(keyEvent({ key: 'k' }), 'Mod+K')).toBe(false);
  });
});

describe('user overrides', () => {
  it('replace the default of a command', () => {
    const bindings = effectiveBindings({ 'palette.open': 'Mod+Shift+Y' });
    expect(bindings.find((binding) => binding.commandId === 'palette.open')?.accelerator).toBe(
      'Mod+Shift+Y',
    );
    // The other commands keep their default.
    expect(bindings.find((binding) => binding.commandId === 'tab.new')?.accelerator).toBe(
      acceleratorFor('tab.new'),
    );
  });

  it('ignore an override for an unknown command', () => {
    const bindings = effectiveBindings({ 'does.not.exist': 'Mod+Q' });
    expect(bindings).toHaveLength(COMMANDS.filter((c) => c.accelerator).length);
    expect(bindings.some((binding) => binding.accelerator === 'Mod+Q')).toBe(false);
  });

  it('leave unbound commands (empty accelerator) out of the key handlers', () => {
    const bindings = effectiveBindings();
    // The route.* commands are palette-only: they must never capture a key.
    expect(bindings.some((binding) => binding.commandId.startsWith('route.'))).toBe(false);
    // Even a user override cannot make an unbound command grab keys by
    // accident — an empty default stays out unless the user binds it.
    const rebound = effectiveBindings({ 'route.hosts': 'Mod+Shift+H' });
    expect(rebound.some((binding) => binding.commandId === 'route.hosts')).toBe(true);
  });

  it('report the accelerator of a command', () => {
    // Ctrl+T belongs to the shell on Linux and Windows (these tests run there).
    expect(acceleratorFor('tab.new')).toBe('Mod+Shift+T');
    expect(acceleratorFor('tab.new', { 'tab.new': 'Mod+Shift+N' })).toBe('Mod+Shift+N');
  });

  it('find the command that already uses an accelerator', () => {
    const bindings = defaultBindings();
    expect(conflictFor(acceleratorFor('tab.new'), bindings)).toBe('tab.new');
    expect(conflictFor('Mod+Shift+Y', bindings)).toBeNull();
    // A command may keep its own accelerator.
    expect(conflictFor(acceleratorFor('tab.new'), bindings, 'tab.new')).toBeNull();
  });

  it('see a conflict that an override creates', () => {
    const bindings = effectiveBindings({ 'tab.new': acceleratorFor('palette.open') });
    expect(conflictFor(acceleratorFor('palette.open'), bindings)).toBe('palette.open');
  });
});

describe('recording a combination', () => {
  const key = (init: KeyboardEventInit): KeyboardEvent =>
    ({
      key: '',
      ctrlKey: false,
      altKey: false,
      shiftKey: false,
      metaKey: false,
      ...init,
    }) as KeyboardEvent;

  it('builds an accelerator from a key event', () => {
    expect(eventToAccelerator(key({ key: 'k', ctrlKey: true }))).toBe('Ctrl+K');
    expect(eventToAccelerator(key({ key: 'k', ctrlKey: true, shiftKey: true }))).toBe(
      'Ctrl+Shift+K',
    );
    expect(eventToAccelerator(key({ key: 'ArrowLeft', altKey: true }))).toBe('Alt+ArrowLeft');
  });

  it('ignores a modifier pressed on its own', () => {
    expect(eventToAccelerator(key({ key: 'Control', ctrlKey: true }))).toBeNull();
    expect(eventToAccelerator(key({ key: 'Shift', shiftKey: true }))).toBeNull();
  });

  it('keeps a named key as it is and a character in upper case', () => {
    expect(eventToAccelerator(key({ key: 'F5' }))).toBe('F5');
    expect(eventToAccelerator(key({ key: 'a' }))).toBe('A');
  });
});

describe('platform defaults', () => {
  /** The accelerators a shell uses as controls (Ctrl plus a bare letter). */
  const bareCtrlLetter = /^Mod\+[A-Za-z]$/;

  it('have no conflicts on either platform', () => {
    expect(findConflicts(defaultBindings(true))).toEqual([]);
    expect(findConflicts(defaultBindings(false))).toEqual([]);
  });

  it('never bind a bare Ctrl+letter on Linux and Windows (the shell owns those)', () => {
    const offenders = defaultBindings(false).filter((binding) =>
      bareCtrlLetter.test(binding.accelerator),
    );
    expect(offenders).toEqual([]);
  });

  it('use Ctrl+Shift+letter for the commands that would collide, and Cmd+letter on macOS', () => {
    const pc = (id: string) => defaultBindings(false).find((b) => b.commandId === id)?.accelerator;
    const mac = (id: string) => defaultBindings(true).find((b) => b.commandId === id)?.accelerator;
    expect(pc('palette.open')).toBe('Mod+Shift+P');
    expect(pc('tab.close')).toBe('Mod+Shift+W');
    expect(pc('host.search')).toBe('Mod+Shift+F');
    expect(mac('palette.open')).toBe('Mod+K');
    expect(mac('tab.close')).toBe('Mod+W');
    // Tab switching keeps its chord everywhere.
    expect(pc('tab.next')).toBe('Mod+Tab');
    expect(pc('tab.previous')).toBe('Mod+Shift+Tab');
  });

  it('keep a stored override working', () => {
    expect(acceleratorFor('palette.open', { 'palette.open': 'Ctrl+K' })).toBe('Ctrl+K');
    expect(
      effectiveBindings({ 'palette.open': 'Ctrl+K' }).find((b) => b.commandId === 'palette.open')
        ?.accelerator,
    ).toBe('Ctrl+K');
  });

  it('shows the chord the way the platform writes it', () => {
    expect(acceleratorLabel('Mod+K', true)).toBe('⌘K');
    expect(acceleratorLabel('Mod+Shift+P', false)).toBe('Ctrl+Shift+P');
    expect(acceleratorLabel('Mod+Plus', false)).toBe('Ctrl++');
  });
});

describe('plus and minus', () => {
  const key = (init: Partial<KeyboardEvent> & { key: string }): KeyboardEvent =>
    ({ ctrlKey: false, metaKey: false, altKey: false, shiftKey: false, ...init }) as KeyboardEvent;

  it('match the + and = keys for Plus, with or without Shift', () => {
    expect(eventMatches(key({ key: '+', ctrlKey: true, shiftKey: true }), 'Ctrl+Plus')).toBe(true);
    expect(eventMatches(key({ key: '=', ctrlKey: true }), 'Ctrl+Plus')).toBe(true);
    expect(eventMatches(key({ key: '+', ctrlKey: true }), 'Ctrl+Plus')).toBe(true);
    expect(eventMatches(key({ key: '=', ctrlKey: false }), 'Ctrl+Plus')).toBe(false);
  });

  it('match the - and _ keys for Minus', () => {
    expect(eventMatches(key({ key: '-', ctrlKey: true }), 'Ctrl+Minus')).toBe(true);
    expect(eventMatches(key({ key: '_', ctrlKey: true, shiftKey: true }), 'Ctrl+Minus')).toBe(true);
    expect(eventMatches(key({ key: 'a', ctrlKey: true }), 'Ctrl+Minus')).toBe(false);
  });

  it('run the font commands of the default bindings', () => {
    const increase = effectiveBindings().find((b) => b.commandId === 'view.increaseFont');
    expect(increase).toBeDefined();
    expect(
      eventMatches(key({ key: '+', ctrlKey: true, shiftKey: true }), increase!.accelerator),
    ).toBe(true);
    expect(matchBinding(key({ key: '-', ctrlKey: true }))?.commandId).toBe('view.decreaseFont');
  });

  it('are recorded as Plus and Minus, never as "++"', () => {
    expect(eventToAccelerator(key({ key: '+', ctrlKey: true, shiftKey: true }))).toBe('Ctrl+Plus');
    expect(eventToAccelerator(key({ key: '=', ctrlKey: true }))).toBe('Ctrl+=');
    expect(eventToAccelerator(key({ key: '-', ctrlKey: true }))).toBe('Ctrl+Minus');
  });

  it('understand a stored Ctrl++ as Ctrl and the plus key', () => {
    expect(eventMatches(key({ key: '+', ctrlKey: true, shiftKey: true }), 'Ctrl++')).toBe(true);
    expect(normalizeAccelerator('Ctrl++')).toBe(normalizeAccelerator('Ctrl+Plus'));
  });
});

describe('matching a command from the window', () => {
  it('finds the command of a key event, with the overrides applied', () => {
    const event = {
      key: 'P',
      ctrlKey: true,
      shiftKey: true,
      altKey: false,
      metaKey: false,
    } as KeyboardEvent;
    expect(matchBinding(event)?.commandId).toBe('palette.open');
    expect(matchBinding(event, { 'palette.open': 'Mod+Shift+Y' })).toBeNull();
  });

  it('does not match a plain control key of the shell', () => {
    const ctrlW = { key: 'w', ctrlKey: true, shiftKey: false } as KeyboardEvent;
    expect(matchBinding(ctrlW)).toBeNull();
  });
});
