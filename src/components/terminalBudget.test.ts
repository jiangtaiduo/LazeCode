import { describe, expect, it } from 'vitest';
import {
  MAX_PTY_TOTAL,
  MAX_TABS_PER_PROJECT,
  canAddTab,
  countPanes,
  planPtyBudget,
  type TerminalTab
} from './terminalBudget';

const tab = (id: string, projectKey: string, panes = 1): TerminalTab => ({
  id,
  name: id,
  projectKey,
  panes: Array.from({ length: panes }, (_, i) => `${id}-pane-${i}`),
  split: panes > 1 ? 'row' : 'none',
  activePaneId: `${id}-pane-0`
});

/** 10 个项目 × 10 个控制台：需求里的极限场景 */
const tenByTen = (): TerminalTab[] =>
  Array.from({ length: 10 }, (_, project) =>
    Array.from({ length: MAX_TABS_PER_PROJECT }, (_, index) =>
      tab(`p${project}-t${index}`, `/ws/${project}`)
    )
  ).flat();

describe('planPtyBudget', () => {
  it('10 项目 × 10 控制台（100 个 PTY）不触发任何回收', () => {
    const tabs = tenByTen();
    expect(countPanes(tabs)).toBe(100);

    const next = [...tabs, tab('p10-t0', '/ws/10')];
    const plan = planPtyBudget(next, 'p10-t0', '/ws/10');

    expect(plan.rejected).toBe(false);
    expect(plan.evicted).toEqual([]);
    expect(plan.tabs).toHaveLength(101);
    expect(plan.tabs).toBe(next);
  });

  it('超过预算时只回收别的项目最早的标签', () => {
    const tabs = Array.from({ length: 12 }, (_, project) =>
      Array.from({ length: MAX_TABS_PER_PROJECT }, (_, index) =>
        tab(`p${project}-t${index}`, `/ws/${project}`)
      )
    ).flat();
    // 当前项目已有 10 个，新增第 11 个 → 121 个 PTY，超预算 1
    const next = [...tabs, tab('cur-new', '/ws/0')];
    const plan = planPtyBudget(next, 'cur-new', '/ws/0');

    expect(plan.rejected).toBe(false);
    // 回收的是最早插入的其它项目的标签；当前项目的 10 个一个不动
    expect(plan.evicted).toEqual(['p1-t0-pane-0']);
    expect(countPanes(plan.tabs)).toBe(MAX_PTY_TOTAL);
    expect(plan.tabs.filter(t => t.projectKey === '/ws/0')).toHaveLength(11);
  });

  it('别的项目还有标签时绝不回收当前项目', () => {
    const others = Array.from({ length: 12 }, (_, project) =>
      Array.from({ length: MAX_TABS_PER_PROJECT }, (_, index) =>
        tab(`o${project}-t${index}`, `/ws/other-${project}`)
      )
    ).flat();
    const next = [...others, tab('cur-old', '/ws/cur'), tab('cur-new', '/ws/cur')];
    // 122 个 PTY：需要回收 2 个
    const plan = planPtyBudget(next, 'cur-new', '/ws/cur');

    expect(plan.rejected).toBe(false);
    expect(plan.evicted).toHaveLength(2);
    expect(plan.tabs.filter(t => t.projectKey === '/ws/cur').map(t => t.id)).toEqual([
      'cur-old',
      'cur-new'
    ]);
  });
});

describe('canAddTab', () => {
  it('9 个标签允许新增，10 个拒绝（按钮与 addTab 共用判定）', () => {
    const nine = Array.from({ length: MAX_TABS_PER_PROJECT - 1 }, (_, i) =>
      tab(`t${i}`, '/ws/a')
    );
    expect(canAddTab(nine, '/ws/a')).toBe(true);
    expect(canAddTab([...nine, tab('t9', '/ws/a')], '/ws/a')).toBe(false);
    // 上限按项目独立计算
    expect(canAddTab([...nine, tab('t9', '/ws/a')], '/ws/b')).toBe(true);
  });
});
