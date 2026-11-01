/**
 * 项目 tab 条的排列顺序：用户拖拽决定的顺序独立于会话快照的插入序。
 *
 * `tabOrderRef` 只记录用户显式排过的（或按打开先后追加的）路径；新项目
 * 追加尾部、关闭/淘汰的路径移除。展示时按该表排序，表里没有的条目
 * （理论瞬时态）按入参顺序排在尾部，保证任何输入都有稳定输出。
 */

/** 按用户顺序表排列 tab：表内的按表序，表外的按原顺序补在尾部。 */
export function orderProjectTabs<T extends { path: string }>(tabs: T[], order: string[]): T[] {
  const known = new Map(order.map((p, i) => [p, i]));
  const sorted = [...tabs];
  sorted.sort((a, b) => {
    const ia = known.get(a.path);
    const ib = known.get(b.path);
    // 都在表内：比表序；都不在：保持原相对顺序（sort 稳定）；混着：表内在前
    if (ia !== undefined && ib !== undefined) return ia - ib;
    if (ia !== undefined) return -1;
    if (ib !== undefined) return 1;
    return 0;
  });
  return sorted;
}

/** 把显示序里第 from 个 tab 移到第 to 个位置（越界钳制），返回新数组。 */
export function applyTabReorder<T>(list: T[], from: number, to: number): T[] {
  const next = [...list];
  const clampedFrom = Math.max(0, Math.min(from, next.length - 1));
  const clampedTo = Math.max(0, Math.min(to, next.length - 1));
  if (clampedFrom === clampedTo) return next;
  const [moved] = next.splice(clampedFrom, 1);
  next.splice(clampedTo, 0, moved);
  return next;
}
