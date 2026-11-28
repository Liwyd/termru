import { api } from '../ipc';
import { resolveRoute } from '../jump';
import { hosts } from './hosts.svelte';
import { route } from './route.svelte';
import { sessions, type HopAnswers, type Pane } from './sessions.svelte';
import type { HopSecrets, Host, SessionSecrets } from '../types';

/** A jump host the dialog asks a password for. */
export interface HopPrompt {
  host: Host;
  /** The credential store holds a password for it (an empty answer uses that one). */
  stored: boolean;
}

/** What the connect dialog is collecting a secret for. */
export interface ConnectRequest {
  host: Host;
  /** A secret for this host already sits in the credential store. */
  secretStored: boolean;
  /** The host itself needs a password typed (false: only its jump hosts do). */
  askTarget: boolean;
  /**
   * The jump hosts of the route that sign in with a password nobody has
   * given yet, in route order: the dialog has one field for each.
   */
  hops: HopPrompt[];
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

  /** Opens a new tab for a host (asking for the secrets when needed). */
  async request(host: Host): Promise<void> {
    const stored = needsSecretUpFront(host) && (await this.#hasStoredSecret(host));
    const askTarget = needsSecretUpFront(host) && !stored;
    const hops = await this.#hopsToAsk(host, {});
    if (!askTarget && hops.length === 0) {
      this.#openTab(host, {}, false);
      return;
    }
    this.pending = { host, secretStored: stored, askTarget, hops, pane: null, reconnecting: false };
  }

  /** Connects a host in an empty pane (the pane picker). */
  async pick(pane: Pane, host: Host): Promise<void> {
    const stored = needsSecretUpFront(host) && (await this.#hasStoredSecret(host));
    const askTarget = needsSecretUpFront(host) && !stored;
    const hops = await this.#hopsToAsk(host, {});
    if (!askTarget && hops.length === 0) {
      await sessions.connectPane(pane, host, {}, false);
      return;
    }
    this.pending = { host, secretStored: stored, askTarget, hops, pane, reconnecting: false };
  }

  /**
   * Reconnects a pane. A password host without a secret to use (a restored
   * pane, or one whose authentication failed) asks for it first.
   */
  async reconnect(pane: Pane): Promise<void> {
    const host = pane.host;
    if (!host) return;
    const target = sessions.paneById(pane.id) ?? pane;
    const authFailed = target.lastFailure?.category === 'auth';
    // The backend keeps the credentials of a session it still knows (the
    // jump hosts' too), and reconnects it without them.
    const backendHolds = await this.#backendKnows(target.sessionId);
    const hops = backendHolds
      ? []
      : await this.#hopsToAsk(host, target.hopSecrets ?? {}, authFailed);
    if (!needsSecretUpFront(host) && hops.length === 0) {
      sessions.retry(target);
      return;
    }
    const stored = needsSecretUpFront(host) && (await this.#hasStoredSecret(host));
    const haveSecret = Boolean(target.secrets.password || target.secrets.passphrase);
    const askTarget =
      needsSecretUpFront(host) && (authFailed || (!stored && !haveSecret && !backendHolds));
    if (!askTarget && hops.length === 0) {
      sessions.retry(target);
      return;
    }
    this.pending = {
      host,
      secretStored: stored,
      askTarget,
      hops,
      pane: target,
      reconnecting: true,
    };
  }

  /** The dialog collected the secrets: go on with the connection. */
  submit(secrets: SessionSecrets, remember: boolean, hops?: HopAnswers): void {
    const request = this.pending;
    this.pending = null;
    if (!request) return;
    const typed = hops && Object.keys(hops.secrets).length > 0 ? hops : undefined;
    if (!request.pane) {
      this.#openTab(request.host, secrets, remember, typed);
    } else if (request.reconnecting) {
      sessions.retry(request.pane, secrets, remember, typed);
    } else {
      void sessions.connectPane(request.pane, request.host, secrets, remember, typed);
    }
  }

  cancel(): void {
    this.pending = null;
  }

  #openTab(host: Host, secrets: SessionSecrets, remember: boolean, hops?: HopAnswers): void {
    sessions.open(host, secrets, remember, hops);
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

  /**
   * The jump hosts of the route to `host` that sign in with a password
   * nobody has given: not typed already (`have`) and not in the credential
   * store. `all` asks for every password hop (a sign-in failed and it is
   * not known which one). A route that cannot be resolved asks nothing: the
   * backend reports why when it builds the connection.
   */
  async #hopsToAsk(host: Host, have: HopSecrets, all = false): Promise<HopPrompt[]> {
    const chain = host.jump_chain ?? [];
    if (chain.length === 0) return [];
    const found = resolveRoute(host.id, host.name, chain, (id) => {
      const hop = hosts.hosts.find((entry) => entry.id === id && !entry.deleted);
      return hop ? { name: hop.name, jump_chain: hop.jump_chain ?? [] } : undefined;
    });
    if (!found.ok) return [];
    const asked: HopPrompt[] = [];
    for (const id of found.route) {
      const hop = hosts.hosts.find((entry) => entry.id === id);
      if (!hop || !hopNeedsPassword(hop)) continue;
      if (have[id]?.password && !all) continue;
      let stored = false;
      try {
        stored = (await api.secretHas(id, 'password')) === true;
      } catch {
        stored = false;
      }
      if (stored && !all) continue;
      asked.push({ host: hop, stored });
    }
    return asked;
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

/**
 * Whether a jump host needs a password from the user: it signs in with one
 * (and an explicit order, if it has one, lists it).
 */
export function hopNeedsPassword(host: Host): boolean {
  if (host.auth.type !== 'password') return false;
  return !host.auth_order || host.auth_order.includes('password');
}

export const connectFlow = new ConnectFlow();
