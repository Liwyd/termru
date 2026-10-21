import { describe, expect, it } from 'vitest';
import {
  collectPanes,
  fromLayoutNode,
  hasPane,
  layoutLeaves,
  leafTree,
  paneCount,
  pruneTree,
  removeNode,
  setRatio,
  splitNode,
  toLayoutNode,
} from './pane-tree';

describe('the pane tree', () => {
  it('holds one leaf', () => {
    const tree = leafTree('p1');
    expect(tree.kind).toBe('leaf');
    expect(collectPanes(tree)).toEqual(['p1']);
    expect(paneCount(tree)).toBe(1);
  });

  it('splits a leaf into two', () => {
    const tree = splitNode(leafTree('p1'), 'p1', 'columns', 'p2');
    expect(tree).not.toBeNull();
    expect(tree?.kind).toBe('split');
    if (tree?.kind !== 'split') return;
    expect(tree.direction).toBe('columns');
    expect(collectPanes(tree)).toEqual(['p1', 'p2']);
    expect(hasPane(tree, 'p2')).toBe(true);
  });

  it('does not split a pane that is not in the tree', () => {
    expect(splitNode(leafTree('p1'), 'other', 'columns', 'p2')).toBeNull();
  });

  it('splits a pane in the middle of the tree', () => {
    let tree = splitNode(leafTree('p1'), 'p1', 'columns', 'p2')!;
    tree = splitNode(tree, 'p2', 'rows', 'p3')!;
    expect(collectPanes(tree)).toEqual(['p1', 'p2', 'p3']);
    expect(paneCount(tree)).toBe(3);
  });

  it('collapses the tree when a pane is removed', () => {
    const tree = splitNode(leafTree('p1'), 'p1', 'columns', 'p2')!;
    const pruned = removeNode(tree, 'p2');
    expect(pruned?.kind).toBe('leaf');
    if (pruned?.kind === 'leaf') expect(pruned.pane).toBe('p1');
  });

  it('keeps the other branch when a pane in the middle is removed', () => {
    let tree = splitNode(leafTree('p1'), 'p1', 'columns', 'p2')!;
    tree = splitNode(tree, 'p2', 'rows', 'p3')!;
    const pruned = removeNode(tree, 'p2');
    expect(pruned).not.toBeNull();
    expect(collectPanes(pruned!)).toEqual(['p1', 'p3']);
  });

  it('leaves the tree unchanged when the pane is not in it', () => {
    const tree = removeNode(leafTree('p1'), 'other');
    expect(tree).not.toBeNull();
    expect(collectPanes(tree!)).toEqual(['p1']);
  });

  it('returns null when the tree becomes empty', () => {
    expect(removeNode(leafTree('p1'), 'p1')).toBeNull();
  });

  it('clamps the ratio of a split', () => {
    const tree = splitNode(leafTree('p1'), 'p1', 'columns', 'p2')!;
    if (tree.kind !== 'split') throw new Error('the tree was not split');
    const bigger = setRatio(tree, tree.id, 5);
    if (bigger.kind !== 'split') throw new Error('the tree was not split');
    expect(bigger.ratio).toBeLessThanOrEqual(0.9);

    const smaller = setRatio(tree, tree.id, -1);
    if (smaller.kind !== 'split') throw new Error('the tree was not split');
    expect(smaller.ratio).toBeGreaterThanOrEqual(0.1);
  });

  it('prunes the leaves that do not exist any more', () => {
    let tree = splitNode(leafTree('p1'), 'p1', 'columns', 'p2')!;
    tree = splitNode(tree, 'p2', 'rows', 'p3')!;
    const pruned = pruneTree(tree, new Set(['p1', 'p3']));
    expect(pruned).not.toBeNull();
    expect(collectPanes(pruned!)).toEqual(['p1', 'p3']);
  });

  it('round-trips through the layout shape', () => {
    let tree = splitNode(leafTree('p1'), 'p1', 'columns', 'p2')!;
    tree = splitNode(tree, 'p2', 'rows', 'p3')!;
    const layout = toLayoutNode(tree, new Map([['p1', 'host-1']]));
    expect(layout.split?.direction).toBe('columns');
    expect(layout.split?.ratio).toBeCloseTo(0.5);
    expect(layoutLeaves(layout).map((leaf) => leaf.host_id)).toEqual(['host-1', null, null]);

    const back = fromLayoutNode(layout);
    expect(collectPanes(back)).toEqual(['p1', 'p2', 'p3']);
  });

  it('keeps the direction and the ratio of a restored layout', () => {
    const layout = {
      split: {
        direction: 'rows' as const,
        ratio: 0.3,
        first: { leaf: { pane: 'p1', host_id: null } },
        second: { leaf: { pane: 'p2', host_id: null } },
      },
    };
    const tree = fromLayoutNode(layout);
    if (tree.kind !== 'split') throw new Error('the layout was not restored');
    expect(tree.direction).toBe('rows');
    expect(tree.ratio).toBeCloseTo(0.3);
  });
});
