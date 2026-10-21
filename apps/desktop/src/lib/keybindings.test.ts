import { describe, expect, it } from 'vitest';
import {
  COMMANDS,
  defaultBindings,
  eventMatches,
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
