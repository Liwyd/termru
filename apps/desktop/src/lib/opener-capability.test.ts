import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

/**
 * The opener plugin is the only way the webview reaches the system browser.
 * Its capability is the fence: web and mail addresses only, no file paths, no
 * "reveal in folder", no choice of application. These tests read the files
 * that define it, so a widened scope fails here before it ships.
 */

const ROOT = new URL('../../', import.meta.url);
const read = (path: string) => readFileSync(new URL(path, ROOT), 'utf8');

interface Entry {
  identifier: string;
  allow?: { url?: string; path?: string }[];
  deny?: unknown[];
}

const capability = JSON.parse(read('src-tauri/capabilities/default.json')) as {
  windows: string[];
  permissions: (string | Entry)[];
};

const openerEntries = capability.permissions.filter((permission) =>
  (typeof permission === 'string' ? permission : permission.identifier).startsWith('opener:'),
);

describe('opener capability', () => {
  it('grants open-url, and nothing else of the plugin', () => {
    const names = openerEntries.map((entry) =>
      typeof entry === 'string' ? entry : entry.identifier,
    );
    expect(names).toEqual(['opener:allow-open-url']);
    // No default set (it also reveals files in the file manager), no open-path.
    expect(names).not.toContain('opener:default');
    expect(names.some((name) => /path|reveal|default-urls/.test(name))).toBe(false);
  });

  it('scopes open-url to http, https and mailto addresses', () => {
    const entry = openerEntries[0] as Entry;
    expect(entry.allow).toBeDefined();
    const urls = (entry.allow ?? []).map((scope) => scope.url);
    expect([...urls].sort()).toEqual(['http://*', 'https://*', 'mailto:*']);
    // Every scope is a URL pattern: no filesystem path rule, no wildcard on its own.
    expect((entry.allow ?? []).every((scope) => scope.path === undefined)).toBe(true);
    expect(urls.every((url) => /^(https?|mailto):/.test(url ?? ''))).toBe(true);
    expect(urls).not.toContain('*');
  });

  it('allows no scheme that opens a program or a file', () => {
    const entry = openerEntries[0] as Entry;
    const urls = (entry.allow ?? []).map((scope) => scope.url ?? '');
    for (const scheme of ['file:', 'javascript:', 'data:', 'ssh:', 'tel:', 'smb:', 'vscode:']) {
      expect(urls.some((url) => url.startsWith(scheme))).toBe(false);
    }
  });

  it('applies to the windows that show terminals, and to no others', () => {
    expect(capability.windows).toEqual(['main', 'session-*']);
  });

  it('is registered in the app and installed in both halves', () => {
    expect(read('src-tauri/Cargo.toml')).toMatch(/^tauri-plugin-opener\s*=\s*"2\./m);
    expect(read('src-tauri/src/lib.rs')).toContain('tauri_plugin_opener::init()');
    const pkg = JSON.parse(read('package.json')) as { dependencies: Record<string, string> };
    expect(pkg.dependencies['@tauri-apps/plugin-opener']).toMatch(/^2\./);
  });

  it('is reached through the seam only', () => {
    const seam = read('src/lib/ipc-backend.ts');
    expect(seam).toContain("from '@tauri-apps/plugin-opener'");
    const links = read('src/lib/terminal/links.ts');
    expect(links).toContain("from '../ipc-backend'");
    expect(links).not.toContain('plugin-opener');
    expect(links).not.toContain('plugin:opener');
  });
});
