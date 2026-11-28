// @vitest-environment jsdom
/**
 * The notify facade (Doc C.04): a copy whitelist, cleaned values, dedup of the
 * same notice, a bounded queue where a failure does not wait behind a routine
 * notice, roles by kind, and the routine-notices preference. The component
 * `Toasts.svelte` is the one adapter that draws them.
 */

import { readFileSync } from 'node:fs';
import { flushSync, mount, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import Toasts from './components/Toasts.svelte';
import {
  EXIT_MS,
  FAILURE_MS,
  MAX_QUEUED,
  MAX_VISIBLE,
  ROUTINE_MS,
  cleanText,
  cleanValues,
  isNoticeKey,
  kindOf,
  notices,
  notify,
  type NoticeKey,
} from './notify.svelte';
import { defaultPrefs, prefs } from './stores/prefs.svelte';
import { click, newTarget } from './test-support/dom';

vi.mock('@tauri-apps/api/core', () => import('./test-support/tauri-core-mock'));

const keys = () => notices.visible.map((notice) => notice.key);

beforeEach(() => {
  vi.useFakeTimers();
  notices.reset();
  notices.setActions({});
  prefs.prefs = defaultPrefs();
});

afterEach(() => {
  notices.reset();
  vi.useRealTimers();
});

describe('the copy whitelist', () => {
  it('shows the title and the sentence of a known key', () => {
    notify({ key: 'host.saved', values: { name: 'atlas-prod' } });
    expect(notices.visible).toHaveLength(1);
    expect(notices.visible[0].title).toBe('Host saved');
    expect(notices.visible[0].description).toBe('atlas-prod · Saved on this device');
    expect(notices.visible[0].kind).toBe('success');
  });

  it('shows nothing for a key that is not in the whitelist', () => {
    const handle = notify({ key: 'terminal.said-so' as NoticeKey });
    expect(notices.visible).toHaveLength(0);
    expect(handle.id).toBe(0);
    // A handle of nothing is safe to dismiss.
    handle.dismiss();
    expect(isNoticeKey('host.saved')).toBe(true);
    expect(isNoticeKey('terminal.said-so')).toBe(false);
    expect(isNoticeKey('toString')).toBe(false);
  });

  it('has a kind for every key, and a failure is an error or a warning', () => {
    expect(kindOf('host.saved')).toBe('success');
    expect(kindOf('host.delete-failed')).toBe('error');
    expect(kindOf('session.pane-limit')).toBe('warning');
    expect(kindOf('link.copied-no-browser')).toBe('info');
  });

  it('writes the sentence from the values, never from the caller’s text', () => {
    notify({
      key: 'host.delete-failed',
      values: { detail: 'the store is locked' },
    });
    expect(notices.visible[0].title).toBe('Could not delete the host');
    expect(notices.visible[0].description).toBe('the store is locked');
  });

  it('says so when a failure gave no reason', () => {
    notify({ key: 'host.delete-failed' });
    expect(notices.visible[0].description).toBe('No reason was given');
  });

  it('puts the numbers of a sync into its sentence', () => {
    notify({ key: 'sync.synced', values: { pushed: 3, received: 1, conflicts: 1 } });
    expect(notices.visible[0].description).toBe('3 sent · 1 received · 1 conflict');
  });
});

describe('cleaning what goes into a sentence', () => {
  it('removes control, bidirectional and zero-width characters', () => {
    expect(cleanText('a\u0000b\u001bc‮d​e⁦f', 80)).toBe('a b c d e f');
  });

  it('puts the text on one line and trims it', () => {
    expect(cleanText('  one\n\n two\t three  ', 80)).toBe('one two three');
  });

  it('caps the length, with an ellipsis', () => {
    const cleaned = cleanText('x'.repeat(500), 40) as string;
    expect(cleaned).toHaveLength(40);
    expect(cleaned.endsWith('…')).toBe(true);
  });

  it('drops a value that is not text, or empty', () => {
    expect(cleanText(42, 10)).toBeUndefined();
    expect(cleanText('   ', 10)).toBeUndefined();
    expect(cleanText(undefined, 10)).toBeUndefined();
    expect(cleanValues({ name: '\u0007' }).name).toBeUndefined();
  });

  it('keeps counts whole and in range', () => {
    expect(cleanValues({ count: -4 }).count).toBe(0);
    expect(cleanValues({ pushed: 3.9 }).pushed).toBe(3);
    expect(cleanValues({ received: Number.NaN }).received).toBeUndefined();
    expect(cleanValues({ conflicts: 1e12 }).conflicts).toBe(1_000_000);
  });

  it('cleans a host name and a server message before they reach the notice', () => {
    notify({ key: 'host.saved', values: { name: 'evil‮\ngpj.name' } });
    expect(notices.visible[0].description).toBe('evil gpj.name · Saved on this device');
    notify({ key: 'sync.failed', values: { detail: `server said:\r\n${'z'.repeat(400)}` } });
    const description = notices.visible.at(-1)?.description ?? '';
    expect(description.length).toBeLessThanOrEqual(220);
    expect(description).not.toMatch(/[\r\n]/);
  });
});

describe('dedup', () => {
  it('shows the same notice once', () => {
    notify({ key: 'clipboard.copy-failed' });
    notify({ key: 'clipboard.copy-failed' });
    notify({ key: 'clipboard.copy-failed' });
    expect(notices.visible).toHaveLength(1);
  });

  it('returns the handle of the notice that is already there', () => {
    const first = notify({ key: 'clipboard.copy-failed' });
    const second = notify({ key: 'clipboard.copy-failed' });
    expect(second.id).toBe(first.id);
  });

  it('treats different values as different notices', () => {
    notify({ key: 'host.deleted', values: { name: 'a' } });
    notify({ key: 'host.deleted', values: { name: 'b' } });
    expect(notices.visible).toHaveLength(2);
  });

  it('merges notices of the same entity and shows the newest text', () => {
    notify({ key: 'host.saved', entityId: 'h1', values: { name: 'staging' } });
    notify({ key: 'host.saved', entityId: 'h1', values: { name: 'staging-api' } });
    expect(notices.visible).toHaveLength(1);
    expect(notices.visible[0].description).toBe('staging-api · Saved on this device');
    // Another entity is another notice.
    notify({ key: 'host.saved', entityId: 'h2', values: { name: 'relay' } });
    expect(notices.visible).toHaveLength(2);
  });

  it('restarts the timer of the notice that came again', () => {
    notify({ key: 'link.copied' });
    vi.advanceTimersByTime(ROUTINE_MS - 1000);
    notify({ key: 'link.copied' });
    vi.advanceTimersByTime(ROUTINE_MS - 1000);
    // Without the restart it would be gone by now.
    expect(keys()).toEqual(['link.copied']);
    vi.advanceTimersByTime(1500 + EXIT_MS);
    expect(notices.visible).toHaveLength(0);
  });

  it('shows the same event again once the notice has left', () => {
    notify({ key: 'link.copied' });
    vi.advanceTimersByTime(ROUTINE_MS + EXIT_MS);
    expect(notices.visible).toHaveLength(0);
    notify({ key: 'link.copied' });
    expect(notices.visible).toHaveLength(1);
  });

  it('merges with a notice that is waiting in the queue', () => {
    notify({ key: 'host.deleted', values: { name: 'a' } });
    notify({ key: 'host.deleted', values: { name: 'b' } });
    notify({ key: 'host.deleted', values: { name: 'c' } });
    expect(notices.queued).toBe(1);
    notify({ key: 'host.deleted', values: { name: 'c' } });
    expect(notices.queued).toBe(1);
  });
});

describe('the bounded queue', () => {
  const routine = (n: number) => notify({ key: 'host.deleted', values: { name: `host-${n}` } });
  const failure = (n: number) =>
    notify({ key: 'host.delete-failed', values: { detail: `reason ${n}` } });

  it('shows at most two notices and queues the rest', () => {
    expect(MAX_VISIBLE).toBe(2);
    for (let n = 1; n <= 5; n += 1) routine(n);
    expect(notices.visible).toHaveLength(MAX_VISIBLE);
    expect(notices.queued).toBe(3);
  });

  it('brings the next one in when a slot frees', () => {
    for (let n = 1; n <= 3; n += 1) routine(n);
    vi.advanceTimersByTime(ROUTINE_MS + EXIT_MS);
    // The first two left; the third took a slot.
    expect(notices.visible.map((notice) => notice.description)).toEqual(['host-3']);
    expect(notices.queued).toBe(0);
  });

  it('bounds the queue: the oldest routine notice is dropped first', () => {
    for (let n = 1; n <= MAX_VISIBLE + MAX_QUEUED + 4; n += 1) routine(n);
    expect(notices.visible).toHaveLength(MAX_VISIBLE);
    expect(notices.queued).toBe(MAX_QUEUED);
    // The newest survive in the queue: drain it and read what came out.
    const seen: string[] = notices.visible.map((notice) => notice.description);
    for (let round = 0; round < 10; round += 1) {
      vi.advanceTimersByTime(ROUTINE_MS + EXIT_MS);
      for (const notice of notices.visible) {
        if (!seen.includes(notice.description)) seen.push(notice.description);
      }
    }
    expect(seen).toContain(`host-${MAX_VISIBLE + MAX_QUEUED + 4}`);
    expect(seen).not.toContain('host-3');
  });

  it('lets a failure take the slot of a routine notice instead of waiting', () => {
    routine(1);
    routine(2);
    failure(1);
    expect(keys()).toContain('host.delete-failed');
    expect(notices.visible.filter((notice) => !notice.leaving)).toHaveLength(MAX_VISIBLE);
    expect(notices.queued).toBe(0);
    // The oldest routine notice is the one that went.
    expect(notices.visible.find((notice) => notice.description === 'host-1')?.leaving).toBe(true);
    expect(notices.visible.find((notice) => notice.description === 'host-2')?.leaving).toBe(false);
  });

  it('queues a failure at the front when every slot holds a failure', () => {
    failure(1);
    failure(2);
    routine(1);
    failure(3);
    expect(notices.queued).toBe(2);
    vi.advanceTimersByTime(FAILURE_MS + EXIT_MS);
    // The failure that waited came before the routine notice that arrived earlier.
    expect(notices.visible[0].description).toBe('reason 3');
  });

  it('never drops a failure to make room for routine notices', () => {
    failure(1);
    failure(2);
    for (let n = 1; n <= MAX_QUEUED + 5; n += 1) failure(100 + n);
    expect(notices.queued).toBeLessThanOrEqual(MAX_QUEUED);
    for (let n = 1; n <= 20; n += 1) routine(n);
    expect(notices.queued).toBeLessThanOrEqual(MAX_QUEUED);
    expect(notices.visible.every((notice) => notice.kind === 'error')).toBe(true);
  });
});

describe('time', () => {
  it('keeps a routine notice for six seconds, then lets it leave', () => {
    notify({ key: 'link.copied' });
    vi.advanceTimersByTime(ROUTINE_MS - 1);
    expect(notices.visible[0].leaving).toBe(false);
    vi.advanceTimersByTime(1);
    expect(notices.visible[0].leaving).toBe(true);
    vi.advanceTimersByTime(EXIT_MS);
    expect(notices.visible).toHaveLength(0);
  });

  it('keeps a failure longer than a routine notice', () => {
    expect(FAILURE_MS).toBeGreaterThan(ROUTINE_MS);
    notify({ key: 'clipboard.copy-failed' });
    vi.advanceTimersByTime(ROUTINE_MS + EXIT_MS);
    expect(notices.visible).toHaveLength(1);
    vi.advanceTimersByTime(FAILURE_MS - ROUTINE_MS - EXIT_MS);
    expect(notices.visible[0].leaving).toBe(true);
  });

  it('waits while the pointer or the focus is on a notice, and picks up where it stopped', () => {
    const { id } = notify({ key: 'link.copied' });
    vi.advanceTimersByTime(4000);
    notices.hold(id);
    vi.advanceTimersByTime(60_000);
    expect(notices.visible[0].leaving).toBe(false);
    notices.release(id);
    // Two seconds were left.
    vi.advanceTimersByTime(1999);
    expect(notices.visible[0].leaving).toBe(false);
    vi.advanceTimersByTime(1);
    expect(notices.visible[0].leaving).toBe(true);
  });

  it('waits while the window is hidden', () => {
    notify({ key: 'link.copied' });
    vi.advanceTimersByTime(3000);
    Object.defineProperty(document, 'visibilityState', { configurable: true, value: 'hidden' });
    document.dispatchEvent(new Event('visibilitychange'));
    vi.advanceTimersByTime(60_000);
    expect(notices.visible[0].leaving).toBe(false);
    Object.defineProperty(document, 'visibilityState', { configurable: true, value: 'visible' });
    document.dispatchEvent(new Event('visibilitychange'));
    vi.advanceTimersByTime(3000);
    expect(notices.visible[0].leaving).toBe(true);
  });

  it('dismisses on request, with an exit', () => {
    const handle = notify({ key: 'link.copied' });
    handle.dismiss();
    expect(notices.visible[0].leaving).toBe(true);
    vi.advanceTimersByTime(EXIT_MS);
    expect(notices.visible).toHaveLength(0);
    // Dismissing twice is harmless.
    handle.dismiss();
  });

  it('removes a queued notice without showing it', () => {
    notify({ key: 'host.deleted', values: { name: 'a' } });
    notify({ key: 'host.deleted', values: { name: 'b' } });
    const queued = notify({ key: 'host.deleted', values: { name: 'c' } });
    queued.dismiss();
    expect(notices.queued).toBe(0);
    vi.advanceTimersByTime(ROUTINE_MS + EXIT_MS);
    expect(notices.visible).toHaveLength(0);
  });
});

describe('the routine notices preference', () => {
  it('shows no routine notice when it is off', () => {
    prefs.prefs = { ...defaultPrefs(), routine_notices: false };
    notify({ key: 'host.saved', values: { name: 'a' } });
    notify({ key: 'link.copied' });
    notify({ key: 'link.copied-no-browser' });
    expect(notices.visible).toHaveLength(0);
  });

  it('still shows failures and warnings when it is off', () => {
    prefs.prefs = { ...defaultPrefs(), routine_notices: false };
    notify({ key: 'clipboard.copy-failed' });
    notify({ key: 'session.pane-limit', values: { count: 8 } });
    expect(keys()).toEqual(['clipboard.copy-failed', 'session.pane-limit']);
  });

  it('shows the preview even when it is off', () => {
    prefs.prefs = { ...defaultPrefs(), routine_notices: false };
    notify({ key: 'prefs.preview' });
    expect(keys()).toEqual(['prefs.preview']);
  });

  it('follows the preference as it changes', () => {
    prefs.prefs = { ...defaultPrefs(), routine_notices: false };
    notify({ key: 'link.copied' });
    expect(notices.visible).toHaveLength(0);
    prefs.prefs = { ...defaultPrefs(), routine_notices: true };
    notify({ key: 'link.copied' });
    expect(notices.visible).toHaveLength(1);
  });
});

describe('the button of a notice', () => {
  it('runs the registered handler and dismisses the notice', () => {
    const retry = vi.fn();
    notices.setActions({ 'retry-sync': retry });
    const { id } = notify({ key: 'sync.failed', values: { detail: 'down' }, action: 'retry-sync' });
    expect(notices.visible[0].action).toEqual({ id: 'retry-sync', label: 'Retry' });
    notices.runAction(id);
    expect(retry).toHaveBeenCalledTimes(1);
    expect(notices.visible[0].leaving).toBe(true);
  });

  it('tells the handler what the notice was about (a host), when it says', () => {
    const review = vi.fn();
    notices.setActions({ 'review-host-exec': review });
    const { id } = notify({
      key: 'host.exec-pending',
      entityId: 'host-7',
      values: { name: 'atlas-prod' },
      action: 'review-host-exec',
    });
    expect(notices.visible[0].kind).toBe('warning');
    expect(notices.visible[0].title).toBe('Approval needed');
    expect(notices.visible[0].description).toBe(
      'Synced settings for atlas-prod need your approval on this device',
    );
    expect(notices.visible[0].action).toEqual({ id: 'review-host-exec', label: 'Review' });
    notices.runAction(id);
    expect(review).toHaveBeenCalledWith('host-7');
  });

  it('says what an import brought in, and a copied forward address', () => {
    notify({ key: 'hosts.imported', values: { count: 1, skipped: 2 } });
    expect(notices.visible[0].description).toBe('1 host added from your SSH config · 2 skipped');
    notify({ key: 'forward.address-copied', values: { place: '127.0.0.1:8080' } });
    expect(notices.visible[1].description).toBe('127.0.0.1:8080');
  });

  it('does nothing for a notice that has no action', () => {
    const retry = vi.fn();
    notices.setActions({ 'retry-sync': retry });
    const { id } = notify({ key: 'link.copied' });
    notices.runAction(id);
    expect(retry).not.toHaveBeenCalled();
    expect(notices.visible[0].leaving).toBe(false);
  });
});

describe('the adapter (Toasts.svelte)', () => {
  let target: HTMLElement;
  let component: ReturnType<typeof mount> | null = null;

  const render = () => {
    target = newTarget();
    component = mount(Toasts, { target });
    flushSync();
  };

  afterEach(() => {
    if (component) unmount(component);
    component = null;
    document.body.replaceChildren();
  });

  const items = () => Array.from(target.querySelectorAll<HTMLElement>('.notice'));

  it('has one labelled region that is always in the page', () => {
    render();
    expect(target.querySelector('section[aria-label="Notices"]')).not.toBeNull();
    expect(items()).toHaveLength(0);
  });

  it('makes a failure role=alert and every other kind role=status', () => {
    render();
    notify({ key: 'clipboard.copy-failed' });
    notify({ key: 'session.pane-limit', values: { count: 8 } });
    flushSync();
    expect(items().map((item) => item.getAttribute('role'))).toEqual(['alert', 'alert']);
    notices.reset();
    flushSync();
    notify({ key: 'link.copied' });
    notify({ key: 'link.copied-no-browser' });
    flushSync();
    expect(items().map((item) => item.getAttribute('role'))).toEqual(['status', 'status']);
  });

  it('shows the title, the sentence and a glyph of its own for each kind', () => {
    render();
    notify({ key: 'host.saved', values: { name: 'atlas-prod' } });
    flushSync();
    const item = items()[0];
    expect(item.querySelector('.title')?.textContent).toBe('Host saved');
    expect(item.querySelector('.description')?.textContent).toBe(
      'atlas-prod · Saved on this device',
    );
    expect(item.querySelector('svg')).not.toBeNull();
    expect(item.classList.contains('success')).toBe(true);
  });

  it('shows what a server sent as plain text', () => {
    render();
    notify({ key: 'sync.failed', values: { detail: '<img src=x onerror=alert(1)><b>x</b>' } });
    flushSync();
    expect(target.querySelector('img')).toBeNull();
    expect(target.querySelector('b')).toBeNull();
    expect(items()[0].textContent).toContain('<img src=x onerror=alert(1)>');
  });

  it('dismisses with a button that has a name', () => {
    render();
    notify({ key: 'link.copied' });
    flushSync();
    const dismiss = target.querySelector<HTMLButtonElement>('[aria-label="Dismiss notification"]');
    expect(dismiss).not.toBeNull();
    click(dismiss as HTMLElement);
    expect(items()[0].classList.contains('leaving')).toBe(true);
    vi.advanceTimersByTime(EXIT_MS);
    flushSync();
    expect(items()).toHaveLength(0);
  });

  it('draws the action as a button and runs it', () => {
    const retry = vi.fn();
    notices.setActions({ 'retry-sync': retry });
    render();
    notify({ key: 'sync.failed', values: { detail: 'down' }, action: 'retry-sync' });
    flushSync();
    const button = items()[0].querySelector<HTMLButtonElement>('.action');
    expect(button?.textContent?.trim()).toBe('Retry');
    click(button as HTMLElement);
    expect(retry).toHaveBeenCalledTimes(1);
  });

  it('never takes the focus', () => {
    const field = document.createElement('input');
    document.body.appendChild(field);
    field.focus();
    render();
    notify({ key: 'clipboard.copy-failed' });
    flushSync();
    expect(document.activeElement).toBe(field);
  });

  it('holds the timer while the pointer is on a notice', () => {
    render();
    notify({ key: 'link.copied' });
    flushSync();
    items()[0].dispatchEvent(new Event('pointerenter'));
    vi.advanceTimersByTime(ROUTINE_MS * 3);
    flushSync();
    expect(items()[0].classList.contains('leaving')).toBe(false);
    items()[0].dispatchEvent(new Event('pointerleave'));
    vi.advanceTimersByTime(ROUTINE_MS);
    flushSync();
    expect(items()[0].classList.contains('leaving')).toBe(true);
  });

  it('is at the top right, under the title bar and the navigation, above the page', () => {
    // jsdom has no layout: the placement is in the stylesheet of the component.
    const source = readSource('components/Toasts.svelte');
    expect(source).toMatch(/top:\s*calc\(var\(--termru-titlebar-h\)\s*\+\s*var\(--termru-nav-h\)/);
    expect(source).toMatch(/right:\s*var\(--termru-space-5\)/);
    expect(source).toMatch(/position:\s*fixed/);
    // The strip does not block the page; only a notice takes clicks.
    expect(source).toMatch(/pointer-events:\s*none/);
    // Opacity and transform only, as CSS transitions, with the reduced-motion tokens.
    expect(source).not.toMatch(/@keyframes/);
    expect(source).not.toMatch(/transition:\s*all/);
    expect(source).toContain('var(--notice-shift)');
  });
});

function readSource(path: string): string {
  return readFileSync(new URL(path, import.meta.url), 'utf8');
}
