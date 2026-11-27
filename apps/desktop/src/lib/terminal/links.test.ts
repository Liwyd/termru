// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { copyText, isOpenGesture, openLink, openableUrl } from './links';

// The opener plugin, as the seam (`ipc-backend.ts`) imports it.
const opener = vi.hoisted(() => ({ openUrl: vi.fn<(url: string) => Promise<void>>() }));
vi.mock('@tauri-apps/plugin-opener', () => opener);

let written: string[] = [];

beforeEach(() => {
  opener.openUrl.mockReset().mockResolvedValue();
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
    expect(await openLink('https://example.com')).toBe('opened');
    expect(opener.openUrl).toHaveBeenCalledTimes(1);
    expect(opener.openUrl).toHaveBeenCalledWith('https://example.com/');
    expect(written).toEqual([]);
  });

  it('opens the normalized address, not the raw text', async () => {
    await openLink('  https://example.com/a b  ');
    expect(opener.openUrl).toHaveBeenCalledWith('https://example.com/a%20b');
  });

  it('copies the address when the opener fails', async () => {
    opener.openUrl.mockRejectedValue(new Error('not allowed by the capability'));
    expect(await openLink('https://example.com')).toBe('copied');
    expect(written).toEqual(['https://example.com/']);
  });

  it('reports a failure when it can neither open nor copy', async () => {
    opener.openUrl.mockRejectedValue(new Error('no browser'));
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

  it('refuses a scheme that is not a web link without calling the opener', async () => {
    for (const bad of ['javascript:alert(1)', 'file:///etc/passwd', 'ssh://host', 'not a url']) {
      expect(await openLink(bad)).toBe('refused');
    }
    expect(opener.openUrl).not.toHaveBeenCalled();
  });
});
