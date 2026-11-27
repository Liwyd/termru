/**
 * The folder hierarchy: the tree the folders page draws, the counts next to
 * it, the rules for where a folder may move, and what deleting one does.
 *
 * Everything here is a pure function of the folders and hosts it is given,
 * so the page, the editor's folder picker and the tests share one source of
 * truth. The backend enforces the same rules when a folder is saved
 * (`termru-store` `validate_parent`); a client that does not know them gets
 * the backend's refusal, the folders page never offers the illegal move.
 */

import type { Group, Host } from './types';

/**
 * Folder nesting limit (`MAX_GROUP_DEPTH` of `termru-core`): a folder may
 * have at most this many parents above it (a top-level folder has none).
 */
export const MAX_GROUP_DEPTH = 8;

/** One folder in the order the tree shows it. */
export interface FolderNode {
  group: Group;
  /** 0 for a top-level folder. */
  depth: number;
  /** `Production / Europe`. */
  path: string;
  /** Hosts in this folder and in the folders under it. */
  count: number;
  /** Hosts directly in this folder. */
  directCount: number;
  /** The direct subfolders. */
  childCount: number;
}

export interface FolderTree {
  /** Every folder, parents before their children, siblings by name. */
  nodes: FolderNode[];
  /** Every host. */
  total: number;
  /** Hosts without a folder. */
  unfiled: number;
}

function bySortOrder(a: Group, b: Group): number {
  if (a.sort_order !== b.sort_order) return a.sort_order - b.sort_order;
  return a.name.localeCompare(b.name) || (a.id < b.id ? -1 : a.id > b.id ? 1 : 0);
}

/** The folders by id, and their children in display order. */
function index(groups: Group[]): {
  byId: Map<string, Group>;
  children: Map<string | null, Group[]>;
} {
  const byId = new Map(groups.map((group) => [group.id, group]));
  const children = new Map<string | null, Group[]>();
  for (const group of groups) {
    // A folder whose parent is gone (or is itself) is shown at the top
    // level: it must stay reachable.
    const parent =
      group.parent_id && group.parent_id !== group.id && byId.has(group.parent_id)
        ? group.parent_id
        : null;
    const siblings = children.get(parent);
    if (siblings) siblings.push(group);
    else children.set(parent, [group]);
  }
  for (const siblings of children.values()) siblings.sort(bySortOrder);
  return { byId, children };
}

/** The ids of the folders from `id` up to the top (the folder itself first). */
function ancestry(byId: Map<string, Group>, id: string): string[] {
  const chain: string[] = [];
  const seen = new Set<string>();
  let current = byId.get(id);
  while (current && !seen.has(current.id)) {
    seen.add(current.id);
    chain.push(current.id);
    current = current.parent_id ? byId.get(current.parent_id) : undefined;
  }
  return chain;
}

/** `Production / Europe`; a data cycle ends the path instead of looping. */
export function folderPath(groups: Group[], id: string): string {
  const byId = new Map(groups.map((group) => [group.id, group]));
  return ancestry(byId, id)
    .reverse()
    .map((entry) => byId.get(entry)?.name ?? '')
    .join(' / ');
}

/** Builds the tree with its counts. */
export function buildFolderTree(groups: Group[], hosts: Host[]): FolderTree {
  const { byId, children } = index(groups);

  const direct = new Map<string, number>();
  const subtree = new Map<string, number>();
  let unfiled = 0;
  for (const host of hosts) {
    if (!host.group_id || !byId.has(host.group_id)) {
      unfiled += 1;
      continue;
    }
    direct.set(host.group_id, (direct.get(host.group_id) ?? 0) + 1);
    for (const id of ancestry(byId, host.group_id)) {
      subtree.set(id, (subtree.get(id) ?? 0) + 1);
    }
  }

  const nodes: FolderNode[] = [];
  const visit = (group: Group, depth: number, parentPath: string, seen: Set<string>): void => {
    if (seen.has(group.id)) return;
    seen.add(group.id);
    const path = parentPath ? `${parentPath} / ${group.name}` : group.name;
    const kids = children.get(group.id) ?? [];
    nodes.push({
      group,
      depth,
      path,
      count: subtree.get(group.id) ?? 0,
      directCount: direct.get(group.id) ?? 0,
      childCount: kids.length,
    });
    for (const child of kids) visit(child, depth + 1, path, seen);
  };
  const seen = new Set<string>();
  for (const root of children.get(null) ?? []) visit(root, 0, '', seen);
  // Folders in a data cycle have no root above them: show them anyway.
  for (const group of [...groups].sort(bySortOrder)) {
    if (!seen.has(group.id)) visit(group, 0, '', seen);
  }
  return { nodes, total: hosts.length, unfiled };
}

/** The id of `id` and of every folder under it. */
export function subtreeIds(groups: Group[], id: string): Set<string> {
  const { children } = index(groups);
  const out = new Set<string>();
  const walk = (current: string): void => {
    if (out.has(current)) return;
    out.add(current);
    for (const child of children.get(current) ?? []) walk(child.id);
  };
  walk(id);
  return out;
}

/** True when the host lives in `folderId` or in a folder under it. */
export function hostIsInSubtree(host: Host, subtree: Set<string>): boolean {
  return host.group_id !== null && subtree.has(host.group_id);
}

/** How many folders a chain has from `id` up to the top (a top-level folder is 1). */
function chainLength(byId: Map<string, Group>, id: string): number {
  return ancestry(byId, id).length;
}

export type MoveVerdict = { ok: true } | { ok: false; reason: string };

/**
 * Whether folder `folderId` (null: a folder that does not exist yet) may
 * have `parentId` as its parent (null: the top level).
 */
export function canMoveUnder(
  groups: Group[],
  folderId: string | null,
  parentId: string | null,
): MoveVerdict {
  if (parentId === null) return { ok: true };
  const byId = new Map(groups.map((group) => [group.id, group]));
  if (!byId.has(parentId)) return { ok: false, reason: 'That folder no longer exists.' };
  if (folderId !== null) {
    if (parentId === folderId) {
      return { ok: false, reason: 'A folder cannot be its own parent.' };
    }
    if (subtreeIds(groups, folderId).has(parentId)) {
      return { ok: false, reason: 'A folder cannot move into one of its own subfolders.' };
    }
  }
  // The new parent and the folders above it are the folder's ancestors.
  if (chainLength(byId, parentId) > MAX_GROUP_DEPTH) {
    return {
      ok: false,
      reason: `Folders can be nested at most ${MAX_GROUP_DEPTH} levels below the top level.`,
    };
  }
  return { ok: true };
}

/** A folder offered as a parent, with the reason when it may not be chosen. */
export interface ParentOption {
  id: string;
  label: string;
  disabled: boolean;
  reason: string | null;
}

/**
 * The parents the folder picker lists for `folderId` (null: a new folder),
 * in tree order. The illegal ones stay in the list, disabled and with the
 * reason, so the user sees why a folder is missing instead of wondering.
 */
export function parentOptions(
  groups: Group[],
  hosts: Host[],
  folderId: string | null,
): ParentOption[] {
  return buildFolderTree(groups, hosts).nodes.map((node) => {
    const verdict = canMoveUnder(groups, folderId, node.group.id);
    return {
      id: node.group.id,
      label: node.path,
      disabled: !verdict.ok,
      reason: verdict.ok ? null : verdict.reason,
    };
  });
}

/** What deleting a folder does (the backend moves its contents up one level). */
export interface DeleteImpact {
  /** Hosts directly in the folder. */
  hosts: number;
  /** Direct subfolders. */
  subfolders: number;
  /** The folder everything moves to; null: the top level (unfiled hosts). */
  destination: string | null;
}

export function deleteImpact(groups: Group[], hosts: Host[], id: string): DeleteImpact {
  const group = groups.find((entry) => entry.id === id);
  const parentId =
    group?.parent_id && groups.some((entry) => entry.id === group.parent_id)
      ? group.parent_id
      : null;
  return {
    hosts: hosts.filter((host) => host.group_id === id).length,
    subfolders: groups.filter((entry) => entry.parent_id === id && entry.id !== id).length,
    destination: parentId ? folderPath(groups, parentId) : null,
  };
}

function plural(count: number, one: string, many: string): string {
  return `${count} ${count === 1 ? one : many}`;
}

/** The sentence of the delete confirmation: what happens to what is inside. */
export function deleteSentence(name: string, impact: DeleteImpact): string {
  const { hosts, subfolders, destination } = impact;
  if (hosts === 0 && subfolders === 0) {
    return `“${name}” is empty. No host is affected.`;
  }
  const parts: string[] = [];
  if (hosts > 0) parts.push(plural(hosts, 'host', 'hosts'));
  if (subfolders > 0) parts.push(plural(subfolders, 'subfolder', 'subfolders'));
  const what = parts.join(' and ');
  const verb = hosts + subfolders === 1 ? 'moves' : 'move';
  const where = destination
    ? `up to “${destination}”`
    : subfolders > 0 && hosts > 0
      ? 'to the top level (the hosts become unfiled)'
      : hosts > 0
        ? 'out of any folder (they become unfiled)'
        : 'to the top level';
  return `${what} in “${name}” ${verb} ${where}. No host is deleted and running sessions are not affected.`;
}
