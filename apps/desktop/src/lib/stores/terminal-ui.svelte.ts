import { getTerminal } from '../terminal/registry';

/** A context menu on a terminal. */
export interface TerminalMenuState {
  paneId: string;
  /** Where the pointer was (or the pane, for the keyboard). */
  x: number;
  y: number;
  /** The web link under the pointer, if any. */
  link: string | null;
  /** True when a pointer opened it (the menu may animate; a key never does). */
  animate: boolean;
}

/**
 * The transient state around the terminals: which pane shows its find bar,
 * and the open context menu. The terminal
 * contents themselves live in the registry, not here.
 */
class TerminalUi {
  /** The panes whose find bar is open. */
  #searching = $state<Record<string, boolean>>({});
  /** Bumped when the find shortcut is pressed again: the field takes the focus. */
  searchFocusTick = $state(0);
  /** The open terminal context menu, or null. */
  menu = $state.raw<TerminalMenuState | null>(null);

  isSearchOpen(paneId: string): boolean {
    return this.#searching[paneId] === true;
  }

  openSearch(paneId: string): void {
    this.#searching[paneId] = true;
    this.searchFocusTick += 1;
  }

  /** Closes the find bar, clears the highlights, and gives the keyboard back. */
  closeSearch(paneId: string): void {
    if (!this.#searching[paneId]) return;
    delete this.#searching[paneId];
    const handle = getTerminal(paneId);
    if (handle && !handle.disposed) {
      handle.search.clearDecorations();
      handle.term.focus();
    }
  }

  /**
   * The page is going away: nothing transient may be waiting when it comes
   * back (an open menu, a find bar over highlights nobody sees any more).
   */
  resetTransient(): void {
    this.menu = null;
    for (const paneId of Object.keys(this.#searching)) {
      const handle = getTerminal(paneId);
      if (handle && !handle.disposed) handle.search.clearDecorations();
    }
    this.#searching = {};
  }

  openMenu(menu: TerminalMenuState): void {
    this.menu = menu;
  }

  closeMenu(): void {
    this.menu = null;
  }
}

export const terminalUi = new TerminalUi();
