import { api } from '../ipc';
import { errorMessage } from './hosts.svelte';
import { toasts } from './toasts.svelte';
import type { Host, SessionSecrets, SessionStatus, SessionUpdate } from '../types';

export interface Tab {
  /** Local tab id (stable across reconnects). */
  id: string;
  /** Backend session id, once the connection was started. */
  sessionId: string | null;
  host: Host;
  title: string;
  status: SessionStatus;
  /** In-memory only; needed to re-open the connection after a failure. */
  secrets: SessionSecrets;
  /** Bumped to force a (re)connect from the terminal view. */
  generation: number;
  exitCode: number | null;
  /** Terminal state reached: the tab shows a reconnect affordance. */
  dead: boolean;
  errorMessage: string | null;
  /** Set while the backend is expected to auto-reconnect. */
  awaitingReconnect: boolean;
  hadReady: boolean;
  lastFailure: { category: string; message: string } | null;
  /**
   * When set, the terminal view opens this session id instead of starting a
   * new connection — the backend re-uses the stored credentials.
   */
  duplicateFrom: string | null;
}

/** How long we wait for an expected auto-reconnect before showing "dead". */
const RECONNECT_WATCHDOG_MS = 30_000;

function newTabId(): string {
  return `tab-${Math.random().toString(36).slice(2, 10)}`;
}

/** Open terminal tabs and their session state. */
export class SessionStore {
  tabs = $state<Tab[]>([]);
  activeId = $state<string | null>(null);

  get active(): Tab | null {
    return this.tabs.find((tab) => tab.id === this.activeId) ?? null;
  }

  open(host: Host, secrets: SessionSecrets): Tab {
    const tab: Tab = {
      id: newTabId(),
      sessionId: null,
      host,
      title: host.name,
      status: { state: 'connecting' },
      secrets,
      generation: 0,
      exitCode: null,
      dead: false,
      errorMessage: null,
      awaitingReconnect: false,
      hadReady: false,
      lastFailure: null,
      duplicateFrom: null,
    };
    this.tabs = [...this.tabs, tab];
    this.activeId = tab.id;
    return tab;
  }

  /**
   * Opens a second session to the same host. The new session re-uses the
   * credentials the backend already holds, so it works even when the
   * frontend no longer has the secret.
   */
  duplicate(tab: Tab): Tab | null {
    if (!tab.sessionId) return null;
    const clone: Tab = {
      ...tab,
      id: newTabId(),
      sessionId: null,
      status: { state: 'connecting' },
      dead: false,
      errorMessage: null,
      exitCode: null,
      awaitingReconnect: false,
      hadReady: false,
      lastFailure: null,
      duplicateFrom: tab.sessionId,
    };
    this.tabs = [...this.tabs, clone];
    this.activeId = clone.id;
    return clone;
  }

  async close(tab: Tab): Promise<void> {
    if (tab.sessionId) {
      try {
        await api.sessionClose(tab.sessionId);
      } catch (err) {
        // The session may already be gone; the tab is closing either way.
        if (!/no session/i.test(errorMessage(err))) {
          toasts.error(errorMessage(err));
        }
      }
    }
    this.tabs = this.tabs.filter((entry) => entry.id !== tab.id);
    if (this.activeId === tab.id) {
      const next = this.tabs[this.tabs.length - 1];
      this.activeId = next ? next.id : null;
    }
  }

  rename(tab: Tab, title: string): void {
    const trimmed = title.trim();
    if (trimmed.length === 0) return;
    tab.title = trimmed;
  }

  setActive(id: string): void {
    if (this.tabs.some((tab) => tab.id === id)) this.activeId = id;
  }

  cycleActive(direction: 1 | -1): void {
    if (this.tabs.length === 0) return;
    const index = this.tabs.findIndex((tab) => tab.id === this.activeId);
    const nextIndex = (index + direction + this.tabs.length) % this.tabs.length;
    this.activeId = this.tabs[nextIndex].id;
  }

  /** Applies a backend status event to a tab. */
  applyStatus(tab: Tab, status: SessionStatus): void {
    tab.status = status;
    tab.errorMessage = null;

    switch (status.state) {
      case 'ready':
        tab.hadReady = true;
        tab.dead = false;
        tab.awaitingReconnect = false;
        tab.lastFailure = null;
        break;
      case 'connecting':
      case 'authenticating':
      case 'reconnecting':
        tab.dead = false;
        tab.awaitingReconnect = false;
        tab.lastFailure = null;
        break;
      case 'failed':
        tab.lastFailure = { category: status.category, message: status.message };
        tab.errorMessage = status.message;
        break;
      case 'closed':
        // A network death after `ready` is retried by the backend; a give-up
        // ("gave up after N attempts") or a non-network failure is final.
        if (this.isRetriable(tab) && !/gave up/i.test(status.reason)) {
          tab.dead = false;
          tab.awaitingReconnect = true;
          this.armWatchdog(tab);
        } else {
          tab.dead = true;
          tab.awaitingReconnect = false;
        }
        break;
    }
  }

  private isRetriable(tab: Tab): boolean {
    if (!tab.hadReady) return false;
    const category = tab.lastFailure?.category;
    return category === 'network' || category === 'timeout';
  }

  private armWatchdog(tab: Tab): void {
    setTimeout(() => {
      if (tab.awaitingReconnect) {
        tab.dead = true;
        tab.awaitingReconnect = false;
        tab.errorMessage = tab.errorMessage ?? 'connection lost';
      }
    }, RECONNECT_WATCHDOG_MS);
  }

  /** Applies a channel update to a tab. */
  applyUpdate(tab: Tab, update: SessionUpdate): void {
    switch (update.kind) {
      case 'status':
        this.applyStatus(tab, update.status);
        break;
      case 'exit':
        tab.exitCode = update.code;
        break;
      case 'data':
        // Terminal bytes are handled by the terminal view itself.
        break;
    }
  }

  /** Forces a fresh connection for a tab (retry after failure). */
  retry(tab: Tab): void {
    tab.dead = false;
    tab.errorMessage = null;
    tab.exitCode = null;
    tab.status = { state: 'connecting' };
    tab.generation += 1;
  }

  /** Local failure (e.g. the backend refused to start the connection). */
  fail(tab: Tab, message: string): void {
    tab.errorMessage = message;
    tab.dead = true;
    tab.awaitingReconnect = false;
    tab.status = { state: 'failed', category: 'local', message };
  }
}

export const sessions = new SessionStore();
