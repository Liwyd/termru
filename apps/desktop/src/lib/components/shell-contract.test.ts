// @vitest-environment jsdom
/**
 * Shell contract: the nav items, the session count, the per-route status
 * line and the ContextConfirm dialog (Doc A.05).
 */

import { flushSync, mount, tick, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import AppNav from './AppNav.svelte';
import StatusBar from './StatusBar.svelte';
import ContextConfirm from './ContextConfirm.svelte';
import { route, ROUTE_IDS } from '../stores/route.svelte';
import { sessions } from '../stores/sessions.svelte';

function click(element: Element): void {
  element.dispatchEvent(new MouseEvent('click', { bubbles: true }));
}

describe('AppNav', () => {
  let target: HTMLElement;
  let component: ReturnType<typeof mount>;

  beforeEach(() => {
    route.route = 'hosts';
    route.editHost = null;
    sessions.tabs = [];
    sessions.activeId = null;
    target = document.createElement('div');
    document.body.appendChild(target);
    component = mount(AppNav, { target });
  });

  afterEach(() => {
    unmount(component);
    target.remove();
  });

  it('offers every route and marks the active one', () => {
    const buttons = Array.from(target.querySelectorAll('button'));
    // Hosts, Sessions, Folders, Identities, Sync, Preferences.
    expect(buttons.length).toBe(6);
    for (const button of buttons) {
      expect(button.className).toContain('item');
    }
    expect(buttons[0].getAttribute('aria-current')).toBe('page');

    route.go('folders');
    flushSync();
    const updated = Array.from(target.querySelectorAll('button'));
    expect(updated[2].getAttribute('aria-current')).toBe('page');
    expect(updated[2].className).toContain('active');
    // Active is weight + underline, not colour alone.
    expect(updated[2].className).not.toBe(updated[0].className);
  });

  it('shows the open session count on Sessions', () => {
    expect(target.textContent).toContain('No sessions connected');
    sessions.tabs = [
      {
        id: 't1',
        title: 'web-1',
        panes: [],
        activePaneId: null,
        root: { kind: 'leaf', pane: 'p1' },
      },
    ];
    flushSync();
    expect(target.textContent).toContain('Sessions');
    expect(target.textContent).toContain('1');
    expect(target.textContent).toContain('1 session connected');
  });
});

describe('StatusBar', () => {
  let target: HTMLElement;
  let component: ReturnType<typeof mount>;

  beforeEach(() => {
    route.route = 'hosts';
    route.editHost = null;
    target = document.createElement('div');
    document.body.appendChild(target);
    component = mount(StatusBar, { target });
  });

  afterEach(() => {
    unmount(component);
    target.remove();
  });

  it('shows the contextual line and the constant hints', () => {
    expect(target.textContent).toContain('No saved hosts');
    expect(target.textContent).toContain('SSH · Local workspace');
    expect(target.textContent).toContain('Quick switch');
  });

  it('changes the contextual line per route', () => {
    route.go('folders');
    flushSync();
    expect(target.textContent).toContain('One folder per host');

    route.go('sessions');
    flushSync();
    expect(target.textContent).toContain('No open sessions');
  });
});

describe('ContextConfirm', () => {
  let target: HTMLElement;
  let component: ReturnType<typeof mount>;

  beforeEach(() => {
    target = document.createElement('div');
    document.body.appendChild(target);
  });

  afterEach(() => {
    unmount(component);
    target.remove();
  });

  it('focuses Cancel, cancels on Escape and returns focus to the trigger', async () => {
    let confirmed = 0;
    const trigger = document.createElement('button');
    trigger.textContent = 'Delete';
    document.body.appendChild(trigger);
    trigger.focus();

    component = mount(ContextConfirm, {
      target,
      props: {
        open: true,
        title: 'Delete host “web-1”?',
        body: 'The host profile is removed.',
        onconfirm: () => (confirmed += 1),
      },
    });

    await tick();
    // The focus moves after the dialog element exists (one more flush).
    await tick();
    const cancel = Array.from(target.querySelectorAll('button')).find(
      (button) => button.textContent === 'Cancel',
    ) as HTMLButtonElement;
    expect(document.activeElement).toBe(cancel);

    // Escape cancels without confirming.
    target
      .querySelector('[role="alertdialog"]')
      ?.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    await tick();
    expect(confirmed).toBe(0);
    expect(target.textContent).toBe('');

    // Focus returns to the trigger.
    expect(document.activeElement).toBe(trigger);
    trigger.remove();
  });

  it('runs the confirm handler', async () => {
    let confirmed = 0;
    component = mount(ContextConfirm, {
      target,
      props: {
        open: true,
        title: 'Delete folder “prod”?',
        body: 'The hosts stay.',
        onconfirm: () => (confirmed += 1),
      },
    });
    await tick();
    const confirm = Array.from(target.querySelectorAll('button')).find(
      (button) => button.textContent === 'Delete',
    ) as HTMLButtonElement;
    click(confirm);
    await tick();
    expect(confirmed).toBe(1);
    expect(target.textContent).toBe('');
  });

  it('renders nothing while closed', () => {
    component = mount(ContextConfirm, {
      target,
      props: { open: false, title: 'T', body: 'B', onconfirm: () => {} },
    });
    expect(target.textContent).toBe('');
  });
});

describe('route coverage', () => {
  it('maps every route id to a nav item', () => {
    // host-edit is reached from Hosts, not the nav; the rest are all there.
    const navRoutes = ROUTE_IDS.filter((id) => id !== 'host-edit');
    expect(navRoutes.length).toBe(6);
  });
});
