// @vitest-environment jsdom
/**
 * The editors (host, folder, identity, sync, settings) in page mode and in
 * dialog mode: what they send, what they refuse and how they close.
 */

import { flushSync, mount, tick, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import FolderEditor from './FolderEditor.svelte';
import HostEditor from './HostEditor.svelte';
import IdentityEditor from './IdentityEditor.svelte';
import SettingsPanel from './SettingsPanel.svelte';
import SyncPanel from './SyncPanel.svelte';
import { hosts } from '../stores/hosts.svelte';
import { route, type HostEditDraft } from '../stores/route.svelte';
import { shortcuts } from '../stores/shortcuts.svelte';
import { sync } from '../stores/sync.svelte';
import { toasts } from '../stores/toasts.svelte';
import { click, findButton, newTarget, press, stubBrowserApis, type } from '../test-support/dom';
import { answers, callsOf, failures, resetTauriMock } from '../test-support/tauri-core-mock';
import type { Group, Host, Identity } from '../types';

vi.mock('@tauri-apps/api/core', () => import('../test-support/tauri-core-mock'));
vi.mock('@tauri-apps/api/event', () => ({ listen: async () => () => {} }));

function host(name: string, overrides: Partial<Host> = {}): Host {
  const now = new Date().toISOString();
  return {
    id: `host-${name}`,
    name,
    address: `${name}.test`,
    port: 22,
    username: 'deploy',
    auth: { type: 'password' },
    group_id: null,
    tags: [],
    favorite: false,
    description: null,
    term: 'xterm-256color',
    keepalive_seconds: 0,
    sort_order: 0,
    created_at: now,
    updated_at: now,
    deleted: false,
    ...overrides,
  };
}

function group(id: string, name: string): Group {
  const now = new Date().toISOString();
  return {
    id,
    name,
    parent_id: null,
    sort_order: 0,
    created_at: now,
    updated_at: now,
    deleted: false,
  };
}

function identity(id: string, name: string): Identity {
  const now = new Date().toISOString();
  return {
    id,
    name,
    comment: null,
    fingerprint: null,
    created_at: now,
    updated_at: now,
    deleted: false,
  };
}

/** The input or select of the label that starts with `text`. */
function field(root: ParentNode, text: string): HTMLInputElement {
  const label = Array.from(root.querySelectorAll('label')).find((entry) =>
    entry.textContent?.trim().startsWith(text),
  );
  const control = label?.querySelector<HTMLInputElement>('input, select');
  if (!control) throw new Error(`no field "${text}"`);
  return control;
}

function submit(form: Element | null): void {
  form?.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }));
  flushSync();
}

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

describe('HostEditor numbers', () => {
  function open(props: Partial<{ onclose: () => void }> = {}) {
    answers.set('host_save', (args: unknown) => (args as { host: Host }).host);
    component = mount(HostEditor, {
      target,
      props: { embedded: true, onmanageidentities: vi.fn(), ...props },
    });
    flushSync();
    type(field(target, 'Name'), 'web-1');
    type(field(target, 'Address'), 'web-1.test');
    type(field(target, 'Username'), 'deploy');
  }

  function saved(): Host {
    return callsOf('host_save').pop()?.args?.host as Host;
  }

  it('saves port 22 and keepalive 0 when the fields were cleared', () => {
    open();
    type(field(target, 'Port'), '');
    type(field(target, 'Keepalive'), '');
    submit(target.querySelector('form'));
    expect(callsOf('host_save')).toHaveLength(1);
    // Never `null`: the backend refuses a null port.
    expect(saved().port).toBe(22);
    expect(saved().keepalive_seconds).toBe(0);
  });

  it('saves the numbers that were typed', () => {
    open();
    type(field(target, 'Port'), '2222');
    type(field(target, 'Keepalive'), '30');
    submit(target.querySelector('form'));
    expect(saved().port).toBe(2222);
    expect(saved().keepalive_seconds).toBe(30);
  });

  it('shows an inline error for a port out of range and does not save', () => {
    open();
    type(field(target, 'Port'), '70000');
    expect(target.querySelector('#port-error')?.textContent).toContain('1 to 65535');
    expect(field(target, 'Port').getAttribute('aria-invalid')).toBe('true');
    submit(target.querySelector('form'));
    expect(callsOf('host_save')).toHaveLength(0);

    type(field(target, 'Port'), 'ssh');
    expect(target.querySelector('#port-error')).not.toBeNull();
    type(field(target, 'Port'), '0');
    expect(target.querySelector('#port-error')).not.toBeNull();

    type(field(target, 'Port'), '22');
    expect(target.querySelector('#port-error')).toBeNull();
  });

  it('refuses a negative or huge keepalive', () => {
    open();
    type(field(target, 'Keepalive'), '-5');
    expect(target.querySelector('#keepalive-error')).not.toBeNull();
    submit(target.querySelector('form'));
    expect(callsOf('host_save')).toHaveLength(0);

    type(field(target, 'Keepalive'), '99999');
    expect(target.querySelector('#keepalive-error')).not.toBeNull();
    type(field(target, 'Keepalive'), '60');
    expect(target.querySelector('#keepalive-error')).toBeNull();
  });

  it('puts a cleared field back to its default when it loses the focus', () => {
    open();
    const port = field(target, 'Port');
    type(port, '');
    port.dispatchEvent(new FocusEvent('blur'));
    flushSync();
    expect(port.value).toBe('22');
  });

  it('closes through onclose after a save', async () => {
    const onclose = vi.fn();
    open({ onclose });
    submit(target.querySelector('form'));
    await vi.waitFor(() => expect(onclose).toHaveBeenCalledTimes(1));
  });

  it('closes through onclose when cancelled, without saving', () => {
    const onclose = vi.fn();
    open({ onclose });
    click(findButton(target, 'Cancel'));
    expect(onclose).toHaveBeenCalledTimes(1);
    expect(callsOf('host_save')).toHaveLength(0);
  });

  it('shows a refusal of the store next to the form, not as a page error', async () => {
    open();
    failures.set('host_save', 'name already used');
    submit(target.querySelector('form'));
    await vi.waitFor(() => expect(target.textContent).toContain('name already used'));
    expect(hosts.error).toBeNull();
  });
});

describe('HostEditor identities', () => {
  it('hands the unsaved form over when managing identities, and takes it back', () => {
    hosts.identities = [identity('i1', 'work')];
    const onmanage = vi.fn<(draft: HostEditDraft) => void>();
    component = mount(HostEditor, {
      target,
      props: { embedded: true, onmanageidentities: onmanage },
    });
    flushSync();
    type(field(target, 'Name'), 'typed so far');
    type(field(target, 'Authentication'), 'private_key');
    type(field(target, 'Identity'), 'i1');
    type(field(target, 'Port'), '2200');
    click(findButton(target, 'Manage identities…'));

    expect(onmanage).toHaveBeenCalledTimes(1);
    const draft = onmanage.mock.calls[0][0];
    expect(draft.host.name).toBe('typed so far');
    expect(draft.host.port).toBe(2200);
    expect(draft.authType).toBe('private_key');
    expect(draft.identityId).toBe('i1');

    // Back from the identities page: the editor is rebuilt from the draft.
    unmount(component);
    route.draft = draft;
    component = mount(HostEditor, {
      target,
      props: { embedded: true, onmanageidentities: onmanage },
    });
    flushSync();
    expect(field(target, 'Name').value).toBe('typed so far');
    expect(field(target, 'Port').value).toBe('2200');
    expect(field(target, 'Authentication').value).toBe('private_key');
    expect(field(target, 'Identity').value).toBe('i1');
    // The draft was consumed: a later edit starts clean.
    expect(route.draft).toBeNull();
  });

  it('opens a new host when no host is being edited (a reload on #host-edit)', () => {
    route.editHost = null;
    component = mount(HostEditor, {
      target,
      props: { embedded: true, onmanageidentities: vi.fn() },
    });
    flushSync();
    expect(target.textContent).toContain('New host');
    expect(field(target, 'Name').value).toBe('');
  });

  it('opens the host being edited', () => {
    route.editHost = host('db-1', { port: 5022 });
    component = mount(HostEditor, {
      target,
      props: { embedded: true, onmanageidentities: vi.fn() },
    });
    flushSync();
    expect(target.textContent).toContain('Edit host');
    expect(field(target, 'Name').value).toBe('db-1');
    expect(field(target, 'Port').value).toBe('5022');
  });
});

describe('page mode', () => {
  it('renders in flow, without a backdrop, a dialog role or a close button', () => {
    for (const Editor of [FolderEditor, IdentityEditor, SyncPanel, SettingsPanel]) {
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
    component = mount(FolderEditor, { target, props: { open: true } });
    flushSync();
    expect(target.querySelector('.backdrop')).not.toBeNull();
    expect(target.querySelector('[role="dialog"][aria-modal="true"]')).not.toBeNull();
  });

  it('closes through onclose (the page goes back)', () => {
    const onclose = vi.fn();
    component = mount(FolderEditor, { target, props: { embedded: true, onclose } });
    flushSync();
    click(findButton(target, 'Close'));
    expect(onclose).toHaveBeenCalledTimes(1);
  });
});

describe('FolderEditor rename', () => {
  beforeEach(() => {
    hosts.groups = [group('g1', 'production')];
  });

  it('renames inline, without a native prompt', async () => {
    const prompt = vi.fn();
    vi.stubGlobal('prompt', prompt);
    component = mount(FolderEditor, { target, props: { embedded: true } });
    flushSync();

    click(findButton(target, 'Rename'));
    const input = target.querySelector<HTMLInputElement>('form.rename input');
    expect(input?.value).toBe('production');
    // The field is focused and its text selected, ready to be replaced.
    expect(document.activeElement).toBe(input);

    type(input as HTMLInputElement, 'staging');
    submit(target.querySelector('form.rename'));
    await vi.waitFor(() => expect(callsOf('group_save')).toHaveLength(1));
    expect((callsOf('group_save')[0].args?.group as Group).name).toBe('staging');
    expect(prompt).not.toHaveBeenCalled();
    vi.unstubAllGlobals();
  });

  it('cancels on Escape and keeps the name', () => {
    component = mount(FolderEditor, { target, props: { embedded: true } });
    flushSync();
    click(findButton(target, 'Rename'));
    const input = target.querySelector<HTMLInputElement>('form.rename input') as HTMLInputElement;
    type(input, 'something else');
    press(input, 'Escape');
    expect(target.querySelector('form.rename')).toBeNull();
    expect(callsOf('group_save')).toHaveLength(0);
    expect(target.textContent).toContain('production');
  });

  it('does not save an empty or unchanged name', async () => {
    component = mount(FolderEditor, { target, props: { embedded: true } });
    flushSync();
    click(findButton(target, 'Rename'));
    type(target.querySelector<HTMLInputElement>('form.rename input') as HTMLInputElement, '   ');
    submit(target.querySelector('form.rename'));
    await tick();
    expect(callsOf('group_save')).toHaveLength(0);

    click(findButton(target, 'Rename'));
    submit(target.querySelector('form.rename'));
    await tick();
    expect(callsOf('group_save')).toHaveLength(0);
  });

  it('closes the dialog on Escape only after the rename was left', () => {
    component = mount(FolderEditor, { target, props: { open: true } });
    flushSync();
    click(findButton(target, 'Rename'));
    const dialog = target.querySelector('[role="dialog"]') as HTMLElement;
    press(dialog, 'Escape');
    // The first Escape leaves the rename...
    expect(target.querySelector('form.rename')).toBeNull();
    expect(target.querySelector('[role="dialog"]')).not.toBeNull();
    // ...the second closes the dialog.
    press(dialog, 'Escape');
    expect(target.querySelector('[role="dialog"]')).toBeNull();
  });
});

describe('IdentityEditor delete', () => {
  beforeEach(() => {
    hosts.identities = [identity('i1', 'work laptop')];
    hosts.hosts = [
      host('web', { auth: { type: 'private_key', identity_id: 'i1' } }),
      host('db', { auth: { type: 'private_key', identity_id: 'i1' } }),
      host('cache', { auth: { type: 'password' } }),
    ];
  });

  function dialog(): HTMLElement {
    return target.querySelector('[role="alertdialog"]') as HTMLElement;
  }

  it('asks first, and names the hosts that use the identity', () => {
    component = mount(IdentityEditor, { target, props: { embedded: true } });
    flushSync();
    click(findButton(target, 'Delete'));

    expect(dialog()).not.toBeNull();
    expect(callsOf('identity_delete')).toHaveLength(0);
    expect(dialog().textContent).toContain('work laptop');
    expect(dialog().textContent).toContain('2 hosts use this identity');
    expect(dialog().textContent).toContain('web, db');
    expect(dialog().textContent).not.toContain('cache');
  });

  it('says so when no host uses it', () => {
    hosts.hosts = [host('cache')];
    component = mount(IdentityEditor, { target, props: { embedded: true } });
    flushSync();
    click(findButton(target, 'Delete'));
    expect(dialog().textContent).toContain('No host uses this identity');
  });

  it('deletes only after the confirmation, and not at all when cancelled', async () => {
    component = mount(IdentityEditor, { target, props: { embedded: true } });
    flushSync();
    click(findButton(target, 'Delete'));
    click(findButton(dialog(), 'Cancel'));
    expect(callsOf('identity_delete')).toHaveLength(0);

    click(findButton(target, 'Delete'));
    click(findButton(dialog(), 'Delete'));
    await vi.waitFor(() => expect(callsOf('identity_delete')).toHaveLength(1));
    expect(callsOf('identity_delete')[0].args).toEqual({ id: 'i1' });
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
