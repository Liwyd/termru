/**
 * Keybinding registry.
 *
 * Commands are the unit of action in the app; every command has a stable id
 * and a default accelerator. The registry detects conflicts (two commands
 * with the same accelerator) so the UI can refuse a conflicting override
 * instead of silently shadowing a binding.
 *
 * Accelerators use the notation `Mod+Shift+K`:
 * * `Mod` = Ctrl on Linux/Windows, Cmd on macOS
 * * `Ctrl` / `Alt` / `Shift` are explicit
 */

export type CommandId =
  | 'palette.open'
  | 'tab.new'
  | 'tab.close'
  | 'tab.next'
  | 'tab.previous'
  | 'tab.duplicate'
  | 'host.new'
  | 'host.search'
  | 'view.toggleTheme'
  | 'view.increaseFont'
  | 'view.decreaseFont';

export interface Command {
  id: CommandId;
  title: string;
  /** Default accelerator, in `Mod+Shift+K` notation. */
  accelerator: string;
  /** Grouping in the palette. */
  group: string;
}

export interface Keybinding {
  commandId: CommandId;
  accelerator: string;
}

/** True when the platform uses Cmd as the primary modifier. */
export function isMac(): boolean {
  return typeof navigator !== 'undefined' && /Mac|iPhone|iPad/.test(navigator.platform ?? '');
}

/** The platform's primary modifier: `Mod` expands to Ctrl or Cmd. */
export function primaryModifier(): 'Ctrl' | 'Cmd' {
  return isMac() ? 'Cmd' : 'Ctrl';
}

/** Expands `Mod` to the platform modifier. */
export function expandAccelerator(accelerator: string): string {
  const mod = primaryModifier();
  return accelerator
    .split('+')
    .map((part) => (part.trim() === 'Mod' ? mod : part.trim()))
    .join('+');
}

/** Normalizes an accelerator for comparison: sorted, canonical case. */
export function normalizeAccelerator(accelerator: string): string {
  return expandAccelerator(accelerator)
    .split('+')
    .map((part) => {
      const key = part.trim();
      if (key.length === 1) return key.toUpperCase();
      return key.charAt(0).toUpperCase() + key.slice(1).toLowerCase();
    })
    .sort((a, b) => modifierRank(a) - modifierRank(b))
    .join('+');
}

function modifierRank(part: string): number {
  switch (part) {
    case 'Ctrl':
      return 0;
    case 'Alt':
      return 1;
    case 'Shift':
      return 2;
    default:
      return 3;
  }
}

/** Every command the app knows, with its default accelerator. */
export const COMMANDS: Command[] = [
  { id: 'palette.open', title: 'Command palette', accelerator: 'Mod+K', group: 'General' },
  { id: 'tab.new', title: 'New tab', accelerator: 'Mod+T', group: 'Tabs' },
  { id: 'tab.close', title: 'Close tab', accelerator: 'Mod+W', group: 'Tabs' },
  { id: 'tab.next', title: 'Next tab', accelerator: 'Mod+Tab', group: 'Tabs' },
  { id: 'tab.previous', title: 'Previous tab', accelerator: 'Mod+Shift+Tab', group: 'Tabs' },
  { id: 'tab.duplicate', title: 'Duplicate tab', accelerator: 'Mod+D', group: 'Tabs' },
  { id: 'host.new', title: 'Add host', accelerator: 'Mod+N', group: 'Hosts' },
  { id: 'host.search', title: 'Search hosts', accelerator: 'Mod+F', group: 'Hosts' },
  { id: 'view.toggleTheme', title: 'Toggle theme', accelerator: 'Mod+Shift+L', group: 'View' },
  { id: 'view.increaseFont', title: 'Increase font size', accelerator: 'Mod+Plus', group: 'View' },
  { id: 'view.decreaseFont', title: 'Decrease font size', accelerator: 'Mod+Minus', group: 'View' },
];

/**
 * Finds conflicting bindings: the same accelerator bound to two different
 * commands. Returns the offending pairs.
 */
export function findConflicts(
  bindings: Keybinding[],
): { accelerator: string; commands: string[] }[] {
  const byAccelerator = new Map<string, Set<string>>();
  for (const binding of bindings) {
    const key = normalizeAccelerator(binding.accelerator);
    const commands = byAccelerator.get(key) ?? new Set<string>();
    commands.add(binding.commandId);
    byAccelerator.set(key, commands);
  }
  const conflicts: { accelerator: string; commands: string[] }[] = [];
  for (const [accelerator, commands] of byAccelerator) {
    if (commands.size > 1) {
      conflicts.push({ accelerator, commands: [...commands].sort() });
    }
  }
  return conflicts.sort((a, b) => a.accelerator.localeCompare(b.accelerator));
}

/** Default bindings derived from the command table. */
export function defaultBindings(): Keybinding[] {
  return COMMANDS.map((command) => ({
    commandId: command.id,
    accelerator: command.accelerator,
  }));
}

/** A key event matches an accelerator. */
export function eventMatches(event: KeyboardEvent, accelerator: string): boolean {
  const parts = expandAccelerator(accelerator)
    .split('+')
    .map((part) => part.trim());
  const key = parts[parts.length - 1];
  const modifiers = new Set(parts.slice(0, -1));

  const wantsCtrl = modifiers.has('Ctrl');
  const wantsCmd = modifiers.has('Cmd');
  const wantsAlt = modifiers.has('Alt');
  const wantsShift = modifiers.has('Shift');

  if (wantsCtrl !== event.ctrlKey) return false;
  if (wantsCmd !== event.metaKey) return false;
  if (wantsAlt !== event.altKey) return false;
  if (wantsShift !== event.shiftKey) return false;

  return event.key.toLowerCase() === key.toLowerCase();
}
