import { describe, expect, it } from 'vitest';
import { buildRefTree, flattenRefTree, refKey, visibleRefKeys } from './refTree';
import type { GitBranch } from '../types';

const b = (name: string, isRemote = false, isTag = false): GitBranch => ({
  name,
  isCurrent: false,
  isRemote,
  isTag
});

describe('refTree', () => {
  it('folds branch names into directories while preserving input order', () => {
    const root = buildRefTree([b('main'), b('feature/login'), b('conflict-a'), b('feature/pay')]);
    const rows = flattenRefTree(root, new Set());
    expect(rows.map(r => `${r.type}:${r.path}:${r.depth}`)).toEqual([
      'ref:main:0',
      'dir:feature:0',
      'ref:feature/login:1',
      'ref:feature/pay:1',
      'ref:conflict-a:0'
    ]);
  });

  it('keeps remote namespace as the first folder', () => {
    const root = buildRefTree([b('origin/main', true), b('origin/feature/x', true)]);
    const rows = flattenRefTree(root, new Set());
    expect(rows.map(r => `${r.type}:${r.path}:${r.depth}`)).toEqual([
      'dir:origin:0',
      'ref:origin/main:1',
      'dir:origin/feature:1',
      'ref:origin/feature/x:2'
    ]);
  });

  it('hides children of collapsed directories but keeps the directory row', () => {
    const root = buildRefTree([b('feature/login'), b('feature/pay'), b('main')]);
    const collapsed = new Set(['feature']);
    const rows = flattenRefTree(root, collapsed);
    expect(rows.map(r => `${r.type}:${r.path}`)).toEqual(['dir:feature', 'ref:main']);
    expect(visibleRefKeys(rows)).toEqual(['local:main']);
  });

  it('uses ref kind in keys so tags and branches never collide', () => {
    expect(refKey(b('v1'))).toBe('local:v1');
    expect(refKey(b('v1', false, true))).toBe('tag:v1');
    expect(refKey(b('origin/v1', true))).toBe('remote:origin/v1');
  });
});
