import { api } from '../ipc';
import {
  collectPanes,
  fromLayoutNode,
  layoutLeaves,
  leafTree,
  pruneTree,
  removeNode,
  setRatio,
  splitNode,
  toLayoutNode,
  type TreeNode,
} from '../pane-tree';
import { errorMessage, hosts } from './hosts.svelte';
import { route } from './route.svelte';
import { toasts } from './toasts.svelte';
import type { Host, SessionSecrets, SessionStatus, SessionUpdate, WorkspaceDoc } from '../types';

export interface Pane {
  /** Pane id (stable, also used in the persisted layout). */
  id: string;
  /** Backend session id, once the connection was started. */
  sessionId: string | null;
  /** The host of the pane; null while the pane is empty. */
  host: Host | null;
  title: string;
  status: SessionStatus;
  /** In-memory only; needed to re-open the connection after a failure. */
  secrets: SessionSecrets;
  /** Bumped to force a (re)connect from the terminal view. */
  generation: number;
  exitCode: number | null;
  /** Terminal state reached: the pane shows a reconnect affordance. */
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
  /** Store the secret in the OS credential store once connected. */
  rememberSecret: boolean;
}

export interface Tab {
  /** Local tab id (stable across reconnects). */
  id: string;
  title: string;
  /** The panes of the tab. */
  panes: Pane[];
  /** The pane that has the focus. */
  activePaneId: string | null;
  /** The split tree: the layout of the panes. */
  root: TreeNode;
}

/** How long we wait for an expected auto-reconnect before showing "dead". */
const RECONNECT_WATCHDOG_MS = 30_000;
/** Debounce for layout persistence. */
const LAYOUT_SAVE_DEBOUNCE_MS = 750;
/** The largest number of panes a tab may have. */
const MAX_PANES_PER_TAB = 16;

function newTabId(): string {
  return crypto.randomUUID();
}

function newPaneId(): string {
  return crypto.randomUUID();
}

/** Open terminal tabs, their panes and their session state. */
export class SessionStore {
  tabs = $state<Tab[]>([]);
  activeId = $state<string | null>(null);
  #layoutTimer: ReturnType<typeof setTimeout> | undefined;

  get active(): Tab | null {
    return this.tabs.find((tab) => tab.id === this.activeId) ?? null;
  }

  /** The pane with the focus, if any. */
  get activePane(): Pane | null {
    const tab = this.active;
    if (!tab) return null;
    return tab.panes.find((pane) => pane.id === tab.activePaneId) ?? null;
  }

  /** Creates a pane for a host. */
  #newPane(host: Host | null, secrets: SessionSecrets, rememberSecret: boolean): Pane {
    return {
      id: newPaneId(),
      sessionId: null,
      host,
      title: host?.name ?? 'New pane',
      status: host ? { state: 'connecting' } : { state: 'closed', reason: 'empty pane' },
      secrets,
      generation: 0,
      exitCode: null,
      dead: host === null,
      errorMessage: null,
      awaitingReconnect: false,
      hadReady: false,
      lastFailure: null,
      duplicateFrom: null,
      restored: false,
      attachTo: null,
      rememberSecret,
    };
  }

  open(host: Host, secrets: SessionSecrets, rememberSecret = false): Tab {
    const pane = this.#newPane(host, secrets, rememberSecret);
    const tab: Tab = {
      id: newTabId(),
      title: host.name,
      panes: [pane],
      activePaneId: pane.id,
      root: leafTree(pane.id),
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
    const source = this.activePaneOf(tab);
    if (!source?.sessionId) return null;
    const clone: Pane = {
      ...source,
      id: newPaneId(),
      sessionId: null,
      status: { state: 'connecting' },
      dead: false,
      errorMessage: null,
      exitCode: null,
      awaitingReconnect: false,
      hadReady: false,
      lastFailure: null,
      duplicateFrom: source.sessionId,
      restored: false,
      attachTo: null,
      rememberSecret: false,
    };
    // The duplicate gets its own tab: the original keeps its panes (and
    // their sessions).
    const next: Tab = {
      id: newTabId(),
      title: tab.title,
      panes: [clone],
      activePaneId: clone.id,
      root: leafTree(clone.id),
    };
    this.tabs = [...this.tabs, next];
    this.scheduleLayoutSave();
    this.activeId = next.id;
    return next;
  }

  /**
   * The tab and the pane as the store holds them.
   *
   * The caller may hold a raw object (a test) or the reactive proxy (the
   * UI). The store always mutates its own instance, so the change is seen
   * by the UI.
   */
  #resolve(tabId: string, paneId: string): { tab: Tab; pane: Pane } | null {
    const tab = this.tabs.find((entry) => entry.id === tabId);
    if (!tab) return null;
    const pane = tab.panes.find((entry) => entry.id === paneId);
    if (!pane) return null;
    return { tab, pane };
  }

  /** The pane of a tab that has the focus. */
  activePaneOf(tab: Tab): Pane | null {
    const stored = this.tabs.find((entry) => entry.id === tab.id) ?? tab;
    return stored.panes.find((pane) => pane.id === stored.activePaneId) ?? null;
  }

  /** The tab that holds a pane. */
  tabOfPane(paneId: string): Tab | null {
    return this.tabs.find((tab) => tab.panes.some((pane) => pane.id === paneId)) ?? null;
  }

  /**
   * Splits the pane with the focus. The new pane is empty: the user picks
   * a host for it.
   */
  splitActive(direction: 'columns' | 'rows'): Pane | null {
    const tab = this.active;
    if (!tab) return null;
    const pane = this.activePaneOf(tab);
    if (!pane) return null;
    if (tab.panes.length >= MAX_PANES_PER_TAB) {
      toasts.error(`A tab can hold at most ${MAX_PANES_PER_TAB} panes.`);
      return null;
    }
    const created = this.#newPane(null, {}, false);
    const root = splitNode(tab.root, pane.id, direction, created.id);
    if (!root) return null;
    tab.root = root;
    tab.panes = [...tab.panes, created];
    tab.activePaneId = created.id;
    this.scheduleLayoutSave();
    return created;
  }

  /** Fills an empty pane with a host and connects it. */
  async connectPane(pane: Pane, host: Host, secrets: SessionSecrets = {}): Promise<void> {
    const found = this.#resolve(this.tabOfPane(pane.id)?.id ?? '', pane.id);
    if (!found) return;
    const { tab, pane: target } = found;
    target.host = host;
    target.title = host.name;
    target.status = { state: 'connecting' };
    target.dead = false;
    target.errorMessage = null;
    target.secrets = secrets;
    target.generation += 1;
    if (tab.title === 'New pane' || tab.panes.length === 1) tab.title = host.name;
    this.scheduleLayoutSave();
  }

  /** Moves the focus to a pane. */
  focusPane(pane: Pane): void {
    const tab = this.tabOfPane(pane.id);
    if (!tab) return;
    tab.activePaneId = pane.id;
    this.activeId = tab.id;
  }

  /** The tab that holds a pane, as the store holds it. */
  #storedTab(paneId: string): Tab | null {
    return this.tabs.find((tab) => tab.panes.some((pane) => pane.id === paneId)) ?? null;
  }

  /** Moves the focus to the next (or previous) pane of the tab. */
  cyclePane(direction: 1 | -1): void {
    const tab = this.active;
    if (!tab || tab.panes.length < 2) return;
    const index = tab.panes.findIndex((pane) => pane.id === tab.activePaneId);
    const next = (index + direction + tab.panes.length) % tab.panes.length;
    tab.activePaneId = tab.panes[next].id;
  }

  /** Adjusts the size of a split. */
  resizeSplit(tab: Tab, splitId: string, ratio: number): void {
    const stored = this.tabs.find((entry) => entry.id === tab.id) ?? tab;
    stored.root = setRatio(stored.root, splitId, ratio);
    this.scheduleLayoutSave();
  }

  /**
   * Closes a pane: the session is closed, the pane is removed and the tree
   * collapses. The tab is closed when its last pane is gone.
   */
  async closePane(pane: Pane): Promise<void> {
    const tab = this.#storedTab(pane.id);
    if (!tab) return;
    const stored = tab.panes.find((entry) => entry.id === pane.id);
    if (stored?.sessionId) {
      try {
        await api.sessionClose(stored.sessionId);
      } catch (err) {
        // The session may already be gone; the pane is closing either way.
        if (!/no session/i.test(errorMessage(err))) {
          toasts.error(errorMessage(err));
        }
      }
    }
    const root = removeNode(tab.root, pane.id);
    const panes = tab.panes.filter((entry) => entry.id !== pane.id);
    if (!root || panes.length === 0) {
      await this.close(tab);
      return;
    }
    const activePaneId =
      tab.activePaneId === pane.id ? (collectPanes(root)[0] ?? null) : tab.activePaneId;
    tab.root = root;
    tab.panes = panes;
    tab.activePaneId = activePaneId;
    this.scheduleLayoutSave();
  }

  async close(tab: Tab): Promise<void> {
    const stored = this.tabs.find((entry) => entry.id === tab.id) ?? tab;
    for (const pane of stored.panes) {
      if (!pane.sessionId) continue;
      try {
        await api.sessionClose(pane.sessionId);
      } catch (err) {
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
    const stored = this.tabs.find((entry) => entry.id === tab.id) ?? tab;
    stored.title = trimmed;
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
   * Persists the tab layout (hosts + order + pane tree) so it survives a
   * restart. Restored tabs are placeholders: the user reconnects them
   * explicitly (secrets are never stored).
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
            root: toLayoutNode(
              tab.root,
              new Map(tab.panes.map((pane) => [pane.id, pane.host?.id ?? null])),
            ),
            active_pane: tab.activePaneId ?? undefined,
          })),
          active_tab: this.activeId ?? undefined,
        },
      ],
      active_window: windowId,
      // Additive field: the page the user was on (documents from before
      // routing simply lack it).
      last_route: route.route,
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
    // Restore the page the user was on (the URL hash wins on a dev reload).
    route.restore(doc?.last_route);
    const windowLayout = doc?.windows?.[0];
    if (!windowLayout || windowLayout.tabs.length === 0) return;

    const byId = new Map(hosts.hosts.map((host) => [host.id, host]));
    const restored: Tab[] = [];
    for (const tabLayout of windowLayout.tabs) {
      const leaves = layoutLeaves(tabLayout.root);
      const panes: Pane[] = [];
      for (const leaf of leaves) {
        const host = leaf.host_id ? byId.get(leaf.host_id) : undefined;
        if (leaf.host_id && !host) continue; // the host was deleted meanwhile
        const pane = this.#newPane(host ?? null, {}, false);
        pane.id = leaf.pane;
        pane.title = host?.name ?? 'New pane';
        pane.status = { state: 'closed', reason: 'not connected' };
        pane.dead = true;
        pane.restored = true;
        panes.push(pane);
      }
      if (panes.length === 0) continue;
      const root = pruneTree(fromLayoutNode(tabLayout.root), new Set(panes.map((pane) => pane.id)));
      if (!root) continue;
      const activePaneId =
        tabLayout.active_pane && panes.some((pane) => pane.id === tabLayout.active_pane)
          ? tabLayout.active_pane
          : (panes[0].id ?? null);
      restored.push({
        id: tabLayout.id || newTabId(),
        title: tabLayout.title ?? panes[0].title,
        panes,
        activePaneId,
        root,
      });
    }
    if (restored.length > 0) {
      this.tabs = restored;
      this.activeId = restored[0].id;
    }
  }

  /**
   * Creates a view for a session owned by another window (the tab was
   * moved). The session itself is not restarted.
   */
  attachTab(sessionId: string, host: Host): Tab {
    const pane: Pane = {
      ...this.#newPane(host, {}, false),
      id: newPaneId(),
      sessionId,
      status: { state: 'connecting' },
      attachTo: sessionId,
    };
    const tab: Tab = {
      id: newTabId(),
      title: host.name,
      panes: [pane],
      activePaneId: pane.id,
      root: leafTree(pane.id),
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
    const pane = this.activePaneOf(tab);
    if (!pane?.sessionId) return;
    try {
      await api.windowOpenSession(pane.sessionId, tab.title);
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

  /** Applies a backend status event to a pane. */
  applyStatus(pane: Pane, status: SessionStatus): void {
    const target = this.#storedPane(pane) ?? pane;
    target.status = status;
    target.errorMessage = null;

    switch (status.state) {
      case 'ready':
        target.hadReady = true;
        target.dead = false;
        target.awaitingReconnect = false;
        target.lastFailure = null;
        void this.rememberSecret(target);
        break;
      case 'connecting':
      case 'authenticating':
      case 'reconnecting':
        target.dead = false;
        target.awaitingReconnect = false;
        target.lastFailure = null;
        break;
      case 'failed':
        target.lastFailure = { category: status.category, message: status.message };
        target.errorMessage = status.message;
        break;
      case 'closed':
        // A network death after `ready` is retried by the backend; a give-up
        // ("gave up after N attempts") or a non-network failure is final.
        if (this.isRetriable(target) && !/gave up/i.test(status.reason)) {
          target.dead = false;
          target.awaitingReconnect = true;
          this.armWatchdog(target);
        } else {
          target.dead = true;
          target.awaitingReconnect = false;
        }
        break;
    }
  }

  /** The pane as the store holds it (the reactive instance). */
  #storedPane(pane: Pane): Pane | null {
    for (const tab of this.tabs) {
      const found = tab.panes.find((entry) => entry.id === pane.id);
      if (found) return found;
    }
    return null;
  }

  private isRetriable(pane: Pane): boolean {
    if (!pane.hadReady) return false;
    const category = pane.lastFailure?.category;
    return category === 'network' || category === 'timeout';
  }

  private armWatchdog(pane: Pane): void {
    setTimeout(() => {
      if (pane.awaitingReconnect) {
        pane.dead = true;
        pane.awaitingReconnect = false;
        pane.errorMessage = pane.errorMessage ?? 'connection lost';
      }
    }, RECONNECT_WATCHDOG_MS);
  }

  /** Applies a channel update to a pane. */
  applyUpdate(pane: Pane, update: SessionUpdate): void {
    switch (update.kind) {
      case 'status':
        this.applyStatus(pane, update.status);
        break;
      case 'exit':
        pane.exitCode = update.code;
        break;
      case 'data':
        // Terminal bytes are handled by the terminal view itself.
        break;
    }
  }

  /** Forces a fresh connection for a pane (retry after failure). */
  retry(pane: Pane): void {
    const target = this.#storedPane(pane) ?? pane;
    target.dead = false;
    target.errorMessage = null;
    target.exitCode = null;
    target.status = { state: 'connecting' };
    target.generation += 1;
  }

  /**
   * Stores the secret in the OS credential store and drops the in-memory
   * copy. Called once the connection is established.
   */
  async rememberSecret(pane: Pane): Promise<void> {
    const target = this.#storedPane(pane) ?? pane;
    if (!target.rememberSecret || !target.host) return;
    const password = target.secrets.password;
    const passphrase = target.secrets.passphrase;
    if (!password && !passphrase) return;
    const kind = password ? 'password' : 'passphrase';
    const value = password ?? passphrase;
    if (!value) return;
    try {
      await api.secretSet(target.host.id, kind, value);
      // The credential store holds it now: do not keep it in the frontend.
      if (password) target.secrets.password = undefined;
      if (passphrase) target.secrets.passphrase = undefined;
      // The hosts table shows the same fact in its Authentication column.
      if (password) hosts.markPasswordStored(target.host.id, true);
      toasts.success('The secret was stored in your system credential store.');
    } catch (err) {
      toasts.error(errorMessage(err));
    }
  }

  /** Local failure (e.g. the backend refused to start the connection). */
  fail(pane: Pane, message: string): void {
    const target = this.#storedPane(pane) ?? pane;
    target.errorMessage = message;
    target.dead = true;
    target.awaitingReconnect = false;
    target.status = { state: 'failed', category: 'local', message };
  }
}

export const sessions = new SessionStore();
