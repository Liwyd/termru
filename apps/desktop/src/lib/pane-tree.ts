import type { PaneNode } from './types';

/**
 * The pane tree of a tab.
 *
 * The tree is the frontend's own shape: a split carries an id (the divider
 * needs one) and the leaf carries the pane id. The persisted layout uses
 * the shape of the backend (`PaneNode`), which is converted at the
 * boundary.
 */
export type TreeNode =
  | { kind: 'leaf'; pane: string }
  | {
      kind: 'split';
      id: string;
      direction: 'columns' | 'rows';
      /** Fraction of the space given to `first`, in (0, 1). */
      ratio: number;
      first: TreeNode;
      second: TreeNode;
    };

/** The ratio a new split starts with. */
export const DEFAULT_RATIO = 0.5;

/** The smallest and largest fraction a divider may take. */
export const MIN_RATIO = 0.1;
export const MAX_RATIO = 0.9;

/** A tree with a single leaf. */
export function leafTree(paneId: string): TreeNode {
  return { kind: 'leaf', pane: paneId };
}

/** The pane ids of the tree, from the left to the right (top to bottom). */
export function collectPanes(node: TreeNode): string[] {
  if (node.kind === 'leaf') return [node.pane];
  return [...collectPanes(node.first), ...collectPanes(node.second)];
}

/** The number of panes in the tree. */
export function paneCount(node: TreeNode): number {
  return collectPanes(node).length;
}

/** True when the tree holds the pane. */
export function hasPane(node: TreeNode, paneId: string): boolean {
  return collectPanes(node).includes(paneId);
}

/**
 * Replaces the leaf with a split: the leaf keeps the first half, the new
 * pane takes the second half. Returns null when the pane is not in the
 * tree.
 */
export function splitNode(
  node: TreeNode,
  paneId: string,
  direction: 'columns' | 'rows',
  newPaneId: string,
): TreeNode | null {
  if (node.kind === 'leaf') {
    if (node.pane !== paneId) return null;
    return {
      kind: 'split',
      id: crypto.randomUUID(),
      direction,
      ratio: DEFAULT_RATIO,
      first: node,
      second: leafTree(newPaneId),
    };
  }
  const first = splitNode(node.first, paneId, direction, newPaneId);
  if (first) return { ...node, first };
  const second = splitNode(node.second, paneId, direction, newPaneId);
  if (second) return { ...node, second };
  return null;
}

/**
 * Removes the leaf with the given pane id.
 *
 * A split that loses one of its children is replaced by the other one, so
 * the tree never holds a single-child split. Returns null when the tree
 * becomes empty; a tree that does not hold the pane is returned unchanged.
 */
export function removeNode(node: TreeNode, paneId: string): TreeNode | null {
  if (node.kind === 'leaf') {
    return node.pane === paneId ? null : node;
  }
  const first = removeNode(node.first, paneId);
  if (first === null) {
    // The whole first branch is gone: keep the second one.
    return node.second;
  }
  const second = removeNode(node.second, paneId);
  if (second === null) return node.first;
  return { ...node, first, second };
}

/** The split that directly holds the pane (the divider next to it). */
export function parentSplit(node: TreeNode, paneId: string): TreeNode | null {
  if (node.kind === 'leaf') return null;
  const inFirst = hasPane(node.first, paneId);
  const inSecond = hasPane(node.second, paneId);
  if (!inFirst && !inSecond) return null;
  if (inFirst && node.first.kind === 'leaf' && node.first.pane === paneId) return node;
  if (inSecond && node.second.kind === 'leaf' && node.second.pane === paneId) return node;
  return (
    parentSplit(inFirst ? node.first : node.second, paneId) ??
    // The pane is deeper: the divider of its own branch is the one that
    // matters for the layout, so the closest split wins.
    null
  );
}

/** Sets the ratio of a split. The ratio is clamped to the allowed range. */
export function setRatio(node: TreeNode, splitId: string, ratio: number): TreeNode {
  if (node.kind === 'leaf') return node;
  if (node.id === splitId) {
    return { ...node, ratio: Math.min(MAX_RATIO, Math.max(MIN_RATIO, ratio)) };
  }
  return {
    ...node,
    first: setRatio(node.first, splitId, ratio),
    second: setRatio(node.second, splitId, ratio),
  };
}

/** The split with the given id, if any. */
export function findSplit(node: TreeNode, splitId: string): TreeNode | null {
  if (node.kind === 'leaf') return null;
  if (node.id === splitId) return node;
  return findSplit(node.first, splitId) ?? findSplit(node.second, splitId);
}

/**
 * Converts the tree into the shape the backend stores.
 *
 * The host of every leaf is looked up in `hosts`: the layout refers to the
 * hosts by id, so a restored layout can be matched against the local
 * hosts.
 */
export function toLayoutNode(
  node: TreeNode,
  hosts: Map<string, string | null> = new Map(),
): PaneNode {
  if (node.kind === 'leaf') {
    return { leaf: { pane: node.pane, host_id: hosts.get(node.pane) ?? null } };
  }
  return {
    split: {
      direction: node.direction,
      ratio: node.ratio,
      first: toLayoutNode(node.first, hosts),
      second: toLayoutNode(node.second, hosts),
    },
  };
}

/**
 * Converts a stored layout into a tree.
 *
 * The layout only carries the host id of a leaf; the caller pairs the
 * leaves with the hosts (a host may have been deleted meanwhile).
 */
export function fromLayoutNode(node: PaneNode): TreeNode {
  if (node.leaf) {
    return { kind: 'leaf', pane: node.leaf.pane };
  }
  const split = node.split;
  if (!split) return { kind: 'leaf', pane: crypto.randomUUID() };
  return {
    kind: 'split',
    id: crypto.randomUUID(),
    direction: split.direction,
    ratio: Math.min(MAX_RATIO, Math.max(MIN_RATIO, split.ratio)),
    first: fromLayoutNode(split.first),
    second: fromLayoutNode(split.second),
  };
}

/** The leaves of a stored layout, with the host they refer to. */
export function layoutLeaves(node: PaneNode): { pane: string; host_id: string | null }[] {
  if (node.leaf) return [{ pane: node.leaf.pane, host_id: node.leaf.host_id ?? null }];
  const split = node.split;
  if (!split) return [];
  return [...layoutLeaves(split.first), ...layoutLeaves(split.second)];
}

/**
 * Removes the leaves that are not in `keep`.
 *
 * A split that loses one of its children is replaced by the other one, so
 * the tree never holds a single-child split.
 */
export function pruneTree(node: TreeNode, keep: Set<string>): TreeNode | null {
  if (node.kind === 'leaf') {
    return keep.has(node.pane) ? node : null;
  }
  const first = pruneTree(node.first, keep);
  const second = pruneTree(node.second, keep);
  if (!first && !second) return null;
  if (!first) return second;
  if (!second) return first;
  return { ...node, first, second };
}
