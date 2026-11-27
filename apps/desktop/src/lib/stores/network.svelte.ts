/**
 * Whether the machine is online, as a reactive value.
 *
 * `navigator.onLine` alone is not reactive, so the window's own `online` and
 * `offline` events keep a rune in step with it. Being offline never means
 * that an SSH session is down: it only tells the sync page and the status
 * bar that the server cannot be reached (Doc A.05).
 */
class Network {
  online = $state(typeof navigator === 'undefined' ? true : navigator.onLine !== false);

  constructor() {
    if (typeof window === 'undefined') return;
    window.addEventListener('online', () => (this.online = true));
    window.addEventListener('offline', () => (this.online = false));
  }
}

export const network = new Network();
