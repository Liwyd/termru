/**
 * The size of a terminal pane has exactly one owner.
 *
 * A pane's container is observed by a single `ResizeObserver`, and every
 * trigger (the observer, a font that finished loading, a new appearance)
 * goes through the same two steps:
 *
 * 1. the grid is fitted inside `requestAnimationFrame`, once per frame no
 *    matter how many triggers came in (a divider drag fires the observer on
 *    every pointer move; the terminal follows it frame by frame);
 * 2. the size is pushed to the backend only after it has stopped changing
 *    (debounced), and only when the grid really changed: the remote shell
 *    gets one SIGWINCH for a drag, not hundreds.
 *
 * `claimResizeOwner` keeps the one-owner rule: a second claim for the same
 * pane (a layout change mounts the new view before the old one is torn
 * down) stops the first owner before it starts.
 */

/** How long the grid must stay unchanged before the backend is told. */
export const PUSH_DEBOUNCE_MS = 150;

/** What the owner does; supplied by the view that hosts the terminal. */
export interface ResizeHooks {
  /** Fits the grid to the container; true when the grid changed. */
  fit: () => boolean | Promise<boolean>;
  /** Tells the backend the settled grid. */
  push: () => void;
}

/** The browser functions the owner uses (replaced in the tests). */
export interface ResizeEnvironment {
  observe: (target: Element, onResize: () => void) => () => void;
  requestFrame: (run: () => void) => number;
  cancelFrame: (id: number) => void;
  setTimer: (run: () => void, ms: number) => ReturnType<typeof setTimeout>;
  clearTimer: (id: ReturnType<typeof setTimeout>) => void;
}

const browser: ResizeEnvironment = {
  observe(target, onResize) {
    const observer = new ResizeObserver(() => onResize());
    observer.observe(target);
    return () => observer.disconnect();
  },
  requestFrame: (run) => requestAnimationFrame(run),
  cancelFrame: (id) => cancelAnimationFrame(id),
  setTimer: (run, ms) => setTimeout(run, ms),
  clearTimer: (id) => clearTimeout(id),
};

export class ResizeOwner {
  readonly paneId: string;
  #hooks: ResizeHooks;
  #env: ResizeEnvironment;
  #stopObserving: (() => void) | null = null;
  #frame: number | null = null;
  #pushTimer: ReturnType<typeof setTimeout> | null = null;
  #stopped = false;
  /** A fit is running (it may wait for the fonts); one at a time. */
  #fitting = false;
  /** A trigger came in while a fit was running. */
  #again = false;

  constructor(paneId: string, hooks: ResizeHooks, env: ResizeEnvironment = browser) {
    this.paneId = paneId;
    this.#hooks = hooks;
    this.#env = env;
  }

  /** Starts observing `target` and fits once for the current size. */
  start(target: Element): void {
    if (this.#stopped || this.#stopObserving) return;
    this.#stopObserving = this.#env.observe(target, () => this.schedule());
    this.schedule();
  }

  /** Asks for a fit; any number of calls within a frame fit once. */
  schedule(): void {
    if (this.#stopped || this.#frame !== null) return;
    this.#frame = this.#env.requestFrame(() => {
      this.#frame = null;
      void this.#fit();
    });
  }

  async #fit(): Promise<void> {
    if (this.#stopped) return;
    if (this.#fitting) {
      this.#again = true;
      return;
    }
    this.#fitting = true;
    try {
      const changed = await this.#hooks.fit();
      if (changed && !this.#stopped) this.#schedulePush();
    } finally {
      this.#fitting = false;
    }
    if (this.#again && !this.#stopped) {
      this.#again = false;
      this.schedule();
    }
  }

  #schedulePush(): void {
    if (this.#pushTimer !== null) this.#env.clearTimer(this.#pushTimer);
    this.#pushTimer = this.#env.setTimer(() => {
      this.#pushTimer = null;
      if (!this.#stopped) this.#hooks.push();
    }, PUSH_DEBOUNCE_MS);
  }

  /** Stops observing and drops whatever is pending. */
  stop(): void {
    if (this.#stopped) return;
    this.#stopped = true;
    this.#stopObserving?.();
    this.#stopObserving = null;
    if (this.#frame !== null) this.#env.cancelFrame(this.#frame);
    if (this.#pushTimer !== null) this.#env.clearTimer(this.#pushTimer);
    this.#frame = null;
    this.#pushTimer = null;
    if (owners.get(this.paneId) === this) owners.delete(this.paneId);
  }

  get stopped(): boolean {
    return this.#stopped;
  }
}

const owners = new Map<string, ResizeOwner>();

/** Makes a new owner for a pane; the pane's previous owner stops. */
export function claimResizeOwner(
  paneId: string,
  hooks: ResizeHooks,
  env?: ResizeEnvironment,
): ResizeOwner {
  owners.get(paneId)?.stop();
  const owner = new ResizeOwner(paneId, hooks, env);
  owners.set(paneId, owner);
  return owner;
}

/** The owner of a pane, if one is running. */
export function resizeOwnerOf(paneId: string): ResizeOwner | undefined {
  return owners.get(paneId);
}

/** How many panes have a running owner (a pane never has two). */
export function resizeOwnerCount(): number {
  return owners.size;
}
