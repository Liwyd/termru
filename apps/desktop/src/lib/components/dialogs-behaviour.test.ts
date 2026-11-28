// @vitest-environment jsdom
/**
 * The dialogs and the context menu: focus on open, focus kept inside, Escape,
 * focus back to the opener, and the secrets they send.
 */

import { flushSync, mount, tick, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import ConnectDialog from './ConnectDialog.svelte';
import PopMenu from './PopMenu.svelte';
import HostKeyDialog from './HostKeyDialog.svelte';
import { notices } from '../notify.svelte';
import { noticeText } from '../test-support/notices';
import { click, findButton, newTarget, press, stubBrowserApis, type } from '../test-support/dom';
import { answers, callsOf, failures, resetTauriMock } from '../test-support/tauri-core-mock';
import type { ConnectRequest } from '../stores/connect.svelte';
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
  notices.reset();
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
  function open(secretStored = false, name = 'web-1') {
    const onsubmit = vi.fn<(secrets: SessionSecrets, remember: boolean) => void>();
    const oncancel = vi.fn();
    const request: ConnectRequest = {
      host: { ...host({ type: 'password' }), name },
      secretStored,
      askTarget: true,
      hops: [],
      pane: null,
      reconnecting: false,
    };
    component = mount(ConnectDialog, { target, props: { request, onsubmit, oncancel } });
    flushSync();
    return { onsubmit, oncancel };
  }

  const submit = () =>
    target
      .querySelector('form')
      ?.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }));

  it('names the host and shows where it connects', () => {
    open();
    expect(target.querySelector('h2')?.textContent).toBe('Connect to web-1');
    expect(target.textContent).toContain('deploy@web-1.test:22');
  });

  it('sends no password when the field is empty (the backend falls back to the keychain)', () => {
    const { onsubmit } = open();
    submit();
    expect(onsubmit).toHaveBeenCalledTimes(1);
    const secrets = onsubmit.mock.calls[0][0];
    expect(secrets.password).toBeUndefined();
    // The wire shape: no `password` key at all once serialized.
    expect(JSON.stringify(secrets)).toBe('{}');
    expect(onsubmit.mock.calls[0][1]).toBe(false);
  });

  it('sends the password that was typed, and whether to keep it', () => {
    const { onsubmit } = open();
    type(target.querySelector('input[type="password"]') as HTMLInputElement, 'hunter2');
    click(target.querySelector('input[type="checkbox"]') as HTMLInputElement);
    submit();
    expect(onsubmit.mock.calls[0][0]).toEqual({ password: 'hunter2' });
    expect(onsubmit.mock.calls[0][1]).toBe(true);
  });

  it('asks for a password and nothing else (a passphrase is the backend’s question)', () => {
    open();
    expect(target.querySelectorAll('input[type="password"]')).toHaveLength(1);
    expect(target.textContent).not.toMatch(/passphrase|private key|agent/i);
  });

  it('tells the user when a password is already saved on this device', () => {
    open(true);
    expect(target.textContent).toContain('A password is saved on this device');
  });

  it('shows a host name as plain text', () => {
    open(false, '<img src=x onerror=alert(1)>');
    expect(target.querySelector('h2')?.textContent).toContain('<img src=x');
    expect(target.querySelector('img')).toBeNull();
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

  it('cancels on a click outside, and on the Cancel button', () => {
    const { oncancel } = open();
    click(target.querySelector('.backdrop') as HTMLElement);
    click(findButton(target, 'Cancel'));
    expect(oncancel).toHaveBeenCalledTimes(2);
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

  const changed = (id: string): HostKeyPromptMessage => ({
    id,
    session_id: 's1',
    request: {
      state: 'changed',
      host: 'rebuilt.test',
      port: 2222,
      old: { key_type: 'ssh-ed25519', fingerprint: 'SHA256:OLDOLDOLD' },
      new: { key_type: 'ssh-ed25519', fingerprint: 'SHA256:NEWNEWNEW' },
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

  const gone = () =>
    vi.waitFor(() => expect(target.querySelector('[role="alertdialog"]')).toBeNull());

  it('queues a second prompt instead of replacing the first', async () => {
    const emit = await openDialog();
    emit(unknown('p1', 'first.test'));
    emit(unknown('p2', 'second.test'));
    expect(target.textContent).toContain('first.test');
    expect(target.textContent).not.toContain('second.test');
    expect(target.textContent).toContain('1 more host key question');

    // Answering the first shows the second, and both are answered.
    click(findButton(target, 'Cancel'));
    await vi.waitFor(() => expect(target.textContent).toContain('second.test'));
    click(findButton(target, 'Verify & connect'));
    await vi.waitFor(() => expect(callsOf('host_key_prompt_resolve')).toHaveLength(2));
    expect(callsOf('host_key_prompt_resolve').map((call) => call.args)).toEqual([
      { id: 'p1', trust: false },
      { id: 'p2', trust: true },
    ]);
    await gone();
  });

  it('shows where it connects, the key type and the fingerprint to compare', async () => {
    const emit = await openDialog();
    emit(unknown('p1', 'first.test'));
    expect(target.querySelector('h2')?.textContent).toBe('Verify this host');
    expect(target.textContent).toContain('first.test:22');
    expect(target.textContent).toContain('ssh-ed25519');
    expect(target.querySelector('[data-testid="fingerprint-new"]')?.textContent).toBe('SHA256:p1');
  });

  it('warns when the host is already known under other key types', async () => {
    const emit = await openDialog();
    emit(unknown('p1', 'first.test', ['ssh-rsa', 'ecdsa-sha2-nistp256']));
    const warning = target.querySelector('[data-testid="other-key-types"]');
    expect(warning).not.toBeNull();
    expect(warning?.textContent).toContain('ssh-rsa, ecdsa-sha2-nistp256');
    expect(warning?.textContent).toContain('ssh-ed25519');
  });

  it('shows how to check the fingerprint for a host that was never seen', async () => {
    const emit = await openDialog();
    emit(unknown('p1', 'first.test'));
    expect(target.querySelector('[data-testid="other-key-types"]')).toBeNull();
    expect(target.textContent).toContain('Compare the fingerprint with a source you trust');
    expect(target.textContent).toContain('ssh-keygen -lf');
  });

  it('blocks a changed key: old and new fingerprints, a warning, and the safe default', async () => {
    const emit = await openDialog();
    emit(changed('p9'));
    expect(target.querySelector('h2')?.textContent).toBe('Host key changed');
    expect(target.querySelector('[data-testid="fingerprint-old"]')?.textContent).toBe(
      'SHA256:OLDOLDOLD',
    );
    expect(target.querySelector('[data-testid="fingerprint-new"]')?.textContent).toBe(
      'SHA256:NEWNEWNEW',
    );
    expect(target.textContent).toContain('rebuilt.test:2222');
    expect(target.textContent).toContain('Connecting is blocked until you decide');
    expect(target.textContent).toContain('replaces the stored key');
    await tick();
    expect(document.activeElement).toBe(findButton(target, 'Cancel'));
    // The accepting button is there, and says what it does.
    expect(findButton(target, 'Verify & connect').classList.contains('danger-fill')).toBe(true);
  });

  it('shows what the server sent as plain text', async () => {
    const emit = await openDialog();
    emit(unknown('p1', '<b onmouseover=alert(1)>evil</b>.test'));
    expect(target.textContent).toContain('<b onmouseover=alert(1)>evil</b>.test');
    expect(target.querySelector('b')).toBeNull();
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
    await gone();
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
    expect(document.activeElement).toBe(findButton(target, 'Cancel'));
  });

  it('cancels on Escape, but not on a click outside', async () => {
    const emit = await openDialog();
    emit(unknown('p1', 'first.test'));
    click(target.querySelector('.backdrop') as HTMLElement);
    expect(callsOf('host_key_prompt_resolve')).toHaveLength(0);
    press(target.querySelector('[role="alertdialog"]') as HTMLElement, 'Escape');
    await vi.waitFor(() => expect(callsOf('host_key_prompt_resolve')).toHaveLength(1));
    expect(callsOf('host_key_prompt_resolve')[0].args).toEqual({ id: 'p1', trust: false });
    await gone();
  });

  it('tells the user when the answer could not be delivered', async () => {
    const emit = await openDialog();
    failures.set('host_key_prompt_resolve', 'the backend went away');
    emit(unknown('p1', 'first.test'));
    click(findButton(target, 'Verify & connect'));
    await vi.waitFor(() => expect(notices.visible.length).toBe(1));
    expect(notices.visible[0].kind).toBe('error');
    expect(noticeText(notices.visible[0])).toContain('the backend went away');
  });

  it('says so when the prompt is no longer active', async () => {
    const emit = await openDialog();
    answers.set('host_key_prompt_resolve', false);
    emit(unknown('p1', 'first.test'));
    click(findButton(target, 'Cancel'));
    await vi.waitFor(() => expect(notices.visible.length).toBe(1));
    expect(notices.visible[0].kind).toBe('info');
  });
});

describe('PopMenu', () => {
  const items = [
    { id: 'connect', label: 'Connect' },
    { id: 'edit', label: 'Edit…' },
    { id: 'off', label: 'Unavailable', disabled: true, reason: 'Not now' },
    { id: 'delete', label: 'Delete', danger: true },
  ];

  /** `animate: false` is a menu the keyboard opened: no motion, first entry focused. */
  function openMenu(animate = false) {
    const onchoose = vi.fn<(id: string) => void>();
    const onclose = vi.fn();
    component = mount(PopMenu, {
      target,
      props: { open: true, x: 10, y: 10, items, label: 'Actions', animate, onchoose, onclose },
    });
    flushSync();
    return { onchoose, onclose };
  }

  const entries = () => Array.from(target.querySelectorAll<HTMLButtonElement>('[role="menuitem"]'));

  it('names itself, and focuses the first entry when the keyboard opened it', async () => {
    openMenu();
    await tick();
    expect(target.querySelector('[role="menu"]')?.getAttribute('aria-label')).toBe('Actions');
    expect(document.activeElement).toBe(entries()[0]);
  });

  it('keeps the focus on the menu, with nothing highlighted, when a pointer opened it', async () => {
    openMenu(true);
    await tick();
    expect(document.activeElement).toBe(target.querySelector('[role="menu"]'));
  });

  it('moves with the arrow keys, Home and End, and wraps; a disabled entry stays reachable', async () => {
    openMenu();
    await tick();
    const menu = target.querySelector('[role="menu"]') as HTMLElement;

    press(menu, 'ArrowDown');
    expect(document.activeElement).toBe(entries()[1]);
    press(menu, 'ArrowDown');
    // Its reason can be read, so it takes the focus.
    expect(document.activeElement).toBe(entries()[2]);
    expect(entries()[2].getAttribute('aria-disabled')).toBe('true');
    expect(entries()[2].title).toBe('Not now');
    press(menu, 'ArrowDown');
    expect(document.activeElement).toBe(entries()[3]);
    press(menu, 'ArrowDown');
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

  it('chooses an entry, and does nothing for a disabled one', async () => {
    const { onchoose, onclose } = openMenu();
    await tick();
    click(entries()[1]);
    expect(onchoose).toHaveBeenCalledWith('edit');
    expect(onclose).toHaveBeenCalledTimes(1);

    click(entries()[2]);
    expect(onchoose).toHaveBeenCalledTimes(1);
    expect(onclose).toHaveBeenCalledTimes(1);
  });
});
