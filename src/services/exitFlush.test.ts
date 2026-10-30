import { describe, expect, it } from 'vitest';
import { planExitFlush, type ExitFlushBuffer } from './exitFlush';

const buffer = (content: string, overrides: Partial<ExitFlushBuffer> = {}): ExitFlushBuffer => ({
  dirty: true,
  language: 'go',
  error: null,
  externalChanged: false,
  content,
  ...overrides
});

describe('planExitFlush', () => {
  it('当前项目与后台快照的脏缓冲都进入写盘计划', () => {
    const plan = planExitFlush([
      { '/ws/a.go': buffer('current') },
      { '/ws/b.go': buffer('snapshot') }
    ]);
    expect(plan.blocked).toBe(0);
    expect(plan.writes).toEqual([
      { path: '/ws/a.go', content: 'current', encoding: undefined },
      { path: '/ws/b.go', content: 'snapshot', encoding: undefined }
    ]);
  });

  it('同一路径只写当前项目版本（嵌套工作区不去覆盖眼前内容）', () => {
    const plan = planExitFlush([
      { '/ws/a.go': buffer('current') },
      { '/ws/a.go': buffer('snapshot') }
    ]);
    expect(plan.writes).toEqual([{ path: '/ws/a.go', content: 'current', encoding: undefined }]);
  });

  it('跳过 clean / 图片 / 读取失败缓冲', () => {
    const plan = planExitFlush([
      {
        '/ws/clean.go': buffer('x', { dirty: false }),
        '/ws/a.png': buffer('x', { language: 'image' }),
        '/ws/b.go': buffer('x', { error: 'denied' })
      }
    ]);
    expect(plan.writes).toEqual([]);
    expect(plan.blocked).toBe(0);
  });

  it('外部修改计入 blocked，调用方据此取消退出', () => {
    const plan = planExitFlush([
      { '/ws/conflict.go': buffer('local', { externalChanged: true }) }
    ]);
    expect(plan.blocked).toBe(1);
    expect(plan.writes).toEqual([]);
  });
});
