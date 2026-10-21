import { fuzzyRank } from '../fuzzy';
import { COMMANDS, type Command, type CommandId } from '../keybindings';

/** A palette entry: a command or a host shortcut. */
export interface PaletteEntry {
  id: string;
  title: string;
  group: string;
  /** Accelerator shown next to the entry. */
  accelerator?: string;
  /** What to run when the entry is chosen. */
  run: () => void;
}

class PaletteStore {
  open = $state(false);
  query = $state('');
  entries = $state<PaletteEntry[]>([]);
  selected = $state(0);

  /** Opens the palette with the given entries. */
  show(entries: PaletteEntry[]): void {
    this.entries = entries;
    this.query = '';
    this.selected = 0;
    this.open = true;
  }

  hide(): void {
    this.open = false;
    this.query = '';
    this.entries = [];
  }

  /** Entries matching the current query, best first. */
  get results(): PaletteEntry[] {
    return fuzzyRank(this.query, this.entries, (entry) => `${entry.group} ${entry.title}`);
  }

  move(delta: number): void {
    const count = this.results.length;
    if (count === 0) return;
    this.selected = (this.selected + delta + count) % count;
  }

  /** Runs the highlighted entry. */
  choose(): void {
    const entry = this.results[this.selected];
    if (!entry) return;
    this.hide();
    entry.run();
  }
}

/** Builds palette entries for the command table. */
export function commandEntries(
  run: (id: CommandId) => void,
  acceleratorOf: (command: Command) => string,
): PaletteEntry[] {
  return COMMANDS.map((command) => ({
    id: command.id,
    title: command.title,
    group: command.group,
    accelerator: acceleratorOf(command),
    run: () => run(command.id),
  }));
}

export const palette = new PaletteStore();
