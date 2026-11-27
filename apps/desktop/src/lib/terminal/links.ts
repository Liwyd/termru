/**
 * Links in terminal output.
 *
 * `@xterm/addon-web-links` finds `http(s)://` text; this module decides what
 * a click does. A link never navigates the webview (the app is a single
 * page: leaving it would lose every terminal), so a click either hands the
 * address to the system's opener or copies it.
 *
 * The system opener is the Tauri opener plugin (`ipc-backend.ts` `openUrl`).
 * Its capability allows http, https and mailto only, and the app only ever
 * hands it web links (`openableUrl`). When the opener fails (no browser, a
 * sandbox that refuses it) the address is copied instead.
 */

import { openUrl } from '../ipc-backend';

/** The schemes a terminal link may open. */
const OPENABLE_PROTOCOLS = new Set(['http:', 'https:']);

/** The longest address handed to the opener. */
const MAX_URL_LENGTH = 4096;

/** The normalized address, or null when it is not a web link. */
export function openableUrl(raw: string): string | null {
  const text = raw.trim();
  if (text.length === 0 || text.length > MAX_URL_LENGTH) return null;
  try {
    const url = new URL(text);
    if (!OPENABLE_PROTOCOLS.has(url.protocol)) return null;
    // Control characters have no place in an address.
    if (/[\u0000-\u001f\u007f]/.test(text)) return null;
    return url.href;
  } catch {
    return null;
  }
}

/** Whether a click on a link should open it (a plain click is a selection). */
export function isOpenGesture(event: Pick<MouseEvent, 'ctrlKey' | 'metaKey'>): boolean {
  return event.ctrlKey || event.metaKey;
}

export type OpenResult = 'opened' | 'copied' | 'refused' | 'failed';

/** Copies text to the clipboard; false when the browser refuses. */
export async function copyText(text: string): Promise<boolean> {
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch {
    return false;
  }
}

/**
 * Opens a link in the system browser; copies it when there is no opener.
 * `refused` is a scheme that is not a web link.
 */
export async function openLink(raw: string): Promise<OpenResult> {
  const url = openableUrl(raw);
  if (!url) return 'refused';
  try {
    await openUrl(url);
    return 'opened';
  } catch {
    return (await copyText(url)) ? 'copied' : 'failed';
  }
}
