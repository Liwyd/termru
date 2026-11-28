// @vitest-environment jsdom
/**
 * Import from `~/.ssh/config`: preview, tick, import, summary. A host the
 * library has already and a host that cannot be valid are listed but cannot
 * be ticked, and say why; a missing file, an empty file and a read error each
 * have their own state.
 */

import { flushSync, mount, tick, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import SshImportDialog from './SshImportDialog.svelte';
import HostsPage from '../pages/HostsPage.svelte';
import { hosts } from '../stores/hosts.svelte';
import { route } from '../stores/route.svelte';
import { sshImport } from '../stores/ssh-import.svelte';
import { notices } from '../notify.svelte';
import { noticeText } from '../test-support/notices';
import { click, findButton, newTarget, stubBrowserApis } from '../test-support/dom';
import { answers, callsOf, failures, resetTauriMock } from '../test-support/tauri-core-mock';
import type { SshConfigCandidate, SshConfigImportReport, SshConfigPreview } from '../types';

vi.mock('@tauri-apps/api/core', () => import('../test-support/tauri-core-mock'));
vi.mock('@tauri-apps/api/event', () => ({ listen: async () => () => {} }));

function candidate(over: Partial<SshConfigCandidate> = {}): SshConfigCandidate {
  return {
    alias: 'web',
    host: 'web.corp.example',
    user: 'deploy',
    port: 22,
    identity_files: [],
    jump_chain: [],
    warnings: [],
    existing_host_id: null,
    problem: null,
    ...over,
  };
}

const BASTION = candidate({
  alias: 'bastion',
  host: 'bastion.corp.example',
  user: 'ops',
  port: 2222,
  identity_files: ['/home/me/.ssh/work_ed25519'],
});
const APP = candidate({
  alias: 'app',
  host: 'app.corp.example',
  jump_chain: ['bastion'],
  identity_files: ['/home/me/.ssh/work_ed25519'],
  warnings: ['uses ProxyCommand, which cannot be imported; the host is imported without it'],
});
const KNOWN = candidate({ alias: 'staging', existing_host_id: 'h-existing' });
const BROKEN = candidate({
  alias: 'legacy',
  host: 'bad host',
  problem: 'address contains invalid characters',
});

function preview(over: Partial<SshConfigPreview> = {}): SshConfigPreview {
  return {
    found: true,
    path: '/home/me/.ssh/config',
    candidates: [BASTION, APP, KNOWN, BROKEN],
    warnings: ['/home/me/.ssh/config:31: a Match block was skipped'],
    ...over,
  };
}

function report(over: Partial<SshConfigImportReport> = {}): SshConfigImportReport {
  return {
    imported: [
      { id: 'n1', name: 'bastion' },
      { id: 'n2', name: 'app' },
    ],
    identities_created: 1,
    skipped: [],
    already_present: [],
    warnings: [],
    ...over,
  };
}

let target: HTMLElement;
let component: ReturnType<typeof mount> | null = null;

async function openDialog(): Promise<void> {
  component = mount(SshImportDialog, { target });
  flushSync();
  void sshImport.show();
  await vi.waitFor(() => expect(sshImport.phase).not.toBe('loading'));
  flushSync();
  await tick();
}

const boxes = () =>
  Array.from(target.querySelectorAll<HTMLInputElement>('li input[type="checkbox"]'));
const rowOf = (alias: string) =>
  Array.from(target.querySelectorAll('li.row')).find((row) =>
    row.textContent?.includes(alias),
  ) as HTMLElement;
const boxOf = (alias: string) => rowOf(alias).querySelector('input') as HTMLInputElement;
const text = () => target.textContent?.replace(/\s+/g, ' ').trim() ?? '';

beforeEach(() => {
  resetTauriMock();
  stubBrowserApis();
  notices.reset();
  sshImport.close();
  sshImport.preview = null;
  sshImport.report = null;
  sshImport.selected = [];
  hosts.hosts = [];
  answers.set('ssh_config_preview', preview());
  answers.set('ssh_config_import', report());
  answers.set('host_list', []);
  target = newTarget();
});

afterEach(() => {
  if (component) unmount(component);
  component = null;
  target.remove();
  document.body.replaceChildren();
});

describe('the import dialog', () => {
  it('reads the file when it opens and says so meanwhile', async () => {
    let release: (value: SshConfigPreview) => void = () => {};
    answers.set(
      'ssh_config_preview',
      () => new Promise<SshConfigPreview>((resolve) => (release = resolve)),
    );
    component = mount(SshImportDialog, { target });
    void sshImport.show();
    flushSync();
    expect(text()).toContain('Reading the file…');
    expect(target.querySelector('[role="status"]')).not.toBeNull();
    release(preview());
    await vi.waitFor(() => expect(sshImport.phase).toBe('ready'));
    flushSync();
    expect(text()).not.toContain('Reading the file…');
  });

  it('lists every host with its address, and what will be referenced', async () => {
    await openDialog();
    expect(target.querySelector('h2')?.textContent).toBe('Import from SSH config');
    expect(text()).toContain('/home/me/.ssh/config');
    expect(boxes()).toHaveLength(4);
    const app = text();
    expect(app).toContain('deploy@app.corp.example:22');
    expect(app).toContain('ops@bastion.corp.example:2222');
    // The route as written, and the key file that will be referenced.
    expect(rowOf('app').textContent).toContain('bastion');
    expect(rowOf('app').textContent).toContain('/home/me/.ssh/work_ed25519');
    expect(rowOf('app').textContent).toContain('uses ProxyCommand, which cannot be imported');
    expect(text()).toContain('a Match block was skipped');
    expect(text()).toContain('Key files are only referenced by their path');
  });

  it('ticks what is new, disables what is not, and says why', async () => {
    await openDialog();
    expect(boxOf('bastion').checked).toBe(true);
    expect(boxOf('app').checked).toBe(true);
    // Already in the library: listed, not tickable, with the reason in view.
    expect(boxOf('staging').disabled).toBe(true);
    expect(boxOf('staging').checked).toBe(false);
    expect(rowOf('staging').textContent).toContain('In your library');
    expect(rowOf('staging').textContent).toContain(
      'Your library already has a host with this address, user and port.',
    );
    // A host that could not be valid carries the reason the import gave.
    expect(boxOf('legacy').disabled).toBe(true);
    expect(rowOf('legacy').textContent).toContain('Cannot import');
    expect(rowOf('legacy').textContent).toContain('address contains invalid characters');
    // The reason is wired to the checkbox for a screen reader.
    const describedBy = boxOf('staging').getAttribute('aria-describedby') ?? '';
    expect(document.getElementById(describedBy)?.textContent).toContain('already has a host');
    expect(text()).toContain('2 of 2 selected');
    expect(findButton(target, 'Import 2 hosts')).toBeTruthy();
  });

  it('lets the user choose, select all and select none', async () => {
    await openDialog();
    click(boxOf('app'));
    expect(text()).toContain('1 of 2 selected');
    expect(findButton(target, 'Import 1 host')).toBeTruthy();
    click(findButton(target, 'Select none'));
    expect(text()).toContain('0 of 2 selected');
    const empty = target.querySelector<HTMLButtonElement>('button[type="submit"]');
    expect(empty?.disabled).toBe(true);
    expect(empty?.getAttribute('title')).toBe('Tick at least one host to import');
    click(findButton(target, 'Select all new'));
    expect(text()).toContain('2 of 2 selected');
  });

  it('says a jump host comes with the host that routes through it', async () => {
    await openDialog();
    click(boxOf('bastion'));
    expect(text()).toContain('Also imports bastion.');
    click(boxOf('bastion'));
    expect(text()).not.toContain('Also imports');
  });

  it('imports what is ticked, then shows the summary and a notice', async () => {
    await openDialog();
    click(boxOf('bastion'));
    click(findButton(target, 'Import 1 host'));
    await vi.waitFor(() => expect(sshImport.phase).toBe('done'));
    flushSync();
    expect(callsOf('ssh_config_import')).toHaveLength(1);
    expect(callsOf('ssh_config_import')[0].args).toEqual({ selection: ['app'] });
    expect(target.querySelector('h2')?.textContent).toBe('Import finished');
    expect(text()).toContain('Imported 2 hosts.');
    expect(text()).toContain(
      '1 key file referenced as an identity. The files stay where they are.',
    );
    // The library is read again, and a notice says what happened.
    expect(callsOf('host_list').length).toBeGreaterThan(0);
    expect(noticeText(notices.visible[0])).toBe(
      'Hosts imported. 2 hosts added from your SSH config · 0 skipped',
    );
    click(findButton(target, 'Done'));
    expect(sshImport.open).toBe(false);
  });

  it('reports what was left out and why, and what the library already had', async () => {
    answers.set(
      'ssh_config_import',
      report({
        imported: [{ id: 'n1', name: 'bastion' }],
        identities_created: 0,
        skipped: [{ alias: 'legacy', reason: 'address contains invalid characters' }],
        already_present: ['staging'],
        warnings: ['a key file could not be found'],
      }),
    );
    await openDialog();
    click(findButton(target, 'Import 2 hosts'));
    await vi.waitFor(() => expect(sshImport.phase).toBe('done'));
    flushSync();
    expect(text()).toContain('Imported 1 host.');
    expect(text()).toContain('Already in your library: staging');
    expect(text()).toContain('legacy · address contains invalid characters');
    expect(text()).toContain('a key file could not be found');
    expect(noticeText(notices.visible[0])).toContain('1 skipped');
  });

  it('keeps the choice and says so when the import fails', async () => {
    failures.set('ssh_config_import', 'database is locked');
    await openDialog();
    click(findButton(target, 'Import 2 hosts'));
    await vi.waitFor(() => expect(sshImport.error).toBe('database is locked'));
    flushSync();
    expect(sshImport.phase).toBe('ready');
    expect(target.querySelector('[role="alert"]')?.textContent).toContain('database is locked');
    expect(boxOf('app').checked).toBe(true);
    expect(notices.visible[0].kind).toBe('error');
  });

  it('shows "no SSH config" when there is no file, and imports nothing', async () => {
    answers.set('ssh_config_preview', preview({ found: false, candidates: [], warnings: [] }));
    await openDialog();
    expect(text()).toContain('No SSH config found');
    expect(text()).toContain('/home/me/.ssh/config');
    expect(target.querySelector('button[type="submit"]')).toBeNull();
    expect(findButton(target, 'Close')).toBeTruthy();
  });

  it('shows an empty state for a file with no importable host entry', async () => {
    answers.set('ssh_config_preview', preview({ candidates: [], warnings: [] }));
    await openDialog();
    expect(text()).toContain('No hosts to import');
    expect(target.querySelector('li.row')).toBeNull();
  });

  it('shows a read error and lets the user try again', async () => {
    failures.set('ssh_config_preview', '/home/me/.ssh/config: permission denied');
    await openDialog();
    expect(sshImport.phase).toBe('error');
    expect(target.querySelector('[role="alert"]')?.textContent).toContain('permission denied');
    failures.delete('ssh_config_preview');
    click(findButton(target, 'Try again'));
    await vi.waitFor(() => expect(sshImport.phase).toBe('ready'));
    flushSync();
    expect(boxes()).toHaveLength(4);
  });

  it('writes what the file says as plain text', async () => {
    answers.set(
      'ssh_config_preview',
      preview({
        candidates: [candidate({ alias: '<img src=x onerror=alert(1)>', warnings: ['a‮b'] })],
        warnings: [],
      }),
    );
    await openDialog();
    expect(target.querySelector('img')).toBeNull();
    // A bidirectional override is written out, not obeyed.
    expect(text()).toContain('a‹U+202E›b');
  });

  it('closes on Cancel and on Escape, and a stale answer does not reopen it', async () => {
    await openDialog();
    click(findButton(target, 'Cancel'));
    expect(sshImport.open).toBe(false);
    answers.set(
      'ssh_config_preview',
      () => new Promise<SshConfigPreview>((resolve) => setTimeout(() => resolve(preview()), 10)),
    );
    void sshImport.show();
    sshImport.close();
    await new Promise((resolve) => setTimeout(resolve, 30));
    expect(sshImport.open).toBe(false);
    expect(sshImport.preview).toEqual(preview());
  });
});

describe('the Hosts page', () => {
  it('has a secondary "Import from SSH config" action next to New host', async () => {
    hosts.loaded = true;
    hosts.loading = false;
    hosts.error = null;
    route.route = 'hosts';
    component = mount(HostsPage, { target, props: { onconnect: () => {}, onopen: () => {} } });
    flushSync();
    const importButton = findButton(target, 'Import from SSH config');
    const newHost = findButton(target, 'New host');
    expect(importButton.classList.contains('primary')).toBe(false);
    expect(newHost.classList.contains('primary')).toBe(true);
    expect(
      importButton.compareDocumentPosition(newHost) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
    click(importButton);
    await vi.waitFor(() => expect(callsOf('ssh_config_preview')).toHaveLength(1));
    expect(sshImport.open).toBe(true);
  });
});
