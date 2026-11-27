/**
 * Fakes of xterm and its addons for the component tests. They record what
 * the app does to the terminal; none of them draws anything.
 *
 *     vi.mock('@xterm/xterm', () => import('../test-support/xterm-mock'))
 *     vi.mock('@xterm/addon-fit', () => import('../test-support/xterm-mock'))
 *     vi.mock('@xterm/addon-webgl', () => import('../test-support/xterm-mock'))
 *     vi.mock('@xterm/addon-search', () => import('../test-support/xterm-mock'))
 *     vi.mock('@xterm/addon-web-links', () => import('../test-support/xterm-mock'))
 */

export const terminals: Terminal[] = [];
export const webglAddons: WebglAddon[] = [];
export const searchAddons: SearchAddon[] = [];
export const linkAddons: WebLinksAddon[] = [];

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
  titleHandler: ((title: string) => void) | null = null;
  resizeHandler: ((size: { cols: number; rows: number }) => void) | null = null;
  addons: unknown[] = [];
  pasted: string[] = [];
  selectAlls = 0;
  clears = 0;
  focuses = 0;

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
  onTitleChange(handler: (title: string) => void) {
    this.titleHandler = handler;
  }
  onResize(handler: (size: { cols: number; rows: number }) => void) {
    this.resizeHandler = handler;
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
    const changed = cols !== this.cols || rows !== this.rows;
    this.cols = cols;
    this.rows = rows;
    if (changed) this.resizeHandler?.({ cols, rows });
  }
  getSelection() {
    return this.selection;
  }
  hasSelection() {
    return this.selection.length > 0;
  }
  selectAll() {
    this.selectAlls += 1;
  }
  clear() {
    this.clears += 1;
  }
  focus() {
    this.focuses += 1;
  }
  paste(text: string) {
    this.pasted.push(text);
  }
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

/** The find addon: records the searches and lets a test report results. */
export class SearchAddon {
  searches: { term: string; direction: 'next' | 'previous'; options: unknown }[] = [];
  clears = 0;
  disposed = false;
  private resultHandler: ((event: { resultIndex: number; resultCount: number }) => void) | null =
    null;
  constructor() {
    searchAddons.push(this);
  }
  activate(_term: Terminal) {}
  onDidChangeResults = (handler: (event: { resultIndex: number; resultCount: number }) => void) => {
    this.resultHandler = handler;
    return { dispose: () => (this.resultHandler = null) };
  };
  /** What the addon reports after a search. */
  report(resultIndex: number, resultCount: number) {
    this.resultHandler?.({ resultIndex, resultCount });
  }
  findNext(term: string, options?: unknown) {
    this.searches.push({ term, direction: 'next', options });
    return true;
  }
  findPrevious(term: string, options?: unknown) {
    this.searches.push({ term, direction: 'previous', options });
    return true;
  }
  clearDecorations() {
    this.clears += 1;
  }
  clearActiveDecoration() {}
  dispose() {
    this.disposed = true;
  }
}

/** The web links addon: keeps the handler so a test can click a link. */
export class WebLinksAddon {
  constructor(
    public handler?: (event: MouseEvent, uri: string) => void,
    public options?: {
      hover?: (event: MouseEvent, text: string) => void;
      leave?: (event: MouseEvent, text: string) => void;
    },
  ) {
    linkAddons.push(this);
  }
  activate(_term: Terminal) {}
  dispose() {}
}

/** Forgets every instance (a test starts from a clean slate). */
export function resetXtermMock(): void {
  terminals.length = 0;
  webglAddons.length = 0;
  searchAddons.length = 0;
  linkAddons.length = 0;
  fitSize.cols = 100;
  fitSize.rows = 30;
}
