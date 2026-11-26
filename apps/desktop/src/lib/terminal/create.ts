/**
 * Builds the xterm instance of a pane and drives its renderer and size.
 *
 * The result is registered in `registry.ts`; a Svelte component only hosts
 * it. Nothing here knows about sessions: the caller supplies the hooks that
 * connect the terminal to the rest of the app.
 */

import { FitAddon } from '@xterm/addon-fit';
import { WebglAddon } from '@xterm/addon-webgl';
import { Terminal } from '@xterm/xterm';
import { registerTerminal, webglCount, type TerminalHandle } from './registry';
import { terminalTheme } from './theme';

/** The look of a terminal (from the preferences). */
export interface TerminalAppearance {
  fontFamily: string;
  fontSize: number;
  lineHeight: number;
  scrollback: number;
  cursorBlink: boolean;
  cursorStyle: 'block' | 'bar' | 'underline';
  copyOnSelect: boolean;
  dark: boolean;
}

/** What the terminal needs from the rest of the app. */
export interface TerminalHooks {
  /** Keystrokes typed into the terminal, to send to the session. */
  onData: (data: string) => void;
  /** True for an app shortcut: xterm must leave the key to the window. */
  isAppKey: (event: KeyboardEvent) => boolean;
}

/** Chrome allows about 16 WebGL contexts; stay well below it. */
const MAX_WEBGL_CONTEXTS = 8;

/** Creates the terminal of a pane inside `host` and registers it. */
export function createTerminal(
  paneId: string,
  host: HTMLElement,
  appearance: TerminalAppearance,
  hooks: TerminalHooks,
): TerminalHandle {
  // xterm renders into its own element, so the element can move between
  // containers. It has no padding: the fit addon measures its parent.
  const element = document.createElement('div');
  element.className = 'terminal-surface';
  element.style.width = '100%';
  element.style.height = '100%';
  host.appendChild(element);

  const term = new Terminal({
    fontFamily: appearance.fontFamily,
    fontSize: appearance.fontSize,
    lineHeight: appearance.lineHeight,
    scrollback: appearance.scrollback,
    cursorBlink: appearance.cursorBlink,
    cursorStyle: appearance.cursorStyle,
    theme: terminalTheme(appearance.dark),
    allowProposedApi: true,
  });
  const fit = new FitAddon();
  term.loadAddon(fit);
  term.open(element);

  const handle: TerminalHandle = {
    paneId,
    term,
    fit,
    element,
    host,
    webgl: null,
    channel: null,
    generation: -1,
    copyOnSelect: appearance.copyOnSelect,
    fontsReady: false,
    disposed: false,
    dispose: () => {},
  };

  term.onData((data) => {
    if (!handle.disposed) hooks.onData(data);
  });

  // The window handler runs the app shortcuts: xterm must not swallow them
  // (it would stop their propagation and send them to the shell).
  term.attachCustomKeyEventHandler((event) => !hooks.isAppKey(event));

  term.onSelectionChange(() => {
    if (!handle.copyOnSelect) return;
    const selection = term.getSelection();
    if (selection.length === 0) return;
    void navigator.clipboard?.writeText(selection).catch(() => undefined);
  });

  const onKeyDown = (event: KeyboardEvent): void => {
    if (!event.ctrlKey || !event.shiftKey) return;
    if (event.key === 'C' || event.key === 'c') {
      const selection = term.getSelection();
      if (!selection) return;
      event.preventDefault();
      void navigator.clipboard?.writeText(selection).catch(() => undefined);
    } else if (event.key === 'V' || event.key === 'v') {
      event.preventDefault();
      void navigator.clipboard
        ?.readText()
        .then((text) => {
          if (text.length > 0) term.paste(text);
        })
        .catch(() => undefined);
    }
  };
  element.addEventListener('keydown', onKeyDown);

  handle.dispose = () => {
    if (handle.disposed) return;
    handle.disposed = true;
    // A late update from the backend must not reach a removed pane.
    if (handle.channel) handle.channel.onmessage = () => {};
    handle.channel = null;
    setAccelerated(handle, false);
    element.removeEventListener('keydown', onKeyDown);
    try {
      term.dispose();
    } catch {
      // Already disposed.
    }
    element.remove();
    handle.host = null;
  };

  registerTerminal(handle);
  return handle;
}

/**
 * Applies the appearance to a live terminal. Returns true when the cell
 * metrics changed, so the caller must fit again.
 */
export function applyAppearance(handle: TerminalHandle, appearance: TerminalAppearance): boolean {
  const options = handle.term.options;
  const metricsChanged =
    options.fontFamily !== appearance.fontFamily ||
    options.fontSize !== appearance.fontSize ||
    options.lineHeight !== appearance.lineHeight;
  options.fontFamily = appearance.fontFamily;
  options.fontSize = appearance.fontSize;
  options.lineHeight = appearance.lineHeight;
  options.scrollback = appearance.scrollback;
  options.cursorBlink = appearance.cursorBlink;
  options.cursorStyle = appearance.cursorStyle;
  options.theme = terminalTheme(appearance.dark);
  handle.copyOnSelect = appearance.copyOnSelect;
  return metricsChanged;
}

/** The longest the first fit waits for the web fonts. */
const FONT_WAIT_MS = 1500;

/** Shared by every terminal: the fonts are a fact of the document. */
let fontsSettled: Promise<void> | null = null;

/**
 * Waits (once) for the web fonts, so the first fit uses the real cell size.
 * The wait is bounded: a font that never arrives must not keep a session
 * from connecting. A font that arrives later is handled by `remeasure`.
 */
export async function whenFontsReady(handle: TerminalHandle): Promise<void> {
  if (handle.fontsReady) return;
  fontsSettled ??= (async () => {
    try {
      const ready = document.fonts?.ready;
      if (!ready) return;
      await Promise.race([
        ready,
        new Promise<void>((resolve) => setTimeout(resolve, FONT_WAIT_MS)),
      ]);
    } catch {
      // No font loading API: fit with what we have.
    }
  })();
  await fontsSettled;
  handle.fontsReady = true;
}

/**
 * Measures the cell size again. xterm measures when the font options change
 * (and after a resize), not when a web font finishes loading; changing the
 * family and restoring it makes it measure with the font that is there now.
 */
export function remeasure(handle: TerminalHandle): void {
  if (handle.disposed) return;
  const options = handle.term.options;
  const family = options.fontFamily;
  if (typeof family !== 'string') return;
  options.fontFamily = `${family} `;
  options.fontFamily = family;
}

/** True while the terminal's container is laid out with a size. */
export function isVisible(handle: TerminalHandle): boolean {
  const host = handle.host;
  return host !== null && host.offsetWidth > 0 && host.offsetHeight > 0;
}

/** The size of the terminal grid. */
export function terminalSize(handle: TerminalHandle): { cols: number; rows: number } {
  return { cols: handle.term.cols, rows: handle.term.rows };
}

/**
 * Fits the grid to the container. Returns true when the grid changed. A
 * hidden terminal is left alone (xterm would compute a 0x0 grid).
 */
export function fitTerminal(handle: TerminalHandle): boolean {
  if (handle.disposed || !isVisible(handle)) return false;
  const before = terminalSize(handle);
  try {
    handle.fit.fit();
  } catch {
    return false;
  }
  const after = terminalSize(handle);
  return before.cols !== after.cols || before.rows !== after.rows;
}

/**
 * Turns the GPU renderer on or off. A hidden terminal does not hold a WebGL
 * context (a browser allows only a few); a lost context falls back to the
 * DOM renderer instead of leaving a blank terminal.
 */
export function setAccelerated(handle: TerminalHandle, on: boolean): void {
  if (!on) {
    const addon = handle.webgl;
    handle.webgl = null;
    try {
      addon?.dispose();
    } catch {
      // The context is already gone.
    }
    return;
  }
  if (handle.webgl || handle.disposed || webglCount() >= MAX_WEBGL_CONTEXTS) return;
  try {
    const addon = new WebglAddon();
    addon.onContextLoss(() => {
      if (handle.webgl === addon) handle.webgl = null;
      try {
        addon.dispose();
      } catch {
        // Already disposed.
      }
    });
    handle.term.loadAddon(addon);
    handle.webgl = addon;
  } catch {
    // No WebGL (software rendering, blocked GPU): keep the DOM renderer —
    // slower, but correct.
    handle.webgl = null;
  }
}
