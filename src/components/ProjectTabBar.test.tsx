import { beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen } from '@testing-library/react';

import { ProjectTabBar } from './ProjectTabBar';

// ResizeObserver：jsdom 没有，测溢出裁剪逻辑需要桩
class ResizeObserverStub implements ResizeObserver {
  observe() {}
  unobserve() {}
  disconnect() {}
}
(window as any).ResizeObserver = ResizeObserverStub;

const tabs = [
  { path: '/w/alpha', name: 'alpha' },
  { path: '/w/beta', name: 'beta' },
  { path: '/w/gamma', name: 'gamma' }
];

function setup(overrides?: { activity?: Record<string, { busy: boolean; outputting: boolean }>; activePath?: string }) {
  const onSelect = vi.fn();
  const onClose = vi.fn();
  const onReorder = vi.fn();
  const commands = { current: { '/w/beta': ['npm', 'vite'] } as Record<string, string[]> };
  render(
    <ProjectTabBar
      tabs={tabs}
      activePath={overrides?.activePath ?? '/w/alpha'}
      activity={overrides?.activity ?? {}}
      activityCommandsRef={commands}
      onSelect={onSelect}
      onClose={onClose}
      onReorder={onReorder}
    />
  );
  return { onSelect, onClose, onReorder };
}

describe('ProjectTabBar', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('渲染全部项目 tab，当前项目带 current 类', () => {
    setup();
    expect(screen.getByTitle(/^alpha\b/)).toBeTruthy();
    expect(screen.getByTitle(/^beta\b/)).toBeTruthy();
    expect(screen.getByTitle(/^gamma\b/)).toBeTruthy();
    expect(screen.getByTitle(/^alpha\b/).className).toContain('current');
    expect(screen.getByTitle(/^beta\b/).className).not.toContain('current');
  });

  it('无活动状态时 tab 为灰框（tab-idle）', () => {
    setup();
    expect(screen.getByTitle(/^alpha\b/).className).toContain('tab-idle');
  });

  it('busy 但无输出 → 静态橙框；busy 且输出 → 跑马灯类', () => {
    setup({
      activity: {
        '/w/beta': { busy: true, outputting: false },
        '/w/gamma': { busy: true, outputting: true }
      }
    });
    expect(screen.getByTitle(/^beta\b/).className).toContain('tab-busy');
    expect(screen.getByTitle(/^beta\b/).className).not.toContain('tab-active-output');
    expect(screen.getByTitle(/^gamma\b/).className).toContain('tab-active-output');
  });

  it('点击 tab 触发 onSelect（传项目路径）', () => {
    const { onSelect } = setup();
    fireEvent.click(screen.getByTitle(/^beta\b/));
    expect(onSelect).toHaveBeenCalledWith('/w/beta');
  });

  it('tooltip 含运行中命令名（来自 activityCommandsRef）', () => {
    setup({ activity: { '/w/beta': { busy: true, outputting: false } } });
    expect(screen.getByTitle(/npm, vite/)).toBeTruthy();
  });

  it('× 按钮触发 onClose 且不冒泡成切换', () => {
    const { onClose, onSelect } = setup();
    const closeBtn = screen.getByTitle(/^beta\b/).querySelector('.project-tab-close') as HTMLElement;
    fireEvent.click(closeBtn);
    expect(onClose).toHaveBeenCalledWith('/w/beta');
    expect(onSelect).not.toHaveBeenCalled();
  });

  it('× 按钮触发 onClose 且不冒泡成切换', () => {
    const { onClose, onSelect } = setup();
    const closeBtn = screen.getByTitle(/^beta\b/).querySelector('.project-tab-close') as HTMLElement;
    fireEvent.click(closeBtn);
    expect(onClose).toHaveBeenCalledWith('/w/beta');
    expect(onSelect).not.toHaveBeenCalled();
  });

  it('中键关闭走 onAuxClick 路径', () => {
    const { onClose } = setup();
    fireEvent(
      screen.getByTitle(/^gamma\b/),
      new MouseEvent('auxclick', { button: 1, bubbles: true, cancelable: true })
    );
    expect(onClose).toHaveBeenCalledWith('/w/gamma');
  });

  it('拖拽（位移过阈值）提交 onReorder 且抑制本次 click 切换', () => {
    const { onReorder, onSelect } = setup();
    const tabEls = Array.from(document.querySelectorAll<HTMLElement>('.project-tab'));
    expect(tabEls).toHaveLength(3);
    // jsdom 的 getBoundingClientRect 全 0：按显示序桩出横向区间
    tabEls.forEach((el, i) => {
      el.getBoundingClientRect = () =>
        ({ left: i * 100, right: (i + 1) * 100, top: 0, bottom: 26, width: 100, height: 26 } as DOMRect);
    });
    const tab = screen.getByTitle(/^beta\b/);
    fireEvent.pointerDown(tab, { button: 0, pointerId: 1, clientX: 130 });
    fireEvent.pointerMove(tab, { pointerId: 1, clientX: 250 });
    fireEvent.pointerUp(tab, { pointerId: 1 });
    expect(onReorder).toHaveBeenCalledWith(1, 2);
    fireEvent.click(tab);
    expect(onSelect).not.toHaveBeenCalled();
  });

  it('未过阈值的 pointer 序列仍是普通点击', () => {
    const { onReorder, onSelect } = setup();
    const tab = screen.getByTitle(/^gamma\b/);
    fireEvent.pointerDown(tab, { button: 0, pointerId: 1, clientX: 100 });
    fireEvent.pointerMove(tab, { pointerId: 1, clientX: 102 });
    fireEvent.pointerUp(tab, { pointerId: 1 });
    expect(onReorder).not.toHaveBeenCalled();
    fireEvent.click(tab);
    expect(onSelect).toHaveBeenCalledWith('/w/gamma');
  });

  it('空项目列表渲染拖拽占位（无 tab）', () => {
    render(
      <ProjectTabBar tabs={[]} activePath="" activity={{}} onSelect={vi.fn()} onClose={vi.fn()} onReorder={vi.fn()} />
    );
    expect(document.querySelector('.project-tabbar-empty')).toBeTruthy();
    expect(document.querySelector('.project-tab')).toBeNull();
  });
});
