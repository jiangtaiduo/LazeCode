// 字符串驻留（GPUI `SharedString` 的平价版，见 Zed 内存分析）。
//
// 同一条路径字符串同时活在 N 个结构里：文件树节点、展开集合、打开标签、
// 缓冲 key、git 状态、搜索结果——每次 `read_directory` 全量刷新都从 JSON
// 重新 parse 出 N 份相等的堆拷贝。驻留保证"同一内容只有一份 canonical
// 拷贝"，各处共享引用：旧树能被 GC 整块回收，而不是留下 N 份漂流副本。
//
// 语义保证：
// - `intern` 是纯函数式替换（返回 canonical 串，不改参数），行为零变化；
// - Map 有上限，超限整体清空——清空只意味着"下次刷新重新规范化"，
//   已发出去的 canonical 串不受影响（调用方持有引用，GC 正常工作）；
// - 只驻留**高重复**的短串（路径、文件名、状态码），不驻留文件内容这类
//   大串（内容各有各的版本，驻留即泄漏）。

/** 驻留表上限：超限清空重建（见上）。文件树全量也就万级节点，5 万富余。 */
export const INTERN_MAP_CAP = 50_000;

const pool = new Map<string, string>();

/** 返回与 `s` 内容相等的 canonical 实例（空串原样返回，不进表）。 */
export function intern(s: string): string {
  if (s === '') return s;
  const hit = pool.get(s);
  if (hit !== undefined) return hit;
  if (pool.size >= INTERN_MAP_CAP) pool.clear();
  pool.set(s, s);
  return s;
}

/** 当前驻留的 distinct 串数量（验证脚本/诊断用）。 */
export function internPoolSize(): number {
  return pool.size;
}

/** 仅测试用：清空驻留表（生产代码不调）。 */
export function clearInternPoolForTest(): void {
  pool.clear();
}

/** 文件树节点的最小形状（path/name）：FileNode 的结构子集。
 * gitStatus 故意不驻留：只有 5 种短取值，收益可忽略，还会与 FileNode 的
 * 字面量联合类型（'M'|'A'|…）冲突。 */
export interface InternableNode {
  path: string;
  name: string;
  children?: InternableNode[] | null;
}

/**
 * 就地驻留一棵文件树（path/name 换成 canonical 实例），返回同一引用。
 * 就地改是故意的：节点对象本身是本轮 JSON 新 parse 的，只换其中的串字段，
 * 不复制节点（复制整树才是我们要消灭的分配）。
 */
export function internFileTree<T extends InternableNode>(nodes: T[]): T[] {
  for (const node of nodes) {
    node.path = intern(node.path);
    node.name = intern(node.name);
    if (node.children) internFileTree(node.children);
  }
  return nodes;
}
