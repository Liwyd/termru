// @vitest-environment jsdom
/**
 * The connect dialog when the route passes jump hosts that need a password:
 * one field per hop, labelled with the hop's name, and a box per hop to keep
 * its password in the credential store.
 */

import { flushSync, mount, tick, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import ConnectDialog from './ConnectDialog.svelte';
import type { ConnectRequest } from '../stores/connect.svelte';
import type { HopAnswers } from '../stores/sessions.svelte';
import { click, newTarget, stubBrowserApis, type } from '../test-support/dom';
import { resetTauriMock } from '../test-support/tauri-core-mock';
import type { Host, SessionSecrets } from '../types';

vi.mock('@tauri-apps/api/core', () => import('../test-support/tauri-core-mock'));

function host(id: string, name: string, over: Partial<Host> = {}): Host {
  const now = new Date().toISOString();
  return {
    id,
    name,
    address: `${name}.test`,
    port: 22,
    username: 'ops',
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
    ...over,
  };
}

let target: HTMLElement;
let component: ReturnType<typeof mount> | null = null;

function open(over: Partial<ConnectRequest> = {}) {
  const onsubmit = vi.fn<(secrets: SessionSecrets, remember: boolean, hops?: HopAnswers) => void>();
  const oncancel = vi.fn();
  const request: ConnectRequest = {
    host: host('app', 'app', { auth: { type: 'agent' } }),
    secretStored: false,
    askTarget: false,
    hops: [
      { host: host('gw', 'gateway'), stored: false },
      { host: host('bastion', 'bastion', { port: 2222 }), stored: false },
    ],
    pane: null,
    reconnecting: false,
    ...over,
  };
  component = mount(ConnectDialog, { target, props: { request, onsubmit, oncancel } });
  flushSync();
  return { onsubmit, oncancel };
}

const fields = () =>
  Array.from(target.querySelectorAll<HTMLInputElement>('input[type="password"]'));
const submit = () => {
  target
    .querySelector('form')
    ?.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }));
  flushSync();
};
const label = (input: HTMLInputElement) =>
  target.querySelector(`label[for="${input.id}"]`)?.textContent?.trim();

beforeEach(() => {
  resetTauriMock();
  stubBrowserApis();
  target = newTarget();
});

afterEach(() => {
  if (component) unmount(component);
  component = null;
  target.remove();
  document.body.replaceChildren();
});

describe('the connect dialog for a route with jump hosts', () => {
  it('has one password field per hop, labelled with its name, and none for the host', () => {
    open();
    expect(fields().map(label)).toEqual(['Password for gateway', 'Password for bastion']);
    expect(target.textContent).toContain(
      'Jump host ops@bastion.test:2222. Used for this connection only.',
    );
    // The host signs in with the agent: it is not asked for anything.
    expect(target.querySelector('h2')?.textContent).toBe('Connect to app');
  });

  it('asks for the host too when it needs a password, after its hops', () => {
    open({ host: host('app', 'app'), askTarget: true });
    expect(fields().map(label)).toEqual([
      'Password for gateway',
      'Password for bastion',
      'Password for app',
    ]);
  });

  it('is a plain Password field when there are no hops', () => {
    open({ host: host('app', 'app'), askTarget: true, hops: [] });
    expect(fields().map(label)).toEqual(['Password']);
  });

  it('focuses the first hop when the host asks for nothing', async () => {
    open();
    await tick();
    expect(document.activeElement).toBe(fields()[0]);
  });

  it('sends what was typed by hop id, and which to keep', () => {
    const { onsubmit } = open();
    type(fields()[0], 'gw-secret');
    type(fields()[1], 'bastion-secret');
    const boxes = target.querySelectorAll<HTMLInputElement>('input[type="checkbox"]');
    expect(boxes).toHaveLength(2);
    expect(target.textContent).toContain(
      'Save bastion’s password in this device’s credential store',
    );
    click(boxes[1]);
    submit();
    expect(onsubmit).toHaveBeenCalledTimes(1);
    const [secrets, remember, hops] = onsubmit.mock.calls[0];
    expect(secrets).toEqual({});
    expect(remember).toBe(false);
    expect(hops).toEqual({
      secrets: { gw: { password: 'gw-secret' }, bastion: { password: 'bastion-secret' } },
      remember: ['bastion'],
    });
  });

  it('keeps the host box and the hop boxes apart', () => {
    const { onsubmit } = open({ host: host('app', 'app'), askTarget: true });
    type(fields()[0], 'g');
    type(fields()[1], 'b');
    type(fields()[2], 'host-pw');
    const boxes = target.querySelectorAll<HTMLInputElement>('input[type="checkbox"]');
    expect(boxes).toHaveLength(3);
    click(boxes[2]);
    submit();
    const [secrets, remember, hops] = onsubmit.mock.calls[0];
    expect(secrets).toEqual({ password: 'host-pw' });
    expect(remember).toBe(true);
    expect(hops?.remember).toEqual([]);
  });

  it('does not go on without the password of a hop that has none, and says which', async () => {
    const { onsubmit } = open();
    type(fields()[0], 'gw-secret');
    submit();
    expect(onsubmit).not.toHaveBeenCalled();
    const bastion = fields()[1];
    expect(bastion.getAttribute('aria-invalid')).toBe('true');
    expect(
      document.getElementById(bastion.getAttribute('aria-describedby') ?? '')?.textContent,
    ).toBe('Enter the password for bastion.');
    expect(fields()[0].getAttribute('aria-invalid')).toBeNull();
    await vi.waitFor(() => expect(document.activeElement).toBe(bastion));
    // Typing clears the complaint, and the next submit goes through.
    type(bastion, 'b');
    expect(bastion.getAttribute('aria-invalid')).toBeNull();
    submit();
    expect(onsubmit).toHaveBeenCalledTimes(1);
  });

  it('lets a hop with a saved password stay empty (that one is used)', () => {
    const { onsubmit } = open({
      hops: [{ host: host('gw', 'gateway'), stored: true }],
    });
    expect(target.textContent).toContain('A password for gateway is saved on this device.');
    submit();
    expect(onsubmit).toHaveBeenCalledTimes(1);
    expect(onsubmit.mock.calls[0][2]).toEqual({ secrets: {}, remember: [] });
  });

  it('forgets what was typed when it is cancelled', () => {
    const { oncancel } = open();
    type(fields()[0], 'typed');
    click(target.querySelector('button.quiet') as HTMLElement);
    expect(oncancel).toHaveBeenCalledTimes(1);
    expect(fields()[0].value).toBe('');
  });

  it('shows a hop name as plain text', () => {
    open({ hops: [{ host: host('x', '<img src=x onerror=alert(1)>'), stored: false }] });
    expect(target.querySelector('img')).toBeNull();
    expect(label(fields()[0])).toBe('Password for <img src=x onerror=alert(1)>');
  });
});
