import type { GitBranch } from '../types';

/**
 * 分支/标签树的节点。entries 保留输入顺序：
 * PyCharm 的分支树先按收藏/当前分支排序，再按名称，目录出现在其第一个
 * 成员的位置，而不是把所有目录统一提到最前面。
 */
export interface RefTreeNode {
  /** 当前段的显示名；根节点为空串 */
  name: string;
  /** 完整路径，用于折叠状态与 React key */
  path: string;
  entries: RefTreeEntry[];
}

export type RefTreeEntry =
  | { type: 'dir'; node: RefTreeNode }
  | { type: 'ref'; ref: GitBranch };

export interface FlatRefRow {
  type: 'dir' | 'ref';
  depth: number;
  path: string;
  ref?: GitBranch;
  node?: RefTreeNode;
}

export const refKey = (ref: GitBranch): string => {
  const kind = ref.isTag ? 'tag' : ref.isRemote ? 'remote' : 'local';
  return `${kind}:${ref.name}`;
};

const dirOf = (node: RefTreeNode, name: string, path: string): RefTreeNode => {
  for (const e of node.entries) {
    if (e.type === 'dir' && e.node.name === name) return e.node;
  }
  const created: RefTreeNode = { name, path, entries: [] };
  node.entries.push({ type: 'dir', node: created });
  return created;
};

/**
 * 按 `/` 把 ref 名折叠成目录树。输入顺序即同一层内的展示顺序。
 * 远程分支保留 `origin/feature/x` 的完整层级（origin 本身也是目录）。
 */
export function buildRefTree(refs: GitBranch[]): RefTreeNode {
  const root: RefTreeNode = { name: '', path: '', entries: [] };
  for (const ref of refs) {
    const parts = ref.name.split('/').filter(Boolean);
    if (parts.length === 0) continue;
    let cur = root;
    let path = '';
    for (let i = 0; i < parts.length - 1; i++) {
      path = path ? `${path}/${parts[i]}` : parts[i];
      cur = dirOf(cur, parts[i], path);
    }
    cur.entries.push({ type: 'ref', ref });
  }
  return root;
}

/**
 * 展开为可见行。折叠目录时不产生其子行；目录自身始终保留（用于再次展开）。
 * `collapsed` 存的是完整 path（不带 local/remote/tag 前缀，调用方按组隔离）。
 */
export function flattenRefTree(
  root: RefTreeNode,
  collapsed: ReadonlySet<string>,
  startDepth = 0
): FlatRefRow[] {
  const out: FlatRefRow[] = [];
  const walk = (node: RefTreeNode, depth: number) => {
    for (const e of node.entries) {
      if (e.type === 'dir') {
        out.push({ type: 'dir', depth, path: e.node.path, node: e.node });
        if (!collapsed.has(e.node.path)) walk(e.node, depth + 1);
      } else {
        out.push({ type: 'ref', depth, path: e.ref.name, ref: e.ref });
      }
    }
  };
  walk(root, startDepth);
  return out;
}

/** 叶子节点在展示顺序中的 key，Shift 连选只在这个序列上工作。 */
export function visibleRefKeys(rows: FlatRefRow[]): string[] {
  return rows.filter(r => r.type === 'ref' && r.ref).map(r => refKey(r.ref!));
}
