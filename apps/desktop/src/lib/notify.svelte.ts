/**
 * The notify facade: every message the app shows as a notice goes through
 * here (Doc C.04).
 *
 * A caller never passes text. It names a notice by key (`host.saved`) and
 * supplies the few values the copy needs (a host name, the reason of a
 * failure). The facade owns what the user reads:
 *
 * - **Copy whitelist.** `COPY` is the only source of titles and
 *   descriptions. A key that is not in it cannot be shown. Values are
 *   cleaned (control and bidirectional characters removed, whitespace
 *   collapsed, length capped) before they are put into a sentence, so a
 *   host name, a server banner or an error string cannot smuggle layout or
 *   a spoofed line into the notice.
 * - **Dedup.** The same notice (same key and entity, or same values) while it
 *   is on screen or waiting is one notice: it stays where it is, takes the
 *   newest text and its timer restarts, instead of stacking a copy. Once it
 *   has left, the same event is news again.
 * - **Bounded queue.** At most `MAX_VISIBLE` notices are on screen and at
 *   most `MAX_QUEUED` wait. A failure never waits behind a routine notice
 *   (it takes the oldest routine slot); when the queue is full the oldest
 *   routine notice is dropped, never a failure.
 * - **Roles.** A failure or a warning is `role="alert"`; the rest are
 *   `role="status"` (see `Toasts.svelte`).
 * - **Routine notices preference.** With `routine_notices` off, success and
 *   info notices are not shown at all. Failures and warnings always are.
 *
 * The component `Toasts.svelte` is the one adapter that renders the list; it
 * holds no policy. Terminal output never reaches this module: nothing here
 * accepts free text from a session.
 */

import { prefs } from './stores/prefs.svelte';

export type NoticeKind = 'success' | 'info' | 'warning' | 'error';

/** The notices the app can show. Adding one means adding its copy below. */
export type NoticeKey =
  // Hosts
  | 'host.saved'
  | 'host.deleted'
  | 'host.delete-failed'
  | 'host.favorite-failed'
  | 'host.moved'
  | 'host.move-failed'
  | 'host.credential-not-stored'
  | 'hosts.refresh-failed'
  // Folders
  | 'folder.created'
  | 'folder.saved'
  | 'folder.deleted'
  | 'folder.delete-failed'
  // Identities
  | 'identity.saved'
  | 'identity.created'
  | 'identity.created-unbound'
  | 'identity.generated'
  | 'identity.passphrase-note'
  | 'identity.bound'
  | 'identity.bind-failed'
  | 'identity.unbound'
  | 'identity.unbind-failed'
  | 'identity.deleted'
  | 'identity.delete-failed'
  | 'identity.load-failed'
  // Secrets on this device
  | 'secret.password-forgotten'
  | 'secret.passphrase-forgotten'
  | 'secret.forget-failed'
  | 'credential.stored'
  | 'credential.store-failed'
  // Clipboard and links
  | 'clipboard.copy-failed'
  | 'clipboard.read-failed'
  | 'ssh.command-copied'
  | 'link.copied'
  | 'link.copied-no-browser'
  | 'link.refused'
  | 'link.failed'
  // Sessions and prompts
  | 'session.unavailable'
  | 'session.host-missing'
  | 'session.attach-failed'
  | 'session.close-failed'
  | 'session.move-failed'
  | 'session.pane-limit'
  | 'prompt.hostkey-inactive'
  | 'prompt.hostkey-failed'
  | 'prompt.auth-inactive'
  | 'prompt.auth-failed'
  // Preferences
  | 'prefs.save-failed'
  | 'prefs.restored'
  | 'prefs.preview'
  // Sync
  | 'sync.signed-in'
  | 'sync.signin-failed'
  | 'sync.signed-out'
  | 'sync.signout-failed'
  | 'sync.synced'
  | 'sync.failed'
  | 'sync.kept-local'
  | 'sync.took-server'
  | 'sync.resolve-failed';

/** The values a notice's copy can use. Everything is cleaned before use. */
export interface NoticeValues {
  /** A name the user chose: a host, folder or identity. */
  name?: string;
  /** Where something went: a folder path. */
  place?: string;
  /** Why it failed (a message from the backend; plain text). */
  detail?: string;
  /** A number of panes. */
  count?: number;
  /** The three numbers of a finished sync. */
  pushed?: number;
  received?: number;
  conflicts?: number;
}

/** What the button of a notice does. The handlers are registered by the app. */
export type NoticeActionId = 'retry-sync';

export interface Notice {
  key: NoticeKey;
  /**
   * What the notice is about (a host id). Two notices with the same key and
   * entity are one notice: the second one replaces the first one's text.
   */
  entityId?: string;
  values?: NoticeValues;
  action?: NoticeActionId;
}

export interface NoticeHandle {
  readonly id: number;
  dismiss(): void;
}

/** One notice as the adapter renders it. */
export interface ActiveNotice {
  id: number;
  key: NoticeKey;
  kind: NoticeKind;
  title: string;
  description: string;
  action: { id: NoticeActionId; label: string } | null;
  /** The notice is leaving: it stays in the list for its exit transition. */
  leaving: boolean;
}

interface Copy {
  kind: NoticeKind;
  title: string;
  /** The sentence under the title; may use the cleaned values. */
  description?: (values: NoticeValues) => string;
  /** Shown even when routine notices are off (a preview of the notice). */
  always?: boolean;
}

/** Notices on screen at once (Doc C.04: at most two). */
export const MAX_VISIBLE = 2;
/** Notices waiting for a slot. */
export const MAX_QUEUED = 6;
/** How long a routine notice stays; a failure stays longer. */
export const ROUTINE_MS = 6000;
export const FAILURE_MS = 10_000;
/** The exit transition (`--motion-context`); the notice leaves the list after it. */
export const EXIT_MS = 160;

const LABELS: Record<NoticeActionId, string> = {
  'retry-sync': 'Retry',
};

const name = (values: NoticeValues): string => values.name ?? 'This item';
const detail = (values: NoticeValues): string => values.detail ?? 'No reason was given';
const dot = (first: string, second: string): string => `${first} · ${second}`;

/** The copy of every notice. Plain text; no markup is ever built from it. */
const COPY: Record<NoticeKey, Copy> = {
  'host.saved': {
    kind: 'success',
    title: 'Host saved',
    description: (v) => dot(name(v), 'Saved on this device'),
  },
  'host.deleted': { kind: 'success', title: 'Host deleted', description: name },
  'host.delete-failed': { kind: 'error', title: 'Could not delete the host', description: detail },
  'host.favorite-failed': {
    kind: 'error',
    title: 'Could not update the favorite',
    description: detail,
  },
  'host.moved': {
    kind: 'success',
    title: 'Host moved',
    description: (v) => dot(name(v), v.place ?? 'Unfiled'),
  },
  'host.move-failed': {
    kind: 'error',
    title: 'Could not move the host',
    description: (v) => dot(name(v), detail(v)),
  },
  'host.credential-not-stored': {
    kind: 'error',
    title: 'Password not stored',
    description: (v) =>
      `Saved ${name(v)}, but the password was not stored (${detail(v)}). Termru asks for it when you connect.`,
  },
  'hosts.refresh-failed': {
    kind: 'error',
    title: 'Could not refresh the hosts',
    description: detail,
  },

  'folder.created': { kind: 'success', title: 'Folder created', description: name },
  'folder.saved': { kind: 'success', title: 'Folder saved', description: name },
  'folder.deleted': { kind: 'success', title: 'Folder deleted', description: name },
  'folder.delete-failed': {
    kind: 'error',
    title: 'Could not delete the folder',
    description: detail,
  },

  'identity.saved': { kind: 'success', title: 'Identity saved', description: name },
  'identity.created': { kind: 'success', title: 'Identity created', description: name },
  'identity.created-unbound': {
    kind: 'error',
    title: 'Key not bound',
    description: (v) => `Created ${name(v)}, but the key was not bound (${detail(v)})`,
  },
  'identity.generated': {
    kind: 'success',
    title: 'Key generated',
    description: (v) => dot(name(v), 'Private key stays on this device'),
  },
  'identity.passphrase-note': {
    kind: 'warning',
    title: 'Passphrase not remembered',
    description: detail,
  },
  'identity.bound': {
    kind: 'success',
    title: 'Key bound',
    description: (v) => dot(name(v), 'Saved on this device'),
  },
  'identity.bind-failed': { kind: 'error', title: 'Could not bind the key', description: detail },
  'identity.unbound': { kind: 'success', title: 'Key unbound', description: name },
  'identity.unbind-failed': {
    kind: 'error',
    title: 'Could not unbind the key',
    description: detail,
  },
  'identity.deleted': { kind: 'success', title: 'Identity deleted', description: name },
  'identity.delete-failed': {
    kind: 'error',
    title: 'Could not delete the identity',
    description: detail,
  },
  'identity.load-failed': {
    kind: 'error',
    title: 'Could not read the key bindings',
    description: detail,
  },

  'secret.password-forgotten': {
    kind: 'success',
    title: 'Password forgotten',
    description: () => 'This device no longer keeps the password for the host',
  },
  'secret.passphrase-forgotten': {
    kind: 'success',
    title: 'Passphrase forgotten',
    description: () => 'This device no longer keeps the key passphrase',
  },
  'secret.forget-failed': {
    kind: 'error',
    title: 'Could not forget the secret',
    description: detail,
  },
  'credential.stored': {
    kind: 'success',
    title: 'Saved to the credential store',
    description: () => 'This device keeps it. It is never synced.',
  },
  'credential.store-failed': {
    kind: 'error',
    title: 'Could not save to the credential store',
    description: detail,
  },

  'clipboard.copy-failed': {
    kind: 'error',
    title: 'Could not copy',
    description: () => 'The clipboard refused the text. Select it and copy it by hand.',
  },
  'clipboard.read-failed': {
    kind: 'error',
    title: 'Could not read the clipboard',
    description: () => 'Termru did not get any text from it',
  },
  'ssh.command-copied': {
    kind: 'success',
    title: 'SSH command copied',
    description: () => 'Paste it into any terminal',
  },
  'link.copied': { kind: 'success', title: 'Link copied' },
  'link.copied-no-browser': {
    kind: 'info',
    title: 'Link copied',
    description: () => 'Termru could not open a browser, so the address is on the clipboard',
  },
  'link.refused': {
    kind: 'error',
    title: 'Link not opened',
    description: () => 'Only http and https links can be opened',
  },
  'link.failed': {
    kind: 'error',
    title: 'Could not open the link',
    description: () => 'It could not be opened or copied',
  },

  'session.unavailable': {
    kind: 'error',
    title: 'Session unavailable',
    description: () => 'That session is no longer running',
  },
  'session.host-missing': {
    kind: 'error',
    title: 'Host missing',
    description: () => 'The host of that session no longer exists',
  },
  'session.attach-failed': {
    kind: 'error',
    title: 'Could not attach the session',
    description: detail,
  },
  'session.close-failed': {
    kind: 'error',
    title: 'Could not close the session',
    description: detail,
  },
  'session.move-failed': {
    kind: 'error',
    title: 'Could not move the session',
    description: detail,
  },
  'session.pane-limit': {
    kind: 'warning',
    title: 'Pane limit reached',
    description: (v) => `A tab holds at most ${v.count ?? 0} panes`,
  },
  'prompt.hostkey-inactive': {
    kind: 'info',
    title: 'Host key question closed',
    description: () => 'It timed out, or another window answered it',
  },
  'prompt.hostkey-failed': {
    kind: 'error',
    title: 'Could not answer the host key question',
    description: detail,
  },
  'prompt.auth-inactive': {
    kind: 'info',
    title: 'Sign-in question closed',
    description: () => 'It timed out, or another window answered it',
  },
  'prompt.auth-failed': {
    kind: 'error',
    title: 'Could not answer the sign-in question',
    description: detail,
  },

  'prefs.save-failed': {
    kind: 'error',
    title: 'Preferences not saved',
    description: detail,
  },
  'prefs.restored': {
    kind: 'success',
    title: 'Preferences restored',
    description: () => 'Every preference is back to its default',
  },
  'prefs.preview': {
    kind: 'success',
    title: 'Host saved',
    description: () => 'web-1 · Saved on this device',
    always: true,
  },

  'sync.signed-in': {
    kind: 'success',
    title: 'Signed in',
    description: (v) => v.name ?? 'Sync is on',
  },
  'sync.signin-failed': { kind: 'error', title: 'Could not sign in', description: detail },
  'sync.signed-out': {
    kind: 'success',
    title: 'Signed out',
    description: () => 'Sync is off. Everything stays on this device.',
  },
  'sync.signout-failed': { kind: 'error', title: 'Could not sign out', description: detail },
  'sync.synced': {
    kind: 'success',
    title: 'Sync finished',
    description: (v) =>
      `${v.pushed ?? 0} sent · ${v.received ?? 0} received · ${plural(v.conflicts ?? 0, 'conflict')}`,
  },
  'sync.failed': { kind: 'error', title: 'Sync failed', description: detail },
  'sync.kept-local': { kind: 'success', title: 'Kept the local copy' },
  'sync.took-server': { kind: 'success', title: 'Took the server copy' },
  'sync.resolve-failed': {
    kind: 'error',
    title: 'Could not resolve the conflict',
    description: detail,
  },
};

function plural(count: number, noun: string): string {
  return `${count} ${noun}${count === 1 ? '' : 's'}`;
}

/** Control, bidirectional-override and zero-width characters. */
// eslint-disable-next-line no-control-regex
const UNSAFE = /[\u0000-\u001f\u007f-\u009f​-‏‪-‮⁦-⁩﻿]/g;

const LIMITS = { name: 80, place: 120, detail: 220 } as const;

/** Plain text of a bounded length: no control characters, one line. */
export function cleanText(raw: unknown, limit: number): string | undefined {
  if (typeof raw !== 'string') return undefined;
  const text = raw.replace(UNSAFE, ' ').replace(/\s+/g, ' ').trim();
  if (text.length === 0) return undefined;
  return text.length > limit ? `${text.slice(0, limit - 1).trimEnd()}…` : text;
}

function cleanCount(raw: unknown): number | undefined {
  return typeof raw === 'number' && Number.isFinite(raw)
    ? Math.min(1_000_000, Math.max(0, Math.trunc(raw)))
    : undefined;
}

/** The values with every field cleaned; a field that is not text is dropped. */
export function cleanValues(values: NoticeValues | undefined): NoticeValues {
  const out: NoticeValues = {};
  if (!values) return out;
  const nameText = cleanText(values.name, LIMITS.name);
  if (nameText) out.name = nameText;
  const place = cleanText(values.place, LIMITS.place);
  if (place) out.place = place;
  const detailText = cleanText(values.detail, LIMITS.detail);
  if (detailText) out.detail = detailText;
  for (const field of ['count', 'pushed', 'received', 'conflicts'] as const) {
    const value = cleanCount(values[field]);
    if (value !== undefined) out[field] = value;
  }
  return out;
}

/** What identifies a notice for the dedup. */
function signatureOf(notice: Notice, values: NoticeValues): string {
  return notice.entityId
    ? `${notice.key}#${notice.entityId}`
    : `${notice.key}|${JSON.stringify(values)}`;
}

/** The routine kinds: they are the ones the preference can silence. */
function isRoutine(kind: NoticeKind): boolean {
  return kind === 'success' || kind === 'info';
}

interface Entry extends ActiveNotice {
  signature: string;
  /** Milliseconds the notice stays once it is on screen. */
  duration: number;
}

interface Timing {
  timer: ReturnType<typeof setTimeout> | null;
  remaining: number;
  startedAt: number;
  /** Pointer or focus is on the notice. */
  holds: number;
}

const NO_HANDLE: NoticeHandle = { id: 0, dismiss() {} };

class NoticeCenter {
  /** The notices on screen, oldest first. */
  visible = $state.raw<ActiveNotice[]>([]);

  /** Waiting for a slot, in the order they will appear. */
  #queue: Entry[] = [];
  /** On screen (a notice that is leaving included), in the order they appeared. */
  #shown: Entry[] = [];
  #entries = new Map<number, Entry>();
  #timing = new Map<number, Timing>();
  #next = 1;
  #hidden = false;
  #actions: Partial<Record<NoticeActionId, () => void>> = {};

  constructor() {
    if (typeof document !== 'undefined') {
      // A notice must not run out while nobody sees the window.
      document.addEventListener('visibilitychange', () => {
        this.#hidden = document.visibilityState === 'hidden';
        for (const id of this.#timing.keys()) this.#schedule(id);
      });
    }
  }

  /** Registers what the button of a notice does (the app wires these once). */
  setActions(handlers: Partial<Record<NoticeActionId, () => void>>): void {
    this.#actions = handlers;
  }

  /** Runs the action of a notice and dismisses it. */
  runAction(id: number): void {
    const entry = this.#entries.get(id);
    const action = entry?.action;
    if (!entry || !action) return;
    this.#actions[action.id]?.();
    this.dismiss(id);
  }

  show(notice: Notice): NoticeHandle {
    const copy = COPY[notice.key] as Copy | undefined;
    // Not in the whitelist: nothing is shown.
    if (!copy) return NO_HANDLE;
    if (isRoutine(copy.kind) && !copy.always && !prefs.prefs.routine_notices) return NO_HANDLE;

    const values = cleanValues(notice.values);
    const signature = signatureOf(notice, values);

    const title = copy.title;
    const description = copy.description ? copy.description(values) : '';
    const duration = isRoutine(copy.kind) || copy.kind === 'warning' ? ROUTINE_MS : FAILURE_MS;

    // The same notice again: it stays where it is, with the newest text and
    // a fresh timer, instead of stacking a copy.
    const existing = [...this.#entries.values()].find(
      (entry) => entry.signature === signature && !entry.leaving,
    );
    if (existing) {
      existing.title = title;
      existing.description = description;
      this.#publish();
      if (this.#timing.has(existing.id)) this.#restart(existing.id);
      return this.#handle(existing.id);
    }

    const entry: Entry = {
      id: this.#next++,
      key: notice.key,
      kind: copy.kind,
      title,
      description,
      action: notice.action ? { id: notice.action, label: LABELS[notice.action] } : null,
      leaving: false,
      signature,
      duration,
    };
    this.#entries.set(entry.id, entry);
    this.#place(entry);
    return this.#handle(entry.id);
  }

  dismiss(id: number): void {
    const entry = this.#entries.get(id);
    if (!entry) return;
    const queued = this.#queue.indexOf(entry);
    if (queued >= 0) {
      this.#queue.splice(queued, 1);
      this.#entries.delete(id);
      return;
    }
    if (entry.leaving) return;
    this.#clearTimer(id);
    this.#timing.delete(id);
    entry.leaving = true;
    this.#publish();
    setTimeout(() => {
      this.#entries.delete(id);
      this.#shown = this.#shown.filter((shown) => shown.id !== id);
      this.#publish();
      this.#fill();
    }, EXIT_MS);
  }

  /** Dismisses everything, queued notices included. */
  clear(): void {
    for (const id of [...this.#entries.keys()]) this.dismiss(id);
  }

  /** Drops everything at once, without an exit (a clean slate for tests). */
  reset(): void {
    for (const id of [...this.#timing.keys()]) this.#clearTimer(id);
    this.#timing.clear();
    this.#entries.clear();
    this.#queue = [];
    this.#shown = [];
    this.visible = [];
  }

  /** Pointer or keyboard focus is on a notice: its timer waits. */
  hold(id: number): void {
    const timing = this.#timing.get(id);
    if (!timing) return;
    timing.holds += 1;
    this.#schedule(id);
  }

  release(id: number): void {
    const timing = this.#timing.get(id);
    if (!timing) return;
    timing.holds = Math.max(0, timing.holds - 1);
    this.#schedule(id);
  }

  /** How many notices wait for a slot (for tests and the adapter). */
  get queued(): number {
    return this.#queue.length;
  }

  /** Puts a new notice on screen, queues it, or makes room for a failure. */
  #place(entry: Entry): void {
    const live = this.#live();
    if (live.length < MAX_VISIBLE) {
      this.#start(entry);
      return;
    }
    if (entry.kind === 'error') {
      // A failure does not wait behind a routine notice: it takes the slot
      // of the oldest one.
      const routine = live.find((candidate) => isRoutine(candidate.kind));
      if (routine) {
        this.dismiss(routine.id);
        this.#start(entry);
        return;
      }
      this.#queue.unshift(entry);
    } else {
      this.#queue.push(entry);
    }
    this.#trimQueue();
  }

  /** Drops the oldest routine notice while the queue is over its bound. */
  #trimQueue(): void {
    while (this.#queue.length > MAX_QUEUED) {
      let index = this.#queue.findIndex((candidate) => isRoutine(candidate.kind));
      if (index < 0) index = this.#queue.length - 1;
      const [dropped] = this.#queue.splice(index, 1);
      this.#entries.delete(dropped.id);
    }
  }

  /** Moves queued notices onto the screen while there is room. */
  #fill(): void {
    while (this.#queue.length > 0 && this.#live().length < MAX_VISIBLE) {
      const next = this.#queue.shift();
      if (next) this.#start(next);
    }
  }

  #start(entry: Entry): void {
    this.#shown.push(entry);
    this.#timing.set(entry.id, {
      timer: null,
      remaining: entry.duration,
      startedAt: Date.now(),
      holds: 0,
    });
    this.#publish();
    this.#schedule(entry.id);
  }

  /** The notices that are on screen and not leaving. */
  #live(): Entry[] {
    return this.#shown.filter((entry) => !entry.leaving);
  }

  #publish(): void {
    this.visible = this.#shown.map((entry) => ({
      id: entry.id,
      key: entry.key,
      kind: entry.kind,
      title: entry.title,
      description: entry.description,
      action: entry.action,
      leaving: entry.leaving,
    }));
  }

  #handle(id: number): NoticeHandle {
    return { id, dismiss: () => this.dismiss(id) };
  }

  #clearTimer(id: number): void {
    const timing = this.#timing.get(id);
    if (timing?.timer) clearTimeout(timing.timer);
    if (timing) timing.timer = null;
  }

  /** Runs, or pauses, the timer of a notice by the holds on it. */
  #schedule(id: number): void {
    const timing = this.#timing.get(id);
    if (!timing) return;
    const holding = timing.holds > 0 || this.#hidden;
    if (timing.timer && holding) {
      timing.remaining = Math.max(0, timing.remaining - (Date.now() - timing.startedAt));
      this.#clearTimer(id);
      return;
    }
    if (!timing.timer && !holding) {
      timing.startedAt = Date.now();
      timing.timer = setTimeout(() => this.dismiss(id), timing.remaining);
    }
  }

  /** A fresh full timer (the same notice arrived again). */
  #restart(id: number): void {
    const timing = this.#timing.get(id);
    const entry = this.#entries.get(id);
    if (!timing || !entry) return;
    this.#clearTimer(id);
    timing.remaining = entry.duration;
    this.#schedule(id);
  }
}

export const notices = new NoticeCenter();

/** Shows a notice by key. The one function the rest of the app calls. */
export function notify(notice: Notice): NoticeHandle {
  return notices.show(notice);
}

/** Whether a notice key is in the copy whitelist (for tests and tooling). */
export function isNoticeKey(key: string): key is NoticeKey {
  return Object.prototype.hasOwnProperty.call(COPY, key);
}

/** The kind of a notice, from its copy. */
export function kindOf(key: NoticeKey): NoticeKind {
  return COPY[key].kind;
}
