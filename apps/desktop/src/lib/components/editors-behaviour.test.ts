// @vitest-environment jsdom
/**
 * The sync and settings panels in page mode and in dialog mode: what they
 * send, what they refuse and how they close. (The host editor, the folders
 * page and the identities page are routed pages with their own tests.)
 */

import { flushSync, mount, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import SettingsPanel from './SettingsPanel.svelte';
import SyncPanel from './SyncPanel.svelte';
import { hosts } from '../stores/hosts.svelte';
import { route } from '../stores/route.svelte';
import { shortcuts } from '../stores/shortcuts.svelte';
import { sync } from '../stores/sync.svelte';
import { toasts } from '../stores/toasts.svelte';
import { click, findButton, newTarget, press, stubBrowserApis, type } from '../test-support/dom';
import { failures, resetTauriMock } from '../test-support/tauri-core-mock';

vi.mock('@tauri-apps/api/core', () => import('../test-support/tauri-core-mock'));
vi.mock('@tauri-apps/api/event', () => ({ listen: async () => () => {} }));

let target: HTMLElement;
let component: ReturnType<typeof mount> | null = null;

beforeEach(() => {
  resetTauriMock();
  stubBrowserApis();
  hosts.hosts = [];
  hosts.groups = [];
  hosts.identities = [];
  hosts.tags = [];
  hosts.saveError = null;
  route.route = 'hosts';
  route.editHost = null;
  route.draft = null;
  shortcuts.cancel();
  toasts.toasts = [];
  target = newTarget();
});

afterEach(() => {
  if (component) unmount(component);
  component = null;
  target.remove();
  document.body.replaceChildren();
});

describe('page mode', () => {
  it('renders in flow, without a backdrop, a dialog role or a close button', () => {
    for (const Editor of [SyncPanel, SettingsPanel]) {
      const holder = newTarget();
      const mounted = mount(Editor, { target: holder, props: { embedded: true } });
      flushSync();
      expect(holder.querySelector('.backdrop')).toBeNull();
      expect(holder.querySelector('[aria-modal]')).toBeNull();
      expect(holder.querySelector('[role="dialog"]')).toBeNull();
      expect(holder.querySelector('[aria-label="Close"]')).toBeNull();
      expect(holder.querySelector('.dialog.embedded')).not.toBeNull();
      unmount(mounted);
      holder.remove();
    }
  });

  it('keeps the modal chrome in dialog mode', () => {
    component = mount(SettingsPanel, { target, props: { open: true } });
    flushSync();
    expect(target.querySelector('.backdrop')).not.toBeNull();
    expect(target.querySelector('[role="dialog"][aria-modal="true"]')).not.toBeNull();
  });
});

describe('SyncPanel form', () => {
  it('keeps what the user typed when the status is reloaded', () => {
    sync.status = null;
    component = mount(SyncPanel, { target, props: { embedded: true } });
    flushSync();
    const url = target.querySelector<HTMLInputElement>('input[type="url"]') as HTMLInputElement;
    const email = target.querySelector<HTMLInputElement>('input[type="email"]') as HTMLInputElement;
    type(url, 'https://typed.example.com');
    type(email, 'me@typed.example.com');

    // A status reload (a sync cycle, a conflict resolved) must not wipe it.
    sync.status = {
      settings: {
        server_url: 'https://stored.example.com',
        email: 'stored@example.com',
        device_name: 'laptop',
        device_id: 'd1',
      },
      signed_in: false,
      pending: 0,
      conflicts: 0,
      cursor: 0,
      last_sync_at: null,
      last_error: null,
    };
    flushSync();
    expect(url.value).toBe('https://typed.example.com');
    expect(email.value).toBe('me@typed.example.com');
  });

  it('fills the form from the stored settings when nothing was typed', () => {
    sync.status = null;
    component = mount(SyncPanel, { target, props: { embedded: true } });
    flushSync();
    sync.status = {
      settings: {
        server_url: 'https://stored.example.com',
        email: 'stored@example.com',
        device_name: 'laptop',
        device_id: 'd1',
      },
      signed_in: false,
      pending: 0,
      conflicts: 0,
      cursor: 0,
      last_sync_at: null,
      last_error: null,
    };
    flushSync();
    expect(target.querySelector<HTMLInputElement>('input[type="url"]')?.value).toBe(
      'https://stored.example.com',
    );
  });
});

describe('SettingsPanel', () => {
  /** Starts recording a key for the first command. */
  function startRecording(): void {
    click(findButton(target, 'Change'));
    expect(shortcuts.recording).not.toBeNull();
  }

  it('ends a recording when closed with the close button', () => {
    component = mount(SettingsPanel, { target, props: { open: true } });
    flushSync();
    startRecording();
    click(target.querySelector('[aria-label="Close"]') as Element);
    expect(shortcuts.recording).toBeNull();
  });

  it('ends a recording when closed by a click on the backdrop', () => {
    component = mount(SettingsPanel, { target, props: { open: true } });
    flushSync();
    startRecording();
    const backdrop = target.querySelector('.backdrop') as HTMLElement;
    backdrop.dispatchEvent(new MouseEvent('click', { bubbles: true }));
    flushSync();
    expect(shortcuts.recording).toBeNull();
  });

  it('ends a recording when cancelled', () => {
    component = mount(SettingsPanel, { target, props: { open: true } });
    flushSync();
    startRecording();
    // The first "Cancel" of the list stops the recording; the one at the
    // bottom closes the panel.
    const buttons = Array.from(target.querySelectorAll('button')).filter(
      (button) => button.textContent?.trim() === 'Cancel',
    );
    click(buttons[buttons.length - 1]);
    expect(shortcuts.recording).toBeNull();
  });

  it('ends a recording when the page is left', () => {
    component = mount(SettingsPanel, { target, props: { embedded: true } });
    flushSync();
    startRecording();
    unmount(component);
    component = null;
    expect(shortcuts.recording).toBeNull();
  });

  it('lets Escape end a recording first, then close the panel', () => {
    component = mount(SettingsPanel, { target, props: { open: true } });
    flushSync();
    startRecording();
    const dialog = target.querySelector('[role="dialog"]') as HTMLElement;
    press(dialog, 'Escape');
    expect(shortcuts.recording).toBeNull();
    expect(target.querySelector('[role="dialog"]')).not.toBeNull();
    press(dialog, 'Escape');
    expect(target.querySelector('[role="dialog"]')).toBeNull();
  });

  it('tells the user when the settings could not be saved, and stays open', async () => {
    const onclose = vi.fn();
    failures.set('prefs_set', 'disk full');
    component = mount(SettingsPanel, { target, props: { embedded: true, onclose } });
    flushSync();
    click(findButton(target, 'Save'));
    await vi.waitFor(() => expect(toasts.toasts.length).toBeGreaterThan(0));
    expect(toasts.toasts.map((toast) => toast.kind)).toEqual(['error']);
    expect(toasts.toasts[0].message).toContain('disk full');
    expect(onclose).not.toHaveBeenCalled();
  });

  it('confirms a save and closes', async () => {
    const onclose = vi.fn();
    component = mount(SettingsPanel, { target, props: { embedded: true, onclose } });
    flushSync();
    click(findButton(target, 'Save'));
    await vi.waitFor(() => expect(onclose).toHaveBeenCalledTimes(1));
    expect(toasts.toasts.map((toast) => toast.kind)).toEqual(['success']);
  });

  it('shows the shortcuts the way the platform writes them', () => {
    component = mount(SettingsPanel, { target, props: { embedded: true } });
    flushSync();
    // Linux/Windows defaults: Ctrl+Shift+letter for the shell-colliding ones.
    expect(target.textContent).toContain('Ctrl+Shift+P');
    expect(target.textContent).not.toContain('Mod+');
  });
});
