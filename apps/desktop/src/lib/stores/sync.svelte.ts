import { api } from '../ipc';
import { errorMessage } from './hosts.svelte';
import { toasts } from './toasts.svelte';
import type { ConflictView, SyncOutcome, SyncStatus } from '../types';

/**
 * The state of the cloud sync feature.
 *
 * The tokens never reach the frontend: the backend keeps them in the OS
 * credential store, so the UI only ever sees the status, the settings and
 * the conflicts.
 */
export class SyncStore {
  status = $state<SyncStatus | null>(null);
  conflicts = $state<ConflictView[]>([]);
  loading = $state(false);
  busy = $state(false);
  error = $state<string | null>(null);

  /** Whether the app is signed in to a sync server. */
  get signedIn(): boolean {
    return this.status?.signed_in ?? false;
  }

  /** The number of local changes waiting to be pushed. */
  get pending(): number {
    return this.status?.pending ?? 0;
  }

  /** Whether the backend reported a failure. */
  get lastError(): string | null {
    return this.error ?? this.status?.last_error ?? null;
  }

  /** The number of unresolved conflicts. */
  get conflictCount(): number {
    return this.status?.conflicts ?? this.conflicts?.length ?? 0;
  }

  /** Reads the status and the conflicts from the backend. */
  async load(): Promise<void> {
    this.loading = true;
    try {
      this.status = await api.syncStatus();
      const conflicts = await api.syncConflicts();
      this.conflicts = Array.isArray(conflicts) ? conflicts : [];
      this.error = this.status?.last_error ?? null;
    } catch (err) {
      this.error = errorMessage(err);
    } finally {
      this.loading = false;
    }
  }

  /** Signs in and stores the tokens in the credential store. */
  async signIn(
    serverUrl: string,
    email: string,
    password: string,
    deviceName: string,
  ): Promise<boolean> {
    this.busy = true;
    this.error = null;
    try {
      this.status = await api.syncSignIn(serverUrl, email, password, deviceName);
      await this.load();
      toasts.success('Signed in to the sync server.');
      return true;
    } catch (err) {
      this.error = errorMessage(err);
      toasts.error(this.error);
      return false;
    } finally {
      this.busy = false;
    }
  }

  /** Forgets the tokens and the server configuration. */
  async signOut(): Promise<void> {
    this.busy = true;
    try {
      await api.syncSignOut();
      this.status = null;
      this.conflicts = [];
      toasts.success('Signed out of the sync server.');
    } catch (err) {
      this.error = errorMessage(err);
      toasts.error(this.error);
    } finally {
      this.busy = false;
    }
  }

  /** Runs a sync cycle. */
  async syncNow(): Promise<SyncOutcome | null> {
    this.busy = true;
    this.error = null;
    try {
      const outcome = await api.syncNow();
      await this.load();
      toasts.success(
        `Synced: ${outcome.pushed} sent, ${outcome.applied} received, ${outcome.conflicts} conflict(s).`,
      );
      return outcome;
    } catch (err) {
      this.error = errorMessage(err);
      toasts.error(this.error);
      return null;
    } finally {
      this.busy = false;
    }
  }

  /** Resolves a conflict in favour of the local or the remote copy. */
  async resolve(recordId: string, keepLocal: boolean): Promise<void> {
    this.busy = true;
    try {
      await api.syncResolveConflict(recordId, keepLocal);
      await this.load();
      toasts.success(keepLocal ? 'Kept the local copy.' : 'Took the server copy.');
    } catch (err) {
      this.error = errorMessage(err);
      toasts.error(this.error);
    } finally {
      this.busy = false;
    }
  }
}

export const sync = new SyncStore();
