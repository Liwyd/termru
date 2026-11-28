import { api } from '../ipc';
import { notify } from '../notify.svelte';
import { errorMessage, hosts } from './hosts.svelte';
import type { SshConfigCandidate, SshConfigImportReport, SshConfigPreview } from '../types';

/** What the import dialog is doing. */
export type ImportPhase = 'loading' | 'ready' | 'error' | 'importing' | 'done';

/** Why a candidate cannot be ticked, or null when it can. */
export function whyNotImportable(candidate: SshConfigCandidate): string | null {
  if (candidate.existing_host_id) {
    return 'Your library already has a host with this address, user and port.';
  }
  return candidate.problem;
}

/**
 * The import of `~/.ssh/config` (ADR 0010): read the file (`ssh_config_preview`),
 * let the user tick the hosts to bring in, import them in one transaction
 * (`ssh_config_import`) and say what happened. Key files are only referenced
 * by path, never read or copied.
 */
class SshImportStore {
  open = $state(false);
  phase = $state<ImportPhase>('loading');
  preview = $state.raw<SshConfigPreview | null>(null);
  report = $state.raw<SshConfigImportReport | null>(null);
  error = $state<string | null>(null);
  /** The aliases that are ticked. */
  selected = $state<string[]>([]);
  /** Counts runs, so an answer for a dialog that was closed meanwhile is ignored. */
  #token = 0;

  /** The candidates that can be imported (new, no problem). */
  get importable(): SshConfigCandidate[] {
    return (this.preview?.candidates ?? []).filter((entry) => whyNotImportable(entry) === null);
  }

  /** Opens the dialog and reads the file. */
  async show(): Promise<void> {
    const token = ++this.#token;
    this.open = true;
    this.phase = 'loading';
    this.error = null;
    this.report = null;
    this.selected = [];
    try {
      const preview = await api.sshConfigPreview();
      if (token !== this.#token) return;
      this.preview = preview;
      // Everything new is ticked: the common case is "bring it all in".
      this.selected = this.importable.map((entry) => entry.alias);
      this.phase = 'ready';
    } catch (err) {
      if (token !== this.#token) return;
      this.preview = null;
      this.error = errorMessage(err);
      this.phase = 'error';
    }
  }

  close(): void {
    this.#token += 1;
    this.open = false;
  }

  toggle(alias: string): void {
    const candidate = this.preview?.candidates.find((entry) => entry.alias === alias);
    if (!candidate || whyNotImportable(candidate) !== null) return;
    this.selected = this.selected.includes(alias)
      ? this.selected.filter((entry) => entry !== alias)
      : [...this.selected, alias];
  }

  selectAll(): void {
    this.selected = this.importable.map((entry) => entry.alias);
  }

  selectNone(): void {
    this.selected = [];
  }

  /**
   * The jump hosts of the ticked hosts that are in the file but not ticked:
   * the import brings them in too (a route needs its hops).
   */
  get alsoImported(): string[] {
    const wanted = new Set(this.selected);
    const known = new Map((this.preview?.candidates ?? []).map((entry) => [entry.alias, entry]));
    const extra = new Set<string>();
    for (const alias of this.selected) {
      for (const hop of known.get(alias)?.jump_chain ?? []) {
        const hopHost = known.get(hop);
        if (hopHost && !wanted.has(hop) && whyNotImportable(hopHost) === null) extra.add(hop);
      }
    }
    return [...extra];
  }

  /** Imports what is ticked. */
  async run(): Promise<void> {
    if (this.phase === 'importing' || this.selected.length === 0) return;
    const token = this.#token;
    this.phase = 'importing';
    this.error = null;
    try {
      const report = await api.sshConfigImport([...this.selected]);
      if (token !== this.#token) {
        // The dialog was closed while it ran: the hosts are there all the same.
        await hosts.load();
        return;
      }
      this.report = report;
      this.phase = 'done';
      await hosts.load();
      notify({
        key: 'hosts.imported',
        values: { count: report.imported.length, skipped: report.skipped.length },
      });
    } catch (err) {
      if (token !== this.#token) return;
      this.error = errorMessage(err);
      // Nothing was stored (one transaction): the choice is still there.
      this.phase = 'ready';
      notify({ key: 'hosts.import-failed', values: { detail: this.error } });
    }
  }
}

export const sshImport = new SshImportStore();
