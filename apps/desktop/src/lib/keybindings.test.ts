import { describe, expect, it } from 'vitest';
import {
  COMMANDS,
  acceleratorFor,
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
    const bindings = effectiveBindings({ 'palette.open': 'Mod+Shift+P' });
    expect(bindings.find((binding) => binding.commandId === 'palette.open')?.accelerator).toBe(
      'Mod+Shift+P',
    );
    // The other commands keep their default.
    expect(bindings.find((binding) => binding.commandId === 'tab.new')?.accelerator).toBe(
      COMMANDS.find((command) => command.id === 'tab.new')?.accelerator,
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
    expect(acceleratorFor('tab.new')).toBe('Mod+T');
    expect(acceleratorFor('tab.new', { 'tab.new': 'Mod+Shift+N' })).toBe('Mod+Shift+N');
  });

  it('find the command that already uses an accelerator', () => {
    const bindings = defaultBindings();
    expect(conflictFor('Mod+T', bindings)).toBe('tab.new');
    expect(conflictFor('Mod+Shift+Q', bindings)).toBeNull();
    // A command may keep its own accelerator.
    expect(conflictFor('Mod+T', bindings, 'tab.new')).toBeNull();
  });

  it('see a conflict that an override creates', () => {
    const bindings = effectiveBindings({ 'tab.new': 'Mod+K' });
    expect(conflictFor('Mod+K', bindings)).toBe('palette.open');
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
