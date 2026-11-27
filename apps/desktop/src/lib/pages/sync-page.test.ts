// @vitest-environment jsdom
/**
 * The Sync page: a real routed page. Signed out it is the sign-in form
 * (validated on blur, nothing sent while it is invalid); signed in it shows
 * the status in words, the conflicts to decide, this device and the account.
 * The status says what is true, including offline.
 */

import { flushSync, mount, tick, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import SyncPage from './SyncPage.svelte';
import { notices } from '../notify.svelte';
import { network } from '../stores/network.svelte';
import { sync } from '../stores/sync.svelte';
import { click, findButton, newTarget, stubBrowserApis, type } from '../test-support/dom';
import { noticeText } from '../test-support/notices';
import { answers, callsOf, failures, resetTauriMock } from '../test-support/tauri-core-mock';
import type { ConflictView, SyncStatus } from '../types';

const bus = vi.hoisted(() => ({
  handlers: new Map<string, (event: { payload: unknown }) => void>(),
}));

vi.mock('@tauri-apps/api/core', () => import('../test-support/tauri-core-mock'));
vi.mock('@tauri-apps/api/event', () => ({
  listen: async (name: string, handler: (event: { payload: unknown }) => void) => {
    bus.handlers.set(name, handler);
    return () => bus.handlers.delete(name);
  },
}));

function status(over: Partial<SyncStatus> = {}): SyncStatus {
  return {
    settings: {
      server_url: 'https://sync.example.com',
      email: 'ops@example.com',
      device_name: 'workstation',
      device_id: 'device-1234',
    },
    signed_in: true,
    pending: 0,
    conflicts: 0,
    cursor: 5,
    last_sync_at: new Date().toISOString(),
    last_error: null,
    ...over,
  };
}

const signedOut = (): SyncStatus =>
  status({
    signed_in: false,
    last_sync_at: null,
    settings: { server_url: '', email: '', device_name: 'workstation', device_id: 'device-1234' },
  });

function conflict(over: Partial<ConflictView> = {}): ConflictView {
  return {
    record_id: 'h1',
    kind: 'host',
    server_rev: 9,
    observed_at: '2026-01-01T00:00:00Z',
    local: { id: 'h1', name: 'staging', address: '10.0.0.1', port: 22, updated_at: 'a' },
    remote: { id: 'h1', name: 'staging', address: '10.0.0.9', port: 22, updated_at: 'b' },
    ...over,
  };
}

let target: HTMLElement;
let component: ReturnType<typeof mount> | null = null;

function give(next: SyncStatus, conflicts: ConflictView[] = []): void {
  answers.set('sync_status', next);
  answers.set('sync_conflicts', conflicts);
}

async function mountPage(): Promise<void> {
  component = mount(SyncPage, { target });
  flushSync();
  await vi.waitFor(() => expect(callsOf('sync_status').length).toBeGreaterThan(0));
  await tick();
  await vi.waitFor(() => expect(sync.loading).toBe(false));
  flushSync();
}

const field = (id: string) => document.getElementById(id) as HTMLInputElement;
const text = () => target.textContent ?? '';

beforeEach(() => {
  resetTauriMock();
  stubBrowserApis();
  notices.reset();
  bus.handlers.clear();
  sync.status = null;
  sync.conflicts = [];
  sync.error = null;
  sync.busy = false;
  network.online = true;
  target = newTarget();
});

afterEach(() => {
  if (component) unmount(component);
  component = null;
  target.remove();
  document.body.replaceChildren();
});

describe('the page anatomy', () => {
  it('has the breadcrumb, the title and one line under it', async () => {
    give(status());
    await mountPage();
    expect(target.querySelector('.crumbs')?.textContent).toMatch(/Workspace\s*\/\s*Cloud sync/);
    expect(target.querySelector('h1')?.textContent).toBe('Sync');
    expect(target.querySelector('.description')?.textContent).toContain('Secrets stay here');
  });

  it('is a work page on the white surface, not a wrapper around a modal', async () => {
    give(status());
    await mountPage();
    expect(target.querySelector('.workpage')).not.toBeNull();
    expect(target.querySelector('[role="dialog"], .backdrop')).toBeNull();
  });
});

describe('signed out', () => {
  beforeEach(() => give(signedOut()));

  it('is the sign-in form, with no Sync now', async () => {
    await mountPage();
    expect(findButton(target, 'Sign in')).toBeTruthy();
    expect(text()).not.toContain('Sync now');
    expect(field('sync-server')).not.toBeNull();
    expect(field('sync-email')).not.toBeNull();
    expect(field('sync-password').type).toBe('password');
    expect(field('sync-device').value).toBe('workstation');
    expect(text()).toContain('Sync is off');
  });

  it('explains a bad server address on blur, next to the field', async () => {
    await mountPage();
    type(field('sync-server'), 'http://sync.example.com');
    field('sync-server').dispatchEvent(new FocusEvent('blur'));
    flushSync();
    expect(field('sync-server').getAttribute('aria-invalid')).toBe('true');
    const message = document.getElementById(
      field('sync-server').getAttribute('aria-describedby') ?? '',
    );
    expect(message?.textContent).toContain('Use https');
  });

  it('accepts plain http for localhost only', async () => {
    await mountPage();
    type(field('sync-server'), 'http://localhost:8080');
    field('sync-server').dispatchEvent(new FocusEvent('blur'));
    flushSync();
    expect(field('sync-server').getAttribute('aria-invalid')).toBeNull();
  });

  it('sends nothing while the form is invalid, and focuses the first bad field', async () => {
    await mountPage();
    click(findButton(target, 'Sign in'));
    target
      .querySelector('form')
      ?.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }));
    flushSync();
    expect(callsOf('sync_sign_in')).toHaveLength(0);
    expect(document.activeElement).toBe(field('sync-server'));
    expect(field('sync-email').getAttribute('aria-invalid')).toBe('true');
    expect(field('sync-password').getAttribute('aria-invalid')).toBe('true');
  });

  it('signs in with the typed values, then forgets the password', async () => {
    answers.set('sync_sign_in', status());
    await mountPage();
    type(field('sync-server'), 'https://sync.example.com/');
    type(field('sync-email'), 'ops@example.com');
    type(field('sync-password'), 'correct horse');
    type(field('sync-device'), 'laptop');
    target
      .querySelector('form')
      ?.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }));
    await vi.waitFor(() => expect(callsOf('sync_sign_in')).toHaveLength(1));
    expect(callsOf('sync_sign_in')[0].args).toEqual({
      serverUrl: 'https://sync.example.com/',
      email: 'ops@example.com',
      password: 'correct horse',
      deviceName: 'laptop',
    });
    await vi.waitFor(() => expect(notices.visible.map((n) => n.key)).toContain('sync.signed-in'));
    // The password never lands in a notice.
    expect(JSON.stringify(notices.visible)).not.toContain('correct horse');
    expect(field('sync-password')?.value ?? '').toBe('');
  });

  it('shows the server’s refusal under the form and in a notice', async () => {
    failures.set('sync_sign_in', 'invalid credentials');
    await mountPage();
    type(field('sync-server'), 'https://sync.example.com');
    type(field('sync-email'), 'ops@example.com');
    type(field('sync-password'), 'nope');
    target
      .querySelector('form')
      ?.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }));
    await vi.waitFor(() =>
      expect(target.querySelector('.problem')?.textContent).toContain('invalid'),
    );
    expect(target.querySelector('.problem')?.getAttribute('role')).toBe('alert');
    expect(notices.visible[0].kind).toBe('error');
  });

  it('keeps what was typed when the status is read again', async () => {
    await mountPage();
    type(field('sync-server'), 'https://typed.example.com');
    sync.status = {
      ...signedOut(),
      settings: {
        server_url: 'https://stored.example.com',
        email: 's@x.io',
        device_name: 'd',
        device_id: '1',
      },
    };
    flushSync();
    expect(field('sync-server').value).toBe('https://typed.example.com');
  });
});

describe('signed in: the status', () => {
  it('says up to date, with a glyph and words, and shows server, account and device', async () => {
    give(status());
    await mountPage();
    const line = target.querySelector('.status');
    expect(line?.getAttribute('role')).toBe('status');
    expect(line?.textContent).toContain('Up to date');
    expect(line?.querySelector('.glyph')).not.toBeNull();
    expect(text()).toContain('https://sync.example.com');
    expect(text()).toContain('ops@example.com');
    expect(text()).toContain('workstation');
    expect(text()).toContain('device-1234');
    expect(text()).toContain('Today,');
  });

  it('counts the changes waiting', async () => {
    give(status({ pending: 3 }));
    await mountPage();
    expect(target.querySelector('.status')?.textContent).toContain('3 changes waiting');
  });

  it('says the last cycle failed, with the reason', async () => {
    give(status({ last_error: 'could not reach the server' }));
    await mountPage();
    const line = target.querySelector('.status');
    expect(line?.classList.contains('failed')).toBe(true);
    expect(line?.textContent).toContain('Last sync failed');
    expect(line?.textContent).toContain('could not reach the server');
  });

  it('says how many conflicts need a decision', async () => {
    give(status({ conflicts: 1 }), [conflict()]);
    await mountPage();
    expect(target.querySelector('.status')?.textContent).toContain('1 conflict to decide');
  });

  it('says Sync offline with the last known state, and that SSH keeps running', async () => {
    give(status());
    await mountPage();
    network.online = false;
    flushSync();
    const line = target.querySelector('.status');
    expect(line?.textContent).toContain('Sync offline');
    expect(line?.textContent).toContain('Last known state');
    expect(line?.textContent).toContain('SSH sessions keep running');
    // Sync now is there, and says why it cannot run.
    const button = findButton(target, 'Sync now');
    expect(button.disabled).toBe(true);
    expect(button.title).toContain('Offline');
  });

  it('shows Never when nothing has synced yet', async () => {
    give(status({ last_sync_at: null }));
    await mountPage();
    expect(text()).toContain('Never');
  });
});

describe('signed in: actions', () => {
  it('runs a sync and reports it in a notice', async () => {
    give(status({ pending: 2 }));
    answers.set('sync_now', { pushed: 2, pulled: 1, applied: 1, conflicts: 0, cursor: 6 });
    await mountPage();
    click(findButton(target, 'Sync now'));
    await vi.waitFor(() => expect(callsOf('sync_now')).toHaveLength(1));
    await vi.waitFor(() => expect(notices.visible.map((n) => n.key)).toContain('sync.synced'));
    expect(noticeText(notices.visible[0])).toContain('2 sent · 1 received · 0 conflicts');
  });

  it('offers a retry in the notice when a sync fails', async () => {
    give(status());
    failures.set('sync_now', 'connection refused');
    await mountPage();
    click(findButton(target, 'Sync now'));
    await vi.waitFor(() => expect(notices.visible).toHaveLength(1));
    expect(notices.visible[0].kind).toBe('error');
    expect(notices.visible[0].action?.id).toBe('retry-sync');
  });

  it('signs out', async () => {
    give(status());
    await mountPage();
    click(findButton(target, 'Sign out'));
    await vi.waitFor(() => expect(callsOf('sync_sign_out')).toHaveLength(1));
  });

  it('opens the form to change the server or account, and hides it again', async () => {
    give(status());
    await mountPage();
    expect(field('sync-server')).toBeNull();
    const toggle = findButton(target, 'Change server or account');
    expect(toggle.getAttribute('aria-expanded')).toBe('false');
    click(toggle);
    expect(field('sync-server').value).toBe('https://sync.example.com');
    expect(field('sync-email').value).toBe('ops@example.com');
    expect(toggle.getAttribute('aria-expanded')).toBe('true');
    click(findButton(target, 'Cancel'));
    expect(field('sync-server')).toBeNull();
  });
});

describe('signed in: conflicts', () => {
  it('lists each record with only the fields that differ', async () => {
    give(status({ conflicts: 1 }), [conflict()]);
    await mountPage();
    const section = target.querySelector('.conflicts') as HTMLElement;
    expect(section.textContent).toContain('staging');
    const rows = Array.from(section.querySelectorAll('tbody tr'));
    expect(rows).toHaveLength(1);
    expect(rows[0].textContent).toContain('address');
    expect(rows[0].textContent).toContain('10.0.0.1');
    expect(rows[0].textContent).toContain('10.0.0.9');
    // Bookkeeping fields are not a difference.
    expect(section.textContent).not.toContain('updated_at');
  });

  it('resolves for the server’s copy or this device’s copy', async () => {
    give(status({ conflicts: 2 }), [conflict(), conflict({ record_id: 'h2' })]);
    await mountPage();
    const buttons = target.querySelectorAll('.conflicts li');
    click(findButton(buttons[0] as HTMLElement, 'Take the server’s copy'));
    await vi.waitFor(() => expect(callsOf('sync_resolve_conflict')).toHaveLength(1));
    expect(callsOf('sync_resolve_conflict')[0].args).toEqual({
      recordId: 'h1',
      keepLocal: false,
    });
    click(findButton(buttons[1] as HTMLElement, 'Keep this device’s copy'));
    await vi.waitFor(() => expect(callsOf('sync_resolve_conflict')).toHaveLength(2));
    expect(callsOf('sync_resolve_conflict')[1].args).toEqual({ recordId: 'h2', keepLocal: true });
  });

  it('says so when one side was deleted', async () => {
    give(status({ conflicts: 1 }), [conflict({ remote: null })]);
    await mountPage();
    expect(target.querySelector('.conflicts')?.textContent).toContain('Deleted on the server');
  });

  it('shows no conflicts section when there are none', async () => {
    give(status());
    await mountPage();
    expect(target.querySelector('.conflicts')).toBeNull();
  });

  it('shows a record name as plain text', async () => {
    give(status({ conflicts: 1 }), [
      conflict({
        local: { name: '<img src=x onerror=alert(1)>', a: 1 },
        remote: { name: 'x', a: 2 },
      }),
    ]);
    await mountPage();
    expect(target.querySelector('img')).toBeNull();
    expect(target.querySelector('.record-name')?.textContent).toContain('<img src=x');
  });
});

describe('signed in: devices', () => {
  it('shows this device, and says the others cannot be listed yet', async () => {
    give(status());
    await mountPage();
    const devices = target.querySelector('.devices') as HTMLElement;
    expect(devices.textContent).toContain('workstation');
    expect(devices.textContent).toContain('This device');
    expect(text()).toContain('cannot list your other devices yet');
  });
});

describe('what the page reads', () => {
  it('reads the status again when a sync was applied', async () => {
    give(status());
    await mountPage();
    const before = callsOf('sync_status').length;
    await vi.waitFor(() => expect(bus.handlers.has('sync-applied')).toBe(true));
    bus.handlers.get('sync-applied')?.({ payload: null });
    await vi.waitFor(() => expect(callsOf('sync_status').length).toBeGreaterThan(before));
  });

  it('stops listening when the page is left', async () => {
    give(status());
    await mountPage();
    await vi.waitFor(() => expect(bus.handlers.has('sync-applied')).toBe(true));
    unmount(component as ReturnType<typeof mount>);
    component = null;
    expect(bus.handlers.has('sync-applied')).toBe(false);
  });

  it('says what syncs and what stays on the device', async () => {
    give(status());
    await mountPage();
    expect(text()).toContain('WHAT SYNCS');
    expect(text()).toContain('Passwords and key passphrases');
    expect(text()).toContain('STAYS ON THIS DEVICE');
  });
});
