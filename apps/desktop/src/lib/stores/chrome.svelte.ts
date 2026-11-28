import { readStored, writeStored } from '../device-storage';

const SIDEBAR_KEY = 'termru.chrome.sidebarCollapsed';
const INSPECTOR_KEY = 'termru.chrome.inspectorOpen';

/** Below this window width the sidebar is an icon rail whatever the user chose. */
export const NARROW_QUERY = '(max-width: 900px)';

/**
 * The frame of the app: whether the sidebar is full or an icon rail, and
 * whether the hosts inspector is open. Both are choices of this device,
 * remembered across restarts (`device-storage.ts`), not preferences that sync.
 */
class ChromeStore {
  /** The user collapsed the sidebar to icons. */
  sidebarCollapsed = $state(readStored(SIDEBAR_KEY) === '1');
  /** The hosts inspector is open (it is open until the user closes it). */
  inspectorOpen = $state(readStored(INSPECTOR_KEY) !== '0');
  /** The window is too narrow for the full sidebar (a plain mirror of the query). */
  narrow = $state(false);
  #watching = false;

  /** The sidebar is drawn as an icon rail: by choice, or because there is no room. */
  get railed(): boolean {
    return this.sidebarCollapsed || this.narrow;
  }

  toggleSidebar(): void {
    // On a narrow window the rail is forced; the choice is for the wide one.
    this.sidebarCollapsed = !this.sidebarCollapsed;
    writeStored(SIDEBAR_KEY, this.sidebarCollapsed ? '1' : '0');
  }

  toggleInspector(): void {
    this.inspectorOpen = !this.inspectorOpen;
    writeStored(INSPECTOR_KEY, this.inspectorOpen ? '1' : '0');
  }

  /** Starts following the window width (once; needs `matchMedia`). */
  watchViewport(): void {
    if (this.#watching) return;
    if (typeof window === 'undefined' || typeof window.matchMedia !== 'function') return;
    this.#watching = true;
    const query = window.matchMedia(NARROW_QUERY);
    this.narrow = query.matches === true;
    const onChange = (event: { matches: boolean }): void => {
      this.narrow = event.matches;
    };
    if (typeof query.addEventListener === 'function') query.addEventListener('change', onChange);
    else if (typeof query.addListener === 'function') query.addListener(onChange);
  }
}

export const chrome = new ChromeStore();
