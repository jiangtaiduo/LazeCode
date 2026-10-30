// 底部工具窗口尺寸策略（Git Log / 控制台各自独立）。
//
// 规格：
// - 首次安装后第一次打开 Git Log：高度 = 当前 app 窗口高度的 40%；
// - 首次打开控制台：最大化（占满工作区）；
// - 用户拖动改过尺寸后：之后打开各面板沿用该面板上次关闭时的高度。
//
// 旧 key `ide_bottom_panel_height` 是 Git / 控制台共享的单一高度，无法判定
// 它属于哪一个面板；本版按新规格重置一次，不再读取它。新 key 按面板拆分，
// 只有拖动落盘；未落盘时 Git 每次首开按当前窗口重算 40%，控制台保持最大化。

export const GIT_PANEL_HEIGHT_KEY = 'ide_git_panel_height';
export const TERMINAL_PANEL_HEIGHT_KEY = 'ide_terminal_panel_height';
export const TERMINAL_MAXIMIZED_KEY = 'ide_terminal_maximized';

export const MIN_PANEL_HEIGHT = 120;
/** 控制台从最大化还原时的默认高度（与旧版共享高度默认值一致） */
export const DEFAULT_TERMINAL_PANEL_HEIGHT = 340;

/**
 * app 内容区的 CSS 布局高度。
 *
 * 不能用 `window.innerHeight`：浏览器预览的 UI 字号走 CSS `zoom`，此时
 * innerHeight 仍是未缩放视口（例如 900），而元素高度按 zoom 放大，40% 会
 * 变成视觉上的 43%。`documentElement.offsetHeight` 是 CSS 布局高度，乘以
 * zoom 后正好是视觉高度。原生 Tauri 走 webview page zoom，同样应使用
 * 布局视口而不是设备视口。
 */
export const appViewportHeight = (): number => {
  if (typeof document === 'undefined') return 0;
  return (
    document.documentElement.offsetHeight ||
    (typeof window !== 'undefined' ? window.innerHeight : 0) ||
    0
  );
};

const readRaw = (key: string): string | null => {
  try {
    return typeof localStorage === 'undefined' ? null : localStorage.getItem(key);
  } catch {
    /* 隐私模式 / 存储被禁用：当作没有历史值 */
    return null;
  }
};

const readStoredHeight = (key: string, min: number, max: number): number | null => {
  const raw = readRaw(key);
  if (raw === null) return null;
  const saved = Number(raw);
  if (!Number.isFinite(saved) || saved <= 0) return null;
  return Math.round(Math.max(min, Math.min(max, saved)));
};

const readStoredFlag = (key: string): boolean | null => {
  const raw = readRaw(key);
  if (raw === '1') return true;
  if (raw === '0') return false;
  return null;
};

/** 存储层允许的最大高度；实际渲染还会被工作区可用高度二次夹紧。 */
export const maxPanelHeight = (viewportHeight: number): number =>
  Math.max(800, Number.isFinite(viewportHeight) ? Math.round(viewportHeight) : 800);

/** 首次打开 Git Log 的默认高度：当前 app 窗口高度的 40%。 */
export const defaultGitPanelHeight = (viewportHeight: number): number => {
  const vh = Number.isFinite(viewportHeight) && viewportHeight > 0 ? viewportHeight : 800;
  return Math.max(
    MIN_PANEL_HEIGHT,
    Math.min(maxPanelHeight(vh), Math.round(vh * 0.4))
  );
};

export interface PanelLayout {
  gitHeight: number;
  terminalHeight: number;
  /** 控制台首次打开默认最大化；有已保存高度但无显式标记时按非最大化处理 */
  terminalMaximized: boolean;
  /** Git 是否有用户落盘的高度；false 时首开按当前窗口重算 40% */
  gitHeightStored: boolean;
}

/**
 * 读取两个面板的初始布局。
 *
 * `terminalMaximized` 缺省为 true 只在从未保存过控制台高度时成立：只要用户
 * 曾经拖过控制台，即使标记写失败也不会在重启后意外又最大化。
 */
export function readPanelLayout(viewportHeight: number): PanelLayout {
  const max = maxPanelHeight(viewportHeight);
  const git = readStoredHeight(GIT_PANEL_HEIGHT_KEY, MIN_PANEL_HEIGHT, max);
  const terminal = readStoredHeight(TERMINAL_PANEL_HEIGHT_KEY, MIN_PANEL_HEIGHT, max);
  return {
    gitHeight: git ?? defaultGitPanelHeight(viewportHeight),
    terminalHeight:
      terminal ??
      Math.max(MIN_PANEL_HEIGHT, Math.min(max, DEFAULT_TERMINAL_PANEL_HEIGHT)),
    terminalMaximized: readStoredFlag(TERMINAL_MAXIMIZED_KEY) ?? terminal === null,
    gitHeightStored: git !== null
  };
}
