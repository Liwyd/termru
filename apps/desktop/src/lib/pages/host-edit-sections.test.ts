// @vitest-environment jsdom
/**
 * The sections of the host editor that data model v2 adds (ADR 0010): jump
 * hosts, port forwards, and what Advanced gained (environment variables, the
 * startup command, agent forwarding, the authentication order). Each is
 * exercised as a user would: add, reorder, remove, leave a field, save.
 */

import { flushSync, mount, tick, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import HostEditPage from './HostEditPage.svelte';
import { hosts } from '../stores/hosts.svelte';
import { execApproval } from '../stores/exec-approval.svelte';
import { identities } from '../stores/identities.svelte';
import { route } from '../stores/route.svelte';
import { notices } from '../notify.svelte';
import { unsaved } from '../stores/unsaved.svelte';
import { click, findButton, newTarget, stubBrowserApis, type } from '../test-support/dom';
import { answers, callsOf, failures, resetTauriMock } from '../test-support/tauri-core-mock';
import type { Host } from '../types';

vi.mock('@tauri-apps/api/core', () => import('../test-support/tauri-core-mock'));
vi.mock('@tauri-apps/api/event', () => ({ listen: async () => () => {} }));
vi.mock('@tauri-apps/api/window', () => ({
  getCurrentWindow: () => ({
    minimize: async () => {},
    toggleMaximize: async () => {},
    close: async () => {},
    onCloseRequested: async () => () => {},
  }),
}));

const STAMP = '2026-01-01T00:00:00Z';

function host(id: string, name: string, over: Partial<Host> = {}): Host {
  return {
    id,
    name,
    address: `${name}.example.test`,
    port: 22,
    username: 'ops',
    auth: { type: 'agent' },
    group_id: null,
    tags: [],
    favorite: false,
    description: null,
    term: 'xterm-256color',
    keepalive_seconds: 0,
    sort_order: 0,
    created_at: STAMP,
    updated_at: STAMP,
    deleted: false,
    ...over,
  };
}

const APP = host('app', 'app');
const BASTION = host('bastion', 'bastion', { port: 2222 });
const GATEWAY = host('gateway', 'gateway');
const GONE = host('gone', 'old-gw', { deleted: true });

let target: HTMLElement;
let component: ReturnType<typeof mount> | null = null;

function open(edited: Host | null): void {
  route.edit(edited);
  component = mount(HostEditPage, { target });
  flushSync();
}

function el<T extends HTMLElement = HTMLInputElement>(id: string): T {
  const found = document.getElementById(id);
  if (!found) throw new Error(`no element #${id}`);
  return found as T;
}

function blur(element: HTMLElement): void {
  element.dispatchEvent(new FocusEvent('blur'));
  element.dispatchEvent(new FocusEvent('focusout', { bubbles: true }));
  flushSync();
}

function message(id: string): string {
  return document.getElementById(`${id}-message`)?.textContent?.trim() ?? '';
}

/** The text of the section with this heading id. */
function section(id: string): HTMLElement {
  const found = target.querySelector<HTMLElement>(`section[aria-labelledby="${id}"]`);
  if (!found) throw new Error(`no section ${id}`);
  return found;
}

/** The text of an element with its whitespace collapsed. */
function text(element: Element): string {
  return element.textContent?.replace(/\s+/g, ' ').trim() ?? '';
}

function pick(select: HTMLSelectElement, value: string): void {
  type(select, value);
}

async function save(): Promise<Host> {
  click(findButton(target, 'Save host'));
  await vi.waitFor(() => expect(callsOf('host_save')).toHaveLength(1));
  return callsOf('host_save')[0].args?.host as Host;
}

beforeEach(() => {
  resetTauriMock();
  stubBrowserApis();
  hosts.hosts = [APP, BASTION, GATEWAY, GONE];
  hosts.groups = [];
  hosts.identities = [];
  hosts.tags = [];
  hosts.saveError = null;
  identities.bindings = {};
  identities.keys = {};
  unsaved.question = null;
  execApproval.close();
  execApproval.current = null;
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

describe('04 / jump hosts', () => {
  it('is empty at first and says what that means', () => {
    open(APP);
    const jump = section('sec-jump');
    expect(jump.querySelector('ol')).toBeNull();
    expect(jump.textContent).toContain('Without one, Termru connects straight to the address.');
    expect(jump.textContent).not.toContain('Route');
  });

  it('offers the other live hosts, never the host itself or a deleted one', () => {
    open(APP);
    const options = Array.from(el<HTMLSelectElement>('host-jump-add').options).map(
      (option) => option.textContent?.replace(/\s+/g, ' ').trim() ?? '',
    );
    expect(options[0]).toBe('Choose a host…');
    expect(options.some((text) => text.startsWith('bastion · ops@bastion.example.test:2222'))).toBe(
      true,
    );
    expect(options.some((text) => text.startsWith('gateway'))).toBe(true);
    expect(options.some((text) => text.startsWith('app '))).toBe(false);
    expect(options.some((text) => text.startsWith('old-gw'))).toBe(false);
  });

  it('adds a hop from the picker and shows the route it makes', () => {
    open(APP);
    pick(el<HTMLSelectElement>('host-jump-add'), 'bastion');
    const items = section('sec-jump').querySelectorAll('ol li');
    expect(items).toHaveLength(1);
    expect(items[0].textContent).toContain('bastion');
    expect(items[0].textContent).toContain('ops@bastion.example.test:2222');
    expect(section('sec-jump').textContent).toContain('you → bastion → app');
    // The host is on the route now, so the picker no longer offers it.
    const offered = Array.from(el<HTMLSelectElement>('host-jump-add').options).map((o) => o.value);
    expect(offered).not.toContain('bastion');
    expect(el<HTMLSelectElement>('host-jump-add').value).toBe('');
  });

  it('puts the hops in order, moves them and removes them', () => {
    open(APP);
    pick(el<HTMLSelectElement>('host-jump-add'), 'bastion');
    pick(el<HTMLSelectElement>('host-jump-add'), 'gateway');
    const names = () =>
      Array.from(section('sec-jump').querySelectorAll('ol li .name')).map((n) => n.textContent);
    expect(names()).toEqual(['bastion', 'gateway']);
    expect(section('sec-jump').textContent).toContain('you → bastion → gateway → app');

    // The first hop cannot go earlier (and says why), the last cannot go later.
    const first = section('sec-jump').querySelector<HTMLElement>(
      'li [aria-label="Move bastion earlier"]',
    );
    expect(first?.getAttribute('aria-disabled')).toBe('true');
    expect(first?.getAttribute('title')).toBe('It is the first hop');

    click(
      section('sec-jump').querySelector<HTMLElement>(
        '[aria-label="Move gateway earlier"]',
      ) as HTMLElement,
    );
    expect(names()).toEqual(['gateway', 'bastion']);
    expect(section('sec-jump').textContent).toContain('you → gateway → bastion → app');

    click(
      section('sec-jump').querySelector<HTMLElement>(
        '[aria-label="Remove gateway from the route"]',
      ) as HTMLElement,
    );
    expect(names()).toEqual(['bastion']);
    expect(section('sec-jump').textContent).toContain('you → bastion → app');
  });

  it("includes the first hop's own jump host in the route and says why", () => {
    hosts.hosts = [APP, host('bastion', 'bastion', { jump_chain: ['gateway'] }), GATEWAY];
    open(APP);
    pick(el<HTMLSelectElement>('host-jump-add'), 'bastion');
    expect(section('sec-jump').textContent).toContain('you → gateway → bastion → app');
    expect(text(section('sec-jump'))).toContain(
      'gateway is on the route too: the first jump host is reached through it.',
    );
  });

  it('refuses a route that is a cycle, next to the picker and wired to it', () => {
    hosts.hosts = [APP, host('bastion', 'bastion', { jump_chain: ['app'] })];
    open(APP);
    pick(el<HTMLSelectElement>('host-jump-add'), 'bastion');
    expect(message('host-jump-add')).toMatch(/is a cycle/);
    const select = el<HTMLSelectElement>('host-jump-add');
    expect(select.getAttribute('aria-invalid')).toBe('true');
    expect(select.getAttribute('aria-describedby')).toBe('host-jump-add-message');
    expect(section('sec-jump').textContent).not.toContain('you →');
  });

  it('names a hop that was deleted or has not arrived yet', () => {
    open(host('app', 'app', { jump_chain: ['vanished'] }));
    const row = section('sec-jump').querySelector('ol li');
    expect(row?.textContent).toContain('Missing host');
    expect(row?.textContent).toContain('It was deleted or has not synced yet');
    expect(message('host-jump-add')).toMatch(/does not exist/);
    // Removing it clears the problem.
    click(section('sec-jump').querySelector<HTMLElement>('[aria-label^="Remove"]') as HTMLElement);
    expect(message('host-jump-add')).not.toMatch(/does not exist/);
  });

  it('stops at 8 hops', () => {
    const library = Array.from({ length: 10 }, (_, index) => host(`h${index}`, `hop${index}`));
    hosts.hosts = [APP, ...library];
    open(host('app', 'app', { jump_chain: library.slice(0, 8).map((entry) => entry.id) }));
    const select = el<HTMLSelectElement>('host-jump-add');
    expect(select.disabled).toBe(true);
    expect(select.options[0].textContent?.trim()).toBe('A route has at most 8 hops');
  });

  it('saves the route, and the save approves this device (the user is looking at it)', async () => {
    open(APP);
    pick(el<HTMLSelectElement>('host-jump-add'), 'bastion');
    const saved = await save();
    expect(saved.jump_chain).toEqual(['bastion']);
    expect(callsOf('host_save')[0].args?.approveExec).toBe(true);
  });

  it('shows what the backend refuses at save next to the picker, not in the footer', async () => {
    failures.set('host_save', 'a jump host of app does not exist (was it deleted?)');
    open(APP);
    pick(el<HTMLSelectElement>('host-jump-add'), 'bastion');
    click(findButton(target, 'Save host'));
    await vi.waitFor(() => expect(callsOf('host_save')).toHaveLength(1));
    await vi.waitFor(() =>
      expect(message('host-jump-add')).toBe('a jump host of app does not exist (was it deleted?)'),
    );
    // The refusal is by the field; the footer has nothing to add to it.
    expect(target.querySelector('.footer .message')?.textContent?.trim()).toBe('');
  });
});

describe('05 / port forwards', () => {
  function addForward(): void {
    click(el<HTMLButtonElement>('host-forwards-add'));
  }

  const rows = () => section('sec-forwards').querySelectorAll('[role="group"][aria-labelledby]');

  it('starts with none and adds a row with the focus on its type', async () => {
    open(APP);
    expect(section('sec-forwards').textContent).toContain('No port forwards.');
    addForward();
    await tick();
    expect(rows()).toHaveLength(1);
    expect(section('sec-forwards').textContent).not.toContain('No port forwards.');
    await vi.waitFor(() => expect(document.activeElement?.id).toMatch(/^host-fwd-\d+-kind$/));
  });

  it('asks for the target of a local forward and not of a SOCKS one', () => {
    open(APP);
    addForward();
    const group = rows()[0] as HTMLElement;
    expect(group.textContent).toContain('Target host');
    expect(group.textContent).toContain('Target port');
    const kind = group.querySelector<HTMLSelectElement>('select') as HTMLSelectElement;
    expect(Array.from(kind.options).map((option) => option.textContent)).toEqual([
      'Local (-L)',
      'Remote (-R)',
      'Dynamic (-D)',
    ]);
    type(kind, 'dynamic');
    expect(group.textContent).not.toContain('Target host');
    expect(group.textContent).toContain('A SOCKS5 proxy');
    type(kind, 'remote');
    expect(group.textContent).toContain('Reached from this machine.');
  });

  it('checks a row when it is left, in the backend’s terms, wired to its field', () => {
    open(APP);
    addForward();
    const group = rows()[0] as HTMLElement;
    const port = group.querySelector<HTMLInputElement>(
      'input[placeholder="8080"]',
    ) as HTMLInputElement;
    expect(port.getAttribute('aria-invalid')).toBeNull();
    blur(port);
    expect(port.getAttribute('aria-invalid')).toBe('true');
    expect(message(port.id)).toBe('Bind port is required.');
    expect(port.getAttribute('aria-describedby')).toBe(`${port.id}-message`);
    type(port, '99999');
    expect(message(port.id)).toMatch(/1 to 65535/);
    type(port, '8080');
    expect(port.getAttribute('aria-invalid')).toBeNull();
    const host_ = group.querySelector<HTMLInputElement>(
      'input[placeholder="db.internal"]',
    ) as HTMLInputElement;
    type(host_, 'bad host');
    blur(host_);
    expect(message(host_.id)).toMatch(/letters, digits/);
  });

  it('writes the forward as it will run and warns about an address others can reach', () => {
    open(APP);
    addForward();
    const group = rows()[0] as HTMLElement;
    const inputs = group.querySelectorAll<HTMLInputElement>('input[type="text"]');
    const [bind, bindPort, targetHost, targetPort] = Array.from(inputs);
    type(bindPort, '8080');
    type(targetHost, 'db.internal');
    type(targetPort, '5432');
    expect(group.querySelector('code')?.textContent).toBe('127.0.0.1:8080 → db.internal:5432');
    expect(group.textContent).not.toContain('Not loopback');
    type(bind, '0.0.0.0');
    expect(group.querySelector('code')?.textContent).toBe('0.0.0.0:8080 → db.internal:5432');
    expect(group.textContent).toContain('Not loopback: other machines on the network can use');
    type(bind, 'localhost');
    expect(group.textContent).not.toContain('Not loopback');
    // The command preview in the rail follows.
    expect(target.querySelector('.command')?.textContent).toBe(
      'ssh -L localhost:8080:db.internal:5432 ops@app.example.test',
    );
  });

  it('turns a forward off without losing it, and removes one', () => {
    open(
      host('app', 'app', {
        forwards: [
          { kind: 'dynamic', bind_port: 1080, enabled: true },
          { kind: 'dynamic', bind_port: 1081, enabled: true },
        ],
      }),
    );
    expect(rows()).toHaveLength(2);
    const toggle = rows()[0].querySelector<HTMLButtonElement>(
      '[role="switch"]',
    ) as HTMLButtonElement;
    expect(toggle.getAttribute('aria-checked')).toBe('true');
    click(toggle);
    expect(toggle.getAttribute('aria-checked')).toBe('false');
    expect(rows()[0].classList.contains('off')).toBe(true);
    // The rail leaves a forward that is off out of the command.
    expect(target.querySelector('.command')?.textContent).toBe('ssh -D 1081 ops@app.example.test');
    click(rows()[0].querySelector<HTMLElement>('[aria-label="Remove forward 1"]') as HTMLElement);
    expect(rows()).toHaveLength(1);
    expect(rows()[0].textContent).toContain('1081');
  });

  it('refuses two forwards on the same listener, on the later row', () => {
    open(APP);
    for (let index = 0; index < 2; index += 1) {
      addForward();
      const group = rows()[index] as HTMLElement;
      type(group.querySelector('select') as HTMLSelectElement, 'dynamic');
      type(
        group.querySelector<HTMLInputElement>('input[placeholder="8080"]') as HTMLInputElement,
        '1080',
      );
    }
    const second = rows()[1].querySelector<HTMLInputElement>(
      'input[placeholder="8080"]',
    ) as HTMLInputElement;
    blur(second);
    expect(message(second.id)).toBe('Another forward already listens on 127.0.0.1:1080.');
    const first = rows()[0].querySelector<HTMLInputElement>(
      'input[placeholder="8080"]',
    ) as HTMLInputElement;
    expect(first.getAttribute('aria-invalid')).toBeNull();
  });

  it('will not save a broken row: it counts as a field, and the focus goes to it', async () => {
    open(APP);
    addForward();
    click(findButton(target, 'Save host'));
    await tick();
    expect(callsOf('host_save')).toHaveLength(0);
    expect(target.querySelector('.footer .message')?.textContent?.trim()).toBe(
      'Fix 1 field before saving.',
    );
    const first = rows()[0].querySelector<HTMLInputElement>('input[aria-invalid="true"]');
    expect(first).not.toBeNull();
    await vi.waitFor(() => expect(document.activeElement).toBe(first));
  });

  it('saves the forwards as the backend takes them', async () => {
    open(APP);
    addForward();
    const group = rows()[0] as HTMLElement;
    const inputs = Array.from(group.querySelectorAll<HTMLInputElement>('input[type="text"]'));
    type(inputs[1], '5433');
    type(inputs[2], 'db.internal');
    type(inputs[3], '5432');
    addForward();
    const second = rows()[1] as HTMLElement;
    type(second.querySelector('select') as HTMLSelectElement, 'dynamic');
    type(
      second.querySelector<HTMLInputElement>('input[placeholder="8080"]') as HTMLInputElement,
      '1080',
    );
    const saved = await save();
    expect(saved.forwards).toEqual([
      {
        kind: 'local',
        bind_port: 5433,
        target_host: 'db.internal',
        target_port: 5432,
        enabled: true,
      },
      { kind: 'dynamic', bind_port: 1080, enabled: true },
    ]);
  });

  it('stops at 32 forwards and says why', () => {
    open(
      host('app', 'app', {
        forwards: Array.from({ length: 32 }, (_, index) => ({
          kind: 'dynamic' as const,
          bind_port: 2000 + index,
          enabled: true,
        })),
      }),
    );
    const add = el<HTMLButtonElement>('host-forwards-add');
    expect(add.getAttribute('aria-disabled')).toBe('true');
    expect(add.getAttribute('title')).toBe('A host has at most 32 port forwards');
    click(add);
    expect(rows()).toHaveLength(32);
  });
});

describe('06 / advanced', () => {
  const advanced = () => section('sec-advanced');

  describe('environment variables', () => {
    it('adds a variable, checks it and sends it', async () => {
      open(APP);
      click(el<HTMLButtonElement>('host-env-add'));
      await vi.waitFor(() =>
        expect(document.activeElement?.getAttribute('aria-label')).toBe('Name of variable 1'),
      );
      const name = document.activeElement as HTMLInputElement;
      type(name, '1BAD');
      blur(name);
      expect(name.getAttribute('aria-invalid')).toBe('true');
      expect(
        document.getElementById(name.getAttribute('aria-describedby') ?? '')?.textContent,
      ).toMatch(/not starting with a digit/);
      type(name, 'LANG');
      expect(name.getAttribute('aria-invalid')).toBeNull();
      const value = advanced().querySelector<HTMLInputElement>(
        '[aria-label="Value of variable 1"]',
      );
      type(value as HTMLInputElement, 'C.UTF-8');
      const saved = await save();
      expect(saved.env).toEqual([{ name: 'LANG', value: 'C.UTF-8' }]);
    });

    it('refuses a name twice and removes a row', () => {
      open(host('app', 'app', { env: [{ name: 'A', value: '1' }] }));
      click(el<HTMLButtonElement>('host-env-add'));
      const second = advanced().querySelector<HTMLInputElement>(
        '[aria-label="Name of variable 2"]',
      );
      type(second as HTMLInputElement, 'A');
      blur(second as HTMLInputElement);
      expect(second?.getAttribute('aria-invalid')).toBe('true');
      click(
        advanced().querySelector<HTMLElement>('[aria-label="Remove variable 2"]') as HTMLElement,
      );
      expect(advanced().querySelector('[aria-label="Name of variable 2"]')).toBeNull();
    });

    it('says the values are synced in plain text', () => {
      open(APP);
      expect(advanced().textContent).toContain('synced in plain text');
    });
  });

  describe('startup command', () => {
    it('takes one line, trims it and sends it, saying others must approve it', async () => {
      open(APP);
      expect(advanced().textContent).toContain(
        'Other devices ask for your approval before it runs.',
      );
      type(el('host-startup'), '  tmux new -A -s main ');
      const saved = await save();
      expect(saved.startup_command).toBe('tmux new -A -s main');
    });

    it('refuses a control character', () => {
      open(APP);
      type(el('host-startup'), 'a\u0007b');
      blur(el('host-startup'));
      expect(el('host-startup').getAttribute('aria-invalid')).toBe('true');
      expect(message('host-startup')).toMatch(/single line/);
    });

    it('refuses one that is too long', () => {
      open(APP);
      type(el('host-startup'), 'x'.repeat(1025));
      blur(el('host-startup'));
      expect(message('host-startup')).toMatch(/at most 1024/);
    });
  });

  describe('agent forwarding', () => {
    it('is off, with the risk said in a line, and is sent when it is turned on', async () => {
      open(APP);
      const box = advanced().querySelector<HTMLInputElement>(
        'input[type="checkbox"]',
      ) as HTMLInputElement;
      expect(box.checked).toBe(false);
      expect(advanced().textContent).toContain("can use your agent's keys");
      expect(advanced().textContent).toContain('Turn it on only for servers you trust.');
      click(box);
      expect(box.checked).toBe(true);
      expect(target.querySelector('.command')?.textContent).toBe('ssh -A ops@app.example.test');
      const saved = await save();
      expect(saved.agent_forward).toBe(true);
    });
  });

  describe('authentication order', () => {
    const radio = (value: string) =>
      advanced().querySelector<HTMLInputElement>(
        `input[name="auth-order"][value="${value}"]`,
      ) as HTMLInputElement;
    const order = () =>
      Array.from(advanced().querySelectorAll('.order-list li .label')).map((n) => n.textContent);

    it('is Automatic, and a custom order starts from the method of the host', () => {
      open(APP);
      expect(radio('automatic').checked).toBe(true);
      expect(advanced().querySelector('.order-list')).toBeNull();
      click(radio('custom'));
      expect(radio('custom').checked).toBe(true);
      expect(order()).toEqual(['SSH agent', 'Keyboard-interactive']);
    });

    it('reorders, removes and adds methods, then sends the order', async () => {
      open(APP);
      click(radio('custom'));
      click(
        advanced().querySelector<HTMLElement>(
          '[aria-label="Try Keyboard-interactive earlier"]',
        ) as HTMLElement,
      );
      expect(order()).toEqual(['Keyboard-interactive', 'SSH agent']);
      pick(el<HTMLSelectElement>('host-auth-add'), 'password');
      expect(order()).toEqual(['Keyboard-interactive', 'SSH agent', 'Password']);
      click(
        advanced().querySelector<HTMLElement>(
          '[aria-label="Remove SSH agent from the order"]',
        ) as HTMLElement,
      );
      expect(order()).toEqual(['Keyboard-interactive', 'Password']);
      const saved = await save();
      expect(saved.auth_order).toEqual(['keyboard_interactive', 'password']);
    });

    it('does not let the list be empty', async () => {
      open(host('app', 'app', { auth_order: ['agent'] }));
      expect(radio('custom').checked).toBe(true);
      click(
        advanced().querySelector<HTMLElement>(
          '[aria-label="Remove SSH agent from the order"]',
        ) as HTMLElement,
      );
      expect(advanced().textContent).toContain('Name at least one method, or choose Automatic.');
      click(findButton(target, 'Save host'));
      await tick();
      expect(callsOf('host_save')).toHaveLength(0);
      click(radio('automatic'));
      expect(advanced().textContent).not.toContain('Name at least one method');
    });

    it('goes back to the default when Automatic is chosen again', async () => {
      open(host('app', 'app', { auth_order: ['password', 'agent'] }));
      click(radio('automatic'));
      const saved = await save();
      expect(saved.auth_order).toBeNull();
    });
  });
});

describe('a host whose synced settings this device has not approved', () => {
  const synced = host('app', 'app', {
    startup_command: 'tmux attach',
    forwards: [
      {
        kind: 'local',
        bind_address: '0.0.0.0',
        bind_port: 9090,
        target_host: 'db',
        target_port: 5432,
        enabled: true,
      },
    ],
  });

  it('says so in the rail, and the review opens from there', async () => {
    answers.set('host_exec_review', {
      host_id: 'app',
      host_name: 'app',
      hash: 'h',
      pending: true,
      fields: { startup_command: 'tmux attach', agent_forward: false, env: [], forwards: [] },
    });
    open(synced);
    await vi.waitFor(() =>
      expect(target.querySelector('.approval')?.textContent).toContain(
        'Not approved on this device',
      ),
    );
    expect(target.querySelector('.approval')?.textContent).toContain(
      'Saving approves exactly what this page shows.',
    );
    click(findButton(target, 'Review them…'));
    await vi.waitFor(() => expect(execApproval.open).toBe(true));
    expect(callsOf('host_exec_review').at(-1)?.args).toEqual({ id: 'app' });
  });

  it('approves them on Save even when nothing was edited, with the hash the review gave', async () => {
    answers.set('host_exec_review', {
      host_id: 'app',
      host_name: 'app',
      hash: 'hash-shown',
      pending: true,
      fields: { startup_command: 'tmux attach', agent_forward: false, env: [], forwards: [] },
    });
    answers.set('host_exec_approve', null);
    open(synced);
    await vi.waitFor(() => expect(target.querySelector('.approval')).not.toBeNull());
    click(findButton(target, 'Save host'));
    await vi.waitFor(() => expect(callsOf('host_exec_approve')).toHaveLength(1));
    expect(callsOf('host_exec_approve')[0].args).toEqual({ id: 'app', hash: 'hash-shown' });
    // Nothing was edited, so the host itself is not written (and not synced again).
    expect(callsOf('host_save')).toHaveLength(0);
    await vi.waitFor(() => expect(route.route).toBe('hosts'));
  });

  it('stays on the page and says why when the settings changed under the review', async () => {
    answers.set('host_exec_review', {
      host_id: 'app',
      host_name: 'app',
      hash: 'old',
      pending: true,
      fields: { startup_command: 'tmux attach', agent_forward: false, env: [], forwards: [] },
    });
    failures.set(
      'host_exec_approve',
      'The synced settings of this host changed after you opened the review. Review them again.',
    );
    route.route = 'host-edit';
    open(synced);
    await vi.waitFor(() => expect(target.querySelector('.approval')).not.toBeNull());
    click(findButton(target, 'Save host'));
    await vi.waitFor(() =>
      expect(target.querySelector('.footer .message')?.textContent).toContain(
        'changed after you opened',
      ),
    );
    expect(route.route).toBe('host-edit');
  });

  it('says nothing once approved, and asks nothing for a plain host', async () => {
    answers.set('host_exec_review', {
      host_id: 'app',
      host_name: 'app',
      hash: 'h',
      pending: false,
      fields: { startup_command: 'tmux attach', agent_forward: false, env: [], forwards: [] },
    });
    open(synced);
    await vi.waitFor(() => expect(callsOf('host_exec_review')).toHaveLength(1));
    await tick();
    expect(target.querySelector('.approval')).toBeNull();
    unmount(component as ReturnType<typeof mount>);
    component = null;
    resetTauriMock();
    open(APP);
    await tick();
    expect(callsOf('host_exec_review')).toHaveLength(0);
  });
});

describe('the command preview', () => {
  it('shows the whole route and the forwards', () => {
    hosts.hosts = [APP, BASTION];
    open(
      host('app', 'app', {
        jump_chain: ['bastion'],
        forwards: [
          {
            kind: 'local',
            bind_address: '127.0.0.1',
            bind_port: 5433,
            target_host: 'db',
            target_port: 5432,
            enabled: true,
          },
          { kind: 'dynamic', bind_port: 1080, enabled: true },
        ],
        agent_forward: true,
        port: 2200,
      }),
    );
    expect(target.querySelector('.command')?.textContent).toBe(
      'ssh -J ops@bastion.example.test:2222 -L 127.0.0.1:5433:db:5432 -D 1080 -A ops@app.example.test -p 2200',
    );
  });
});
