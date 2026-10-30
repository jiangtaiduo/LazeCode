import { beforeEach, describe, expect, it } from 'vitest';
import {
  GIT_PANEL_HEIGHT_KEY,
  TERMINAL_MAXIMIZED_KEY,
  TERMINAL_PANEL_HEIGHT_KEY,
  defaultGitPanelHeight,
  readPanelLayout
} from './panelLayout';

describe('panelLayout', () => {
  beforeEach(() => {
    window.localStorage.clear();
  });

  it('首次安装：Git 为窗口高度 40%，控制台默认最大化', () => {
    const layout = readPanelLayout(1000);
    expect(layout.gitHeight).toBe(400);
    expect(layout.terminalHeight).toBe(340);
    expect(layout.terminalMaximized).toBe(true);
    expect(layout.gitHeightStored).toBe(false);
  });

  it('Git 首开高度按当前窗口计算，不被旧的 800px 上限截断', () => {
    expect(defaultGitPanelHeight(3000)).toBe(1200);
    expect(defaultGitPanelHeight(400)).toBe(160);
    expect(defaultGitPanelHeight(0)).toBe(320);
  });

  it('Git 与控制台使用各自保存的高度', () => {
    window.localStorage.setItem(GIT_PANEL_HEIGHT_KEY, '512.7');
    window.localStorage.setItem(TERMINAL_PANEL_HEIGHT_KEY, '280');
    window.localStorage.setItem(TERMINAL_MAXIMIZED_KEY, '0');

    const layout = readPanelLayout(1000);
    expect(layout.gitHeight).toBe(513);
    expect(layout.terminalHeight).toBe(280);
    expect(layout.terminalMaximized).toBe(false);
    expect(layout.gitHeightStored).toBe(true);
  });

  it('有保存高度但没有最大化标记时不再强制最大化', () => {
    window.localStorage.setItem(TERMINAL_PANEL_HEIGHT_KEY, '280');
    expect(readPanelLayout(1000).terminalMaximized).toBe(false);
  });

  it('恢复明确保存的最大化状态', () => {
    window.localStorage.setItem(TERMINAL_PANEL_HEIGHT_KEY, '280');
    window.localStorage.setItem(TERMINAL_MAXIMIZED_KEY, '1');
    expect(readPanelLayout(1000).terminalMaximized).toBe(true);
  });

  it('显式取消最大化且尚未拖动时使用默认非最大高度', () => {
    window.localStorage.setItem(TERMINAL_MAXIMIZED_KEY, '0');
    const layout = readPanelLayout(1000);
    expect(layout.terminalHeight).toBe(340);
    expect(layout.terminalMaximized).toBe(false);
  });

  it('越界值夹紧到允许范围', () => {
    window.localStorage.setItem(GIT_PANEL_HEIGHT_KEY, '1');
    window.localStorage.setItem(TERMINAL_PANEL_HEIGHT_KEY, '99999');

    const layout = readPanelLayout(1000);
    expect(layout.gitHeight).toBe(120);
    expect(layout.terminalHeight).toBe(1000);
    expect(layout.gitHeightStored).toBe(true);
  });

  it('损坏值回退到默认值', () => {
    window.localStorage.setItem(GIT_PANEL_HEIGHT_KEY, 'abc');
    window.localStorage.setItem(TERMINAL_PANEL_HEIGHT_KEY, '-5');

    const layout = readPanelLayout(1000);
    expect(layout.gitHeight).toBe(400);
    expect(layout.terminalHeight).toBe(340);
    expect(layout.gitHeightStored).toBe(false);
  });

  it('旧共享高度无法归属到单一面板，按新规格重置', () => {
    window.localStorage.setItem('ide_bottom_panel_height', '596');

    const layout = readPanelLayout(1000);
    expect(layout.gitHeight).toBe(400);
    expect(layout.terminalHeight).toBe(340);
    expect(layout.terminalMaximized).toBe(true);
    expect(layout.gitHeightStored).toBe(false);
  });
});
