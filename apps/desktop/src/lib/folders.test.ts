import { describe, expect, it } from 'vitest';
import {
  MAX_GROUP_DEPTH,
  buildFolderTree,
  canMoveUnder,
  deleteImpact,
  deleteSentence,
  folderPath,
  parentOptions,
  subtreeIds,
} from './folders';
import type { Group, Host } from './types';

function group(id: string, name: string, parent: string | null = null, sort = 0): Group {
  return {
    id,
    name,
    parent_id: parent,
    sort_order: sort,
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
    deleted: false,
  };
}

function host(id: string, groupId: string | null): Host {
  return {
    id,
    name: id,
    address: `${id}.test`,
    port: 22,
    username: 'ops',
    auth: { type: 'agent' },
    group_id: groupId,
    tags: [],
    favorite: false,
    description: null,
    term: 'xterm-256color',
    keepalive_seconds: 0,
    sort_order: 0,
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
    deleted: false,
  };
}

// The folders of frame 5: Production (Europe, Edge), Sandbox; five hosts.
const groups = [
  group('prod', 'Production', null, 0),
  group('eu', 'Europe', 'prod', 1),
  group('edge', 'Edge', 'prod', 2),
  group('sand', 'Sandbox', null, 3),
];
const hosts = [
  host('atlas', 'eu'),
  host('relay', 'edge'),
  host('staging', 'sand'),
  host('build', 'sand'),
  host('archive', null),
];

describe('the folder tree', () => {
  it('orders parents before their children and counts the hosts under each folder', () => {
    const tree = buildFolderTree(groups, hosts);
    expect(tree.nodes.map((node) => [node.group.name, node.depth, node.count])).toEqual([
      ['Production', 0, 2],
      ['Europe', 1, 1],
      ['Edge', 1, 1],
      ['Sandbox', 0, 2],
    ]);
    expect(tree.total).toBe(5);
    expect(tree.unfiled).toBe(1);
  });

  it('keeps the direct count apart from the one that includes the subfolders', () => {
    const production = buildFolderTree(groups, hosts).nodes[0];
    expect(production.directCount).toBe(0);
    expect(production.count).toBe(2);
    expect(production.childCount).toBe(2);
    expect(production.path).toBe('Production');
    expect(buildFolderTree(groups, hosts).nodes[1].path).toBe('Production / Europe');
  });

  it('treats a host whose folder is gone as unfiled', () => {
    const tree = buildFolderTree(groups, [host('lost', 'deleted-folder')]);
    expect(tree.unfiled).toBe(1);
  });

  it('shows a folder whose parent is gone at the top level', () => {
    const tree = buildFolderTree([group('orphan', 'Orphan', 'missing')], []);
    expect(tree.nodes.map((node) => [node.group.name, node.depth])).toEqual([['Orphan', 0]]);
  });

  it('survives a cycle in the data: every folder is shown once', () => {
    const cyclic = [group('a', 'A', 'b'), group('b', 'B', 'a')];
    const tree = buildFolderTree(cyclic, []);
    expect(tree.nodes.map((node) => node.group.id).sort()).toEqual(['a', 'b']);
    expect(folderPath(cyclic, 'a')).not.toBe('');
  });

  it('sorts siblings by position, then by name', () => {
    const tree = buildFolderTree(
      [group('b', 'Beta', null, 0), group('a', 'Alpha', null, 0), group('z', 'Zed', null, -1)],
      [],
    );
    expect(tree.nodes.map((node) => node.group.name)).toEqual(['Zed', 'Alpha', 'Beta']);
  });

  it('counts 10 000 hosts quickly', () => {
    const many = Array.from({ length: 10_000 }, (_, i) =>
      host(`h${i}`, i % 3 === 0 ? 'eu' : 'sand'),
    );
    const start = performance.now();
    const tree = buildFolderTree(groups, many);
    expect(performance.now() - start).toBeLessThan(250);
    expect(tree.nodes.find((node) => node.group.id === 'prod')?.count).toBe(3334);
  });
});

describe('the cycle guard', () => {
  it('a folder cannot be its own parent', () => {
    const verdict = canMoveUnder(groups, 'prod', 'prod');
    expect(verdict).toEqual({ ok: false, reason: 'A folder cannot be its own parent.' });
  });

  it('a folder cannot move into any of its descendants', () => {
    for (const descendant of ['eu', 'edge']) {
      const verdict = canMoveUnder(groups, 'prod', descendant);
      expect(verdict.ok, descendant).toBe(false);
      if (!verdict.ok) expect(verdict.reason).toContain('own subfolders');
    }
    // Deeper than one level.
    const deep = [...groups, group('paris', 'Paris', 'eu')];
    expect(canMoveUnder(deep, 'prod', 'paris').ok).toBe(false);
    expect(canMoveUnder(deep, 'eu', 'paris').ok).toBe(false);
  });

  it('allows the legal moves: a sibling, the top level, a parent, a new folder anywhere', () => {
    expect(canMoveUnder(groups, 'eu', 'edge').ok).toBe(true);
    expect(canMoveUnder(groups, 'eu', null).ok).toBe(true);
    expect(canMoveUnder(groups, 'eu', 'prod').ok).toBe(true);
    expect(canMoveUnder(groups, 'prod', 'sand').ok).toBe(true);
    expect(canMoveUnder(groups, null, 'eu').ok).toBe(true);
  });

  it('refuses a parent that no longer exists', () => {
    const verdict = canMoveUnder(groups, 'eu', 'gone');
    expect(verdict.ok).toBe(false);
  });

  it('refuses a nesting the backend would refuse', () => {
    // MAX_GROUP_DEPTH + 1 folders in a chain: a new folder cannot go below the last.
    const chain: Group[] = [];
    for (let level = 0; level <= MAX_GROUP_DEPTH; level += 1) {
      chain.push(group(`l${level}`, `L${level}`, level === 0 ? null : `l${level - 1}`));
    }
    expect(canMoveUnder(chain, null, `l${MAX_GROUP_DEPTH - 1}`).ok).toBe(true);
    const verdict = canMoveUnder(chain, null, `l${MAX_GROUP_DEPTH}`);
    expect(verdict.ok).toBe(false);
    if (!verdict.ok) expect(verdict.reason).toContain(`${MAX_GROUP_DEPTH} levels`);
  });

  it('lists every folder as a parent option, the illegal ones disabled with their reason', () => {
    const options = parentOptions(groups, hosts, 'prod');
    expect(options.map((option) => [option.label, option.disabled])).toEqual([
      ['Production', true],
      ['Production / Europe', true],
      ['Production / Edge', true],
      ['Sandbox', false],
    ]);
    expect(options[0].reason).toContain('own parent');
    expect(options[1].reason).toContain('own subfolders');
    expect(options[3].reason).toBeNull();
    // A new folder may go anywhere.
    expect(parentOptions(groups, hosts, null).every((option) => !option.disabled)).toBe(true);
  });

  it('finds the subtree of a folder', () => {
    expect([...subtreeIds(groups, 'prod')].sort()).toEqual(['edge', 'eu', 'prod']);
    expect([...subtreeIds(groups, 'eu')]).toEqual(['eu']);
  });
});

describe('deleting a folder', () => {
  it('counts what moves and where it goes', () => {
    expect(deleteImpact(groups, hosts, 'eu')).toEqual({
      hosts: 1,
      subfolders: 0,
      destination: 'Production',
    });
    expect(deleteImpact(groups, hosts, 'prod')).toEqual({
      hosts: 0,
      subfolders: 2,
      destination: null,
    });
    expect(deleteImpact(groups, hosts, 'sand')).toEqual({
      hosts: 2,
      subfolders: 0,
      destination: null,
    });
  });

  it('says what happens to the hosts and the subfolders', () => {
    expect(deleteSentence('Europe', { hosts: 1, subfolders: 0, destination: 'Production' })).toBe(
      '1 host in “Europe” moves up to “Production”. No host is deleted and running sessions are not affected.',
    );
    expect(deleteSentence('Production', { hosts: 3, subfolders: 2, destination: null })).toContain(
      '3 hosts and 2 subfolders in “Production” move to the top level (the hosts become unfiled).',
    );
    expect(deleteSentence('Sandbox', { hosts: 2, subfolders: 0, destination: null })).toContain(
      '2 hosts in “Sandbox” move out of any folder (they become unfiled)',
    );
    expect(
      deleteSentence('Edge', { hosts: 0, subfolders: 1, destination: 'Production' }),
    ).toContain('1 subfolder in “Edge” moves up to “Production”');
    expect(deleteSentence('Empty', { hosts: 0, subfolders: 0, destination: null })).toBe(
      '“Empty” is empty. No host is affected.',
    );
  });
});
