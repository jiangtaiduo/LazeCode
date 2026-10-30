// 项目会话快照的淘汰策略（纯逻辑，便于单测）。
//
// 快照只包含文件树 + 编辑缓冲（几百 KB），淘汰它是 UI/内存优化；
// 而终端 PTY 代表用户正在跑的任务。两者生命周期不同：打开第 21 个项目
// 不应顺手 kill 掉最久未切换项目的 shell。

export interface ProjectPrunePlan {
  /** 可以安全删除的快照路径（无终端、且不是当前项目） */
  prunable: string[];
  /** 超出上限但因为有终端而被 pin 住的数量（用于状态提示） */
  pinnedOverflow: number;
}

/**
 * 选出可淘汰的项目快照。
 *
 * - 当前项目永不淘汰；
 * - `live` 中的项目（有常驻终端）永不淘汰；
 * - `cap` 只统计「非当前项目」的快照（与 App 原逻辑一致：当前项目不占快照名额）；
 * - 超出 `cap` 的数量优先由无终端的项目承担；全部都有终端时只报告
 *   `pinnedOverflow`，由调用方提示用户，而不是杀任务。
 */
export function planProjectPrune(
  paths: string[],
  keepPath: string,
  live: Set<string>,
  cap: number
): ProjectPrunePlan {
  const candidates = paths.filter(p => p !== keepPath);
  const excess = candidates.length - cap;
  if (excess <= 0) return { prunable: [], pinnedOverflow: 0 };

  const prunable = candidates.filter(p => !live.has(p)).slice(0, excess);
  return { prunable, pinnedOverflow: excess - prunable.length };
}
