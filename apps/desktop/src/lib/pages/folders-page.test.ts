// @vitest-environment jsdom
/**
 * The folders page (frame 5): the tree with its counts, the list of what is
 * in the selected folder, Move, the edit rail with the cycle guard (in the
 * page and again from the backend) and the delete confirmation.
 */

import { flushSync, mount, tick, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import FoldersPage from './FoldersPage.svelte';
import { hosts } from '../stores/hosts.svelte';
import { notices } from '../notify.svelte';
import { noticeText } from '../test-support/notices';
import { click, findButton, newTarget, stubBrowserApis, type } from '../test-support/dom';
import { answers, callsOf, failures, resetTauriMock } from '../test-support/tauri-core-mock';
import type { Group, Host } from '../types';

vi.mock('@tauri-apps/api/core', () => import('../test-support/tauri-core-mock'));
vi.mock('@tauri-apps/api/event', () => ({ listen: async () => () => {} }));

function group(id: string, name: string, parent: string | null = null, sort = 0): Group {
  return {
    id,
    name,
    parent_id: parent,
    sort_order: sort,
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
    deleted: false,
  };
}

function host(id: string, groupId: string | null): Host {
  return {
    id,
    name: id,
    address: `${id}.test`,
    port: 22,
    username: 'ops',
    auth: { type: 'agent' },
    group_id: groupId,
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

let target: HTMLElement;
let component: ReturnType<typeof mount> | null = null;

function mountPage(): void {
  component = mount(FoldersPage, { target });
  flushSync();
}

function treeItem(label: string): HTMLButtonElement {
  const found = Array.from(target.querySelectorAll<HTMLButtonElement>('button.item')).find(
    (button) => button.querySelector('.label')?.textContent?.trim() === label,
  );
  if (!found) throw new Error(`no tree item "${label}"`);
  return found;
}

function field(id: string): HTMLInputElement {
  return document.getElementById(id) as HTMLInputElement;
}

function dialog(): HTMLElement | null {
  return document.body.querySelector('[role="alertdialog"]');
}

beforeEach(() => {
  resetTauriMock();
  stubBrowserApis();
  hosts.groups = [
    group('prod', 'Production', null, 0),
    group('eu', 'Europe', 'prod', 1),
    group('edge', 'Edge', 'prod', 2),
    group('sand', 'Sandbox', null, 3),
  ];
  hosts.hosts = [
    host('atlas', 'eu'),
    host('relay', 'edge'),
    host('staging', 'sand'),
    host('build', 'sand'),
    host('archive', null),
  ];
  hosts.identities = [];
  hosts.tags = [];
  hosts.saveError = null;
  hosts.folderSelection = 'all';
  notices.reset();
  target = newTarget();
  answers.set('group_save', (args: unknown) => (args as { group: Group }).group);
  answers.set('host_save', (args: unknown) => (args as { host: Host }).host);
});

afterEach(() => {
  if (component) unmount(component);
  component = null;
  target.remove();
  document.body.replaceChildren();
});

describe('the page', () => {
  it('has the page anatomy and three columns', () => {
    mountPage();
    expect(target.querySelector('nav[aria-label="Breadcrumb"]')?.textContent).toMatch(
      /Workspace\s*\/\s*Organization/,
    );
    expect(target.querySelector('h1')?.textContent).toBe('Folders');
    expect(target.textContent).toContain('Organize connections in a small, predictable hierarchy.');
    expect(findButton(target, 'New folder')).toBeTruthy();
    expect(target.querySelector('nav[aria-label="Folders"]')).not.toBeNull();
    expect(target.querySelector('section.detail')).not.toBeNull();
    expect(target.querySelector('aside.rail')).not.toBeNull();
    // No modal covers the nav.
    expect(target.querySelector('[aria-modal]')).toBeNull();
  });

  it('draws the tree with its counts and the unfiled hosts', () => {
    mountPage();
    const rows = Array.from(target.querySelectorAll('button.item')).map((button) =>
      [
        button.querySelector('.label')?.textContent,
        button.querySelector('.count')?.textContent,
      ].join(':'),
    );
    expect(rows).toEqual([
      'All hosts:5',
      'Production:2',
      'Europe:1',
      'Edge:1',
      'Sandbox:2',
      'Unfiled:1',
    ]);
    expect(treeItem('All hosts').getAttribute('aria-current')).toBe('true');
  });

  it('selecting a folder lists its hosts and fills the rail', () => {
    mountPage();
    click(treeItem('Europe'));
    expect(target.querySelector('h2')?.textContent).toBe('Europe');
    expect(target.querySelector('.trail')?.textContent).toBe('Production');
    expect(target.querySelector('.count-label')?.textContent).toBe('1 HOST');
    const names = Array.from(target.querySelectorAll('.hostlist .host-name')).map(
      (el) => el.textContent,
    );
    expect(names).toEqual(['atlas']);
    expect(field('folder-name').value).toBe('Europe');
    expect(field('folder-parent').value).toBe('prod');
    expect(target.querySelector('.rail .wp-label')?.textContent).toBe('EDIT FOLDER');
  });

  it('a parent folder lists the hosts of its subfolders too, as its count says', () => {
    mountPage();
    click(treeItem('Production'));
    expect(target.querySelector('.count-label')?.textContent).toContain('2 HOSTS');
    expect(target.querySelector('.count-label')?.textContent).toContain('2 SUBFOLDERS');
    const lines = Array.from(target.querySelectorAll('.hostlist .target')).map(
      (el) => el.textContent,
    );
    expect(lines).toEqual([
      'ops@atlas.test:22 · Production / Europe',
      'ops@relay.test:22 · Production / Edge',
    ]);
  });

  it('Unfiled lists the hosts without a folder', () => {
    mountPage();
    click(treeItem('Unfiled'));
    expect(
      Array.from(target.querySelectorAll('.hostlist .host-name')).map((el) => el.textContent),
    ).toEqual(['archive']);
  });

  it('the rail of "All hosts" explains that it is a view, not a folder', () => {
    mountPage();
    expect(target.querySelector('aside.rail form')).toBeNull();
    expect(target.querySelector('aside.rail')?.textContent).toContain(
      'is a view of your hosts, not a folder',
    );
  });

  it('says so when the folder is empty', () => {
    hosts.groups = [...hosts.groups, group('empty', 'Empty')];
    mountPage();
    click(treeItem('Empty'));
    expect(target.textContent).toContain('No hosts in “Empty” yet.');
  });

  it('lists many hosts a page at a time', () => {
    hosts.hosts = Array.from({ length: 120 }, (_, i) =>
      host(`h${String(i).padStart(3, '0')}`, null),
    );
    mountPage();
    expect(target.querySelectorAll('.hostlist .host').length).toBe(50);
    click(treeItem('All hosts'));
    click(findButton(target, 'Show 70 more of 70'));
    expect(target.querySelectorAll('.hostlist .host').length).toBe(120);
  });
});

describe('moving hosts', () => {
  it('moves a host with the Move menu, and says where', async () => {
    mountPage();
    click(treeItem('Sandbox'));
    const select = target.querySelector<HTMLSelectElement>(
      'select[aria-label="Move staging to another folder"]',
    );
    expect(select).not.toBeNull();
    // The folder it is in cannot be chosen again.
    const current = Array.from(select?.options ?? []).find((option) => option.value === 'sand');
    expect(current?.disabled).toBe(true);
    type(select as HTMLSelectElement, 'eu');
    await vi.waitFor(() => expect(callsOf('host_save')).toHaveLength(1));
    expect((callsOf('host_save')[0].args?.host as Host).group_id).toBe('eu');
    await vi.waitFor(() =>
      expect(noticeText(notices.visible.at(-1))).toBe('Host moved. staging · Production / Europe'),
    );
    // The tree follows.
    await vi.waitFor(() =>
      expect(treeItem('Europe').querySelector('.count')?.textContent).toBe('2'),
    );
    expect(treeItem('Sandbox').querySelector('.count')?.textContent).toBe('1');
  });

  it('takes a host out of its folder', async () => {
    mountPage();
    click(treeItem('Sandbox'));
    type(
      target.querySelector<HTMLSelectElement>(
        'select[aria-label="Move build to another folder"]',
      ) as HTMLSelectElement,
      '__none__',
    );
    await vi.waitFor(() => expect(callsOf('host_save')).toHaveLength(1));
    expect((callsOf('host_save')[0].args?.host as Host).group_id).toBeNull();
    await vi.waitFor(() =>
      expect(treeItem('Unfiled').querySelector('.count')?.textContent).toBe('2'),
    );
  });

  it('offers a host from another folder with "Move here" after "Choose hosts"', async () => {
    mountPage();
    click(treeItem('Europe'));
    // The examples first, the full searchable list on request.
    expect(target.querySelector('#available-label')?.textContent).toContain('EXAMPLES');
    click(findButton(target, 'Choose hosts'));
    expect(target.querySelector('#available-label')?.textContent).not.toContain('EXAMPLES');
    type(
      target.querySelector('input[aria-label="Search hosts to move"]') as HTMLInputElement,
      'arch',
    );
    const names = Array.from(target.querySelectorAll('.pick-name')).map((el) => el.textContent);
    expect(names).toEqual(['archive']);
    click(target.querySelector('button[aria-label="Move archive here"]') as HTMLElement);
    await vi.waitFor(() => expect(callsOf('host_save')).toHaveLength(1));
    expect((callsOf('host_save')[0].args?.host as Host).group_id).toBe('eu');
  });

  it('reports a store that refuses the move', async () => {
    mountPage();
    click(treeItem('Sandbox'));
    failures.set('host_save', 'the store is read-only');
    type(
      target.querySelector<HTMLSelectElement>(
        'select[aria-label="Move staging to another folder"]',
      ) as HTMLSelectElement,
      'eu',
    );
    await vi.waitFor(() => expect(notices.visible.at(-1)?.kind).toBe('error'));
    expect(noticeText(notices.visible.at(-1))).toContain('the store is read-only');
    expect(treeItem('Sandbox').querySelector('.count')?.textContent).toBe('2');
  });

  it('has no "Choose hosts" for "All hosts": there is no "here"', () => {
    mountPage();
    expect(() => findButton(target, 'Choose hosts')).toThrow();
    expect(target.textContent).toContain('Choose a folder in the tree to move hosts into it.');
  });
});

describe('the edit rail and the cycle guard', () => {
  it('offers only legal parents: not itself, not its own subfolders', () => {
    mountPage();
    click(treeItem('Production'));
    const options = Array.from(
      (field('folder-parent') as unknown as HTMLSelectElement).options,
    ).map((option) => [option.value, option.disabled]);
    expect(options).toEqual([
      ['', false],
      ['prod', true],
      ['eu', true],
      ['edge', true],
      ['sand', false],
    ]);
    expect(target.querySelector('aside.rail')?.textContent).toContain(
      'cannot be its own parent or be moved into one of its descendants',
    );
  });

  it('refuses a forced illegal parent in the page, without asking the backend', async () => {
    mountPage();
    click(treeItem('Production'));
    // Even if a disabled option was somehow chosen, the page checks again.
    const select = field('folder-parent') as unknown as HTMLSelectElement;
    select.value = 'eu';
    select.dispatchEvent(new Event('change', { bubbles: true }));
    flushSync();
    click(findButton(target, 'Save folder'));
    await tick();
    expect(callsOf('group_save')).toHaveLength(0);
    expect(target.querySelector('aside.rail')?.textContent).toContain(
      'cannot move into one of its own subfolders',
    );
  });

  it('shows the backend refusal when the page offered something the backend refuses', async () => {
    mountPage();
    click(treeItem('Sandbox'));
    failures.set('group_save', 'group hierarchy contains a cycle');
    type(field('folder-name'), 'Sandbox 2');
    click(findButton(target, 'Save folder'));
    await vi.waitFor(() =>
      expect(target.querySelector('.form-error')?.textContent).toBe(
        'group hierarchy contains a cycle',
      ),
    );
  });

  it('renames a folder and moves it under another', async () => {
    mountPage();
    click(treeItem('Sandbox'));
    type(field('folder-name'), 'Staging');
    type(field('folder-parent'), 'prod');
    click(findButton(target, 'Save folder'));
    await vi.waitFor(() => expect(callsOf('group_save')).toHaveLength(1));
    expect(callsOf('group_save')[0].args?.group).toMatchObject({
      id: 'sand',
      name: 'Staging',
      parent_id: 'prod',
    });
    await vi.waitFor(() =>
      expect(noticeText(notices.visible.at(-1))).toBe('Folder saved. Staging'),
    );
    expect(treeItem('Staging').getAttribute('aria-current')).toBe('true');
  });

  it('does not save an empty name', async () => {
    mountPage();
    click(treeItem('Sandbox'));
    type(field('folder-name'), '   ');
    click(findButton(target, 'Save folder'));
    await tick();
    expect(callsOf('group_save')).toHaveLength(0);
    expect(field('folder-name').getAttribute('aria-invalid')).toBe('true');
    expect(document.getElementById('folder-name-message')?.textContent).toBe(
      'A folder needs a name.',
    );
  });

  it('keeps Save folder disabled, with its reason, while nothing changed', () => {
    mountPage();
    click(treeItem('Sandbox'));
    const save = findButton(target, 'Save folder');
    expect(save.disabled).toBe(true);
    expect(save.title).toBe('No changes to save');
    type(field('folder-name'), 'Other');
    expect(save.disabled).toBe(false);
    click(findButton(target, 'Cancel'));
    expect(field('folder-name').value).toBe('Sandbox');
  });

  it('"New folder" opens the rail blank, with the selected folder as the parent', async () => {
    mountPage();
    click(treeItem('Production'));
    click(findButton(target, 'New folder'));
    await tick();
    expect(target.querySelector('.rail .wp-label')?.textContent).toBe('NEW FOLDER');
    expect(field('folder-name').value).toBe('');
    expect(field('folder-parent').value).toBe('prod');
    expect(document.activeElement).toBe(field('folder-name'));

    type(field('folder-name'), 'Asia');
    click(findButton(target, 'Create folder'));
    await vi.waitFor(() => expect(callsOf('group_save')).toHaveLength(1));
    expect(callsOf('group_save')[0].args?.group).toMatchObject({ name: 'Asia', parent_id: 'prod' });
    await vi.waitFor(() => expect(noticeText(notices.visible.at(-1))).toBe('Folder created. Asia'));
    // The new folder is selected and its rail edits it.
    expect(treeItem('Asia').getAttribute('aria-current')).toBe('true');
    expect(target.querySelector('.rail .wp-label')?.textContent).toBe('EDIT FOLDER');
  });

  it('a refreshed list does not wipe what is being typed', () => {
    mountPage();
    click(treeItem('Sandbox'));
    type(field('folder-name'), 'Typing…');
    hosts.groups = hosts.groups.map((entry) => ({ ...entry }));
    flushSync();
    expect(field('folder-name').value).toBe('Typing…');
  });
});

describe('deleting a folder', () => {
  it('asks first, and says what happens to the hosts and the subfolders', async () => {
    mountPage();
    click(treeItem('Production'));
    click(findButton(target, 'Delete folder'));
    await vi.waitFor(() => expect(dialog()).not.toBeNull());
    const text = dialog()?.textContent ?? '';
    expect(text).toContain('Delete “Production”?');
    expect(text).toContain('2 subfolders in “Production” move to the top level');
    expect(text).toContain('No host is deleted and running sessions are not affected.');
    // The safe answer holds the focus.
    await vi.waitFor(() => expect(document.activeElement?.textContent?.trim()).toBe('Cancel'));
    expect(callsOf('group_delete')).toHaveLength(0);
  });

  it('names the folder the hosts move up to', async () => {
    mountPage();
    click(treeItem('Europe'));
    click(findButton(target, 'Delete folder'));
    await vi.waitFor(() =>
      expect(dialog()?.textContent).toContain('1 host in “Europe” moves up to “Production”'),
    );
  });

  it('deletes only after the confirmation, and not at all when cancelled', async () => {
    mountPage();
    click(treeItem('Europe'));
    click(findButton(target, 'Delete folder'));
    await vi.waitFor(() => expect(dialog()).not.toBeNull());
    click(findButton(dialog() as HTMLElement, 'Cancel'));
    await vi.waitFor(() => expect(dialog()).toBeNull());
    expect(callsOf('group_delete')).toHaveLength(0);

    answers.set('group_list', [
      group('prod', 'Production'),
      group('edge', 'Edge', 'prod'),
      group('sand', 'Sandbox'),
    ]);
    answers.set('host_list', [host('atlas', 'prod'), host('relay', 'edge'), host('archive', null)]);
    click(findButton(target, 'Delete folder'));
    await vi.waitFor(() => expect(dialog()).not.toBeNull());
    click(findButton(dialog() as HTMLElement, 'Delete folder'));
    await vi.waitFor(() => expect(callsOf('group_delete')).toHaveLength(1));
    expect(callsOf('group_delete')[0].args).toEqual({ id: 'eu' });
    await vi.waitFor(() =>
      expect(noticeText(notices.visible.at(-1))).toBe('Folder deleted. Europe'),
    );
    // The selection moved up to the parent, and the lists were read again.
    expect(target.querySelector('h2')?.textContent).toBe('Production');
    expect(() => treeItem('Europe')).toThrow();
  });

  it('reports a store that refuses the delete', async () => {
    mountPage();
    click(treeItem('Sandbox'));
    failures.set('group_delete', 'the store is read-only');
    click(findButton(target, 'Delete folder'));
    await vi.waitFor(() => expect(dialog()).not.toBeNull());
    click(findButton(dialog() as HTMLElement, 'Delete folder'));
    await vi.waitFor(() => expect(notices.visible.at(-1)?.kind).toBe('error'));
    expect(noticeText(notices.visible.at(-1))).toContain('the store is read-only');
  });
});
