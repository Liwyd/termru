/**
 * A request from the palette (or the sidebar) to a page that may not exist yet:
 * "start a new folder", "start a new identity", "show this section". The page
 * consumes it when it mounts, or at once when it is already open (the same
 * hand-off `hosts.searchFocus` makes for the hosts search field).
 */
export type PageIntent =
  | { kind: 'new-folder' }
  | { kind: 'new-identity' }
  /** A section of the Preferences page, by the id of its heading. */
  | { kind: 'preferences-section'; id: string };

class PageIntents {
  pending = $state.raw<PageIntent | null>(null);

  request(intent: PageIntent): void {
    this.pending = intent;
  }

  /** Takes the pending intent when it is of `kind`; anything else stays for its page. */
  take<K extends PageIntent['kind']>(kind: K): Extract<PageIntent, { kind: K }> | null {
    const intent = this.pending;
    if (!intent || intent.kind !== kind) return null;
    this.pending = null;
    return intent as Extract<PageIntent, { kind: K }>;
  }
}

export const intents = new PageIntents();
