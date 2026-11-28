// @vitest-environment jsdom
/**
 * The host editor page (frame 4): numbered sections, inline validation with
 * aria wiring, the save flows, the local credential rail and the
 * unsaved-changes guard on a page change and on the window closing.
 */

import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { flushSync, mount, tick, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import HostEditPage from './HostEditPage.svelte';
import { connectFlow } from '../stores/connect.svelte';
import { hosts } from '../stores/hosts.svelte';
import { identities } from '../stores/identities.svelte';
import { route } from '../stores/route.svelte';
import { notices } from '../notify.svelte';
import { noticeText } from '../test-support/notices';
import { unsaved } from '../stores/unsaved.svelte';
import { click, findButton, newTarget, stubBrowserApis, type } from '../test-support/dom';
import { answers, callsOf, failures, resetTauriMock } from '../test-support/tauri-core-mock';
import type { Group, Host, Identity } from '../types';

vi.mock('@tauri-apps/api/core', () => import('../test-support/tauri-core-mock'));
vi.mock('@tauri-apps/api/event', () => ({ listen: async () => () => {} }));

/** The window of the app: tests drive its close request by hand. */
const closeHandlers: ((event: { preventDefault(): void }) => void)[] = [];
const closeWindow = vi.fn();
vi.mock('@tauri-apps/api/window', () => ({
  getCurrentWindow: () => ({
    minimize: async () => {},
    toggleMaximize: async () => {},
    close: closeWindow,
    onCloseRequested: async (handler: (event: { preventDefault(): void }) => void) => {
      closeHandlers.push(handler);
      return () => {
        closeHandlers.splice(closeHandlers.indexOf(handler), 1);
      };
    },
  }),
}));

function host(overrides: Partial<Host> = {}): Host {
  return {
    id: 'h1',
    name: 'staging-api',
    address: '198.51.100.12',
    port: 22,
    username: 'dev',
    auth: { type: 'password' },
    group_id: 'g-sand',
    tags: [],
    favorite: false,
    description: null,
    term: 'xterm-256color',
    keepalive_seconds: 0,
    sort_order: 0,
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
    deleted: false,
    ...overrides,
  };
}

function group(id: string, name: string): Group {
  return {
    id,
    name,
    parent_id: null,
    sort_order: 0,
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
    deleted: false,
  };
}

function identity(id: string, name: string): Identity {
  return {
    id,
    name,
    comment: null,
    fingerprint: 'SHA256:abc',
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
    deleted: false,
  };
}

let target: HTMLElement;
let component: ReturnType<typeof mount> | null = null;

function input(id: string): HTMLInputElement {
  const element = document.getElementById(id);
  if (!element) throw new Error(`no field #${id}`);
  return element as HTMLInputElement;
}

function blur(element: HTMLElement): void {
  element.dispatchEvent(new Event('blur'));
  flushSync();
}

function message(id: string): string {
  return document.getElementById(`${id}-message`)?.textContent?.trim() ?? '';
}

function open(edited: Host | null): void {
  route.edit(edited);
  component = mount(HostEditPage, { target });
  flushSync();
}

/** The question the unsaved-changes guard has on screen. */
function dialog(): HTMLElement | null {
  return document.body.querySelector('[role="alertdialog"]');
}

beforeEach(() => {
  resetTauriMock();
  stubBrowserApis();
  closeHandlers.length = 0;
  closeWindow.mockReset();
  hosts.hosts = [];
  hosts.groups = [group('g-sand', 'Sandbox'), group('g-prod', 'Production')];
  hosts.identities = [identity('i-work', 'work-ed25519')];
  hosts.tags = [];
  hosts.saveError = null;
  identities.bindings = {};
  identities.keys = {};
  unsaved.question = null;
  route.draft = null;
  route.editHost = null;
  window.location.hash = '#hosts';
  route.route = 'hosts';
  notices.reset();
  target = newTarget();
  answers.set('host_save', (args: unknown) => (args as { host: Host }).host);
  answers.set('identity_bindings', []);
});

afterEach(() => {
  if (component) unmount(component);
  component = null;
  target.remove();
  document.body.replaceChildren();
  vi.restoreAllMocks();
});

describe('the page', () => {
  it('shows the anatomy: one header line, a section index, titled groups, footer', () => {
    open(host());
    // One line: a way back, the title, the host being edited. No breadcrumb, no description.
    expect(target.querySelector('nav[aria-label="Breadcrumb"], .description')).toBeNull();
    expect(target.querySelector('header.head h1')?.textContent).toBe('Edit connection');
    expect(target.querySelector('header.head .count')?.textContent).toBe('staging-api');
    expect(target.querySelector('header.head [aria-label="Back to hosts"]')).not.toBeNull();
    expect(target.textContent).not.toContain('Credentials stay on this device.');
    // The groups, in order, with sentence-case titles (no numbered mono eyebrow).
    const titles = Array.from(target.querySelectorAll('section[aria-labelledby] > header h2')).map(
      (el) => el.textContent,
    );
    expect(titles).toEqual([
      'Connection',
      'Sign-in',
      'Jump hosts',
      'Forwards',
      'Advanced',
      'Organization',
    ]);
    expect(target.querySelector('.step, .wp-label')).toBeNull();
    // The index on the left lists the same groups and marks the one being read.
    const index = target.querySelector('nav[aria-label="Sections of this host"]');
    expect(
      Array.from(index?.querySelectorAll('button') ?? []).map((b) => b.textContent?.trim()),
    ).toEqual(titles);
    expect(index?.querySelectorAll('[aria-current="location"]')).toHaveLength(1);
    for (const label of ['Cancel', 'Save host', 'Save & connect']) {
      expect(findButton(target, label)).toBeTruthy();
    }
    // No modal covers the nav: the editor is a page, not a dialog.
    expect(target.querySelector('[aria-modal]')).toBeNull();
  });

  it('keeps the header to one line for a long name: the name is cut, its title has it all', () => {
    const longName =
      'customer-facing-reporting-service-eu-central-1-blue-green-canary-deployment-primary';
    open(host({ name: longName }));
    const count = target.querySelector<HTMLElement>('header.head .count');
    expect(count?.textContent).toBe(longName);
    expect(count?.title).toBe(longName);
    // The stylesheet ends it in an ellipsis instead of wrapping.
    const source = readFileSync(
      resolve(import.meta.dirname, '../components/PageHeader.svelte'),
      'utf8',
    );
    expect(source).toMatch(/\.count \{[^}]*text-overflow: ellipsis;[^}]*white-space: nowrap/);
  });

  it('is a new connection when no host is edited, with the name focused', () => {
    open(null);
    expect(target.querySelector('header.head h1')?.textContent).toBe('New connection');
    expect(target.querySelector('header.head .count')).toBeNull();
    expect(input('host-name').value).toBe('');
    expect(document.activeElement).toBe(input('host-name'));
  });

  it('the way back in the header returns to Hosts', () => {
    route.go('hosts');
    open(host());
    click(target.querySelector('header.head [aria-label="Back to hosts"]') as Element);
    expect(route.route).toBe('hosts');
  });

  it('offers password, identity and agent, and says keyboard-interactive is automatic', () => {
    open(host());
    const radios = Array.from(target.querySelectorAll<HTMLInputElement>('input[name="auth"]'));
    expect(radios.map((radio) => radio.value)).toEqual(['password', 'private_key', 'agent']);
    expect(target.textContent).toContain('keyboard-interactive');
    expect(target.textContent).toContain('handled automatically');

    radios[2].checked = true;
    radios[2].dispatchEvent(new Event('change', { bubbles: true }));
    flushSync();
    expect(document.getElementById('host-password')).toBeNull();
    expect(target.textContent).toContain('asks your running SSH agent');
  });

  it('shows the connection preview as an ssh command', () => {
    open(host());
    expect(target.querySelector('.command')?.textContent).toBe('ssh dev@198.51.100.12');
    type(input('host-port'), '2222');
    expect(target.querySelector('.command')?.textContent).toBe('ssh dev@198.51.100.12 -p 2222');
    expect(target.textContent).toContain('Password authentication');
    expect(target.textContent).toContain('Sandbox / staging-api');
  });

  it('breaks the preview between words, never inside one', () => {
    open(host());
    const words = Array.from(target.querySelectorAll('.command .tok')).map((el) => el.textContent);
    expect(words).toEqual(['ssh', 'dev@198.51.100.12']);
    expect(target.querySelector('.command')?.textContent).toBe('ssh dev@198.51.100.12');
  });

  it('takes you to a group from the index: its heading gets the focus', () => {
    open(host());
    const index = target.querySelector('nav[aria-label="Sections of this host"]') as HTMLElement;
    click(findButton(index, 'Jump hosts'));
    expect(document.activeElement).toBe(document.getElementById('sec-jump'));
    expect(
      findButton(index, 'Jump hosts').getAttribute('aria-current'),
      'the group you chose is the current one',
    ).toBe('location');
  });

  it('keeps what the device stores for the host in the Sign-in group, next to the field', async () => {
    answers.set('secret_has', true);
    open(host());
    await vi.waitFor(() =>
      expect(target.querySelector('[aria-labelledby="sec-auth"] .stored')?.textContent).toContain(
        'A password is stored on this device',
      ),
    );
    // There is no rail of credentials any more: the summary holds the preview only.
    expect(
      target.querySelector('aside[aria-label="Summary of this host"]')?.textContent,
    ).not.toContain('Forget password');
  });

  it('shows 0 keepalive as Automatic (30 s)', () => {
    open(host({ keepalive_seconds: 0 }));
    expect(input('host-keepalive').value).toBe('');
    expect(input('host-keepalive').placeholder).toBe('Automatic (30 s)');
    expect(message('host-keepalive')).toContain('Automatic (30 s)');
    type(input('host-keepalive'), '45');
    expect(message('host-keepalive')).toContain('every 45 s');
  });
});

describe('inline validation', () => {
  it('shows an error under a field when it is left, linked with aria-describedby', () => {
    open(null);
    const name = input('host-name');
    expect(name.getAttribute('aria-invalid')).toBeNull();
    blur(name);
    expect(name.getAttribute('aria-invalid')).toBe('true');
    expect(name.getAttribute('aria-describedby')).toBe('host-name-message');
    expect(document.getElementById('host-name-message')?.textContent).toBe('Name is required.');

    type(name, 'web-1');
    expect(name.getAttribute('aria-invalid')).toBeNull();
    expect(name.getAttribute('aria-describedby')).toBeNull();
  });

  it('says what is wrong with an address and confirms a good one', () => {
    open(null);
    const address = input('host-address');
    type(address, 'bad host');
    blur(address);
    expect(message('host-address')).toContain("letters, digits, '.'");
    type(address, '198.51.100.12');
    expect(message('host-address')).toBe('✓ Valid address · documentation range');
  });

  it('checks the port and keepalive as the backend does', () => {
    open(host());
    type(input('host-port'), '70000');
    blur(input('host-port'));
    expect(message('host-port')).toBe('Port must be a whole number from 1 to 65535.');
    type(input('host-keepalive'), '9999');
    blur(input('host-keepalive'));
    expect(message('host-keepalive')).toContain('0 (automatic)');
    type(input('host-port'), '');
    blur(input('host-port'));
    // An empty port is put back to its default.
    expect(input('host-port').value).toBe('22');
  });

  it('does not save an invalid form: it names the fields, focuses the first, writes nothing', async () => {
    open(null);
    click(findButton(target, 'Save host'));
    await tick();
    expect(callsOf('host_save')).toHaveLength(0);
    expect(target.querySelector('.footer .message')?.textContent).toBe(
      'Fix 3 fields before saving.',
    );
    expect(document.activeElement).toBe(input('host-name'));
    expect(input('host-address').getAttribute('aria-invalid')).toBe('true');
    expect(input('host-username').getAttribute('aria-invalid')).toBe('true');
    expect(route.route).toBe('host-edit');
  });

  it('asks for an identity when the method is a key', () => {
    open(host());
    const radios = Array.from(target.querySelectorAll<HTMLInputElement>('input[name="auth"]'));
    radios[1].checked = true;
    radios[1].dispatchEvent(new Event('change', { bubbles: true }));
    flushSync();
    click(findButton(target, 'Save host'));
    expect(message('host-identity')).toBe('Choose an identity for key authentication.');
  });

  it('puts a refusal of the backend next to its field and stays on the page', async () => {
    open(null);
    type(input('host-name'), 'web-1');
    type(input('host-address'), 'web-1.test');
    type(input('host-username'), 'deploy');
    failures.set('host_save', 'address contains invalid characters (allowed: letters, digits)');
    click(findButton(target, 'Save host'));
    await vi.waitFor(() => expect(message('host-address')).toContain('invalid characters'));
    expect(input('host-address').getAttribute('aria-invalid')).toBe('true');
    expect(route.route).toBe('host-edit');
    expect(hosts.error).toBeNull();

    // A refusal that is not about a field goes to the footer.
    failures.set('host_save', 'database is locked');
    type(input('host-address'), 'web-1.test');
    click(findButton(target, 'Save host'));
    await vi.waitFor(() =>
      expect(target.querySelector('.footer .message')?.textContent).toBe('database is locked'),
    );
  });
});

describe('saving', () => {
  function fill(): void {
    type(input('host-name'), '  web-1  ');
    type(input('host-address'), ' web-1.test ');
    type(input('host-username'), 'deploy');
  }

  it('saves a trimmed host, announces it and returns to the page it came from', async () => {
    route.go('hosts');
    open(null);
    fill();
    click(findButton(target, 'Save host'));
    await vi.waitFor(() => expect(route.route).toBe('hosts'));
    const saved = callsOf('host_save')[0].args?.host as Host;
    expect(saved).toMatchObject({
      name: 'web-1',
      address: 'web-1.test',
      username: 'deploy',
      port: 22,
      keepalive_seconds: 0,
      auth: { type: 'password' },
      group_id: null,
    });
    expect(noticeText(notices.visible.at(-1))).toBe('Host saved. web-1 · Saved on this device');
  });

  it('keeps the typed password in the credential store, not in the host', async () => {
    open(null);
    fill();
    type(input('host-password'), 's3cret-pass');
    click(findButton(target, 'Save host'));
    await vi.waitFor(() => expect(callsOf('secret_set')).toHaveLength(1));
    expect(callsOf('secret_set')[0].args).toMatchObject({ kind: 'password', value: 's3cret-pass' });
    expect(JSON.stringify(callsOf('host_save')[0].args)).not.toContain('s3cret-pass');
  });

  it('does not store the password when the box is unticked', async () => {
    open(null);
    fill();
    type(input('host-password'), 's3cret-pass');
    const box = target.querySelector<HTMLInputElement>(
      'input[type="checkbox"]',
    ) as HTMLInputElement;
    box.checked = false;
    box.dispatchEvent(new Event('change', { bubbles: true }));
    flushSync();
    click(findButton(target, 'Save host'));
    await vi.waitFor(() => expect(route.route).not.toBe('host-edit'));
    expect(callsOf('secret_set')).toHaveLength(0);
  });

  it('says so when the credential store is missing, and still saves the host', async () => {
    open(null);
    fill();
    type(input('host-password'), 's3cret-pass');
    failures.set('secret_set', 'no system credential store is available');
    click(findButton(target, 'Save host'));
    await vi.waitFor(() => expect(callsOf('host_save')).toHaveLength(1));
    await vi.waitFor(() => expect(notices.visible.length).toBeGreaterThan(0));
    expect(notices.visible.at(-1)?.kind).toBe('error');
    expect(noticeText(notices.visible.at(-1))).toContain('password was not stored');
    expect(noticeText(notices.visible.at(-1))).not.toContain('s3cret-pass');
  });

  it('writes nothing for a host that has no change', () => {
    route.go('hosts');
    open(host());
    click(findButton(target, 'Save host'));
    expect(callsOf('host_save')).toHaveLength(0);
    expect(route.route).toBe('hosts');
  });

  it('Save & connect saves, then starts the connection', async () => {
    const request = vi.spyOn(connectFlow, 'request').mockResolvedValue();
    open(host({ auth: { type: 'agent' } }));
    type(input('host-name'), 'staging-api-2');
    click(findButton(target, 'Save & connect'));
    await vi.waitFor(() => expect(request).toHaveBeenCalledTimes(1));
    expect(callsOf('host_save')).toHaveLength(1);
    expect((request.mock.calls[0][0] as Host).name).toBe('staging-api-2');
  });

  it('puts a new tag on the host', async () => {
    answers.set('tag_save', (args: unknown) => (args as { tag: object }).tag);
    open(host());
    type(target.querySelector('input[aria-label="New tag"]') as HTMLInputElement, 'edge');
    click(findButton(target, 'Add tag'));
    await vi.waitFor(() => expect(hosts.tags.map((tag) => tag.name)).toEqual(['edge']));
    expect(callsOf('tag_save')).toHaveLength(1);
    const chip = target.querySelector('.chip');
    expect(chip?.getAttribute('aria-pressed')).toBe('true');
  });
});

describe('the local credential rail', () => {
  it('says a password is stored, and forgets it on request', async () => {
    answers.set('secret_has', true);
    open(host());
    await vi.waitFor(() =>
      expect(target.textContent).toContain('A password is stored on this device'),
    );
    // Asking the credential store is not an edit.
    expect(target.querySelector('.pill')).toBeNull();

    click(findButton(target, 'Forget password'));
    await vi.waitFor(() => expect(callsOf('secret_delete')).toHaveLength(1));
    expect(callsOf('secret_delete')[0].args).toEqual({ hostId: 'h1', kind: 'password' });
    await vi.waitFor(() =>
      expect(target.textContent).toContain('No password is stored for this host on this device'),
    );
    expect(noticeText(notices.visible.at(-1))).toContain('Password forgotten');
    expect(target.querySelector('.pill')).toBeNull();
  });

  it('reports a credential store that refuses to forget', async () => {
    answers.set('secret_has', true);
    open(host());
    await vi.waitFor(() => expect(target.textContent).toContain('A password is stored'));
    failures.set('secret_delete', 'the credential store is locked');
    click(findButton(target, 'Forget password'));
    await vi.waitFor(() => expect(notices.visible.at(-1)?.kind).toBe('error'));
    expect(noticeText(notices.visible.at(-1))).toContain('the credential store is locked');
    expect(target.textContent).toContain('A password is stored');
  });

  it('says nothing is stored for a new host, and offers nothing to forget', () => {
    open(null);
    expect(target.textContent).toContain('No password is stored for this host on this device');
    expect(() => findButton(target, 'Forget password')).toThrow();
  });

  it('shows the key file and offers to forget a stored passphrase for a key host', async () => {
    hosts.identities = [identity('i-work', 'work-ed25519')];
    answers.set('identity_bindings', [{ identityId: 'i-work', keyPath: '/home/demo/.ssh/work' }]);
    answers.set('secret_has', true);
    open(host({ auth: { type: 'private_key', identity_id: 'i-work' } }));
    await vi.waitFor(() => expect(target.textContent).toContain('/home/demo/.ssh/work'));
    await vi.waitFor(() =>
      expect(target.textContent).toContain('The passphrase is stored on this device'),
    );
    click(findButton(target, 'Forget passphrase'));
    await vi.waitFor(() => expect(callsOf('secret_delete')).toHaveLength(1));
    expect(callsOf('secret_delete')[0].args).toEqual({ hostId: 'h1', kind: 'passphrase' });
  });
});

describe('the unsaved-changes guard', () => {
  it('shows the indicator while something is unsaved', () => {
    open(host());
    expect(target.querySelector('.pill')).toBeNull();
    type(input('host-name'), 'renamed');
    expect(target.querySelector('.pill')?.textContent).toContain('Unsaved changes');
    type(input('host-name'), 'staging-api');
    expect(target.querySelector('.pill')).toBeNull();
  });

  it('leaves a clean page without a question', () => {
    open(host());
    route.go('folders');
    expect(route.route).toBe('folders');
    expect(unsaved.question).toBeNull();
  });

  it('asks before leaving with changes, in a dialog of its own (never window.confirm)', async () => {
    const confirm = vi.spyOn(window, 'confirm');
    open(host());
    type(input('host-name'), 'renamed');
    route.go('folders');
    flushSync();
    expect(route.route).toBe('host-edit');
    expect(unsaved.question?.kind).toBe('leave');
    await vi.waitFor(() => expect(dialog()).not.toBeNull());
    expect(dialog()?.textContent).toContain('Discard unsaved changes?');
    expect(dialog()?.textContent).toContain('Leaving this page discards them');
    // The safe answer holds the focus.
    await vi.waitFor(() =>
      expect(document.activeElement?.textContent?.trim()).toBe('Keep editing'),
    );
    expect(confirm).not.toHaveBeenCalled();
  });

  it('"Keep editing" stays, with the form as it was', async () => {
    open(host());
    type(input('host-name'), 'renamed');
    route.go('folders');
    flushSync();
    click(findButton(document.body, 'Keep editing'));
    expect(unsaved.question).toBeNull();
    expect(route.route).toBe('host-edit');
    expect(input('host-name').value).toBe('renamed');
    await vi.waitFor(() => expect(dialog()).toBeNull());
  });

  it('"Discard changes" goes where the user was going', () => {
    open(host());
    type(input('host-name'), 'renamed');
    route.go('folders');
    flushSync();
    click(findButton(document.body, 'Discard changes'));
    expect(route.route).toBe('folders');
    expect(callsOf('host_save')).toHaveLength(0);
  });

  it('guards the other ways out: Cancel, the shortcuts, another edit, the address bar', () => {
    route.go('hosts');
    open(host());
    type(input('host-name'), 'renamed');

    click(findButton(target, 'Cancel'));
    expect(unsaved.question).not.toBeNull();
    expect(route.route).toBe('host-edit');
    unsaved.keep();

    route.edit(host({ id: 'h2', name: 'other' }));
    expect(unsaved.question).not.toBeNull();
    expect(route.editHost?.id).toBe('h1');
    unsaved.keep();

    window.location.hash = '#sync';
    window.dispatchEvent(new HashChangeEvent('hashchange'));
    expect(unsaved.question).not.toBeNull();
    expect(window.location.hash).toBe('#host-edit');
    expect(route.route).toBe('host-edit');
  });

  it('Cancel on a clean form just goes back', () => {
    route.go('hosts');
    open(host());
    click(findButton(target, 'Cancel'));
    expect(route.route).toBe('hosts');
    expect(unsaved.question).toBeNull();
  });

  it('does not ask after a save: the form is clean', async () => {
    route.go('hosts');
    open(host());
    type(input('host-name'), 'renamed');
    click(findButton(target, 'Save host'));
    await vi.waitFor(() => expect(route.route).toBe('hosts'));
    expect(unsaved.question).toBeNull();
  });

  it('does not ask when the form is parked for the identities page', async () => {
    open(host());
    const radios = Array.from(target.querySelectorAll<HTMLInputElement>('input[name="auth"]'));
    radios[1].checked = true;
    radios[1].dispatchEvent(new Event('change', { bubbles: true }));
    flushSync();
    type(input('host-name'), 'typed so far');
    click(findButton(target, 'Manage identities…'));
    expect(route.route).toBe('identities');
    expect(unsaved.question).toBeNull();
    expect(route.draft?.host.name).toBe('typed so far');

    // Back from the identities page: the form is as it was left, still unsaved.
    unmount(component as ReturnType<typeof mount>);
    route.route = 'host-edit';
    component = mount(HostEditPage, { target });
    flushSync();
    expect(input('host-name').value).toBe('typed so far');
    expect(target.querySelector('.pill')).not.toBeNull();
    expect(route.draft).toBeNull();
  });

  it('holds back the window closing while something is unsaved', async () => {
    open(host());
    await vi.waitFor(() => expect(closeHandlers).toHaveLength(1));
    const event = { preventDefault: vi.fn() };

    // Clean: the window closes.
    closeHandlers[0](event);
    expect(event.preventDefault).not.toHaveBeenCalled();

    type(input('host-name'), 'renamed');
    closeHandlers[0](event);
    expect(event.preventDefault).toHaveBeenCalledTimes(1);
    expect(unsaved.question?.kind).toBe('close');
    await vi.waitFor(() =>
      expect(dialog()?.textContent).toContain('Closing the window discards them'),
    );
    expect(closeWindow).not.toHaveBeenCalled();

    click(findButton(document.body, 'Discard and close'));
    expect(closeWindow).toHaveBeenCalledTimes(1);
    // The window asks again once told to close; this time it may.
    const second = { preventDefault: vi.fn() };
    closeHandlers[0](second);
    expect(second.preventDefault).not.toHaveBeenCalled();
  });

  it('stops listening for the window when the page is gone', async () => {
    open(host());
    await vi.waitFor(() => expect(closeHandlers).toHaveLength(1));
    unmount(component as ReturnType<typeof mount>);
    component = null;
    await vi.waitFor(() => expect(closeHandlers).toHaveLength(0));
    expect(unsaved.dirty).toBe(false);
  });
});

describe('the status bar', () => {
  it('says nothing has been applied while the form has unsaved changes', async () => {
    const { default: StatusBar } = await import('../components/StatusBar.svelte');
    const bar = newTarget();
    const mounted = mount(StatusBar, { target: bar });
    open(host());
    flushSync();
    expect(bar.textContent).toContain('Editing staging-api');
    type(input('host-name'), 'renamed');
    flushSync();
    expect(bar.textContent).toContain('Unsaved changes · No connection settings have been applied');
    unmount(mounted);
    bar.remove();
  });
});
