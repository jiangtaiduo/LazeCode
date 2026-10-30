import { describe, expect, it } from 'vitest';
import { planProjectPrune } from './projectPrune';

const paths = (n: number) => Array.from({ length: n }, (_, i) => `/ws/${i}`);

describe('planProjectPrune', () => {
  it('10 个项目远低于上限：不淘汰、不提示', () => {
    expect(planProjectPrune(paths(10), '/ws/0', new Set(), 20)).toEqual({
      prunable: [],
      pinnedOverflow: 0
    });
  });

  it('22 个项目全部有终端：一个都不淘汰，只报告超限', () => {
    const all = paths(22);
    expect(planProjectPrune(all, '/ws/0', new Set(all), 20)).toEqual({
      prunable: [],
      pinnedOverflow: 1
    });
  });

  it('22 个项目仅 1 个无终端：淘汰无终端的那个，其余保留', () => {
    const all = paths(22);
    const live = new Set(all.filter(p => p !== '/ws/7'));
    const plan = planProjectPrune(all, '/ws/0', live, 20);
    expect(plan.prunable).toEqual(['/ws/7']);
    expect(plan.pinnedOverflow).toBe(0);
  });

  it('当前项目即使无终端也不在候选里', () => {
    const all = paths(23);
    const live = new Set(all.filter(p => p !== '/ws/0'));
    const plan = planProjectPrune(all, '/ws/0', live, 20);
    expect(plan.prunable).toEqual([]);
    expect(plan.pinnedOverflow).toBe(2);
  });
});
