// @vitest-environment jsdom
/**
 * The authentication question dialog: what it shows for a keyboard-
 * interactive round and for a key passphrase, how it answers, queues and
 * closes questions, and that the passphrase is only remembered once the
 * connection authenticated.
 */

import { flushSync, mount, tick, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import AuthPromptDialog from './AuthPromptDialog.svelte';
import { sessions } from '../stores/sessions.svelte';
import { notices } from '../notify.svelte';
import { noticeText } from '../test-support/notices';
import { click, findButton, newTarget, press, stubBrowserApis, type } from '../test-support/dom';
import { answers, callsOf, failures, resetTauriMock } from '../test-support/tauri-core-mock';
import type { AuthPromptMessage } from '../types';

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

function kbd(id: string, overrides: Partial<AuthPromptMessage['request']> = {}): AuthPromptMessage {
  return {
    id,
    session_id: 's1',
    host_id: 'host-1',
    request: {
      kind: { type: 'keyboard_interactive' },
      name: 'Two-factor',
      instructions: 'Enter the code from your app.',
      prompts: [
        { text: 'Verification code: ', echo: true },
        { text: 'PIN: ', echo: false },
      ],
      ...overrides,
    },
  };
}

function passphrase(id: string, retry = false, identity = true): AuthPromptMessage {
  return {
    id,
    session_id: 's1',
    host_id: 'host-1',
    request: {
      kind: {
        type: 'passphrase',
        identity: identity ? { id: 'identity-1', name: 'work key' } : null,
        retry,
      },
      name: '',
      instructions: '',
      prompts: [{ text: 'Passphrase for work key', echo: false }],
    },
  };
}

async function openDialog(): Promise<(event: string, payload: unknown) => void> {
  component = mount(AuthPromptDialog, { target });
  flushSync();
  await vi.waitFor(() => {
    expect(bus.handlers.has('auth-prompt')).toBe(true);
    expect(bus.handlers.has('auth-prompt-closed')).toBe(true);
  });
  return (event, payload) => {
    bus.handlers.get(event)?.({ payload });
    flushSync();
  };
}

const dialog = () => target.querySelector<HTMLElement>('[role="dialog"]');
const inputs = () =>
  Array.from(target.querySelectorAll<HTMLInputElement>('input:not([type="checkbox"])'));
const submitForm = () => {
  target
    .querySelector('form')
    ?.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }));
  flushSync();
};

describe('a keyboard-interactive question', () => {
  it('shows the title, the instructions and one field per prompt', async () => {
    const emit = await openDialog();
    emit('auth-prompt', kbd('a1'));

    expect(dialog()?.getAttribute('aria-modal')).toBe('true');
    expect(target.querySelector('h2')?.textContent).toBe('Two-factor');
    expect(target.textContent).toContain('Enter the code from your app.');
    const fields = inputs();
    expect(fields).toHaveLength(2);
    expect(target.textContent).toContain('Verification code:');
    expect(target.textContent).toContain('PIN:');
    // Only a secret is hidden: a visible prompt shows what is typed.
    expect(fields[0].type).toBe('text');
    expect(fields[1].type).toBe('password');
    // No passphrase options for a server's question.
    expect(target.querySelector('input[type="checkbox"]')).toBeNull();
  });

  it('sends the answers in order and closes', async () => {
    const emit = await openDialog();
    emit('auth-prompt', kbd('a1'));
    type(inputs()[0], '123456');
    type(inputs()[1], '4321');
    submitForm();

    await vi.waitFor(() => expect(callsOf('auth_prompt_resolve')).toHaveLength(1));
    expect(callsOf('auth_prompt_resolve')[0].args).toEqual({
      id: 'a1',
      answers: ['123456', '4321'],
    });
    await vi.waitFor(() => expect(dialog()).toBeNull());
  });

  it('submits with the Continue button', async () => {
    const emit = await openDialog();
    emit('auth-prompt', kbd('a1'));
    expect(findButton(target, 'Continue').type).toBe('submit');
    type(inputs()[0], '1');
    click(findButton(target, 'Continue'));
    await vi.waitFor(() => expect(callsOf('auth_prompt_resolve')).toHaveLength(1));
  });

  it('cancels with the button and on Escape', async () => {
    const emit = await openDialog();
    emit('auth-prompt', kbd('a1'));
    click(findButton(target, 'Cancel'));
    await vi.waitFor(() => expect(callsOf('auth_prompt_resolve')).toHaveLength(1));
    expect(callsOf('auth_prompt_resolve')[0].args).toEqual({ id: 'a1', answers: null });

    emit('auth-prompt', kbd('a2'));
    press(dialog() as HTMLElement, 'Escape');
    await vi.waitFor(() => expect(callsOf('auth_prompt_resolve')).toHaveLength(2));
    expect(callsOf('auth_prompt_resolve')[1].args).toEqual({ id: 'a2', answers: null });
    await vi.waitFor(() => expect(dialog()).toBeNull());
  });

  it('does not answer on a click outside: only a button or Escape does', async () => {
    const emit = await openDialog();
    emit('auth-prompt', kbd('a1'));
    click(target.querySelector('.backdrop') as HTMLElement);
    expect(callsOf('auth_prompt_resolve')).toHaveLength(0);
    expect(dialog()).not.toBeNull();
  });

  it('moves the focus to the first field of the next question', async () => {
    const emit = await openDialog();
    emit('auth-prompt', kbd('a1'));
    emit('auth-prompt', kbd('a2'));
    await tick();
    click(findButton(target, 'Cancel'));
    await vi.waitFor(() => expect(callsOf('auth_prompt_resolve')).toHaveLength(1));
    await tick();
    expect(document.activeElement).toBe(inputs()[0]);
  });

  it('puts the focus on the first field and keeps it inside the dialog', async () => {
    const emit = await openDialog();
    emit('auth-prompt', kbd('a1'));
    await tick();
    expect(document.activeElement).toBe(inputs()[0]);

    // Tab from the last control wraps to the first one (the trap).
    const submit = findButton(target, 'Continue');
    submit.focus();
    press(submit, 'Tab');
    expect(dialog()?.contains(document.activeElement)).toBe(true);
    expect(document.activeElement).toBe(inputs()[0]);
  });

  it('queues a second question instead of replacing the first', async () => {
    const emit = await openDialog();
    emit('auth-prompt', kbd('a1', { name: 'First round' }));
    emit('auth-prompt', kbd('a2', { name: 'Second round' }));
    expect(target.querySelector('h2')?.textContent).toBe('First round');
    expect(target.textContent).toContain('1 more question waiting');

    type(inputs()[0], 'x');
    submitForm();
    await vi.waitFor(() => expect(target.querySelector('h2')?.textContent).toBe('Second round'));
    // The next question starts with empty fields, not with the old answers.
    expect(inputs().map((field) => field.value)).toEqual(['', '']);
    expect(target.textContent).not.toContain('more question');

    submitForm();
    await vi.waitFor(() => expect(callsOf('auth_prompt_resolve')).toHaveLength(2));
    expect(callsOf('auth_prompt_resolve').map((call) => (call.args as { id: string }).id)).toEqual([
      'a1',
      'a2',
    ]);
  });

  it('ignores the same question delivered twice', async () => {
    const emit = await openDialog();
    emit('auth-prompt', kbd('a1'));
    emit('auth-prompt', kbd('a1'));
    expect(target.textContent).not.toContain('more question');
  });

  it('drops a question the backend closed (timeout, closed tab, answered elsewhere)', async () => {
    const emit = await openDialog();
    emit('auth-prompt', kbd('a1'));
    emit('auth-prompt', kbd('a2', { name: 'Kept' }));
    emit('auth-prompt-closed', { id: 'a1' });
    expect(target.querySelector('h2')?.textContent).toBe('Kept');
    emit('auth-prompt-closed', { id: 'a2' });
    await vi.waitFor(() => expect(dialog()).toBeNull());
    expect(callsOf('auth_prompt_resolve')).toHaveLength(0);
  });

  it('shows server text as text, never as markup', async () => {
    const emit = await openDialog();
    emit(
      'auth-prompt',
      kbd('a1', { name: '<img src=x onerror=alert(1)>', instructions: '<b>bold</b>' }),
    );
    expect(target.querySelector('img')).toBeNull();
    expect(target.querySelector('b')).toBeNull();
    expect(target.textContent).toContain('<b>bold</b>');
  });

  it('tells the user when the answer could not be delivered', async () => {
    const emit = await openDialog();
    failures.set('auth_prompt_resolve', 'the backend went away');
    emit('auth-prompt', kbd('a1'));
    submitForm();
    await vi.waitFor(() => expect(notices.visible.length).toBe(1));
    expect(notices.visible[0].kind).toBe('error');
    expect(noticeText(notices.visible[0])).toContain('the backend went away');
  });

  it('says so when the question is no longer active', async () => {
    const emit = await openDialog();
    answers.set('auth_prompt_resolve', false);
    emit('auth-prompt', kbd('a1'));
    submitForm();
    await vi.waitFor(() => expect(notices.visible.length).toBe(1));
    expect(notices.visible[0].kind).toBe('info');
  });
});

describe('a passphrase question', () => {
  it('asks for the passphrase with a hidden field and offers to remember it', async () => {
    const emit = await openDialog();
    emit('auth-prompt', passphrase('p1'));
    expect(target.querySelector('h2')?.textContent).toBe('Passphrase required');
    expect(target.textContent).toContain('Passphrase for work key');
    expect(inputs()).toHaveLength(1);
    expect(inputs()[0].type).toBe('password');
    expect(target.querySelector('input[type="checkbox"]')).not.toBeNull();
    expect(findButton(target, 'Unlock')).toBeTruthy();
    expect(target.querySelector('[role="alert"]')).toBeNull();
  });

  it('says so when the previous passphrase was wrong', async () => {
    const emit = await openDialog();
    emit('auth-prompt', passphrase('p1', true));
    expect(target.querySelector('[role="alert"]')?.textContent).toContain('did not unlock');
  });

  it('does not offer to remember a passphrase of a key without an identity', async () => {
    const emit = await openDialog();
    emit('auth-prompt', passphrase('p1', false, false));
    expect(target.querySelector('input[type="checkbox"]')).toBeNull();
  });

  it('stores nothing unless it was asked to, and not before the session is ready', async () => {
    const emit = await openDialog();
    emit('auth-prompt', passphrase('p1'));
    type(inputs()[0], 's3cret');
    submitForm();
    await vi.waitFor(() => expect(callsOf('auth_prompt_resolve')).toHaveLength(1));
    expect(callsOf('auth_prompt_resolve')[0].args).toEqual({ id: 'p1', answers: ['s3cret'] });
    expect(callsOf('secret_set')).toHaveLength(0);
  });

  it('forgets the passphrase when the answer could not be delivered', async () => {
    const emit = await openDialog();
    const host = {
      id: 'host-1',
      auth: { type: 'private_key', identity_id: 'identity-1' },
    } as never;
    const tab = sessions.open(host, {}, false);
    const pane = sessions.activePaneOf(tab);
    if (!pane) throw new Error('the tab has no pane');
    pane.sessionId = 's1';

    answers.set('auth_prompt_resolve', false);
    emit('auth-prompt', passphrase('p1'));
    type(inputs()[0], 'typed');
    target.querySelector<HTMLInputElement>('input[type="checkbox"]')?.click();
    flushSync();
    submitForm();
    await vi.waitFor(() => expect(notices.visible.length).toBe(1));

    // The question was gone: the session may well succeed with some other
    // passphrase, and this one must not be stored for it.
    sessions.applyStatus(pane, { state: 'ready' });
    await tick();
    expect(callsOf('secret_set')).toHaveLength(0);
  });

  it('remembers the passphrase once the session is ready — not when it fails', async () => {
    const emit = await openDialog();
    answers.set('auth_prompt_resolve', true);
    const host = {
      id: 'host-1',
      name: 'web-1',
      address: 'web-1.test',
      port: 22,
      username: 'deploy',
      auth: { type: 'private_key', identity_id: 'identity-1' },
    } as never;

    // A pane whose session is s1 (the connection that asked).
    const tab = sessions.open(host, {}, false);
    const pane = sessions.activePaneOf(tab);
    if (!pane) throw new Error('the tab has no pane');
    pane.sessionId = 's1';

    emit('auth-prompt', passphrase('p1'));
    type(inputs()[0], 'wrong-guess');
    target.querySelector<HTMLInputElement>('input[type="checkbox"]')?.click();
    flushSync();
    submitForm();
    await vi.waitFor(() => expect(callsOf('auth_prompt_resolve')).toHaveLength(1));

    // The connection fails: the wrong passphrase must not reach the store.
    sessions.applyStatus(pane, { state: 'failed', category: 'auth', message: 'no' });
    await tick();
    expect(callsOf('secret_set')).toHaveLength(0);

    // A second try with the right one succeeds.
    emit('auth-prompt', passphrase('p2', true));
    type(inputs()[0], 'right-one');
    target.querySelector<HTMLInputElement>('input[type="checkbox"]')?.click();
    flushSync();
    submitForm();
    await vi.waitFor(() => expect(callsOf('auth_prompt_resolve')).toHaveLength(2));
    expect(callsOf('secret_set')).toHaveLength(0);

    sessions.applyStatus(pane, { state: 'ready' });
    await vi.waitFor(() => expect(callsOf('secret_set')).toHaveLength(1));
    expect(callsOf('secret_set')[0].args).toEqual({
      hostId: 'host-1',
      kind: 'passphrase',
      value: 'right-one',
    });
  });
});
