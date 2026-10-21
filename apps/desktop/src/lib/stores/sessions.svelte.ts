import { api } from '../ipc';
import { errorMessage, hosts } from './hosts.svelte';
import { toasts } from './toasts.svelte';
import type { Host, SessionSecrets, SessionStatus, SessionUpdate, WorkspaceDoc } from '../types';

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
  /** Restored from a saved layout (not connected yet). */
  restored: boolean;
  /** Set when the view renders a session owned by another window. */
  attachTo: string | null;
}

/** How long we wait for an expected auto-reconnect before showing "dead". */
const RECONNECT_WATCHDOG_MS = 30_000;
/** Debounce for layout persistence. */
const LAYOUT_SAVE_DEBOUNCE_MS = 750;

function newTabId(): string {
  return crypto.randomUUID();
}

/** Open terminal tabs and their session state. */
export class SessionStore {
  tabs = $state<Tab[]>([]);
  activeId = $state<string | null>(null);
  #layoutTimer: ReturnType<typeof setTimeout> | undefined;

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
      restored: false,
      attachTo: null,
    };
    this.tabs = [...this.tabs, tab];
    this.scheduleLayoutSave();
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
      restored: false,
      attachTo: null,
    };
    this.tabs = [...this.tabs, clone];
    this.scheduleLayoutSave();
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
    this.scheduleLayoutSave();
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

  /** Moves a tab to a new index (drag & drop reordering). */
  reorder(from: number, to: number): void {
    // Invalid indices (stale drag state) are ignored, never clamped.
    if (from === to || from < 0 || from >= this.tabs.length) return;
    if (to < 0 || to >= this.tabs.length) return;
    const next = [...this.tabs];
    const [moved] = next.splice(from, 1);
    if (!moved) return;
    next.splice(to, 0, moved);
    this.tabs = next;
    this.scheduleLayoutSave();
  }

  /**
   * Persists the tab layout (hosts + order) so it survives a restart.
   * Restored tabs are placeholders: the user reconnects them explicitly
   * (secrets are never stored).
   */
  scheduleLayoutSave(): void {
    if (this.#layoutTimer) clearTimeout(this.#layoutTimer);
    this.#layoutTimer = setTimeout(() => {
      void this.saveLayout();
    }, LAYOUT_SAVE_DEBOUNCE_MS);
  }

  async saveLayout(): Promise<void> {
    const doc = this.toLayoutDoc();
    try {
      await api.layoutSave(doc);
    } catch {
      // Layout persistence is best-effort; never break the session flow.
    }
  }

  /** Builds the layout document for the current tabs. */
  toLayoutDoc(): WorkspaceDoc {
    const windowId = 'window-main';
    return {
      version: 1,
      windows: [
        {
          id: windowId,
          tabs: this.tabs.map((tab) => ({
            id: tab.id,
            title: tab.title,
            root: { leaf: { pane: `pane-${tab.id}`, host_id: tab.host.id } },
            active_pane: `pane-${tab.id}`,
          })),
          active_tab: this.activeId ?? undefined,
        },
      ],
      active_window: windowId,
    };
  }

  /**
   * Restores the tab layout. Restored tabs are **not** connected: they show
   * a reconnect affordance, because the credentials are not stored.
   */
  async restoreLayout(): Promise<void> {
    let doc: WorkspaceDoc | null = null;
    try {
      doc = await api.layoutLoad();
    } catch {
      return;
    }
    const windowLayout = doc?.windows?.[0];
    if (!windowLayout || windowLayout.tabs.length === 0) return;

    const byId = new Map(hosts.hosts.map((host) => [host.id, host]));
    const restored: Tab[] = [];
    for (const tabLayout of windowLayout.tabs) {
      const leaf = tabLayout.root.leaf;
      const host = leaf?.host_id ? byId.get(leaf.host_id) : undefined;
      if (!host) continue; // the host was deleted meanwhile
      restored.push({
        id: tabLayout.id || newTabId(),
        sessionId: null,
        host,
        title: tabLayout.title ?? host.name,
        status: { state: 'closed', reason: 'not connected' },
        secrets: {},
        generation: 0,
        exitCode: null,
        dead: true,
        errorMessage: null,
        awaitingReconnect: false,
        hadReady: false,
        lastFailure: null,
        duplicateFrom: null,
        restored: true,
        attachTo: null,
      });
    }
    if (restored.length === 0) return;
    this.tabs = restored;
    this.activeId = restored[0].id;
  }

  /**
   * Creates a view for a session owned by another window (the tab was
   * moved). The session itself is not restarted.
   */
  attachTab(sessionId: string, host: Host): Tab {
    const tab: Tab = {
      id: newTabId(),
      sessionId,
      host,
      title: host.name,
      status: { state: 'connecting' },
      secrets: {},
      generation: 0,
      exitCode: null,
      dead: false,
      errorMessage: null,
      awaitingReconnect: false,
      hadReady: false,
      lastFailure: null,
      duplicateFrom: null,
      restored: false,
      attachTo: sessionId,
    };
    this.tabs = [...this.tabs, tab];
    this.activeId = tab.id;
    return tab;
  }

  /**
   * Moves a tab to a new window. The session keeps running in the
   * background; the new window attaches to it.
   */
  async moveToNewWindow(tab: Tab): Promise<void> {
    if (!tab.sessionId) return;
    try {
      await api.windowOpenSession(tab.sessionId, tab.title);
      // The view moves: drop it here without closing the session.
      this.tabs = this.tabs.filter((entry) => entry.id !== tab.id);
      if (this.activeId === tab.id) {
        const next = this.tabs[this.tabs.length - 1];
        this.activeId = next ? next.id : null;
      }
    } catch (err) {
      toasts.error(errorMessage(err));
    }
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
