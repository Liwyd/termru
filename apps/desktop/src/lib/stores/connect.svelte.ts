import { api } from '../ipc';
import { hosts } from './hosts.svelte';
import { route } from './route.svelte';
import { sessions, type Pane } from './sessions.svelte';
import type { Host, SessionSecrets } from '../types';

/** What the connect dialog is collecting a secret for. */
export interface ConnectRequest {
  host: Host;
  /** A secret for this host already sits in the credential store. */
  secretStored: boolean;
  /**
   * The pane the connection is for: null opens a new tab; otherwise the
   * pane is connected (it was empty) or reconnected (it was closed).
   */
  pane: Pane | null;
  /** Whether `pane` is being reconnected (true) or filled (false). */
  reconnecting: boolean;
}

/**
 * The flow that starts a connection and asks for the secret when it needs
 * one: a new tab, a host picked for an empty pane, or the reconnect of a pane
 * (a restored pane, or one that failed to authenticate).
 *
 * The dialog is shown only for a password host whose password is neither
 * supplied nor stored in the credential store; with a stored secret the
 * backend falls back to the credential store on its own, so the secrets are
 * left out. A host with a key file or the agent never asks up front: whether
 * the key needs a passphrase is only known once it is read, and the backend
 * asks then (the authentication prompt), so an unencrypted key connects
 * without a question.
 */
class ConnectFlow {
  pending = $state.raw<ConnectRequest | null>(null);

  /** Opens a new tab for a host (asking for the secret when needed). */
  async request(host: Host): Promise<void> {
    if (!needsSecretUpFront(host)) {
      this.#openTab(host, {}, false);
      return;
    }
    const stored = await this.#hasStoredSecret(host);
    if (stored) {
      this.#openTab(host, {}, false);
      return;
    }
    this.pending = { host, secretStored: stored, pane: null, reconnecting: false };
  }

  /** Connects a host in an empty pane (the pane picker). */
  async pick(pane: Pane, host: Host): Promise<void> {
    const stored = needsSecretUpFront(host) && (await this.#hasStoredSecret(host));
    if (!needsSecretUpFront(host) || stored) {
      await sessions.connectPane(pane, host, {}, false);
      return;
    }
    this.pending = { host, secretStored: stored, pane, reconnecting: false };
  }

  /**
   * Reconnects a pane. A password host without a secret to use (a restored
   * pane, or one whose authentication failed) asks for it first.
   */
  async reconnect(pane: Pane): Promise<void> {
    const host = pane.host;
    if (!host) return;
    const target = sessions.paneById(pane.id) ?? pane;
    if (!needsSecretUpFront(host)) {
      sessions.retry(target);
      return;
    }
    const stored = await this.#hasStoredSecret(host);
    const authFailed = target.lastFailure?.category === 'auth';
    const haveSecret = Boolean(target.secrets.password || target.secrets.passphrase);
    // The backend keeps the credentials of a session it still knows, and
    // reconnects it without them.
    const backendHolds = await this.#backendKnows(target.sessionId);
    const needsPrompt =
      authFailed || (host.auth.type === 'password' && !stored && !haveSecret && !backendHolds);
    if (!needsPrompt) {
      sessions.retry(target);
      return;
    }
    this.pending = { host, secretStored: stored, pane: target, reconnecting: true };
  }

  /** The dialog collected the secret: go on with the connection. */
  submit(secrets: SessionSecrets, remember: boolean): void {
    const request = this.pending;
    this.pending = null;
    if (!request) return;
    if (!request.pane) {
      this.#openTab(request.host, secrets, remember);
    } else if (request.reconnecting) {
      sessions.retry(request.pane, secrets, remember);
    } else {
      void sessions.connectPane(request.pane, request.host, secrets, remember);
    }
  }

  cancel(): void {
    this.pending = null;
  }

  #openTab(host: Host, secrets: SessionSecrets, remember: boolean): void {
    sessions.open(host, secrets, remember);
    this.pending = null;
    // The session is the point of the app: show it right away.
    route.go('sessions');
  }

  /**
   * Whether the password of a host sits in the credential store (the other
   * methods need no secret up front). The hosts table shows the same fact.
   */
  async #hasStoredSecret(host: Host): Promise<boolean> {
    if (host.auth.type !== 'password') return false;
    let stored = false;
    try {
      stored = (await api.secretHas(host.id, 'password')) === true;
    } catch {
      stored = false;
    }
    hosts.markPasswordStored(host.id, stored);
    return stored;
  }

  async #backendKnows(sessionId: string | null): Promise<boolean> {
    if (!sessionId) return false;
    try {
      return (await api.sessionList()).some((entry) => entry.id === sessionId);
    } catch {
      return false;
    }
  }
}

/**
 * Whether the connect dialog has to collect a secret before connecting:
 * only for a password. A key's passphrase and the questions of a
 * keyboard-interactive server are asked by the backend when they come up.
 */
function needsSecretUpFront(host: Host): boolean {
  return host.auth.type === 'password';
}

export const connectFlow = new ConnectFlow();
