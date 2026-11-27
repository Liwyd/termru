// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { answers, callsOf, failures, resetTauriMock } from '../test-support/tauri-core-mock';
import { copyText, isOpenGesture, openLink, openableUrl } from './links';

vi.mock('@tauri-apps/api/core', () => import('../test-support/tauri-core-mock'));

let written: string[] = [];

beforeEach(() => {
  resetTauriMock();
  written = [];
  Object.defineProperty(navigator, 'clipboard', {
    configurable: true,
    value: { writeText: async (text: string) => void written.push(text) },
  });
});

afterEach(() => {
  vi.restoreAllMocks();
});

describe('terminal links', () => {
  it('opens web links only', () => {
    expect(openableUrl('https://example.com/a?b=1')).toBe('https://example.com/a?b=1');
    expect(openableUrl('http://192.0.2.10:3000/health')).toBe('http://192.0.2.10:3000/health');
    expect(openableUrl('  https://example.com  ')).toBe('https://example.com/');
    expect(openableUrl('javascript:alert(1)')).toBeNull();
    expect(openableUrl('file:///etc/passwd')).toBeNull();
    expect(openableUrl('data:text/html,<b>x</b>')).toBeNull();
    expect(openableUrl('ssh://host')).toBeNull();
    expect(openableUrl('not a url')).toBeNull();
    expect(openableUrl('')).toBeNull();
    expect(openableUrl(`https://example.com/${'a'.repeat(5000)}`)).toBeNull();
  });

  it('opens on Ctrl or Cmd click, not on a plain click', () => {
    expect(isOpenGesture({ ctrlKey: true, metaKey: false })).toBe(true);
    expect(isOpenGesture({ ctrlKey: false, metaKey: true })).toBe(true);
    expect(isOpenGesture({ ctrlKey: false, metaKey: false })).toBe(false);
  });

  it('hands the address to the system opener', async () => {
    answers.set('plugin:opener|open_url', null);
    expect(await openLink('https://example.com')).toBe('opened');
    expect(callsOf('plugin:opener|open_url')[0].args).toEqual({ url: 'https://example.com/' });
    expect(written).toEqual([]);
  });

  it('copies the address when the app has no opener', async () => {
    failures.set('plugin:opener|open_url', 'plugin opener not found');
    expect(await openLink('https://example.com')).toBe('copied');
    expect(written).toEqual(['https://example.com/']);
  });

  it('reports a failure when it can neither open nor copy', async () => {
    failures.set('plugin:opener|open_url', 'plugin opener not found');
    Object.defineProperty(navigator, 'clipboard', {
      configurable: true,
      value: {
        writeText: async () => {
          throw new Error('denied');
        },
      },
    });
    expect(await openLink('https://example.com')).toBe('failed');
    expect(await copyText('x')).toBe(false);
  });

  it('refuses a scheme that is not a web link without calling the backend', async () => {
    expect(await openLink('javascript:alert(1)')).toBe('refused');
    expect(callsOf('plugin:opener|open_url')).toHaveLength(0);
  });
});
