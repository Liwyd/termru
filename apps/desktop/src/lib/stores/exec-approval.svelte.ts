import { api } from '../ipc';
import { notify } from '../notify.svelte';
import { errorMessage } from './hosts.svelte';
import type { HostExecReview } from '../types';

/**
 * The review of the exec-like fields of a host (ADR 0010, "synced fields that
 * act"): the startup command, agent forwarding, environment variables and the
 * forwards that are not loopback only.
 *
 * They arrive through sync, so each device keeps its own approval of them. A
 * connection to a host whose fields are not approved leaves them out and says
 * so; this is the dialog that shows exactly what was left out and approves it.
 *
 * What is approved is never what the frontend says: `approve` sends the hash
 * the backend computed for the review, and the backend recomputes it from the
 * stored host and refuses it when the fields changed after the review.
 */
class ExecApprovalStore {
  /** The review on screen (kept while the dialog plays its exit); null when there is none. */
  current = $state.raw<HostExecReview | null>(null);
  /** The dialog is open. */
  open = $state(false);
  /** A review is being read (the dialog opens when it is). */
  loading = $state(false);
  busy = $state(false);
  /** Why the review could not be read or approved. */
  error = $state<string | null>(null);
  /** Counts reviews, so a slow answer for an earlier one is ignored. */
  #token = 0;

  /**
   * Opens the review of a host: reads what its fields are right now, then
   * shows the dialog (with its content in place, so the focus lands on the
   * button). A host that cannot be read opens it with the reason.
   */
  async show(hostId: string): Promise<void> {
    const token = ++this.#token;
    this.loading = true;
    this.error = null;
    // Another host's settings are not shown while these are read.
    if (this.current?.host_id !== hostId) this.current = null;
    try {
      const loaded = await api.hostExecReview(hostId);
      if (token !== this.#token) return;
      this.current = loaded;
      this.open = true;
    } catch (err) {
      if (token !== this.#token) return;
      this.current = null;
      this.error = errorMessage(err);
      this.open = true;
    } finally {
      if (token === this.#token) this.loading = false;
    }
  }

  close(): void {
    this.#token += 1;
    this.open = false;
    this.busy = false;
    this.loading = false;
  }

  /** Approves what the review shows. */
  async approve(): Promise<void> {
    const shown = this.current;
    if (!shown || this.busy) return;
    this.busy = true;
    this.error = null;
    try {
      await api.hostExecApprove(shown.host_id, shown.hash);
      notify({
        key: 'host.exec-approved',
        entityId: shown.host_id,
        values: { name: shown.host_name },
      });
      this.close();
    } catch (err) {
      const reason = errorMessage(err);
      // The fields may have changed after the review: show what they are
      // now, and say why the click did not approve them.
      await this.show(shown.host_id);
      this.error = reason;
    } finally {
      this.busy = false;
    }
  }
}

export const execApproval = new ExecApprovalStore();
