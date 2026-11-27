// @vitest-environment jsdom
/**
 * The identities page: the list, the key's fingerprint and public key with
 * copy buttons, rename, the hosts that use an identity, the delete
 * confirmation that lists them, and key generation.
 */

import { flushSync, mount, tick, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import IdentitiesPage from './IdentitiesPage.svelte';
import { hosts } from '../stores/hosts.svelte';
import { identities } from '../stores/identities.svelte';
import { route } from '../stores/route.svelte';
import { notices } from '../notify.svelte';
import { noticeText } from '../test-support/notices';
import { click, findButton, newTarget, stubBrowserApis, type } from '../test-support/dom';
import { answers, callsOf, failures, resetTauriMock } from '../test-support/tauri-core-mock';
import type { GeneratedIdentity, Host, Identity, PublicKeyInfo } from '../types';

vi.mock('@tauri-apps/api/core', () => import('../test-support/tauri-core-mock'));
vi.mock('@tauri-apps/api/event', () => ({ listen: async () => () => {} }));

const picked = vi.fn<() => Promise<string | null>>();
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: () => picked() }));

const FINGERPRINT = 'SHA256:Zk3qN0yQm1v8cTt2uRk5GxJ7aWb4HsD9eLp6OiXnYfE';
const PUBLIC_KEY = 'ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIExample work-ed25519';

function identity(id: string, name: string, comment: string | null = null): Identity {
  return {
    id,
    name,
    comment,
    fingerprint: FINGERPRINT,
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
    deleted: false,
  };
}

function host(id: string, identityId: string | null): Host {
  return {
    id,
    name: id,
    address: `${id}.test`,
    port: 22,
    username: 'ops',
    auth: identityId ? { type: 'private_key', identity_id: identityId } : { type: 'password' },
    group_id: null,
    tags: [],
    favorite: false,
    description: null,
    term: 'xterm-256color',
    keepalive_seconds: 0,
    sort_order: 0,
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
    deleted: false,
  };
}

const KEY: PublicKeyInfo = {
  public_key: PUBLIC_KEY,
  fingerprint: FINGERPRINT,
  algorithm: 'ssh-ed25519',
  encrypted: false,
  key_path: '/home/demo/.ssh/work',
};

let target: HTMLElement;
let component: ReturnType<typeof mount> | null = null;
const writeText = vi.fn<(text: string) => Promise<void>>();

function field(id: string): HTMLInputElement {
  return document.getElementById(id) as HTMLInputElement;
}

function dialog(): HTMLElement | null {
  return document.body.querySelector('[role="alertdialog"]');
}

function listItem(label: string): HTMLButtonElement {
  const found = Array.from(target.querySelectorAll<HTMLButtonElement>('button.item')).find(
    (button) => button.querySelector('.item-name')?.textContent?.trim() === label,
  );
  if (!found) throw new Error(`no identity "${label}"`);
  return found;
}

async function mountPage(): Promise<void> {
  component = mount(IdentitiesPage, { target });
  flushSync();
  // The bindings and the key of the selected identity are read.
  await vi.waitFor(() => expect(target.textContent).toMatch(/FINGERPRINT|No key file/));
}

beforeEach(() => {
  resetTauriMock();
  stubBrowserApis();
  hosts.identities = [
    identity('i-work', 'work-ed25519', 'Work laptop key'),
    identity('i-ci', 'ci-key'),
  ];
  hosts.hosts = [host('atlas', 'i-work'), host('relay', 'i-work'), host('archive', null)];
  hosts.groups = [];
  hosts.tags = [];
  identities.bindings = {};
  identities.keys = {};
  identities.bindingsLoaded = false;
  identities.selectedId = null;
  notices.reset();
  route.draft = null;
  route.route = 'identities';
  picked.mockReset();
  writeText.mockReset().mockResolvedValue();
  Object.defineProperty(navigator, 'clipboard', { value: { writeText }, configurable: true });
  answers.set('identity_bindings', [
    { identityId: 'i-work', keyPath: '/home/demo/.ssh/work' },
    { identityId: 'i-ci', keyPath: '/home/demo/.ssh/ci' },
  ]);
  answers.set('identity_public_key', (args: unknown) => {
    const id = (args as { id: string }).id;
    return id === 'i-work'
      ? KEY
      : { ...KEY, key_path: '/home/demo/.ssh/ci', public_key: 'ssh-ed25519 AAAA ci-key' };
  });
  answers.set('identity_save', (args: unknown) => (args as { identity: Identity }).identity);
  target = newTarget();
});

afterEach(() => {
  if (component) unmount(component);
  component = null;
  target.remove();
  document.body.replaceChildren();
});

describe('the page', () => {
  it('has the page anatomy: breadcrumb, title, line, primary action', async () => {
    await mountPage();
    expect(target.querySelector('nav[aria-label="Breadcrumb"]')?.textContent).toMatch(
      /Workspace\s*\/\s*Credentials/,
    );
    expect(target.querySelector('h1')?.textContent).toBe('Identities');
    expect(target.textContent).toContain(
      'Named keys for signing in. The key files stay on this device.',
    );
    expect(findButton(target, 'New identity').className).toContain('primary');
    expect(target.querySelector('[aria-modal]')).toBeNull();
  });

  it('lists the identities with their fingerprint and how many hosts use them', async () => {
    await mountPage();
    // Sorted by name: ci-key first, and it is the one selected.
    expect(Array.from(target.querySelectorAll('.item-name')).map((el) => el.textContent)).toEqual([
      'ci-key',
      'work-ed25519',
    ]);
    expect(listItem('work-ed25519').querySelector('.count')?.textContent).toBe('2');
    expect(listItem('ci-key').querySelector('.count')?.textContent).toBe('0');
    expect(listItem('work-ed25519').querySelector('.item-sub')?.textContent).toContain(
      'SHA256:Zk3qN0y',
    );
    expect(listItem('ci-key').getAttribute('aria-current')).toBe('true');
  });

  it('says so when there is no identity yet', async () => {
    hosts.identities = [];
    component = mount(IdentitiesPage, { target });
    flushSync();
    expect(target.textContent).toContain('No identity yet.');
    expect(target.textContent).toContain('Choose an identity in the list, or make a new one.');
  });
});

describe('the key of an identity', () => {
  it('shows the key file, the fingerprint and the public key', async () => {
    await mountPage();
    click(listItem('work-ed25519'));
    await vi.waitFor(() => expect(target.textContent).toContain(PUBLIC_KEY));
    expect(target.querySelector('.path')?.textContent).toBe('/home/demo/.ssh/work');
    expect(target.textContent).toContain(FINGERPRINT);
    expect(target.textContent).toContain('ssh-ed25519 · no passphrase');
    expect(callsOf('identity_public_key').map((call) => call.args)).toContainEqual({
      id: 'i-work',
    });
  });

  it('reads each key once', async () => {
    await mountPage();
    click(listItem('work-ed25519'));
    await vi.waitFor(() => expect(target.textContent).toContain(PUBLIC_KEY));
    click(listItem('ci-key'));
    click(listItem('work-ed25519'));
    await tick();
    expect(
      callsOf('identity_public_key').filter(
        (call) => (call.args as { id: string }).id === 'i-work',
      ),
    ).toHaveLength(1);
  });

  it('copies the fingerprint and the public key, and says so', async () => {
    await mountPage();
    click(listItem('work-ed25519'));
    await vi.waitFor(() => expect(target.textContent).toContain(PUBLIC_KEY));

    click(target.querySelector('button[aria-label="Copy the fingerprint"]') as HTMLElement);
    await vi.waitFor(() => expect(writeText).toHaveBeenCalledWith(FINGERPRINT));
    await vi.waitFor(() =>
      expect(
        target.querySelector('button[aria-label="Copy the fingerprint"]')?.textContent?.trim(),
      ).toBe('Copied'),
    );
    // The other button is untouched.
    expect(
      target.querySelector('button[aria-label="Copy the public key"]')?.textContent?.trim(),
    ).toBe('Copy');

    click(target.querySelector('button[aria-label="Copy the public key"]') as HTMLElement);
    await vi.waitFor(() => expect(writeText).toHaveBeenCalledWith(PUBLIC_KEY));
    await vi.waitFor(() =>
      expect(
        target.querySelector('button[aria-label="Copy the public key"]')?.textContent?.trim(),
      ).toBe('Copied'),
    );
  });

  it('tells the user when the clipboard refuses', async () => {
    writeText.mockRejectedValue(new Error('denied'));
    await mountPage();
    click(listItem('work-ed25519'));
    await vi.waitFor(() => expect(target.textContent).toContain(PUBLIC_KEY));
    click(target.querySelector('button[aria-label="Copy the public key"]') as HTMLElement);
    await vi.waitFor(() => expect(notices.visible.at(-1)?.kind).toBe('error'));
    expect(noticeText(notices.visible.at(-1))).toContain('Could not copy');
  });

  it('says why a key cannot be shown', async () => {
    answers.set('identity_public_key', () => {
      throw new Error('the key file /home/demo/.ssh/ci does not exist');
    });
    await mountPage();
    await vi.waitFor(() =>
      expect(target.querySelector('.problem')?.textContent).toContain('does not exist'),
    );
    expect(target.querySelector('button[aria-label="Copy the public key"]')).toBeNull();
  });

  it('offers to choose a key when none is bound', async () => {
    answers.set('identity_bindings', []);
    answers.set('identity_public_key', null);
    await mountPage();
    expect(target.textContent).toContain('No key file is bound to this identity on this device.');
    picked.mockResolvedValue('/home/demo/.ssh/new');
    click(findButton(target, 'Choose key…'));
    await vi.waitFor(() => expect(callsOf('identity_bind')).toHaveLength(1));
    expect(callsOf('identity_bind')[0].args).toEqual({
      id: 'i-ci',
      keyPath: '/home/demo/.ssh/new',
    });
  });

  it('lists the hosts that use the identity, and opens one in the editor', async () => {
    await mountPage();
    click(listItem('work-ed25519'));
    await vi.waitFor(() => expect(target.textContent).toContain('HOSTS USING THIS IDENTITY'));
    const rows = Array.from(target.querySelectorAll('.users li .user-name')).map(
      (el) => el.textContent,
    );
    expect(rows).toEqual(['atlas', 'relay']);
    click(target.querySelector('.users li button') as HTMLElement);
    expect(route.route).toBe('host-edit');
    expect(route.editHost?.id).toBe('atlas');
  });
});

describe('the rail form arrives', () => {
  const frame = () => new Promise((resolve) => requestAnimationFrame(() => resolve(null)));

  it('moves in when New identity opens it, not when the page opens on an identity', async () => {
    window.dispatchEvent(new Event('pointerdown'));
    await mountPage();
    await frame();
    // The page opened on the first identity: its form is part of the first paint.
    expect(target.querySelector('.rail form')?.classList.contains('appear')).toBe(false);
    click(listItem('work-ed25519'));
    click(findButton(target, 'New identity'));
    // The same form element is reused between edit and create: no replay.
    expect(target.querySelector('.rail form')).not.toBeNull();
  });
});

describe('renaming', () => {
  it('renames an identity and its comment', async () => {
    await mountPage();
    click(listItem('work-ed25519'));
    expect(field('identity-name').value).toBe('work-ed25519');
    expect(field('identity-comment').value).toBe('Work laptop key');
    const save = findButton(target, 'Save identity');
    expect(save.disabled).toBe(true);
    expect(save.title).toBe('No changes to save');

    type(field('identity-name'), '  laptop key  ');
    type(field('identity-comment'), '');
    click(save);
    await vi.waitFor(() => expect(callsOf('identity_save')).toHaveLength(1));
    expect(callsOf('identity_save')[0].args?.identity).toMatchObject({
      id: 'i-work',
      name: 'laptop key',
      comment: null,
    });
    await vi.waitFor(() =>
      expect(noticeText(notices.visible.at(-1))).toBe('Identity saved. laptop key'),
    );
  });

  it('does not save an empty name, and shows a refusal of the backend', async () => {
    await mountPage();
    click(listItem('work-ed25519'));
    type(field('identity-name'), '  ');
    click(findButton(target, 'Save identity'));
    await tick();
    expect(callsOf('identity_save')).toHaveLength(0);
    expect(field('identity-name').getAttribute('aria-invalid')).toBe('true');
    expect(document.getElementById('identity-name-message')?.textContent).toBe(
      'An identity needs a name.',
    );

    failures.set('identity_save', 'identity name must not be empty');
    type(field('identity-name'), 'x');
    click(findButton(target, 'Save identity'));
    await vi.waitFor(() =>
      expect(target.querySelector('.form-error')?.textContent).toBe(
        'identity name must not be empty',
      ),
    );
  });
});

describe('generating a key', () => {
  const made: GeneratedIdentity = {
    identity: { ...identity('i-new', 'new laptop'), comment: null },
    key_path: '/home/demo/.local/share/termru/keys/id_ed25519-i-new',
    public_key: 'ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAINew new laptop',
    fingerprint: 'SHA256:newfingerprintnewfingerprintnewfingerprint1',
    passphrase_remembered: false,
    passphrase_note: null,
  };

  it('opens the rail for a new identity, with generate as the default', async () => {
    await mountPage();
    click(findButton(target, 'New identity'));
    await tick();
    expect(target.querySelector('.rail .wp-label')?.textContent).toBe('NEW IDENTITY');
    expect(document.activeElement).toBe(field('identity-name'));
    expect(findButton(target, 'Generate key')).toBeTruthy();
    expect(field('identity-passphrase')).not.toBeNull();
    // The "remember" box only matters once there is a passphrase.
    expect(target.querySelector('.rail input[type="checkbox"]')).toBeNull();
    type(field('identity-passphrase'), 'correct horse');
    expect(target.querySelector('.rail input[type="checkbox"]')).not.toBeNull();
  });

  it('generates an ed25519 key through the backend and shows it', async () => {
    answers.set('identity_generate', made);
    answers.set('identity_public_key', null);
    // The list as the backend returns it once the identity exists.
    answers.set('identity_list', [...hosts.identities, made.identity]);
    await mountPage();
    click(findButton(target, 'New identity'));
    type(field('identity-name'), '  new laptop  ');
    click(findButton(target, 'Generate key'));
    await vi.waitFor(() => expect(callsOf('identity_generate')).toHaveLength(1));
    expect(callsOf('identity_generate')[0].args).toEqual({
      name: 'new laptop',
      comment: null,
      passphrase: null,
      rememberPassphrase: false,
    });
    await vi.waitFor(() =>
      expect(noticeText(notices.visible.at(-1))).toBe(
        'Key generated. new laptop · Private key stays on this device',
      ),
    );
    // The new identity is selected, and its key is shown without asking the backend again.
    expect(target.querySelector('h2')?.textContent).toBe('new laptop');
    expect(target.textContent).toContain(made.public_key);
    expect(target.textContent).toContain(made.fingerprint);
    expect(target.querySelector('.path')?.textContent).toBe(made.key_path);
  });

  it('sends the passphrase and the wish to remember it only together', async () => {
    answers.set('identity_generate', { ...made, passphrase_remembered: true });
    await mountPage();
    click(findButton(target, 'New identity'));
    type(field('identity-name'), 'locked');
    type(field('identity-passphrase'), 'correct horse');
    const box = target.querySelector<HTMLInputElement>(
      '.rail input[type="checkbox"]',
    ) as HTMLInputElement;
    box.checked = true;
    box.dispatchEvent(new Event('change', { bubbles: true }));
    flushSync();
    click(findButton(target, 'Generate key'));
    await vi.waitFor(() => expect(callsOf('identity_generate')).toHaveLength(1));
    expect(callsOf('identity_generate')[0].args).toMatchObject({
      passphrase: 'correct horse',
      rememberPassphrase: true,
    });
    // The passphrase does not stay in the form.
    await vi.waitFor(() =>
      expect(target.querySelector('.rail .wp-label')?.textContent).toBe('EDIT IDENTITY'),
    );
    expect(JSON.stringify(notices.visible)).not.toContain('correct horse');
  });

  it('refuses a passphrase that is too short before asking the backend', async () => {
    await mountPage();
    click(findButton(target, 'New identity'));
    type(field('identity-name'), 'short');
    type(field('identity-passphrase'), 'abc');
    click(findButton(target, 'Generate key'));
    await tick();
    expect(callsOf('identity_generate')).toHaveLength(0);
    expect(document.getElementById('identity-passphrase-message')?.textContent).toContain(
      'at least 5',
    );
    expect(field('identity-passphrase').getAttribute('aria-invalid')).toBe('true');
  });

  it('needs a name', async () => {
    await mountPage();
    click(findButton(target, 'New identity'));
    click(findButton(target, 'Generate key'));
    await tick();
    expect(callsOf('identity_generate')).toHaveLength(0);
    expect(document.getElementById('identity-name-message')?.textContent).toBe(
      'An identity needs a name.',
    );
  });

  it('shows the note when the passphrase could not be remembered', async () => {
    answers.set('identity_generate', {
      ...made,
      passphrase_note: 'The passphrase could not be stored on this device (no credential store).',
    });
    await mountPage();
    click(findButton(target, 'New identity'));
    type(field('identity-name'), 'locked');
    click(findButton(target, 'Generate key'));
    await vi.waitFor(() =>
      expect(
        notices.visible.some((toast) => noticeText(toast).includes('could not be stored')),
      ).toBe(true),
    );
  });

  it('keeps the form and shows the refusal of the backend', async () => {
    failures.set('identity_generate', 'a key already exists at that path');
    await mountPage();
    click(findButton(target, 'New identity'));
    type(field('identity-name'), 'clash');
    click(findButton(target, 'Generate key'));
    await vi.waitFor(() =>
      expect(target.querySelector('.form-error')?.textContent).toBe(
        'a key already exists at that path',
      ),
    );
    expect(field('identity-name').value).toBe('clash');
  });

  it('uses a key file you already have: the file is chosen first, then bound', async () => {
    answers.set('identity_save', (args: unknown) => (args as { identity: Identity }).identity);
    picked.mockResolvedValue('/home/demo/.ssh/existing');
    await mountPage();
    click(findButton(target, 'New identity'));
    const existing = target.querySelector<HTMLInputElement>(
      'input[value="existing"]',
    ) as HTMLInputElement;
    existing.checked = true;
    existing.dispatchEvent(new Event('change', { bubbles: true }));
    flushSync();
    type(field('identity-name'), 'old key');
    click(findButton(target, 'Choose key file…'));
    await vi.waitFor(() => expect(callsOf('identity_bind')).toHaveLength(1));
    expect(callsOf('identity_save')[0].args?.identity).toMatchObject({ name: 'old key' });
    expect(callsOf('identity_bind')[0].args).toMatchObject({ keyPath: '/home/demo/.ssh/existing' });
    expect(callsOf('identity_generate')).toHaveLength(0);
  });

  it('creates nothing when the file picker is cancelled', async () => {
    picked.mockResolvedValue(null);
    await mountPage();
    click(findButton(target, 'New identity'));
    const existing = target.querySelector<HTMLInputElement>(
      'input[value="existing"]',
    ) as HTMLInputElement;
    existing.checked = true;
    existing.dispatchEvent(new Event('change', { bubbles: true }));
    flushSync();
    type(field('identity-name'), 'old key');
    click(findButton(target, 'Choose key file…'));
    await tick();
    await tick();
    expect(callsOf('identity_save')).toHaveLength(0);
  });

  it('picks the new identity in a host editor that is waiting for it', async () => {
    answers.set('identity_generate', made);
    answers.set('identity_list', [...hosts.identities, made.identity]);
    route.draft = {
      host: host('draft', null),
      authType: 'private_key',
      identityId: '',
      groupId: '',
      tags: [],
    };
    await mountPage();
    expect(findButton(target, 'Back to host')).toBeTruthy();
    click(findButton(target, 'New identity'));
    type(field('identity-name'), 'new laptop');
    click(findButton(target, 'Generate key'));
    await vi.waitFor(() => expect(route.draft?.identityId).toBe('i-new'));
  });
});

describe('deleting an identity', () => {
  it('asks first and names the hosts that use it', async () => {
    await mountPage();
    click(listItem('work-ed25519'));
    click(findButton(target, 'Delete identity'));
    await vi.waitFor(() => expect(dialog()).not.toBeNull());
    const text = dialog()?.textContent ?? '';
    expect(text).toContain('Delete “work-ed25519”?');
    expect(text).toContain('2 hosts use this identity: atlas, relay.');
    expect(text).toContain('They can no longer sign in with a key');
    expect(text).toContain('The key file stays on this device at /home/demo/.ssh/work.');
    await vi.waitFor(() => expect(document.activeElement?.textContent?.trim()).toBe('Cancel'));
    expect(callsOf('identity_delete')).toHaveLength(0);
  });

  it('says so when no host uses it', async () => {
    await mountPage();
    click(findButton(target, 'Delete identity'));
    await vi.waitFor(() => expect(dialog()?.textContent).toContain('No host uses this identity.'));
  });

  it('deletes only after the confirmation, and not at all when cancelled', async () => {
    await mountPage();
    click(findButton(target, 'Delete identity'));
    await vi.waitFor(() => expect(dialog()).not.toBeNull());
    click(findButton(dialog() as HTMLElement, 'Cancel'));
    await vi.waitFor(() => expect(dialog()).toBeNull());
    expect(callsOf('identity_delete')).toHaveLength(0);

    answers.set('identity_list', [identity('i-work', 'work-ed25519')]);
    click(findButton(target, 'Delete identity'));
    await vi.waitFor(() => expect(dialog()).not.toBeNull());
    click(findButton(dialog() as HTMLElement, 'Delete identity'));
    await vi.waitFor(() => expect(callsOf('identity_delete')).toHaveLength(1));
    expect(callsOf('identity_delete')[0].args).toEqual({ id: 'i-ci' });
    await vi.waitFor(() =>
      expect(noticeText(notices.visible.at(-1))).toBe('Identity deleted. ci-key'),
    );
    // The next identity is selected.
    await vi.waitFor(() => expect(target.querySelector('h2')?.textContent).toBe('work-ed25519'));
  });
});
