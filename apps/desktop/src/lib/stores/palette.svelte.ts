import { fuzzyMatch } from '../fuzzy';
import { COMMANDS, type Command, type CommandId } from '../keybindings';

/** A palette entry: a command, a page, a host, a running session. */
export interface PaletteEntry {
  id: string;
  title: string;
  /** The section it belongs to (`Hosts`, `Go to`, `Panes`…). */
  group: string;
  /** Accelerator shown next to the entry. */
  accelerator?: string;
  /** A second line of text on the entry: an address, a state. */
  detail?: string;
  /** Text the search matches that is not shown (a folder, a synonym). */
  keywords?: string;
  /** What Enter does, for the hint under the list (`Connect`, `Open`, `Run`). */
  verb?: string;
  /** Left out of the list until the user types: a long tail of hosts. */
  searchOnly?: boolean;
  /** What to run when the entry is chosen. */
  run: () => void;
  /** What Shift+Enter does: a second action on the same entry (edit a host). */
  alt?: { verb: string; run: () => void };
}

/** How many results the list shows: ranking 10 000 hosts must not draw 10 000 rows. */
export const MAX_RESULTS = 100;

class PaletteStore {
  open = $state(false);
  /** Raw: a library of 10 000 hosts is 10 000 entries, none of them reactive. */
  entries = $state.raw<PaletteEntry[]>([]);
  selected = $state(0);
  #query = $state('');

  /** The search text. Typing starts the choice over at the best match. */
  get query(): string {
    return this.#query;
  }

  set query(value: string) {
    this.#query = value;
    this.selected = 0;
  }

  /** Opens the palette with the given entries. */
  show(entries: PaletteEntry[]): void {
    this.entries = entries;
    this.#query = '';
    this.selected = 0;
    this.open = true;
  }

  hide(): void {
    this.open = false;
    this.#query = '';
    this.selected = 0;
    this.entries = [];
  }

  /**
   * Entries matching the current query, best first. Without a query it is the
   * start list in the order given (the long tail of hosts is left out until
   * the user types). Derived: one ranking per keystroke, not one per read.
   */
  results = $derived.by((): PaletteEntry[] => {
    const query = this.#query;
    if (query.trim().length === 0) {
      return this.entries.filter((entry) => !entry.searchOnly).slice(0, MAX_RESULTS);
    }
    const needle = query.trim().toLowerCase();
    const ranked: { entry: PaletteEntry; index: number; score: number }[] = [];
    this.entries.forEach((entry, index) => {
      const match = fuzzyMatch(
        query,
        `${entry.title} ${entry.group} ${entry.detail ?? ''} ${entry.keywords ?? ''}`,
      );
      if (!match) return;
      // What is in the title outranks what only matches letter by letter.
      const title = entry.title.toLowerCase();
      const bonus = title.startsWith(needle) ? 80 : title.includes(needle) ? 50 : 0;
      ranked.push({ entry, index, score: match.score + bonus });
    });
    ranked.sort((a, b) => (b.score === a.score ? a.index - b.index : b.score - a.score));
    return ranked.slice(0, MAX_RESULTS).map(({ entry }) => entry);
  });

  move(delta: number): void {
    const count = this.results.length;
    if (count === 0) return;
    this.selected = (this.selected + delta + count) % count;
  }

  /** Runs the highlighted entry (`alt`: its second action, when it has one). */
  choose(alt = false): void {
    const entry = this.results[this.selected];
    if (!entry) return;
    const action = alt && entry.alt ? entry.alt.run : entry.run;
    this.hide();
    action();
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
