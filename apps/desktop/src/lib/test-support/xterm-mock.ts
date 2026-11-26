/**
 * Fakes of xterm and its addons for the component tests. They record what
 * the app does to the terminal; none of them draws anything.
 *
 *     vi.mock('@xterm/xterm', () => import('../test-support/xterm-mock'))
 *     vi.mock('@xterm/addon-fit', () => import('../test-support/xterm-mock'))
 *     vi.mock('@xterm/addon-webgl', () => import('../test-support/xterm-mock'))
 */

export const terminals: Terminal[] = [];
export const webglAddons: WebglAddon[] = [];

/** The grid the fit addon computes (the tests set it). */
export const fitSize = { cols: 100, rows: 30 };

export class Terminal {
  cols = 80;
  rows = 24;
  options: Record<string, unknown>;
  element: HTMLElement | null = null;
  disposed = false;
  resets = 0;
  writes: Uint8Array[] = [];
  selection = '';
  dataHandler: ((data: string) => void) | null = null;
  selectionHandler: (() => void) | null = null;
  keyHandler: ((event: KeyboardEvent) => boolean) | null = null;
  addons: unknown[] = [];

  constructor(options: Record<string, unknown> = {}) {
    this.options = { ...options };
    terminals.push(this);
  }

  onData(handler: (data: string) => void) {
    this.dataHandler = handler;
  }
  onSelectionChange(handler: () => void) {
    this.selectionHandler = handler;
  }
  attachCustomKeyEventHandler(handler: (event: KeyboardEvent) => boolean) {
    this.keyHandler = handler;
  }
  loadAddon(addon: unknown) {
    this.addons.push(addon);
    (addon as { activate?: (term: Terminal) => void }).activate?.(this);
  }
  open(element: HTMLElement) {
    this.element = element;
  }
  write(data: Uint8Array) {
    this.writes.push(data);
  }
  reset() {
    this.resets += 1;
  }
  resize(cols: number, rows: number) {
    this.cols = cols;
    this.rows = rows;
  }
  getSelection() {
    return this.selection;
  }
  paste(_text: string) {}
  dispose() {
    this.disposed = true;
  }
}

export class FitAddon {
  term: Terminal | null = null;
  activate(term: Terminal) {
    this.term = term;
  }
  fit() {
    this.term?.resize(fitSize.cols, fitSize.rows);
  }
}

export class WebglAddon {
  disposed = false;
  private lossHandler: (() => void) | null = null;
  constructor() {
    webglAddons.push(this);
  }
  onContextLoss(handler: () => void) {
    this.lossHandler = handler;
  }
  /** The GPU took the context away. */
  loseContext() {
    this.lossHandler?.();
  }
  dispose() {
    this.disposed = true;
  }
}

/** Forgets every instance (a test starts from a clean slate). */
export function resetXtermMock(): void {
  terminals.length = 0;
  webglAddons.length = 0;
  fitSize.cols = 100;
  fitSize.rows = 30;
}
