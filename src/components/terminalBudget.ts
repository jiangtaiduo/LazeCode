// 终端会话预算（纯逻辑，供组件与单测共用）。
//
// 每个分屏都是一个常驻 PTY：shell 进程 + 读线程 + 2 个 fd + 原生终端
// （scrollback 20k 行 / 128MB 上限）。预算必须有界，但下限必须覆盖产品
// 目标：**10 个项目 × 每个 10 个控制台同时存活**。旧值 30 只能容纳 3 个项目，
// 第 4 个项目开始就会静默回收别的项目的标签并 kill 掉正在跑的任务。

/** 一个终端标签：可含多个分屏，每个分屏一个 PTY 会话。 */
export interface TerminalTab {
  /** 标签 id（不是 PTY 会话 id；一个标签可含多个分屏） */
  id: string;
  name: string;
  /** 所属工作区（窗口内多项目切换时按项目分组保活） */
  projectKey: string;
  /** 该标签下的 PTY 会话 id，按分屏顺序排列 */
  panes: string[];
  /** none=单屏；row=左右分屏；column=上下分屏 */
  split: 'none' | 'row' | 'column';
  /** 当前获得键盘焦点的分屏 */
  activePaneId: string;
}

/** 单项目控制台标签上限（产品目标：10 个） */
export const MAX_TABS_PER_PROJECT = 10;
/** 单个标签最多分屏数 */
export const MAX_PANES_PER_TAB = 4;
/**
 * 全窗口常驻 PTY 上限（分屏也计入）。
 *
 * 120 = 10 个项目 × 10 个控制台（100）+ 20 个分屏余量。超过时才回收
 * 其它项目的旧标签；当前项目的标签只会在「无其它项目可回收」时被回收，
 * 且至少保留一个，避免把正在操作的项目清空。
 *
 * 资源量级（实测，120×40 网格）：fd 2/会话、线程 1/会话；空屏 RSS 约
 * 0.1MB/会话，2k 行输出约 2MB/会话，20k 行 scrollback 写满约 16.6MB/会话
 * （100 会话 ≈ 1.7GB）。macOS GUI 启动态的 fd 软上限 256 需要
 * `ensure_fd_limit()` 提升，否则约 85 个会话就 EMFILE（见 lib.rs）。
 */
export const MAX_PTY_TOTAL = 120;

export function countPanes(tabs: TerminalTab[]): number {
  return tabs.reduce((sum, tab) => sum + tab.panes.length, 0);
}

/** 当前项目是否还能新增控制台标签（按钮禁用态与 addTab 共用同一判定）。 */
export function canAddTab(tabs: TerminalTab[], projectKey: string): boolean {
  return tabs.filter(t => t.projectKey === projectKey).length < MAX_TABS_PER_PROJECT;
}

export interface PtyBudgetPlan {
  /** 回收后应保存的标签列表；rejected=true 时与传入值相同，调用方不得应用 */
  tabs: TerminalTab[];
  /** 被回收标签的全部 PTY 会话 id，调用方负责 pty_close */
  evicted: string[];
  /** 预算内找不到可回收标签：本次新增必须取消（保持原有「不清空当前项目」语义） */
  rejected: boolean;
}

/**
 * 在「已包含新标签」的列表上执行 PTY 预算回收。
 *
 * 回收优先级：别的项目最早的标签 → 当前项目最早的标签（当前项目至少留一个）。
 * 只做计划、不产生副作用，PTY 的终止与 everShown 清理由调用方按 `evicted` 执行。
 */
export function planPtyBudget(
  tabsWithNew: TerminalTab[],
  keepTabId: string,
  projectKey: string
): PtyBudgetPlan {
  let tabs = tabsWithNew;
  let total = countPanes(tabs);
  const evicted: string[] = [];

  while (total > MAX_PTY_TOTAL) {
    const victim =
      tabs.find(tab => tab.id !== keepTabId && tab.projectKey !== projectKey) ??
      tabs.find(
        tab =>
          tab.id !== keepTabId &&
          tab.projectKey === projectKey &&
          tabs.filter(other => other.projectKey === projectKey).length > 1
      );
    if (!victim) {
      // 宁可拒绝这次新增，也不回收当前项目最后一个标签
      return { tabs: tabsWithNew, evicted: [], rejected: true };
    }
    tabs = tabs.filter(tab => tab.id !== victim.id);
    total -= victim.panes.length;
    evicted.push(...victim.panes);
  }

  return { tabs, evicted, rejected: false };
}
