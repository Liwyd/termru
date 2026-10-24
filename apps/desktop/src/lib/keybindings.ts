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
  | 'view.decreaseFont'
  | 'sync.open'
  | 'settings.open'
  | 'pane.splitRight'
  | 'pane.splitDown'
  | 'pane.close'
  | 'pane.next'
  | 'pane.previous'
  | 'route.hosts'
  | 'route.sessions'
  | 'route.folders'
  | 'route.identities'
  | 'route.sync'
  | 'route.preferences';

export interface Command {
  id: CommandId;
  title: string;
  /** Default accelerator, in `Mod+Shift+K` notation. Empty = unbound. */
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
  { id: 'sync.open', title: 'Cloud sync', accelerator: 'Mod+Shift+S', group: 'Sync' },
  { id: 'settings.open', title: 'Settings', accelerator: 'Mod+,', group: 'View' },
  {
    id: 'pane.splitRight',
    title: 'Split the pane to the right',
    accelerator: 'Mod+Shift+E',
    group: 'Panes',
  },
  {
    id: 'pane.splitDown',
    title: 'Split the pane below',
    accelerator: 'Mod+Shift+O',
    group: 'Panes',
  },
  { id: 'pane.close', title: 'Close the pane', accelerator: 'Mod+Shift+W', group: 'Panes' },
  { id: 'pane.next', title: 'Next pane', accelerator: 'Mod+Alt+ArrowRight', group: 'Panes' },
  { id: 'pane.previous', title: 'Previous pane', accelerator: 'Mod+Alt+ArrowLeft', group: 'Panes' },
  // Route commands are reachable from the palette; they carry no default
  // accelerator so they never collide with page-specific keys. The nav and
  // `settings.open` / `sync.open` cover the keyboard paths.
  { id: 'route.hosts', title: 'Go to Hosts', accelerator: '', group: 'Navigate' },
  { id: 'route.sessions', title: 'Go to Sessions', accelerator: '', group: 'Navigate' },
  { id: 'route.folders', title: 'Go to Folders', accelerator: '', group: 'Navigate' },
  { id: 'route.identities', title: 'Go to Identities', accelerator: '', group: 'Navigate' },
  { id: 'route.sync', title: 'Go to Sync', accelerator: '', group: 'Navigate' },
  { id: 'route.preferences', title: 'Go to Preferences', accelerator: '', group: 'Navigate' },
];

/**
 * Finds conflicting bindings: the same accelerator bound to two different
 * commands. Returns the offending pairs. Unbound commands (empty
 * accelerator) never conflict.
 */
export function findConflicts(
  bindings: Keybinding[],
): { accelerator: string; commands: string[] }[] {
  const byAccelerator = new Map<string, Set<string>>();
  for (const binding of bindings) {
    if (!binding.accelerator.trim()) continue;
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

/**
 * The bindings in effect: the defaults, with the user's overrides on top.
 *
 * An override for a command that does not exist is ignored — the map is
 * synced between devices, and a device may know more commands than another.
 * Unbound commands (empty accelerator) are kept out of the list: they are
 * palette-only and must never capture a key.
 */
export function effectiveBindings(overrides: Record<string, string> = {}): Keybinding[] {
  const known = new Set(COMMANDS.map((command) => command.id));
  return COMMANDS.filter((command) => {
    const accelerator = overrides[command.id] ?? command.accelerator;
    return known.has(command.id) && accelerator.trim().length > 0;
  }).map((command) => ({
    commandId: command.id,
    accelerator:
      known.has(command.id) && overrides[command.id] ? overrides[command.id] : command.accelerator,
  }));
}

/** The accelerator of one command, with the overrides applied. */
export function acceleratorFor(
  commandId: CommandId,
  overrides: Record<string, string> = {},
): string {
  return overrides[commandId] ?? COMMANDS.find((c) => c.id === commandId)?.accelerator ?? '';
}

/**
 * The command that already uses an accelerator, if any.
 *
 * `except` leaves one command out of the search, so a command can keep its
 * own accelerator.
 */
export function conflictFor(
  accelerator: string,
  bindings: Keybinding[],
  except?: CommandId,
): CommandId | null {
  if (!accelerator.trim()) return null;
  const wanted = normalizeAccelerator(accelerator);
  for (const binding of bindings) {
    if (binding.commandId === except) continue;
    if (!binding.accelerator.trim()) continue;
    if (normalizeAccelerator(binding.accelerator) === wanted) return binding.commandId;
  }
  return null;
}

/** The title of a command, for a message about a conflict. */
export function commandTitle(commandId: CommandId): string {
  return COMMANDS.find((command) => command.id === commandId)?.title ?? commandId;
}

/** The keys that are modifiers on their own and carry no binding. */
const MODIFIER_KEYS = new Set(['Control', 'Shift', 'Alt', 'Meta', 'AltGraph', 'CapsLock']);

/**
 * Turns a key event into an accelerator (`Ctrl+Shift+K`).
 *
 * Returns null when the event carries no key (a modifier pressed on its
 * own), so the recorder can wait for a real combination.
 */
export function eventToAccelerator(event: KeyboardEvent): string | null {
  const key = event.key;
  if (!key || MODIFIER_KEYS.has(key)) return null;

  const parts: string[] = [];
  if (event.ctrlKey) parts.push('Ctrl');
  if (event.altKey) parts.push('Alt');
  if (event.shiftKey) parts.push('Shift');
  if (event.metaKey) parts.push('Cmd');

  // A single character is stored in upper case, a named key (F5, ArrowLeft)
  // as it is.
  const name = key.length === 1 ? key.toUpperCase() : key;
  parts.push(name);
  return parts.join('+');
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
