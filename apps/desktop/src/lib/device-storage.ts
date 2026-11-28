/**
 * Small, device-local interface state (a collapsed sidebar, the sort of the
 * host list). It never leaves this machine and is never synced: preferences
 * that roam live in `Preferences`. Storage can be missing or refuse (a private
 * window, blocked site data), so every read and write is guarded and the
 * interface renders its default without it.
 */

/** The stored text for a key, or null when there is none or storage is unavailable. */
export function readStored(key: string): string | null {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}

/** Remembers a value; a refusal is fine (it is not remembered). */
export function writeStored(key: string, value: string): void {
  try {
    localStorage.setItem(key, value);
  } catch {
    // Not remembered across restarts.
  }
}
