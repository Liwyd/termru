// @vitest-environment jsdom
/**
 * The dialogs and the context menu: focus on open, focus kept inside, Escape,
 * focus back to the opener, and the secrets they send.
 */

import { flushSync, mount, tick, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import ConnectDialog from './ConnectDialog.svelte';
import ContextMenu from './ContextMenu.svelte';
import HostKeyDialog from './HostKeyDialog.svelte';
import { toasts } from '../stores/toasts.svelte';
import { click, findButton, newTarget, press, stubBrowserApis, type } from '../test-support/dom';
import { answers, callsOf, failures, resetTauriMock } from '../test-support/tauri-core-mock';
import type { Host, HostKeyPromptMessage, SessionSecrets } from '../types';

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

function host(auth: Host['auth'] = { type: 'password' }): Host {
  const now = new Date().toISOString();
  return {
    id: 'host-1',
    name: 'web-1',
    address: 'web-1.test',
    port: 22,
    username: 'deploy',
    auth,
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
  };
}

let target: HTMLElement;
let component: ReturnType<typeof mount> | null = null;
let opener: HTMLButtonElement;

beforeEach(() => {
  resetTauriMock();
  stubBrowserApis();
  toasts.toasts = [];
  bus.handlers.clear();
  opener = document.createElement('button');
  opener.textContent = 'opener';
  document.body.appendChild(opener);
  opener.focus();
  target = newTarget();
});

afterEach(() => {
  if (component) unmount(component);
  component = null;
  document.body.replaceChildren();
});

describe('ConnectDialog', () => {
  function open(auth: Host['auth'] = { type: 'password' }) {
    const onsubmit = vi.fn<(secrets: SessionSecrets, remember: boolean) => void>();
    const oncancel = vi.fn();
    component = mount(ConnectDialog, {
      target,
      props: { host: host(auth), onsubmit, oncancel },
    });
    flushSync();
    return { onsubmit, oncancel };
  }

  it('sends no password when the field is empty (the backend falls back to the keychain)', () => {
    const { onsubmit } = open();
    target
      .querySelector('form')
      ?.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }));
    expect(onsubmit).toHaveBeenCalledTimes(1);
    const secrets = onsubmit.mock.calls[0][0];
    expect(secrets.password).toBeUndefined();
    // The wire shape: no `password` key at all once serialized.
    expect(JSON.stringify(secrets)).toBe('{}');
  });

  it('sends the password that was typed', () => {
    const { onsubmit } = open();
    type(target.querySelector('input[type="password"]') as HTMLInputElement, 'hunter2');
    target
      .querySelector('form')
      ?.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }));
    expect(onsubmit.mock.calls[0][0]).toEqual({ password: 'hunter2' });
  });

  it('sends no passphrase when the field is empty', () => {
    const { onsubmit } = open({ type: 'private_key', identity_id: 'i1' });
    target
      .querySelector('form')
      ?.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }));
    expect(JSON.stringify(onsubmit.mock.calls[0][0])).toBe('{}');
  });

  it('focuses the password field, keeps Tab inside and cancels on Escape', async () => {
    const { oncancel } = open();
    await tick();
    const password = target.querySelector('input[type="password"]') as HTMLInputElement;
    expect(document.activeElement).toBe(password);

    // Tab from the last control wraps to the first.
    const connect = findButton(target, 'Connect');
    connect.focus();
    const tabEvent = press(connect, 'Tab');
    expect(tabEvent.defaultPrevented).toBe(true);
    expect(document.activeElement).toBe(password);

    press(target.querySelector('[role="dialog"]') as HTMLElement, 'Escape');
    expect(oncancel).toHaveBeenCalledTimes(1);
  });

  it('gives the focus back to the opener when it goes away', async () => {
    open();
    await tick();
    expect(document.activeElement).not.toBe(opener);
    unmount(component as ReturnType<typeof mount>);
    component = null;
    expect(document.activeElement).toBe(opener);
  });
});

describe('HostKeyDialog', () => {
  const unknown = (id: string, hostName: string, others?: string[]): HostKeyPromptMessage => ({
    id,
    session_id: 's1',
    request: {
      state: 'unknown',
      host: hostName,
      port: 22,
      key: { key_type: 'ssh-ed25519', fingerprint: `SHA256:${id}` },
      ...(others ? { other_key_types: others } : {}),
    },
  });

  async function openDialog(): Promise<(payload: unknown) => void> {
    component = mount(HostKeyDialog, { target });
    flushSync();
    await vi.waitFor(() => expect(bus.handlers.has('host-key-prompt')).toBe(true));
    return (payload) => {
      bus.handlers.get('host-key-prompt')?.({ payload });
      flushSync();
    };
  }

  it('queues a second prompt instead of replacing the first', async () => {
    const emit = await openDialog();
    emit(unknown('p1', 'first.test'));
    emit(unknown('p2', 'second.test'));
    expect(target.textContent).toContain('first.test');
    expect(target.textContent).not.toContain('second.test');
    expect(target.textContent).toContain('1 more host key question');

    // Answering the first shows the second, and both are answered.
    click(findButton(target, 'Reject'));
    await vi.waitFor(() => expect(target.textContent).toContain('second.test'));
    click(findButton(target, 'Trust'));
    await vi.waitFor(() => expect(callsOf('host_key_prompt_resolve')).toHaveLength(2));
    expect(callsOf('host_key_prompt_resolve').map((call) => call.args)).toEqual([
      { id: 'p1', trust: false },
      { id: 'p2', trust: true },
    ]);
    expect(target.querySelector('[role="alertdialog"]')).toBeNull();
  });

  it('warns when the host is already known under other key types', async () => {
    const emit = await openDialog();
    emit(unknown('p1', 'first.test', ['ssh-rsa', 'ecdsa-sha2-nistp256']));
    const warning = target.querySelector('[data-testid="other-key-types"]');
    expect(warning).not.toBeNull();
    expect(warning?.textContent).toContain('ssh-rsa, ecdsa-sha2-nistp256');
    expect(warning?.textContent).toContain('ssh-ed25519');
  });

  it('shows the plain explanation for a host that was never seen', async () => {
    const emit = await openDialog();
    emit(unknown('p1', 'first.test'));
    expect(target.querySelector('[data-testid="other-key-types"]')).toBeNull();
    expect(target.textContent).toContain('The server key is not known yet');
  });

  it('drops a prompt the backend closed (timeout, closed tab, answered elsewhere)', async () => {
    const emit = await openDialog();
    emit(unknown('p1', 'first.test'));
    emit(unknown('p2', 'second.test'));
    const close = bus.handlers.get('host-key-prompt-closed');
    expect(close).toBeDefined();
    close?.({ payload: { id: 'p1' } });
    flushSync();
    expect(target.textContent).toContain('second.test');
    expect(target.textContent).not.toContain('first.test');
    close?.({ payload: { id: 'p2' } });
    flushSync();
    expect(target.querySelector('[role="alertdialog"]')).toBeNull();
    expect(callsOf('host_key_prompt_resolve')).toHaveLength(0);
  });

  it('ignores the same prompt delivered twice', async () => {
    const emit = await openDialog();
    emit(unknown('p1', 'first.test'));
    emit(unknown('p1', 'first.test'));
    expect(target.textContent).not.toContain('more host key question');
  });

  it('puts the focus on the safe button', async () => {
    const emit = await openDialog();
    emit(unknown('p1', 'first.test'));
    await tick();
    expect(document.activeElement).toBe(findButton(target, 'Reject'));
  });

  it('rejects on Escape', async () => {
    const emit = await openDialog();
    emit(unknown('p1', 'first.test'));
    press(target.querySelector('[role="alertdialog"]') as HTMLElement, 'Escape');
    await vi.waitFor(() => expect(callsOf('host_key_prompt_resolve')).toHaveLength(1));
    expect(callsOf('host_key_prompt_resolve')[0].args).toEqual({ id: 'p1', trust: false });
    expect(target.querySelector('[role="alertdialog"]')).toBeNull();
  });

  it('tells the user when the answer could not be delivered', async () => {
    const emit = await openDialog();
    failures.set('host_key_prompt_resolve', 'the backend went away');
    emit(unknown('p1', 'first.test'));
    click(findButton(target, 'Trust'));
    await vi.waitFor(() => expect(toasts.toasts.length).toBe(1));
    expect(toasts.toasts[0].kind).toBe('error');
    expect(toasts.toasts[0].message).toContain('the backend went away');
  });

  it('says so when the prompt is no longer active', async () => {
    const emit = await openDialog();
    answers.set('host_key_prompt_resolve', false);
    emit(unknown('p1', 'first.test'));
    click(findButton(target, 'Reject'));
    await vi.waitFor(() => expect(toasts.toasts.length).toBe(1));
    expect(toasts.toasts[0].kind).toBe('info');
  });
});

describe('ContextMenu', () => {
  const items = [
    { id: 'connect', label: 'Connect' },
    { id: 'edit', label: 'Edit…' },
    { id: 'off', label: 'Unavailable', disabled: true },
    { id: 'delete', label: 'Delete', danger: true },
  ];

  function openMenu() {
    const onchoose = vi.fn<(id: string) => void>();
    const onclose = vi.fn();
    component = mount(ContextMenu, {
      target,
      props: { open: true, x: 10, y: 10, items, onchoose, onclose },
    });
    flushSync();
    return { onchoose, onclose };
  }

  const entries = () => Array.from(target.querySelectorAll<HTMLButtonElement>('[role="menuitem"]'));

  it('focuses the first entry when it opens', async () => {
    openMenu();
    await tick();
    expect(document.activeElement).toBe(entries()[0]);
  });

  it('moves with the arrow keys, Home and End, skipping disabled entries', async () => {
    openMenu();
    await tick();
    const menu = target.querySelector('[role="menu"]') as HTMLElement;

    press(menu, 'ArrowDown');
    expect(document.activeElement).toBe(entries()[1]);
    press(menu, 'ArrowDown');
    // The disabled entry is skipped.
    expect(document.activeElement).toBe(entries()[3]);
    press(menu, 'ArrowDown');
    // And it wraps.
    expect(document.activeElement).toBe(entries()[0]);
    press(menu, 'ArrowUp');
    expect(document.activeElement).toBe(entries()[3]);
    press(menu, 'Home');
    expect(document.activeElement).toBe(entries()[0]);
    press(menu, 'End');
    expect(document.activeElement).toBe(entries()[3]);
  });

  it('closes on Escape and returns the focus to the opener', async () => {
    const { onclose } = openMenu();
    await tick();
    window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    expect(onclose).toHaveBeenCalled();

    // The parent closes it: the focus goes back to where it was.
    unmount(component as ReturnType<typeof mount>);
    component = null;
    expect(document.activeElement).toBe(opener);
  });

  it('chooses an entry with Enter (a focused button)', async () => {
    const { onchoose } = openMenu();
    await tick();
    click(entries()[1]);
    expect(onchoose).toHaveBeenCalledWith('edit');
  });
});
