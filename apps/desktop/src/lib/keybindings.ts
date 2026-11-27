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
 * * `Plus` / `Minus` name the `+` and `-` keys (`+` is the separator)
 *
 * The app shortcuts share the keyboard with a shell. On macOS `Cmd+<letter>`
 * never reaches a shell, but on Linux and Windows `Ctrl+<letter>` is a
 * readline/terminal control (Ctrl+W deletes a word, Ctrl+D ends input,
 * Ctrl+K kills the line…). The commands that would collide therefore have a
 * second default for Linux and Windows: `Ctrl+Shift+<letter>`.
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
  /**
   * The default on Linux and Windows when `accelerator` would collide with
   * a shell control. Absent = the same as `accelerator`.
   */
  pcAccelerator?: string;
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

/**
 * The parts of an accelerator. A stored `Ctrl++` (from before `Plus` had a
 * name) still means Ctrl and the plus key.
 */
function splitAccelerator(accelerator: string): string[] {
  const canonical = accelerator.trim().replace(/\+\+$/, '+Plus');
  return canonical.split('+').map((part) => part.trim());
}

/** Expands `Mod` to the platform modifier. */
export function expandAccelerator(accelerator: string): string {
  const mod = primaryModifier();
  return splitAccelerator(accelerator)
    .map((part) => (part === 'Mod' ? mod : part))
    .join('+');
}

/** The default accelerator of a command on this platform. */
export function defaultAccelerator(command: Command, mac: boolean = isMac()): string {
  return mac ? command.accelerator : (command.pcAccelerator ?? command.accelerator);
}

/** How an accelerator is shown to the user (`⌘K` on macOS, `Ctrl+Shift+P` elsewhere). */
export function acceleratorLabel(accelerator: string, mac: boolean = isMac()): string {
  const parts = splitAccelerator(accelerator).map((part) => {
    if (part === 'Mod') return mac ? 'Cmd' : 'Ctrl';
    if (part === 'Plus') return '+';
    if (part === 'Minus') return '-';
    return part;
  });
  if (!mac) return parts.join('+');
  const glyphs: Record<string, string> = { Cmd: '⌘', Ctrl: '⌃', Alt: '⌥', Shift: '⇧' };
  return parts.map((part) => glyphs[part] ?? part).join('');
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
  {
    id: 'palette.open',
    title: 'Command palette',
    accelerator: 'Mod+K',
    pcAccelerator: 'Mod+Shift+P',
    group: 'General',
  },
  {
    id: 'tab.new',
    title: 'New tab',
    accelerator: 'Mod+T',
    pcAccelerator: 'Mod+Shift+T',
    group: 'Tabs',
  },
  {
    id: 'tab.close',
    title: 'Close tab',
    accelerator: 'Mod+W',
    pcAccelerator: 'Mod+Shift+W',
    group: 'Tabs',
  },
  { id: 'tab.next', title: 'Next tab', accelerator: 'Mod+Tab', group: 'Tabs' },
  { id: 'tab.previous', title: 'Previous tab', accelerator: 'Mod+Shift+Tab', group: 'Tabs' },
  {
    id: 'tab.duplicate',
    title: 'Duplicate tab',
    accelerator: 'Mod+D',
    pcAccelerator: 'Mod+Shift+D',
    group: 'Tabs',
  },
  {
    id: 'host.new',
    title: 'Add host',
    accelerator: 'Mod+N',
    pcAccelerator: 'Mod+Shift+N',
    group: 'Hosts',
  },
  {
    id: 'host.search',
    title: 'Search hosts',
    accelerator: 'Mod+F',
    pcAccelerator: 'Mod+Shift+F',
    group: 'Hosts',
  },
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
  {
    id: 'pane.close',
    title: 'Close the pane',
    accelerator: 'Mod+Shift+W',
    // Ctrl+Shift+W closes the tab on Linux and Windows.
    pcAccelerator: 'Mod+Shift+Q',
    group: 'Panes',
  },
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
export function defaultBindings(mac: boolean = isMac()): Keybinding[] {
  return COMMANDS.map((command) => ({
    commandId: command.id,
    accelerator: defaultAccelerator(command, mac),
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
  return COMMANDS.filter((command) => {
    const accelerator = overrides[command.id] ?? defaultAccelerator(command);
    return accelerator.trim().length > 0;
  }).map((command) => ({
    commandId: command.id,
    accelerator: overrides[command.id] ? overrides[command.id] : defaultAccelerator(command),
  }));
}

/**
 * The accelerator of one command, with the overrides applied. `mac` asks for
 * another platform's default (the Preferences table shows both columns); an
 * override applies to every platform as it was recorded.
 */
export function acceleratorFor(
  commandId: CommandId,
  overrides: Record<string, string> = {},
  mac: boolean = isMac(),
): string {
  const command = COMMANDS.find((entry) => entry.id === commandId);
  return overrides[commandId] ?? (command ? defaultAccelerator(command, mac) : '');
}

/**
 * The keys of an accelerator as separate labels (`['Ctrl', 'Shift', 'K']`, or
 * `['⌘', '⇧', 'K']` on macOS), for a table that sets them apart. An empty
 * accelerator has no keys.
 */
export function acceleratorParts(accelerator: string, mac: boolean = isMac()): string[] {
  if (!accelerator.trim()) return [];
  const glyphs: Record<string, string> = { Cmd: '⌘', Ctrl: '⌃', Alt: '⌥', Shift: '⇧' };
  return splitAccelerator(accelerator).map((part) => {
    if (part === 'Mod') return mac ? '⌘' : 'Ctrl';
    if (part === 'Plus') return '+';
    if (part === 'Minus') return '-';
    return mac ? (glyphs[part] ?? part) : part;
  });
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

  // `+` and `-` have names (`+` separates the parts). Their shifted forms
  // (`=` is `+` on a US keyboard, `_` is `-`) are the same key, and the
  // shift they need is not part of the shortcut.
  const named = key === '+' ? 'Plus' : key === '-' || key === '_' ? 'Minus' : null;

  const parts: string[] = [];
  if (event.ctrlKey) parts.push('Ctrl');
  if (event.altKey) parts.push('Alt');
  if (event.shiftKey && named === null) parts.push('Shift');
  if (event.metaKey) parts.push('Cmd');

  // A single character is stored in upper case, a named key (F5, ArrowLeft)
  // as it is.
  parts.push(named ?? (key.length === 1 ? key.toUpperCase() : key));
  return parts.join('+');
}

/** Whether an event's key is the key an accelerator names. */
function keyMatches(eventKey: string, accelKey: string): boolean {
  const wanted = accelKey.toLowerCase();
  if (wanted === 'plus') return eventKey === '+' || eventKey === '=';
  if (wanted === 'minus') return eventKey === '-' || eventKey === '_';
  return eventKey.toLowerCase() === wanted;
}

/** A key event matches an accelerator. */
export function eventMatches(event: KeyboardEvent, accelerator: string): boolean {
  const parts = splitAccelerator(expandAccelerator(accelerator));
  const key = parts[parts.length - 1];
  const modifiers = new Set(parts.slice(0, -1));

  const wantsCtrl = modifiers.has('Ctrl');
  const wantsCmd = modifiers.has('Cmd');
  const wantsAlt = modifiers.has('Alt');
  const wantsShift = modifiers.has('Shift');
  // `+` and `-` are typed with Shift on many layouts: Shift does not decide.
  const shiftFree = /^(plus|minus)$/i.test(key);

  if (wantsCtrl !== event.ctrlKey) return false;
  if (wantsCmd !== event.metaKey) return false;
  if (wantsAlt !== event.altKey) return false;
  if (!shiftFree && wantsShift !== event.shiftKey) return false;

  return keyMatches(event.key, key);
}

/**
 * The command a key event is bound to, if any. The terminal uses this to
 * leave app shortcuts to the window instead of sending them to the shell.
 */
export function matchBinding(
  event: KeyboardEvent,
  overrides: Record<string, string> = {},
): Keybinding | null {
  for (const binding of effectiveBindings(overrides)) {
    if (eventMatches(event, binding.accelerator)) return binding;
  }
  return null;
}
