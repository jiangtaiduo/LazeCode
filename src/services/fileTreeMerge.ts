// 文件树深度合并：整树/子树刷新时保留已展开且已加载的更深子树。
//
// read_directory 的 maxDepth=2 会在截断处把深层节点写成 children=null。
// 用新读结果整体替换时，展开中的深层目录会被「折叠」（children 丢失），
// 用户看到的陈旧性之外还多一层视觉抖动。这里按路径对齐新旧两份子树：
// 新节点被截断（children 非数组）且该目录在展开集合里、旧节点已加载时，
// 沿用旧 children（递归应用，旧子树里同样截断的展开目录继续保留）。
//
// 陈旧性边界（有意取舍）：沿用的旧子树由两条路径兜底新鲜度——
// 1. 目录监听按「变更目录」精确重读（fs://dir-change → refreshTreeSubdir），
//    展开中的目录天然在监听范围内（递归监听）；
// 2. 展开动作本身现读（ProjectDrawer.toggleFolder 不再跳过已有 children）。

import type { FileNode } from '../types';

export function preserveExpandedChildren(
  fresh: FileNode[],
  prev: FileNode[] | undefined,
  expandedPaths: Set<string>
): FileNode[] {
  if (!Array.isArray(prev) || prev.length === 0) return fresh;
  const prevByPath = new Map(prev.map(node => [node.path, node]));
  return fresh.map(node => {
    const old = prevByPath.get(node.path);
    if (!old || !node.isDirectory) return node;
    if (Array.isArray(node.children)) {
      // 新读到了这一层：继续向深处合并（这一层里被截断的展开目录沿用旧的）
      if (node.children.length === 0) return node; // 真空目录：新读为空就是空
      return {
        ...node,
        children: preserveExpandedChildren(node.children, old.children, expandedPaths)
      };
    }
    // 新读被 maxDepth 截断：展开中且旧子树已加载时沿用
    if (!expandedPaths.has(node.path)) return node;
    if (!Array.isArray(old.children)) return node;
    return { ...node, children: old.children };
  });
}
