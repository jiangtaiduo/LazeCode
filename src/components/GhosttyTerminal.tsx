// 真实终端面板 · 自适应尺寸与多会话支持
//
// VT 解析、键盘编码、选区、搜索全部在 Rust 侧（原生 libghostty-vt）完成。
// 尺寸根据容器宽高通过 ResizeObserver 自动计算列数与行数，无需用户手动调节。

import React, { useCallback, useEffect, useImperativeHandle, useMemo, useRef, useState } from 'react';
import { appConfirm } from '../services/confirmDialog';
import { IconDockBottom, IconDockRight, IconGrip, IconWindowClose, IconWindowMaximize, IconPlus } from './FileIcons';
import {
  CURSOR_STYLE,
  RowFlag,
  frameCellInto,
  WIDE_SPACER_TAIL,
  WIDE_SPACER_HEAD,
  type CellFrame,
  type FrameStyle,
  type FrameCell
} from '../ghostty-vt/cellFrame';
import { spawnPty, subscribeExit, subscribePwd } from '../services/pty';
import {
  MAX_PANES_PER_TAB,
  MAX_TABS_PER_PROJECT,
  MAX_PTY_TOTAL,
  canAddTab,
  countPanes,
  planPtyBudget,
  type TerminalTab
} from './terminalBudget';
import { useDirtySignal } from '../hooks/useDirtySignal';
import type { PtyHandle } from '../services/pty';
import { invokeCommand } from '../services/tauriBridge';
import { TerminalSource, type SearchOutcome } from '../services/terminalSource';
import { CellDiffCache } from '../ghostty-vt/cellFrame';
import { domKeyToInput, KEY_ACTION } from '../ghostty-vt/domKeys';
import { TerminalTabContextMenu } from './TerminalTabContextMenu';
import { TerminalA11yMirror } from './TerminalA11yMirror';
import { frameToA11yText } from '../ghostty-vt/a11y';
import type { TerminalTabMenuState } from './TerminalTabContextMenu';
import { t } from '../services/i18n';
import {
  attachFilesToPasteText,
  pasteHasFiles,
  toAttachFiles,
  wrapBracketedPaste,
} from '../services/terminalAttach';
import type { TerminalAttachFile } from '../services/terminalAttach';

/**
 * 逐格读取的共享暂存槽（模块级，跨帧复用）。
 *
 * 绘制循环每格都要读一次单元格；若每次返回新对象，60fps 下每秒会
 * 产生约 29 万个短命对象，GC 全部落在帧预算里。两个槽分别承载
 * 「run 首格」与「内层循环的下一格」：首格的样式要在整个 run 的
 * 比对过程中持续有效，不能和内层循环共用同一个槽。
 * paint 是同步且单线程执行的（rAF 驱动，不会重入），共享安全。
 */
const frameCellScratchA: FrameCell = {
  codepoint: 0,
  wide: 0,
  hasText: false,
  style: {
    fg: null,
    bg: null,
    bold: false,
    italic: false,
    faint: false,
    inverse: false,
    invisible: false,
    strikethrough: false
  }
};
const frameCellScratchB: FrameCell = {
  codepoint: 0,
  wide: 0,
  hasText: false,
  style: {
    fg: null,
    bg: null,
    bold: false,
    italic: false,
    faint: false,
    inverse: false,
    invisible: false,
    strikethrough: false
  }
};

interface GhosttyTerminalProps {
  /** PTY 工作目录 */
  cwd?: string;
  /** 指定 shell（默认取 $SHELL / COMSPEC） */
  shell?: string;
  /** 终端字号（跟随偏好设置） */
  fontSize?: number;
  /** UI 变体/配色变化的触发键：变化时重新读取 CSS 主题色 */
  themeKey?: string;
  /** 关闭此终端面板 */
  onClose?: () => void;
  /** 面板当前是否可见；重新可见时若无任何会话则自动补一个 */
  visible?: boolean;
  /** 所属工作区路径：会话按项目分组保活，切换项目互不影响 */
  projectKey?: string;
  /** 当前窗口仍打开的项目；不在列表里的项目终端会话会被回收 */
  openProjects?: string[];
  /** 停靠位置：bottom 底部（默认）| right 贴右侧，由父组件持久化 */
  placement?: 'bottom' | 'right';
  /** 正在拖拽把柄时命中的吸附预览区（用于把柄反馈高亮） */
  snapTarget?: 'right' | 'bottom' | null;
  /** 按住把柄开始拖拽：父组件负责吸附判定与位置切换（Pointer Capture 保证出窗也能收到松手） */
  onGripPointerDown?: (e: React.PointerEvent) => void;
  /** 一键切换停靠位置（底部 ⇄ 右侧） */
  onTogglePlacement?: () => void;
  /** 当前面板是否处于最大化状态（贴底时高度扩展至 85vh） */
  maximized?: boolean;
  /** 最大化/还原面板切换 */
  onToggleMaximize?: () => void;
  /**
   * 终端 spawn 的工作目录。单文件工作区的 projectKey 是文件路径，不能直接
   * 当 cwd 用；父组件传文件所在目录，目录工作区传工作区本身。
   */
  spawnCwd?: string;
  /** 状态栏提示（标签上限 / 预算耗尽等）；未提供时静默。 */
  onNotice?: (message: string) => void;
  /** 有常驻终端的项目变化：父组件据此 pin 住这些项目的快照，避免被淘汰。 */
  onLiveProjectsChange?: (projects: string[]) => void;
}

/** 父组件（关闭项目）在销毁 PTY 前调用：有命令在跑时弹确认。 */
export interface GhosttyTerminalHandle {
  confirmProjectClose: (projectKey: string) => Promise<boolean>;
}

interface Rgb {
  r: number;
  g: number;
  b: number;
}

/** CSS 主题变量 -> 0xRRGGBB；解析失败返回 null，让调用方回退。 */
function cssColorToInt(raw: string): number | null {
  const value = raw.trim();
  const hex = /^#([0-9a-f]{3}|[0-9a-f]{6})$/i.exec(value);
  if (hex) {
    const h = hex[1];
    const full = h.length === 3 ? h.split('').map(c => c + c).join('') : h;
    return parseInt(full, 16);
  }
  const rgb = /^rgba?\((\d+)\s*,\s*(\d+)\s*,\s*(\d+)/i.exec(value);
  if (rgb) {
    return (Number(rgb[1]) << 16) | (Number(rgb[2]) << 8) | Number(rgb[3]);
  }
  return null;
}

const FALLBACK_FG: Rgb = { r: 255, g: 255, b: 255 };
const FALLBACK_BG: Rgb = { r: 40, g: 44, b: 52 };

const FONT_FAMILY = '"JetBrains Mono", "SF Mono", Menlo, Consolas, "PingFang SC", "Hiragino Sans GB", "Microsoft YaHei", monospace';
const DEFAULT_FONT_SIZE = 13;

/**
 * Kitty 图形协议里的「Unicode 占位格」码点。
 *
 * 走占位格模式的应用（omp、yazi、ranger 等）不直接给放置坐标，而是在网格里
 * 写 U+10EEEE + 组合附加符，由终端把图片画在这些格子上。终端若不认这个协议，
 * 这些格子就会被当普通字符渲染成豆腐块 —— 用户看到的「一堆方块」。
 * 这里显式不画字形：图片由 canvas 覆盖绘制。
 */
const PLACEHOLDER_CODEPOINT = 0x10eeee;

/** 该码点是否属于占位格（含组合附加符区间，避免残留豆腐块） */
const isPlaceholderCodepoint = (cp: number): boolean =>
  cp === PLACEHOLDER_CODEPOINT || (cp >= 0x0300 && cp <= 0x036f);

/** 一次图片放置（后端 `vt_images` 的 camelCase DTO） */
interface KittyPlacement {
  imageId: number;
  placementId: number;
  /** 视口列/行（格子坐标，可为负） */
  x: number;
  y: number;
  cols: number;
  rows: number;
  pixelWidth: number;
  pixelHeight: number;
  sourceX: number;
  sourceY: number;
  sourceWidth: number;
  sourceHeight: number;
  z: number;
  isVirtual: boolean;
}

interface KittyImageMeta {
  id: number;
  width: number;
  height: number;
  generation: number;
}


const CURSOR_BLINK_MS = 530;
/** 删光拼音后的退格豁免窗口：此窗口内的非合成 Backspace/Delete 视为
 *  「取消那一下」吞掉不进 PTY；窗口过期 = 确无拼音，退格正常删 shell。 */
const PREEDIT_CANCEL_WINDOW_MS = 100;
const SELECTION_FILL = 'rgba(53, 116, 240, 0.35)';

const rgbCss = (c: Rgb | null, fallback: Rgb): string => {
  const v = c ?? fallback;
  return `rgb(${v.r},${v.g},${v.b})`;
};

const rgbFromInt = (value: number | null, fallback: Rgb): Rgb => {
  if (value === null) return fallback;
  return { r: (value >> 16) & 0xff, g: (value >> 8) & 0xff, b: value & 0xff };
};

function sameStyle(x: FrameStyle, y: FrameStyle): boolean {
  return (
    x.bold === y.bold &&
    x.italic === y.italic &&
    x.faint === y.faint &&
    x.inverse === y.inverse &&
    x.strikethrough === y.strikethrough &&
    x.fg === y.fg &&
    x.bg === y.bg
  );
}

/**
 * 一行是否与上一帧逐格相同。
 *
 * packed u64 拆成两个 u32 存在 `words` 里，逐格比两个 u32 即可覆盖
 * 码点、宽度、样式索引、has_text 全部字段。样式索引相同即样式相同
 * （样式表由 Rust 侧按内容生成，同样式必得同一索引）。
 */
function rowUnchanged(prevWords: Uint32Array, words: Uint32Array, row: number, cols: number): boolean {
  const base = row * cols * 2;
  for (let i = 0; i < cols * 2; i++) {
    if (prevWords[base + i] !== words[base + i]) return false;
  }
  return true;
}

const generateSessionId = () => `term_${Date.now()}_${Math.random().toString(36).slice(2, 9)}`;

/** OSC 7 的 pwd 是 file:// URI；OSC 9/1337 通常是裸路径。统一成可显示路径。 */
function normalizePwd(value: string): string {
  if (!value.startsWith('file://')) return value;
  try {
    const url = new URL(value);
    return decodeURIComponent(url.pathname);
  } catch {
    return value;
  }
}

const URL_RE = /https?:\/\/[^\s<>"'`]+/g;

/**
 * 从一行的 cell 文本里找点击位置附近的 http(s) 链接（Cmd+Click 打开）。
 * 宽度字符的 x 与 JS 字符串下标不完全等价；链接通常是 ASCII，误差可接受。
 */
function urlAtCell(frame: CellFrame, col: number, row: number): string | null {
  if (row < 0 || row >= frame.rows) return null;
  const scratch: FrameCell = { codepoint: 0, wide: 0, hasText: false, style: frame.styles[0] };
  let line = '';
  for (let x = 0; x < frame.cols; x++) {
    frameCellInto(frame, x, row, scratch);
    line += scratch.hasText && (scratch.cluster || scratch.codepoint > 0)
      ? (scratch.cluster ?? String.fromCodePoint(scratch.codepoint))
      : ' ';
  }
  URL_RE.lastIndex = 0;
  for (let m = URL_RE.exec(line); m; m = URL_RE.exec(line)) {
    const start = m.index;
    const end = start + m[0].length;
    if (col >= start && col < end) {
      return m[0].replace(/[),.;]+$/, '');
    }
  }
  return null;
}

/**
 * 由宿主内容尺寸推导终端网格（cols/rows）。
 *
 * `contentW/H` 必须是 `.terminal-canvas-host` 的 **content-box** 尺寸
 * （`ResizeObserver.contentRect` 或 `offsetWidth/offsetHeight` 都已是扣掉
 * padding 后的值）。历史上这里按 `getBoundingClientRect() - 16` 手扣 padding，
 * 与 RO 的 contentRect 口径不一致，同一宿主会算出两套行列 —— 表现就是
 * PTY 停在旧网格、prompt 折行、光标被挤到第二行行首，整片区域不出内容。
 * 现在两条路径都收敛到这个纯函数，口径只有一处。
 *
 * 单元格尺寸给 0（度量尚未完成）时退回默认值，避免除零。
 */
export function gridSizeFromHost(
  contentW: number,
  contentH: number,
  cellW: number,
  cellH: number
): { cols: number; rows: number } {
  // 宿主不可见（display:none 时盒子为 0）：保持一个合法的占位网格，
  // 由调用方决定是否真的下发 resize
  const w = Number.isFinite(contentW) ? contentW : 0;
  const h = Number.isFinite(contentH) ? contentH : 0;
  const cw = cellW > 0 ? cellW : 8;
  const ch = cellH > 0 ? cellH : 18;
  return {
    cols: Math.max(20, Math.floor(w / cw)),
    rows: Math.max(4, Math.floor(h / ch))
  };
}

/**
 * 显式终止一个 PTY 会话。
 *
 * 只有在这里调：组件的 cleanup（SingleGhosttySession 卸载时）覆盖不到
 * 「面板收起但终端容器不卸载」这条路径 —— App.tsx 的 onClose 只是
 * setActiveBottomTool(null)，GhosttyTerminal 仍挂着，会话继续跑。
 * fire-and-forget：命令失败时后端已记日志，UI 上无需额外提示。
 */
const closePtyById = (sessionId: string) => {
  void invokeCommand('pty_close', { sessionId }).catch(() => {
    // 会话已不在（例如 shell 自己退出了）：无需处理
  });
};

/**
 * 关闭会话前的确认：终端里还有命令在跑时先问一句（对齐 PyCharm / Terminal.app
 * 的「进程仍在运行」提示）。判定在后端做（`ps -t <tty>` 看终端上除 shell 之外
 * 还有没有进程），前端只负责把命令名摆到弹窗里。
 * 探测失败一律按「不忙」处理：宁可少提示，也不能让用户关不掉控制台。
 */
const confirmCloseSessions = async (sessionIds: string[]): Promise<boolean> => {
  if (!sessionIds.length) return true;
  const names: string[] = [];
  await Promise.all(
    sessionIds.map(async sessionId => {
      try {
        const info = await invokeCommand<{ busy: boolean; commands: string[] }>(
          'pty_session_busy',
          { sessionId }
        );
        if (info?.busy) names.push(...(info.commands ?? []));
      } catch {
        // 会话已不在（shell 自己退出过）：没什么可确认的
      }
    })
  );
  if (!names.length) return true;
  const shown = [...new Set(names)].slice(0, 3).join('、');
  return appConfirm(t().dialog.consoleRunningConfirm(shown), {
    title: t().dialog.consoleRunningTitle,
    kind: 'warning',
    okLabel: t().dialog.consoleRunningOk,
    cancelLabel: t().dialog.cancel
  });
};

export const GhosttyTerminal = React.forwardRef<GhosttyTerminalHandle, GhosttyTerminalProps>(function GhosttyTerminal({ cwd, shell, onClose, fontSize = DEFAULT_FONT_SIZE, themeKey, visible = true, projectKey = '', openProjects = [], placement = 'bottom', snapTarget = null, onGripPointerDown, onTogglePlacement, maximized = false, onToggleMaximize, spawnCwd, onNotice, onLiveProjectsChange }, ref) {
  // 多会话管理：每个会话与窗口都拥有唯一的 session_id，避免跨窗口/跨实例会话冲突与卡死
  // 会话编号持续递增：关闭「控制台 1」后新开的会话不会重名
  // 会话按 projectKey（工作区路径）分组：窗口内切换项目时其它项目的会话
  // 保持挂载（display:none）继续运行，PTP 与屏幕状态都不受影响
  const nameCounter = useRef(1);
  /** 新增标签的确认/状态更新是异步的：防连点与自动补标签重入。 */
  const addTabBusy = useRef(false);
  const [tabs, setTabs] = useState<TerminalTab[]>([]);
  /** 最新 tabs / projectKey 的同步镜像：await 确认框返回后不能再读渲染期闭包。 */
  const tabsRef = useRef<TerminalTab[]>(tabs);
  tabsRef.current = tabs;
  const projectKeyRef = useRef(projectKey);
  projectKeyRef.current = projectKey;
  // 每个项目记住自己的活动标签，切回时恢复
  const [activeTabByProject, setActiveTabByProject] = useState<Record<string, string>>({});
  // 懒激活：面板首次可见时才挂载会话组件（即才 spawn PTY），
  // 应用启动时不偷偷开 shell；一旦激活过，隐藏面板也不卸载，会话得以保留
  const [everShown, setEverShown] = useState<Set<string>>(() => new Set());
  // 终端标签栏右键菜单（对齐 PyCharm Terminal 标签右键：Close/Close Others/Close All/Rename）
  const [tabMenu, setTabMenu] = useState<TerminalTabMenuState>({ x: 0, y: 0, tabId: null });
  // 重命名输入：tabs[].name 直接改，session id 不变，PTY 不受影响
  const [renamingTabId, setRenamingTabId] = useState<string | null>(null);
  const [renameDraft, setRenameDraft] = useState('');

  const projectTabs = tabs.filter(t => t.projectKey === projectKey);
  const canAdd = canAddTab(tabs, projectKey);
  /**
   * 有常驻终端的项目集合（稳定签名）。父组件用它 pin 住这些项目的会话快照：
   * 快照淘汰是 UI 优化，不能顺手 kill 掉正在跑的任务。
   */
  const liveProjectsSig = useMemo(
    () => [...new Set(tabs.map(t => t.projectKey))].filter(Boolean).sort().join('\0'),
    [tabs]
  );
  const lastLiveProjectsSig = useRef('');
  useEffect(() => {
    if (liveProjectsSig === lastLiveProjectsSig.current) return;
    lastLiveProjectsSig.current = liveProjectsSig;
    onLiveProjectsChange?.(liveProjectsSig ? liveProjectsSig.split('\0') : []);
  }, [liveProjectsSig, onLiveProjectsChange]);
  /**
   * 当前活动标签 id，带自愈：
   * activeTabByProject 里可能存着一个已不存在 tab 的 id —— 会话自己退出
   * (removeExitedPane) / 关标签都没同步清理它。此时活动 id 失配，表现为
   * 标签栏没有高亮、内容区空白。
   *
   * 所以读的时候校验「它真的在 projectTabs 里」，不在就回落到第一个标签。
   * 比在每个移除点都记得清理更稳：新增移除路径不会重新引入该缺陷。
   */
  const storedActiveId = activeTabByProject[projectKey];
  const storedActiveExists = !!storedActiveId && projectTabs.some(t => t.id === storedActiveId);
  const activeTabId = storedActiveExists ? storedActiveId : (projectTabs[0]?.id ?? '');
  const activeTabIdRef = useRef(activeTabId);
  activeTabIdRef.current = activeTabId;

  useEffect(() => {
    if (!visible) return;
    const tab = tabs.find(t => t.id === activeTabId && t.projectKey === projectKey);
    if (!tab) return;
    setEverShown(prev => {
      let next: Set<string> | null = null;
      for (const paneId of tab.panes) {
        if (!prev.has(paneId)) {
          if (!next) next = new Set(prev);
          next.add(paneId);
        }
      }
      return next ?? prev;
    });
  }, [visible, activeTabId, tabs, projectKey]);

  const addTab = useCallback(async () => {
    if (!projectKey) return;
    // 每个分屏对应一个常驻 PTY（shell 进程 + 读线程 + fd），设上限防资源无界消耗。
    // 按钮在达到上限时禁用；这里兜住自动补标签等非按钮路径。
    const currentTabs = tabsRef.current;
    if (!canAddTab(currentTabs, projectKey)) {
      onNotice?.(t().terminal.tabLimitReached(MAX_TABS_PER_PROJECT));
      return;
    }
    if (addTabBusy.current) return;
    addTabBusy.current = true;
    try {
      const name = t().terminal.tabName(nameCounter.current++);
      const newId = generateSessionId();
      const paneId = generateSessionId();
      const newTab: TerminalTab = {
        id: newId,
        name,
        projectKey,
        panes: [paneId],
        split: 'none',
        activePaneId: paneId
      };
      let next: TerminalTab[] = [...currentTabs, newTab];
      // 全窗口 PTY 预算：分屏也计入。优先回收别的项目的旧标签，当前项目的
      // 最后一个标签不回收；一个标签可能带多个分屏，因此按 PTY 数循环扣减。
      // 计划是纯函数（terminalBudget.ts），100 个常驻会话不触发任何回收。
      let plan = planPtyBudget(next, newId, projectKey);
      if (plan.rejected) {
        onNotice?.(t().terminal.consoleBudgetExceeded);
        return;
      }
      // 预算回收会 kill 掉别的项目里可能正在跑命令的终端：先走与关标签
      // 相同的「进程仍在运行」确认。取消则整个新增放弃。
      const confirmedEvicted = [...plan.evicted];
      if (plan.evicted.length && !(await confirmCloseSessions(plan.evicted))) return;

      // 确认框可能停留很久：期间 shell 自己退出、别的标签被关，tabs 已变。
      // 用 ref 重新读，再从最新状态重算预算，不能把旧快照写回去。
      if (projectKeyRef.current !== projectKey) return;
      next = [...tabsRef.current, newTab];
      plan = planPtyBudget(next, newId, projectKey);
      if (plan.rejected) {
        onNotice?.(t().terminal.consoleBudgetExceeded);
        return;
      }
      const hasNewVictims = plan.evicted.some(id => !confirmedEvicted.includes(id));
      if (hasNewVictims && !(await confirmCloseSessions(plan.evicted))) return;

      next = plan.tabs;
      const evicted = plan.evicted;
      if (evicted.length) {
        evicted.forEach(pane => closePtyById(pane));
        setEverShown(prev => {
          let updated: Set<string> | null = null;
          for (const pane of evicted) {
            if (prev.has(pane)) {
              if (!updated) updated = new Set(prev);
              updated.delete(pane);
            }
          }
          return updated ?? prev;
        });
      }
      tabsRef.current = next;
      setTabs(next);
      setActiveTabByProject(prev => ({ ...prev, [projectKey]: newId }));
    } finally {
      addTabBusy.current = false;
    }
  }, [projectKey, onNotice]);

  // 切换项目已淘汰（会话快照超过上限）时，终端会话也要跟着回收，
  // 否则隐藏 PTY 会无限累积。openProjects 为空同样要清：关闭最后一个
  // 项目后 projectKey 变空、快照列表清空，旧 tabs 不能留到重开同一项目时
  // 被复用（它们的 PTY 已经终止，会显示成死终端）。
  useEffect(() => {
    if (tabs.length === 0) return;
    const keep = new Set([projectKey, ...openProjects].filter(Boolean));
    const stale = tabs.filter(t => !keep.has(t.projectKey));
    if (stale.length === 0) return;
    const staleIds = new Set(stale.map(t => t.id));
    const stalePanes = new Set(stale.flatMap(t => t.panes));
    setTabs(prev => prev.filter(t => !staleIds.has(t.id)));
    setEverShown(prev => {
      const next = new Set(prev);
      stalePanes.forEach(id => next.delete(id));
      return next;
    });
    setActiveTabByProject(prev => {
      const next = { ...prev };
      stale.forEach(t => {
        if (next[t.projectKey]) delete next[t.projectKey];
      });
      return next;
    });
    stalePanes.forEach(id => closePtyById(id));
  }, [openProjects, projectKey, tabs]);

  // 切换到某个项目且该项目还没有会话时，自动补一个新会话
  useEffect(() => {
    if (!visible || !projectKey) return;
    if (tabs.some(t => t.projectKey === projectKey)) return;
    void addTab();
  }, [visible, projectKey, tabs, addTab]);

  const closeTab = async (tabId: string, e?: React.MouseEvent) => {
    e?.stopPropagation();
    const closed = tabsRef.current.find(t => t.id === tabId);
    if (!closed) return;
    // 关掉会 kill 掉 shell 里的进程：有命令在跑时先确认（PyCharm 同款行为）
    if (!(await confirmCloseSessions(closed.panes))) return;
    // 确认框期间 shell 退出 / 别的标签关闭都会改 tabs；从最新 ref 重取，
    // 不能把 await 前的数组写回去（那会把已退出的标签复活成死标签）。
    const remaining = tabsRef.current.filter(t => t.id !== tabId);
    tabsRef.current = remaining;
    setTabs(remaining);
    // 显式终止 PTY：面板收起时 GhosttyTerminal 不卸载，SingleGhosttySession
    // 的 cleanup 不会跑，Rust 侧的 shell 子进程、读线程与 fd 会一直留着。
    // 后端对不存在的 session 是幂等 no-op，重复调安全。
    closed.panes.forEach(pane => closePtyById(pane));
    // everShown 只用于「曾经可见过就不再懒挂载」，会话已终结，
    // 条目留着只会单调增长；删掉后同 id 重开会重新懒激活。
    setEverShown(prev => {
      let next: Set<string> | null = null;
      for (const pane of closed.panes) {
        if (prev.has(pane)) {
          if (!next) next = new Set(prev);
          next.delete(pane);
        }
      }
      return next ?? prev;
    });
    if (activeTabIdRef.current === tabId) {
      const siblings = remaining.filter(t => t.projectKey === closed.projectKey);
      const closedIdx = siblings.findIndex(t => t.id === tabId);
      const nextActive = siblings[Math.max(0, closedIdx - 1)];
      setActiveTabByProject(prev => ({ ...prev, [closed.projectKey]: nextActive?.id ?? '' }));
    }
    // 当前项目的最后一个标签被关闭：终止会话并收起面板；下次打开将创建全新会话。
    // 用 projectKeyRef 判断：确认框期间用户可能已切到别的项目，不能把新面板收起。
    if (closed.projectKey === projectKeyRef.current && !remaining.some(t => t.projectKey === closed.projectKey)) {
      onClose?.();
    }
  };

  // 关闭其他 / 关闭全部：一次 setTabs 完成，避免逐个 closeTab 时后一次
  // 回调读到前一次还没提交的快照（React 18+ 批处理下表现为菜单点了没反应）。
  // 关闭其他 → 保留被右键的标签，面板不收起。
  // 关闭全部 → 当前项目无剩余标签，与 closeTab 的末标签语义一致：收起面板。
  const closeOtherTabsInProject = async (keepTabId: string) => {
    const doomed = tabs.filter(t => t.projectKey === projectKey && t.id !== keepTabId);
    if (!doomed.length) return;
    const doomedPanes = doomed.flatMap(t => t.panes);
    if (!(await confirmCloseSessions(doomedPanes))) return;
    setTabs(prev => prev.filter(t => !(t.projectKey === projectKey && t.id !== keepTabId)));
    setEverShown(prev => {
      const next = new Set(prev);
      doomedPanes.forEach(id => next.delete(id));
      return next;
    });
    doomedPanes.forEach(id => closePtyById(id));
  };

  const closeAllTabsInProject = async () => {
    const doomed = tabs.filter(t => t.projectKey === projectKey);
    if (!doomed.length) return;
    const doomedPanes = doomed.flatMap(t => t.panes);
    if (!(await confirmCloseSessions(doomedPanes))) return;
    setTabs(prev => prev.filter(t => t.projectKey !== projectKey));
    setEverShown(prev => {
      const next = new Set(prev);
      doomedPanes.forEach(id => next.delete(id));
      return next;
    });
    setActiveTabByProject(prev => ({ ...prev, [projectKey]: '' }));
    // 同 closeTab：onClose 只收起面板、组件不卸载，PTY 必须显式终止
    doomedPanes.forEach(id => closePtyById(id));
    onClose?.();
  };

  const renameTab = (tabId: string, name: string) => {
    const trimmed = name.trim();
    if (!trimmed) return;
    setTabs(prev => prev.map(t => (t.id === tabId ? { ...t, name: trimmed } : t)));
  };

  /**
   * 某个分屏的 shell 退出：只移除该分屏；标签内还有分屏时保留标签。
   * 最后一个分屏退出才移除标签，并在当前项目无标签时收起面板。
   *
   * 不需要在这里同步 activeTabByProject —— activeTabId 的读侧会自愈
   * （见它的注释）：死 id 自动回落到同项目第一个标签。
   */
  const removeExitedPane = useCallback(
    (tabId: string, paneId: string) => {
      const tab = tabs.find(t => t.id === tabId);
      const lastPaneOfProject =
        !!tab &&
        tab.panes.length === 1 &&
        tab.projectKey === projectKey &&
        !tabs.some(t => t.id !== tabId && t.projectKey === projectKey);
      setTabs(prev => {
        const current = prev.find(t => t.id === tabId);
        if (!current) return prev;
        const panes = current.panes.filter(id => id !== paneId);
        if (panes.length === 0) return prev.filter(t => t.id !== tabId);
        const nextActive =
          current.activePaneId === paneId
            ? panes[Math.max(0, current.panes.indexOf(paneId) - 1)]
            : current.activePaneId;
        return prev.map(t =>
          t.id === tabId
            ? {
                ...t,
                panes,
                activePaneId: nextActive,
                split: panes.length === 1 ? 'none' : t.split
              }
            : t
        );
      });
      setEverShown(prev => {
        if (!prev.has(paneId)) return prev;
        const next = new Set(prev);
        next.delete(paneId);
        return next;
      });
      if (lastPaneOfProject) onClose?.();
    },
    [tabs, projectKey, onClose]
  );

  /** 分屏：新建一个 PTY，并把它设为该标签的活动分屏。 */
  const splitPane = useCallback(
    (tabId: string, direction: 'row' | 'column') => {
      const tab = tabs.find(t => t.id === tabId);
      if (!tab || tab.panes.length >= MAX_PANES_PER_TAB) return;
      if (countPanes(tabs) >= MAX_PTY_TOTAL) {
        onNotice?.(t().terminal.consoleBudgetExceeded);
        return;
      }
      const paneId = generateSessionId();
      setTabs(prev =>
        prev.map(t =>
          t.id === tabId
            ? {
                ...t,
                panes: [...t.panes, paneId],
                split: direction,
                activePaneId: paneId
              }
            : t
        )
      );
      if (visible && tab.projectKey === projectKey) {
        setEverShown(prev => new Set(prev).add(paneId));
      }
    },
    [tabs, visible, projectKey, onNotice]
  );

  /** 关闭指定分屏：先确认，再 kill PTY；最后一个分屏关闭时走标签移除逻辑。 */
  const closePane = useCallback(
    async (tabId: string, paneId: string) => {
      const tab = tabs.find(t => t.id === tabId);
      if (!tab || tab.panes.length <= 1) return;
      if (!(await confirmCloseSessions([paneId]))) return;
      void closePtyById(paneId);
      removeExitedPane(tabId, paneId);
    },
    [tabs, removeExitedPane]
  );

  const setActivePane = useCallback((tabId: string, paneId: string) => {
    setTabs(prev =>
      prev.map(t => (t.id === tabId && t.activePaneId !== paneId ? { ...t, activePaneId: paneId } : t))
    );
  }, []);

  /** 父组件关闭项目前调用：只检查该项目拥有的 tabs，有命令在跑则弹确认。 */
  const confirmProjectClose = useCallback(
    (key: string) =>
      confirmCloseSessions(
        tabsRef.current.filter(t => t.projectKey === key).flatMap(t => t.panes)
      ),
    []
  );
  useImperativeHandle(ref, () => ({ confirmProjectClose }), [confirmProjectClose]);

  return (
    <div className="ghostty-terminal-wrapper">
      {/* Terminal Tab Bar（只显示当前项目的会话；双击顶栏最大化/还原） */}
      <div className="terminal-header-bar" onDoubleClick={() => onToggleMaximize?.()}>
        <div className="terminal-tabs-list" onDoubleClick={e => e.stopPropagation()}>
          {projectTabs.map(tab => {
            const active = tab.id === activeTabId;
            return (
              <div
                key={tab.id}
                className={`terminal-tab-item ${active ? 'active' : ''}`}
                onClick={() => setActiveTabByProject(prev => ({ ...prev, [projectKey]: tab.id }))}
                onContextMenu={e => {
                  e.preventDefault();
                  e.stopPropagation();
                  setRenamingTabId(null);
                  setTabMenu({ x: e.clientX, y: e.clientY, tabId: tab.id });
                }}
              >
                <span className="tab-term-icon">{'>_'}</span>
                {renamingTabId === tab.id ? (
                  <input
                    className="tab-term-rename"
                    value={renameDraft}
                    autoFocus
                    onChange={ev => setRenameDraft(ev.target.value)}
                    onClick={ev => ev.stopPropagation()}
                    onBlur={() => {
                      renameTab(tab.id, renameDraft);
                      setRenamingTabId(null);
                    }}
                    onKeyDown={ev => {
                      // Esc 取消、Enter 提交；不冒泡，避免触发全局按键
                      ev.stopPropagation();
                      if (ev.key === 'Escape') {
                        setRenamingTabId(null);
                      } else if (ev.key === 'Enter') {
                        renameTab(tab.id, renameDraft);
                        setRenamingTabId(null);
                      }
                    }}
                  />
                ) : (
                  <span className="tab-term-name">{tab.name}</span>
                )}
                <button
                  className="tab-term-close"
                  onClick={e => closeTab(tab.id, e)}
                  title={t().terminal.closeTab}
                >
                  <IconWindowClose size={12} />
                </button>
              </div>
            );
          })}
          <button
            className="terminal-add-btn"
            onClick={() => void addTab()}
            disabled={!canAdd}
            title={
              canAdd
                ? t().terminal.newTab
                : t().terminal.tabLimitReached(MAX_TABS_PER_PROJECT)
            }
          >
            <IconPlus size={13} />
          </button>
        </div>

        <div className="terminal-header-actions">
          {onGripPointerDown && (
            <span
              className={`terminal-dock-grip${snapTarget ? ' snapping' : ''}`}
              onPointerDown={onGripPointerDown}
              title={t().terminal.gripDrag}
            >
              <IconGrip size={12} />
            </span>
          )}
          {onTogglePlacement && (
            <button
              className="terminal-tool-btn"
              onClick={onTogglePlacement}
              title={placement === 'right' ? t().terminal.moveToBottom : t().terminal.dockToRight}
            >
              {placement === 'right' ? <IconDockBottom size={13} /> : <IconDockRight size={13} />}
            </button>
          )}
          {onToggleMaximize && (
            <button
              className="terminal-tool-btn"
              onClick={onToggleMaximize}
              title={maximized ? t().terminal.restoreHeight : t().terminal.maximizeHeight}
            >
              <IconWindowMaximize size={12} />
            </button>
          )}
          {onClose && (
            <button className="terminal-tool-btn" onClick={onClose} title={t().terminal.minimizeClosePanel}>
              <IconWindowClose size={13} />
            </button>
          )}
        </div>
      </div>

      {/* Terminal View：所有已激活过的分屏都保持挂载（后台继续运行），
          仅当前项目的活动标签可见；切走的项目/标签以 display:none 保活 */}
      <div className="terminal-body-container">
        {tabs.map(tab => {
          const tabVisible = tab.projectKey === projectKey && tab.id === activeTabId;
          return (
            <div
              key={tab.id}
              className={`terminal-tab-panes terminal-split-${tab.split}`}
              style={{ display: tabVisible ? 'flex' : 'none' }}
            >
              {tab.panes.map(paneId =>
                everShown.has(paneId) ? (
                  <div
                    key={paneId}
                    className={`terminal-pane${tabVisible && tab.activePaneId === paneId ? ' active' : ''}`}
                    onMouseDown={() => setActivePane(tab.id, paneId)}
                  >
                    {tab.panes.length > 1 && (
                      <button
                        type="button"
                        className="terminal-pane-close"
                        title={t().terminal.closePane}
                        onMouseDown={event => event.stopPropagation()}
                        onClick={event => {
                          event.stopPropagation();
                          void closePane(tab.id, paneId);
                        }}
                      >
                        <IconWindowClose size={10} />
                      </button>
                    )}
                    <SingleGhosttySession
                      sessionId={paneId}
                      cwd={
                        tab.projectKey === projectKey
                          ? spawnCwd ?? tab.projectKey ?? cwd
                          : tab.projectKey
                      }
                      shell={shell}
                      fontSize={fontSize}
                      themeKey={themeKey}
                      active={visible && tabVisible && tab.activePaneId === paneId}
                      onScreen={visible && tabVisible}
                      placement={placement}
                      onExited={() => removeExitedPane(tab.id, paneId)}
                      onActivate={() => setActivePane(tab.id, paneId)}
                    />
                  </div>
                ) : null
              )}
            </div>
          );
        })}
      </div>

      <TerminalTabContextMenu
        menu={tabMenu}
        onClose={() => setTabMenu({ x: 0, y: 0, tabId: null })}
        onCloseTab={closeTab}
        onCloseOthers={closeOtherTabsInProject}
        onCloseAll={closeAllTabsInProject}
        onRename={tabId => {
          const t = tabs.find(x => x.id === tabId);
          if (!t) return;
          setRenameDraft(t.name);
          setRenamingTabId(tabId);
        }}
        cwd={tabs.find(x => x.id === tabMenu.tabId)?.projectKey}
        onCopyWorkingDirectory={cwd => {
          // 剪贴板失败静默：菜单只负责把路径丢给系统，状态栏提示留给后续需求
          navigator.clipboard?.writeText(cwd).catch(() => {});
        }}
        canSplit={
          (tabs.find(x => x.id === tabMenu.tabId)?.panes.length ?? MAX_PANES_PER_TAB) < MAX_PANES_PER_TAB &&
          countPanes(tabs) < MAX_PTY_TOTAL
        }
        canClosePane={(tabs.find(x => x.id === tabMenu.tabId)?.panes.length ?? 0) > 1}
        onSplitRight={tabId => splitPane(tabId, 'row')}
        onSplitDown={tabId => splitPane(tabId, 'column')}
        onClosePane={tabId => {
          const tab = tabs.find(x => x.id === tabId);
          if (tab && tab.panes.length > 1) void closePane(tab.id, tab.activePaneId);
        }}
      />
    </div>
  );
});

interface SingleGhosttySessionProps {
  sessionId: string;
  cwd?: string;
  shell?: string;
  fontSize?: number;
  themeKey?: string;
  /** 面板可见且这是当前活动会话：挂载/激活后自动聚焦画布 */
  active?: boolean;
  /**
   * 该会话当前是否在屏上（终端面板可见 + 所属标签是项目活动标签）。
   * 分屏标签里的所有分屏都为 true，即使只有 activePaneId 持有焦点：
   * 它们都在画，都需要 PTY 脏信号。隐藏标签/项目的会话为 false，
   * 不订阅事件（见 useDirtySignal）。
   */
  onScreen?: boolean;
  /** 停靠位置：底部 ⇄ 右侧切换后宿主主轴变了，必须重算网格 */
  placement?: 'bottom' | 'right';
  /** 会话退出（shell 结束 / 被关闭）时回调：父组件移除对应分屏 */
  onExited?: () => void;
  /** 用户点进该分屏：父组件把它设为标签内的活动分屏 */
  onActivate?: () => void;
}

const SingleGhosttySession: React.FC<SingleGhosttySessionProps> = ({ sessionId, cwd, shell, fontSize = DEFAULT_FONT_SIZE, themeKey, active = false, onScreen = active, placement = 'bottom', onExited, onActivate }) => {
  // 字体规格由字号推导，改动后重新度量单元格网格
  const fontSpec = `${fontSize}px ${FONT_FAMILY}`;
  const hostRef = useRef<HTMLDivElement>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const sourceRef = useRef<TerminalSource | null>(null);
  const ptyRef = useRef<PtyHandle | null>(null);


  // compositionend 后标记：中文上屏的时序是 compositionend → 紧跟一个 keydown
  // （用户按回车/空格确认候选词）。此时 isComposing 已经是 false，
  // 若不抑制，这个 keydown 会被当成普通按键多发一个字符（如空格）。
  const compositionDoneRef = useRef(false);

  // 删光拼音的判定时间戳：preedit 非空→空的跳变（任意来源：keydown 顶部
  // value 同步、onInput、composition 空数据）记下时刻。实测 WebKit 在
  // 「退格删光拼音」时既不发 compositionend 也不发 compositionupdate ——
  // 残影与吞字符两个 bug 同源；setTimeout 与补发 keydown 的次序也由实现
  // 决定（上一版因此失效）。时间戳窗口与事件次序无关。
  const preeditClearedAtRef = useRef(0);

  // 合成中的 preedit（拼音）文本：PTY 收不到（未上屏），canvas 必须自己画，
  // 否则输入期间屏幕毫无反馈（隐藏 textarea 是 opacity:0）。
  const compositionTextRef = useRef('');

  // 单元格网格（由真实字体度量得出）
  const cellW = useRef(8);
  const cellH = useRef(18);

  // 选区拖拽状态
  const dragStart = useRef<{ x: number; y: number } | null>(null);
  const dragEnd = useRef<{ x: number; y: number } | null>(null);
  /** 按下鼠标时的锚点：单击（锚点=抬起点）不产生选区，只有拖出范围才算选中 */
  const dragAnchor = useRef<{ x: number; y: number } | null>(null);
  /** 是否处于拖拽中（mousedown 打开，window 级 mouseup 收尾） */
  const dragging = useRef(false);
  /** 终端是否请求鼠标跟踪（轮询缓存；Shift 仍强制进入本地选区） */
  const mouseTracking = useRef(false);
  /** 正在转发给 TUI 的鼠标按键；非 null 时移动事件走 mouse motion */
  const mouseForwardButton = useRef<number | null>(null);
  const mouseLastCell = useRef<{ x: number; y: number } | null>(null);
  /** 选区文本缓存：mouseup 时从引擎取一次，Cmd+C / copy 事件直接取用，避免异步拿不到 */
  const selectionText = useRef('');
  /**
   * 上一次真正绘制过的选区行范围。
   * canvas 是叠加绘制：选区缩小/清空时，离开选区的行「内容没变」会被行级比对
   * 跳过，旧高亮就永远留在画面上（点击空白后选中不消失的根因）。
   * 绘制时对比新旧范围，把并集统统标成必须重绘。
   */
  const paintedSelRows = useRef<{ min: number; max: number } | null>(null);
  /** 上一帧视口顶行号（scrollbar.offset；-1 = 尚无帧）。滚动位移量的真实来源 */
  const lastScrollOffset = useRef(-1);

  // 上一次绘制时的格尺寸。字号/面板变化会改格宽格高，此时所有行的
  // 像素位置都失效，必须全屏重绘而不是行级比对。
  const prevCellSize = useRef({ w: 0, h: 0 });

  // 上一帧真正把光标块画在了哪一行（-1 = 没画：光标隐藏或闪烁相位是灭）。
  // 行级脏判定靠它决定要擦掉哪一行。
  const cursorDrawnRow = useRef(-1);
  const cursorDrawnCell = useRef<{ row: number; col: number } | null>(null);

  // 光标闪烁
  const blinkOn = useRef(true);

  // 渲染合并
  const dirty = useRef(true);
  /** 隐藏轮询的唤醒入口：切回屏上时立即恢复 rAF，不等最多 1s 的兜底定时器 */
  const wakeLoop = useRef<(() => void) | null>(null);
  /** 脏信号回调的稳定引用：useDirtySignal 的 effect 依赖它 */
  const markDirty = useCallback(() => {
    dirty.current = true;
  }, []);
  // 只有屏上会话订阅 PTY 脏信号：100 个后台控制台常驻时，每帧事件若都进
  // WebView 会变成 6000 次 eval/s（见 useDirtySignal 注释）。切回屏上时
  // hook 会主动 markDirty 补一帧，原生引擎的脏状态不会丢。
  useDirtySignal(sessionId, onScreen, markDirty);
  // 从隐藏变为屏上：立即踢醒渲染循环，避免最多 1s 内还显示切走前的旧帧
  useEffect(() => {
    if (onScreen) wakeLoop.current?.();
  }, [onScreen]);
  const inFlight = useRef(false);
  /** 光标状态镜像：paint 高频执行，值未变时靠它跳过 setState */
  const cursorInfoRef = useRef<{ visible: boolean; style: number; blinking: boolean } | null>(null);
  const pendingScroll = useRef(0);
  /**
   * 最近一次滚轮所在的单元格与修饰键。
   * 转发路径（程序开了鼠标跟踪 / 备用屏转方向键）要用 1 基格坐标，
   * 本地回滚不用 —— 但两者共用同一个下发口，故一起记。
   */
  const wheelAt = useRef<{ x: number; y: number; mods: number }>({ x: 0, y: 0, mods: 0 });
  const lastFrame = useRef<CellFrame | null>(null);
  /**
   * 上一帧真正画过的内容副本（行级重绘的比对基准）。
   *
   * 必须是拷贝：增量帧路径下 `frame.words` 与缓存缓冲是同一个对象，
   * 下一帧会被就地改写，引用它做比对等于自己跟自己比。
   */
  const paintBaseline = useRef<Uint32Array | null>(null);
  /** 上一帧画过的行语义标记：OSC 133 prompt 行变化时强制重绘 */
  const paintRowFlags = useRef<Uint16Array | null>(null);
  /** 上一次绘制时的网格尺寸（用于识别 resize 后选区失效）。 */
  const paintGrid = useRef<{ cols: number; rows: number } | null>(null);
  /** canvas 上一次写入的 CSS 尺寸（避免每帧无条件写样式触发同步布局）。 */
  const canvasCssSize = useRef<{ w: number; h: number }>({ w: -1, h: -1 });
  /**
   * 帧缓存：增量路径的屏幕。
   *
   * 全量帧重建它，增量帧就地覆盖脏行，paint 始终从它取整屏视图。
   * 没有它就只能每帧拉全量 —— 那正是本次优化要消灭的开销。
   */
  const frameCache = useRef(new CellDiffCache());
  /**
   * 内联图片（Kitty 图形协议）缓存：imageId → 已解码位图 + 内容代际。
   * 代际变化（同 id 重传）时丢弃旧位图重新解码。
   */
  const kittyImages = useRef(new Map<number, { bitmap: ImageBitmap; generation: number }>());
  /** 当前视口的图片放置（每帧随快照一起刷新；滚动即变） */
  const kittyPlacements = useRef<KittyPlacement[]>([]);
  /** 图片查询在途标记：避免每帧并发发起同一批 IPC */
  const kittyFetching = useRef(false);
  /** 图片放置/像素发生变化 → 下一帧必须整屏重绘（行级比对看不到图片） */
  const kittyDirty = useRef(false);
  /** 无图片会话的 vt_images 探测帧计数：每 10 次刷新探一次（见 refresh） */
  const kittyProbeFrames = useRef(0);
  /**
   * IME 锚点去重键：`${cursorX},${cursorY},${cellW},${cellH}`。
   * 输入法候选框跟随隐藏 textarea 的矩形，paint 每帧都要保证 textarea
   * 钉在光标格上；用键比对避免光标未动时反复写 style 触发布局。
   */
  const imeAnchorKey = useRef('');

  const [cols, setCols] = useState(100);
  const [rows, setRows] = useState(24);
  // 网格尺寸的 ref 镜像：ResizeObserver 只读当前行列做比对，
  // 若把 cols/rows 写进它的依赖数组，每次 setCols/setRows 都会
  // disconnect + 重建 observer（LeakAudit 实测 4 次重建），
  // 而 observer 重建本身又会触发一次尺寸回调，形成放大回路。
  const gridRef = useRef({ cols: 100, rows: 24 });
  gridRef.current = { cols, rows };
  /**
   * PTY 实际生效的网格尺寸（`spawn` 与 `pty_resize` 的最近一次成功下发值）。
   *
   * 为什么不能拿 `gridRef`（前端 state）代替：state 在 spawn 之前就可能已被
   * 首次 applyHostSize 推进到宿主算出的行列，而那一刻 `ptyRef.current` 还是
   * null（spawn 尚未 resolve），resize 是空操作。等 spawn 完成再次
   * applyHostSize 时，`next` 与 `gridRef` 已经相等 → 提前 return，
   * PTY 永远停在 spawn 时的 100×24。表现就是引擎视口长度 24 而前端按宿主
   * 高度算出 42 行来画：canvas 下半部分是空行，用户看到「内容只占上半屏」。
   */
  const ptySizeRef = useRef({ cols: 0, rows: 0, cellW: 0, cellH: 0, synced: false });
  /**
   * 宿主尺寸签名（`${offsetWidth}x${offsetHeight}`）：渲染循环据此判断是否需要
   * 重新同步网格。抵消 ResizeObserver 漏回调的场景，见 applyHostSize 的注释。
   */
  const lastHostSig = useRef('');
  const [cursorInfo, setCursorInfo] = useState<{
    visible: boolean;
    style: number;
    blinking: boolean;
  } | null>(null);
  const [ready, setReady] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // 屏幕阅读器镜像：Canvas 本身没有可读文本，按帧节流同步到隐藏的 role=log。
  const [a11yText, setA11yText] = useState('');
  const a11yLastAt = useRef(0);
  const a11yLastText = useRef('');
  // shell integration 报告的 cwd；未启用时回退到 spawn 的 cwd。
  const [pwd, setPwd] = useState<string | undefined>(cwd);
  useEffect(() => setPwd(cwd), [cwd]);
  const [searchOpen, setSearchOpen] = useState(false);
  const [searchQuery, setSearchQuery] = useState('');
  const [searchOutcome, setSearchOutcome] = useState<SearchOutcome | null>(null);
  const [searchBusy, setSearchBusy] = useState(false);
  const searchInputRef = useRef<HTMLInputElement>(null);

  // 字体度量
  const measureCell = useCallback((): void => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const ctx = canvas.getContext('2d');
    if (!ctx) return;
    ctx.font = fontSpec;
    const sample = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789';
    const sampleWidth = ctx.measureText(sample).width;
    // 必须取整：`pty_resize` 的 cell_width_px / cell_height_px 是 u32，
    // 小数会被 Tauri 的 IPC 反序列化整条拒绝（"invalid type: floating point"），
    // 于是每次 applyHostSize 都静默失败，PTY 停在 spawn 时的网格，
    // canvas 只画旧行数、下方露出宿主底色 —— 「内容只占上半屏」。
    // 等宽字体的格宽取整后仍是整数，绘制侧按同一值取整不会错位。
    cellW.current = Math.max(1, Math.round(sampleWidth / sample.length));
    cellH.current = Math.max(1, Math.round(fontSize * 1.38));
  }, [fontSize]);

  // 绘制一帧
  const updateA11y = useCallback((frame: CellFrame): void => {
    const now = typeof performance !== 'undefined' ? performance.now() : Date.now();
    // 500ms 节流：读屏不需要 60fps，频繁替换 live region 反而会打断朗读。
    if (now - a11yLastAt.current < 500) return;
    a11yLastAt.current = now;
    const text = frameToA11yText(frame);
    if (text !== a11yLastText.current) {
      a11yLastText.current = text;
      setA11yText(text);
    }
  }, []);

  const paint = useCallback((frame: CellFrame): void => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const ctx = canvas.getContext('2d');
    if (!ctx) return;

    // 逐格读取的暂存对象：绘制循环每格都要读一次，返回新对象会让
    // 60fps 下每秒产生 ~29 万个短命对象。两个独立槽位分别承载
    // 「run 首格」与「待比较的下一格」—— 首格的样式要在整个 run
    // 的比对里持续有效，不能和内层循环共用同一个槽。
    const cellA = frameCellScratchA;
    const cellB = frameCellScratchB;

    const cw = cellW.current;
    const ch = cellH.current;
    const dpr = window.devicePixelRatio || 1;
    const termCols = frame.cols;
    const termRows = frame.rows;
    const widthPx = termCols * cw;
    // 画布高度必须贴住宿主：`termRows` 来自 PTY，resize 尚未落地（或失败）
    // 时它小于宿主容量，canvas 只画到旧行数，余下的高度露出
    // `.terminal-canvas-host` 的 CSS 底色 —— 用户看到「内容只占上半屏、
    // 下半空白」。这里把绘制高度抬到「不小于宿主」并按整行取整，
    // 空缺的行由下面的默认底色铺满。
    const hostEl = hostRef.current;
    const hostH = hostEl && hostEl.clientHeight > 4 ? hostEl.clientHeight : 0;
    const hostRows = hostH > 0 && ch > 0 ? Math.ceil(hostH / ch) : 0;
    const drawRows = Math.max(termRows, hostRows);
    const heightPx = drawRows * ch;

    // 行边界按设备像素取整再换回 CSS 坐标：ch/cw 常是分数，fillRect 抗锯齿
    // 会让相邻两行在边界设备像素上各盖一部分 —— 若该像素下层是旧内容（如
    // 选区蓝），两次 source-over 后残留 旧色×p(1-p)≈8% 的 1px 细线；行内容
    // 未变时 rowUnchanged 不再重擦，细线就此冻结（框选后点击留下蓝色细线的根因）。
    // 同一组整数边界让相邻行的矩形无缝拼接、对每个设备像素恰好全覆盖一次。
    const rowTopDev = (r: number) => Math.round(r * ch * dpr);
    const rowTop = (r: number) => rowTopDev(r) / dpr;
    const rowBottom = (r: number) => rowTopDev(r + 1) / dpr;

    const targetWidth = Math.round(widthPx * dpr);
    const targetHeight = Math.round(heightPx * dpr);
    // 尺寸变化（字号调节、面板 resize）必须全屏重绘：
    // canvas 赋 width/height 会清空内容，且格宽变了所有行的位置都失效
    const sizeChanged = canvas.width !== targetWidth || canvas.height !== targetHeight
      || cellW.current !== prevCellSize.current.w || cellH.current !== prevCellSize.current.h;
    if (sizeChanged) {
      canvas.width = targetWidth;
      canvas.height = targetHeight;
      prevCellSize.current = { w: cellW.current, h: cellH.current };
    }
    // CSS 尺寸只在真的变化时写：这两个赋值会让布局失效，而 paint 后面
    // 还要读 `canvas.offsetLeft/Top`（IME 锚点）—— 每帧无条件写就等于
    // 每帧强制一次同步布局。稳态下 cols/rows 与格宽都不变，写一次就够。
    if (canvasCssSize.current.w !== widthPx || canvasCssSize.current.h !== heightPx) {
      canvas.style.width = `${widthPx}px`;
      canvas.style.height = `${heightPx}px`;
      canvasCssSize.current = { w: widthPx, h: heightPx };
    }

    ctx.save();
    ctx.scale(dpr, dpr);

    const defaultBg = rgbFromInt(frame.defaultBg, FALLBACK_BG);
    const defaultFg = rgbFromInt(frame.defaultFg, FALLBACK_FG);

    // 行级脏判定。终端帧的特点是「大部分行完全不变」：光标在动、
    // 一行在刷新，其余行原封不动。全屏重绘要把每格的字形重排一遍
    // 再 fillText，行数一多（40+ 行）就成了主要绘制成本。
    //
    // 策略：保留**上一帧真正画过的**内容作为基准，逐行比对 packed u64。
    // 必须是独立的一份拷贝，不能引用 `frame.words`：增量帧路径下
    // `frame.words` 就是缓存自己的缓冲（`frameView` 零拷贝），
    // 下一帧 `applyDiff` 会就地改它 —— 自己跟自己比恒「未变」，
    // 除了光标行以外的内容会永远不再重绘（实测过的画面冻结）。
    const baseline = paintBaseline.current;
    const prevWords =
      baseline && baseline.length === frame.words.length ? baseline : null;
    // 图片放置变化（增删/移位/换图）没有对应的单元格变更，行级比对会跳过
    // 这些行，旧像素就留在 canvas 上。这类帧强制整屏重绘（图片不常见，代价可控）。
    const kittyRepaint = kittyDirty.current;
    kittyDirty.current = false;
    const fullRedraw = !prevWords || sizeChanged || kittyRepaint;
    const rowFlags = frame.rowFlags;
    const prevRowFlags =
      paintRowFlags.current && paintRowFlags.current.length === rowFlags.length
        ? paintRowFlags.current
        : null;

    const cursorVisible = frame.cursorVisible && (!cursorInfo?.blinking || blinkOn.current);
    // 光标所在的两行必须重绘：旧位置要擦掉、新位置要画上。
    // 光标形状/闪烁相位变化只动这两行，不动整个屏幕。
    //
    // `cursorDrawnRow` 记上一帧真的把光标块画在了哪一行（-1 = 没画，
    // 因为光标隐藏或闪烁相位是灭）。闪烁相位翻转时它是灭之前的行，
    // 这一行必须重绘才能把块擦掉。
    // 合成 preedit 画在光标行：合成期间该行也必须每帧重绘，否则内容未变 +
    // 闪烁相位为灭时会被 rowUnchanged 跳过，拼音的增删更新画不上去。
    const preeditActive = compositionTextRef.current.length > 0;
    const cursorRow = (cursorVisible || preeditActive) && frame.cursorY < termRows ? frame.cursorY : -1;
    const prevCursorRow = cursorDrawnRow.current;
    const previousCursorCell = cursorDrawnCell.current;
    const cursorCleanupRow = previousCursorCell &&
      (previousCursorCell.row !== frame.cursorY || previousCursorCell.col !== frame.cursorX)
      ? previousCursorCell.row
      : -1;

    // 网格尺寸变化（面板拖拽 / PTY resize 视口重排）：内容按新列宽重新折行，
    // 视口坐标系整体失效，旧端点无法映射 —— 清掉本地选区，避免蓝色残影。
    const prevGrid = paintGrid.current;
    if (prevGrid && (prevGrid.cols !== frame.cols || prevGrid.rows !== frame.rows)) {
      dragStart.current = null;
      dragEnd.current = null;
      dragAnchor.current = null;
      selectionText.current = '';
    }

    // 视口滚动在这里统一吸收（滚轮/程序滚动/回到底部跳变都算）：选区端点是
    // 视口坐标，内容整体上移 dy 行端点就要减 dy，否则高亮钉在旧行上——
    // 「框选后留下蓝色条纹」的根因。用 scrollbar.offset 的真实位移而非请求的
    // 滚动量：顶部/底部钳制后的实际滚动、跳转位移都以此为准。
    // 引擎侧选区存的是绝对网格坐标，天然随内容走，这里只管本地 overlay。
    const curOffset = frame.scrollbar.offset;
    const lastOffset = lastScrollOffset.current;
    lastScrollOffset.current = curOffset;
    if (lastOffset >= 0 && curOffset !== lastOffset) {
      const dy = curOffset - lastOffset;
      if (dragStart.current) dragStart.current = { x: dragStart.current.x, y: dragStart.current.y - dy };
      if (dragEnd.current) dragEnd.current = { x: dragEnd.current.x, y: dragEnd.current.y - dy };
      if (dragAnchor.current) dragAnchor.current = { x: dragAnchor.current.x, y: dragAnchor.current.y - dy };
    }

    if (fullRedraw) {
      ctx.fillStyle = rgbCss(defaultBg, FALLBACK_BG);
      ctx.fillRect(0, 0, widthPx, heightPx);
    }

    ctx.font = fontSpec;
    ctx.textBaseline = 'top';

    // 选区高亮绘制范围。选区变化时内容行本身没变，行级比对会跳过它们，
    // 高亮就永远画不上去；反过来，选区缩小/清空时离开选区的行也必须重绘，
    // 否则旧高亮会残留（canvas 叠加绘制）。这里取「上次画过的范围 ∪ 本次范围」，
    // 两端都覆盖，画完再记录本次范围。
    const sStart = dragStart.current;
    const sEnd = dragEnd.current;
    const curSelMin = sStart && sEnd ? Math.min(sStart.y, sEnd.y) : -1;
    const curSelMax = sStart && sEnd ? Math.max(sStart.y, sEnd.y) : -1;
    const prevSel = paintedSelRows.current;
    const selMinRow = curSelMin < 0 ? (prevSel ? prevSel.min : -1) : prevSel ? Math.min(curSelMin, prevSel.min) : curSelMin;
    const selMaxRow = curSelMax < 0 ? (prevSel ? prevSel.max : -1) : prevSel ? Math.max(curSelMax, prevSel.max) : curSelMax;
    paintedSelRows.current = curSelMin < 0 ? null : { min: curSelMin, max: curSelMax };

    for (let row = 0; row < termRows; row++) {
      let redrawRow = fullRedraw;
      if (!redrawRow) {
        if (row === cursorRow || row === prevCursorRow || row === cursorCleanupRow) {
          // 光标移动：擦掉整行重画（光标块本身可能覆盖字形，不能只补矩形）
          redrawRow = true;
        } else if (row >= selMinRow && row <= selMaxRow) {
          // 选区覆盖的行：内容可能没变，但高亮层要跟着选区走
          redrawRow = true;
        } else if (prevRowFlags !== null && prevRowFlags[row] !== rowFlags[row]) {
          // OSC 133 prompt 标记变化：行内容可能没变，但提示带必须跟着变
          redrawRow = true;
        } else if (prevWords !== null && rowUnchanged(prevWords, frame.words, row, termCols)) {
          continue;
        } else {
          redrawRow = true;
        }
      }
      if (redrawRow) {
        // 先整行擦成默认底色。必须做：canvas 是叠加绘制，不擦的话
        // 上一帧的残影（尤其是背景色不同的 run）会透出来。
        // 边界用设备像素取整值，保证与相邻行、与绘制矩形完全无缝覆盖。
        ctx.fillStyle = rgbCss(defaultBg, FALLBACK_BG);
        const rTop = rowTop(row);
        ctx.fillRect(0, rTop, widthPx, rowBottom(row) - rTop);
        if (rowFlags[row] & RowFlag.Prompt) {
          // 极淡的 prompt 带：不遮字形，只提示 shell integration 的命令起点
          ctx.fillStyle = 'rgba(53, 116, 240, 0.07)';
          ctx.fillRect(0, rTop, widthPx, rowBottom(row) - rTop);
        }
      }

      const y = rowTop(row);
      let col = 0;
      while (col < termCols) {
        const first = frameCellInto(frame, col, row, cellA);
        const firstStyle = first.style;
        const isSpacer = first.wide === WIDE_SPACER_TAIL || first.wide === WIDE_SPACER_HEAD;
        if (isSpacer) {
          col++;
          continue;
        }

        let runLen = 1;
        let runHasGlyph = first.hasText && first.codepoint > 0x20 && !isPlaceholderCodepoint(first.codepoint);
        while (col + runLen < termCols) {
          const next = frameCellInto(frame, col + runLen, row, cellB);
          const nextSpacer = next.wide === WIDE_SPACER_TAIL || next.wide === WIDE_SPACER_HEAD;
          if (nextSpacer) {
            runLen++;
            continue;
          }
          if (!sameStyle(firstStyle, next.style)) break;
          if (next.hasText && next.codepoint > 0x20 && !isPlaceholderCodepoint(next.codepoint)) {
            runHasGlyph = true;
          }
          runLen++;
        }

        const runWidth = runLen * cw;
        const x = col * cw;

        let bg = firstStyle.bg !== null ? rgbFromInt(firstStyle.bg, defaultBg) : null;
        let fg = firstStyle.fg !== null ? rgbFromInt(firstStyle.fg, defaultFg) : defaultFg;

        if (firstStyle.inverse) {
          const tmp = bg ?? defaultBg;
          bg = fg;
          fg = tmp;
        }

        if (bg) {
          ctx.fillStyle = rgbCss(bg, defaultBg);
          ctx.fillRect(x, y, runWidth, rowBottom(row) - y);
        }

        if (runHasGlyph) {
          ctx.fillStyle = rgbCss(fg, defaultFg);
          if (firstStyle.bold) ctx.font = `bold ${fontSpec}`;
          else if (firstStyle.italic) ctx.font = `italic ${fontSpec}`;
          else ctx.font = fontSpec;

          // 字形一律逐格锚定到格缘，不合并成一次 fillText。
          // 原因见下面注释：字体的自然步进 ≠ 取整格宽，合并即累积偏差。

          const textYOffset = Math.round((ch - fontSize) / 2);
          // 逐格锚定到 `i * cw` 后，文字末端与 cursorX 对应的格缘严格重合，
          // 与光标/选区/图片放置共用同一套格子语义。代价是每格多出
          // (cw - 自然步进) = 0.17px 的空隙，亚像素级看不见；
          // 换来的是零累积、零字形变形（另一条路是把整行横向缩放
          // cw/步进，但会让字宽拉伸 2.2%）。
          for (let i = 0; i < runLen; i++) {
            const cell = frameCellInto(frame, col + i, row, cellB);
            // spacer 无字形，只收束相邻段
            if (cell.wide === WIDE_SPACER_TAIL || cell.wide === WIDE_SPACER_HEAD) {
              continue;
            }
            // 占位格（U+10EEEE）不画字形：图片由 canvas 覆盖绘制，
            // 交给字体只会得到一堆豆腐块
            if (!cell.hasText || cell.codepoint <= 0x20 || isPlaceholderCodepoint(cell.codepoint)) {
              continue;
            }
            ctx.fillText(
              cell.cluster ?? String.fromCodePoint(cell.codepoint),
              (col + i) * cw,
              y + textYOffset
            );
          }
        }

        col += runLen;
      }
    }

    // 内联图片（Kitty 图形协议）：画在文字之上、选区高亮之下。
    // 坐标用视口格坐标换算，与格子渲染共用同一套 rowTop/cw，滚动即随帧更新。
    const kittyNow = kittyPlacements.current;
    if (kittyNow.length) {
      for (const p of kittyNow) {
        const entry = kittyImages.current.get(p.imageId);
        if (!entry) continue;
        // 占位格模式的图片必须与它覆盖的格子严格对齐：用格子尺寸反算渲染
        // 尺寸，而不是后端按整数格子像素算出的值（差半个像素就会出现错缝）。
        const dw = p.isVirtual && p.cols > 0 ? p.cols * cw : p.pixelWidth;
        const dh = p.isVirtual && p.rows > 0 ? p.rows * ch : p.pixelHeight;
        if (dw <= 0 || dh <= 0) continue;
        const sw = p.sourceWidth || entry.bitmap.width;
        const sh = p.sourceHeight || entry.bitmap.height;
        try {
          ctx.drawImage(
            entry.bitmap,
            p.sourceX,
            p.sourceY,
            sw,
            sh,
            p.x * cw,
            rowTop(p.y),
            dw,
            dh
          );
        } catch {
          // 位图已关闭（图片被替换）：丢弃缓存，下一帧重新拉
          entry.bitmap.close();
          kittyImages.current.delete(p.imageId);
        }
      }
    }

    // 绘制选区高亮
    if (sStart && sEnd) {
      ctx.fillStyle = SELECTION_FILL;
      const minRow = Math.min(sStart.y, sEnd.y);
      const maxRow = Math.max(sStart.y, sEnd.y);

      for (let r = minRow; r <= maxRow; r++) {
        let cStart = 0;
        let cEnd = termCols - 1;
        if (minRow === maxRow) {
          cStart = Math.min(sStart.x, sEnd.x);
          cEnd = Math.max(sStart.x, sEnd.x);
        } else if (r === minRow) {
          cStart = sStart.y <= sEnd.y ? sStart.x : sEnd.x;
        } else if (r === maxRow) {
          cEnd = sStart.y <= sEnd.y ? sEnd.x : sStart.x;
        }

        const sx = cStart * cw;
        const sw = (cEnd - cStart + 1) * cw;
        const sy = rowTop(r);
        ctx.fillRect(sx, sy, sw, rowBottom(r) - sy);
      }
    }

    // 绘制光标 / 合成 preedit
    const preedit = compositionTextRef.current;
    // 纯空白 preedit 不画：fillText 看不见，但下划线会在行尾留一道短横残影
    if (preedit && preedit.trim() && frame.cursorX < termCols && frame.cursorY < termRows) {
      // 合成输入期间：拼音画在光标格并让位块光标。textYOffset 与字形绘制
      // 同一公式，preedit 与已上屏文字的基线严格对齐。
      const cx = frame.cursorX * cw;
      const cy = rowTop(frame.cursorY);
      ctx.font = fontSpec; // run 绘制可能改成 bold/italic，preedit 用常规字重
      ctx.fillStyle = rgbCss(defaultFg, FALLBACK_FG);
      const textYOffset = Math.round((ch - fontSize) / 2);
      ctx.fillText(preedit, cx, cy + textYOffset);
      // 只画预编辑文本，不再叠加底部横线：横线会被用户误认为光标残影，
      // 且与终端真实 block/underline 光标叠在同一行时尤其明显。
      cursorDrawnRow.current = frame.cursorY;
      cursorDrawnCell.current = { row: frame.cursorY, col: frame.cursorX };
    } else if (cursorVisible && frame.cursorX < termCols && frame.cursorY < termRows) {
      const cx = frame.cursorX * cw;
      const cy = rowTop(frame.cursorY);
      const cbot = rowBottom(frame.cursorY);
      ctx.fillStyle = rgbCss(defaultFg, FALLBACK_FG);

      switch (frame.cursorStyle) {
        case CURSOR_STYLE.UNDERLINE:
          ctx.fillRect(cx, cbot - 2, cw, 2);
          break;
        case CURSOR_STYLE.BAR:
          ctx.fillRect(cx, cy, 2, cbot - cy);
          break;
        case CURSOR_STYLE.BLOCK_HOLLOW:
          // wire 3=block_hollow：线框块。此前没有该分支，落 default 被画成
          // 细竖条 —— 用户看到的「光标样式不对」（行尾 ▌）。
          ctx.strokeStyle = rgbCss(defaultFg, FALLBACK_FG);
          ctx.lineWidth = 1;
          ctx.strokeRect(cx + 0.5, cy + 0.5, cw - 1, cbot - cy - 1);
          break;
        case CURSOR_STYLE.BLOCK:
        default:
          // 未知样式回退实心块（此前 default 指向 BAR 分支）
          ctx.globalAlpha = 0.75;
          ctx.fillRect(cx, cy, cw, cbot - cy);
          ctx.globalAlpha = 1.0;
          break;
      }
      // 记下这一帧画了光标的行：下一帧的行级判定靠它擦除
      cursorDrawnRow.current = frame.cursorY;
      cursorDrawnCell.current = { row: frame.cursorY, col: frame.cursorX };
      // 保留上一帧真正画过的单元格：光标闪烁灭时可能已经移动到新列，
      // 若此时把锚点清空，下一帧只擦新位置，旧 underline / block 会留下轨迹。
      // 下一帧恢复可见时 cursorCleanupRow 会强制重绘旧行。
    }

    ctx.restore();

    // IME 候选框跟随光标：macOS/WKWebView 把候选框锚定在焦点元素的
    // 矩形上，textarea 必须实时钉在光标格上（自身坐标系内），否则
    // 候选框会固定出现在窗口左上角。cursorX/Y 越界（待换行等瞬态）
    // 时保留上一次锚点不回跳。
    const ta = textareaRef.current;
    if (ta && frame.cursorX < termCols && frame.cursorY < termRows) {
      const anchorKey = `${frame.cursorX},${frame.cursorY},${cw},${ch}`;
      if (anchorKey !== imeAnchorKey.current) {
        imeAnchorKey.current = anchorKey;
        // canvas.offsetLeft/Top 与 textarea 的包含块同源（都是
        // .terminal-canvas-host 的定位坐标系），相加即光标格左上角
        ta.style.left = `${canvas.offsetLeft + frame.cursorX * cw}px`;
        ta.style.top = `${canvas.offsetTop + rowTop(frame.cursorY)}px`;
        ta.style.width = `${cw}px`;
        ta.style.height = `${rowBottom(frame.cursorY) - rowTop(frame.cursorY)}px`;
      }
    }

    // 更新光标状态
    const nextCursor = {
      visible: frame.cursorVisible,
      style: frame.cursorStyle,
      blinking: blinkOn.current
    };
    if (
      cursorInfoRef.current &&
      cursorInfoRef.current.visible === nextCursor.visible &&
      cursorInfoRef.current.style === nextCursor.style &&
      cursorInfoRef.current.blinking === nextCursor.blinking
    ) {
      // 高频输出下每帧都到这里：值未变时直接跳过 setState，避免无谓的渲染调度
    } else {
      cursorInfoRef.current = nextCursor;
      setCursorInfo(nextCursor);
    }

    // 记录本帧真正画过的内容与网格尺寸：下一帧的行级比对、resize 判定
    // 都以这份拷贝为准（见 paintBaseline 的注释）。
    if (!paintBaseline.current || paintBaseline.current.length !== frame.words.length) {
      paintBaseline.current = new Uint32Array(frame.words.length);
    }
    paintBaseline.current.set(frame.words);
    paintRowFlags.current = rowFlags.slice();
    paintGrid.current = { cols: frame.cols, rows: frame.rows };
    updateA11y(frame);
  }, [fontSpec, fontSize, updateA11y]);

  /** 从全量帧取光标等标量，供缓存导出视图 */
  const frameCursorOf = (frame: CellFrame) => ({
    cursorX: frame.cursorX,
    cursorY: frame.cursorY,
    cursorVisible: frame.cursorVisible,
    cursorPendingWrap: frame.cursorPendingWrap,
    defaultFg: frame.defaultFg,
    defaultBg: frame.defaultBg,
    cursorColor: frame.cursorColor,
    cursorStyle: frame.cursorStyle,
    scrollbar: frame.scrollbar
  });

  // 取快照
  //
  // 增量优先：`snapshotDiff` 只送回变动的行（帧小一个量级），合并进
  // 帧缓存后照旧走 `paint` 的行级比对 —— 两条路径汇到同一处绘制。
  //
  // `needsFull` 由调用方设置：首帧、resize、字号变化、缓存失效时整屏重取。
  // 增量帧解码失败（协议错位）自动退回一次全量，不把终端留在旧画面上。
  /**
   * 刷新内联图片：拉放置几何（每帧都可能变）并补齐缺失的像素。
   *
   * 像素只在缓存缺失或代际变化时拉，且走二进制回包；解码成 ImageBitmap
   * 后画在 canvas 上。任何失败都静默（图片是增强，不该影响终端可用性）。
   */
  const refreshKittyImages = useCallback(async (): Promise<void> => {
    if (kittyFetching.current) return;
    kittyFetching.current = true;
    try {
      const meta = await invokeCommand<{
        generation: number;
        placements: KittyPlacement[];
        images: KittyImageMeta[];
      }>('vt_images', { sessionId });
      const next = meta?.placements ?? [];
      const changed =
        next.length !== kittyPlacements.current.length ||
        next.some((p, i) => {
          const prev = kittyPlacements.current[i];
          return (
            !prev ||
            prev.imageId !== p.imageId ||
            prev.x !== p.x ||
            prev.y !== p.y ||
            prev.pixelWidth !== p.pixelWidth ||
            prev.pixelHeight !== p.pixelHeight
          );
        });
      kittyPlacements.current = next;
      if (changed) kittyDirty.current = true;

      for (const img of meta?.images ?? []) {
        const cached = kittyImages.current.get(img.id);
        if (cached && cached.generation === img.generation) continue;
        try {
          const buf = await invokeCommand<ArrayBuffer>('vt_image_data', {
            sessionId,
            imageId: img.id
          });
          if (!buf || buf.byteLength <= 8) continue;
          const view = new DataView(buf);
          const width = view.getUint32(0, true);
          const height = view.getUint32(4, true);
          if (!width || !height || buf.byteLength < 8 + width * height * 4) continue;
          const pixels = new Uint8ClampedArray(buf, 8, width * height * 4);
          const bitmap = await createImageBitmap(new ImageData(pixels, width, height));
          cached?.bitmap.close();
          kittyImages.current.set(img.id, { bitmap, generation: img.generation });
          kittyDirty.current = true;
        } catch {
          // 像素还没就绪（分片未传完）：下一帧再试
        }
      }
    } catch {
      // 会话退出 / 引擎未启用图形：按「没有图片」处理。
      // 必须同时清掉 placements 并把行标脏：占位格（U+10EEEE）仍会被引擎
      // 打上，paint 因此跳过字形绘制；placements 留着旧值的话，paint 会
      // 继续用旧几何把旧位图画回画布 —— 图片位置留下一块不随滚动变化的贴图。
      if (kittyPlacements.current.length > 0) {
        kittyPlacements.current = [];
        kittyDirty.current = true;
      }
    } finally {
      kittyFetching.current = false;
    }
  }, [sessionId]);

  const refresh = useCallback(
    async (needsFull: boolean): Promise<void> => {
      // 图片查询节流：有图（存在 placements 或位图）时每帧查（滚动会改几何），
      // 无图时每 10 帧探一次 —— 否则每次刷新都要多一条 vt_images IPC，与
      // vt_snapshot_diff 抢同一条 wry custom-protocol worker（洪水输出下
      // 就是「每帧两次同步 IPC」的来源之一）。
      if (kittyPlacements.current.length > 0 || kittyImages.current.size > 0) {
        void refreshKittyImages();
      } else if (++kittyProbeFrames.current >= 10) {
        kittyProbeFrames.current = 0;
        void refreshKittyImages();
      }
      const source = sourceRef.current;
      if (!source) return;
      const cache = frameCache.current;
      // 全量重建的四个入口：调用方要求、缓存还没建、尺寸对不上、缓存已失效。
      const full = needsFull || cache.needsFullRebuild;

      /** 拉一帧全量并整屏重建（首帧 / resize / 缓存失效 / 增量降级共用）。 */
      const applyFullFrame = async (): Promise<void> => {
        const frame = await source.snapshot();
        cache.reset(frame);
        paint(cache.frameView(frameCursorOf(frame)));
        lastFrame.current = frame;
      };

      try {
        if (full) {
          await applyFullFrame();
          return;
        }

        const diff = await source.snapshotDiff(false);
        if (diff.cols !== cache.cols || diff.gridRows !== cache.rows) {
          // 尺寸变了：增量帧的行号对上不齐，必须整屏重建
          await applyFullFrame();
          return;
        }

        cache.applyDiff(diff);
        if (cache.needsFullRebuild) {
          // 合并过程中样式表把 16 位下标用尽（极长跑的高彩输出）：这一帧的
          // 颜色已不可信，立刻用全量帧把缓存换成引擎的干净快照。
          await applyFullFrame();
          return;
        }
        const cursor = {
          cursorX: diff.cursorX,
          cursorY: diff.cursorY,
          cursorVisible: diff.cursorVisible,
          cursorPendingWrap: diff.cursorPendingWrap,
          defaultFg: cache.lastDefaultFg,
          defaultBg: cache.lastDefaultBg,
          cursorColor: diff.cursorColor,
          cursorStyle: diff.cursorStyle,
          scrollbar: diff.scrollbar
        };
        // 默认色只在增量帧真的送达时才更新：未设置（-1）不能覆盖缓存里
        // 已有的有效值，否则主题色会被抹成回退色。
        if (diff.defaultFg !== null) cache.lastDefaultFg = diff.defaultFg;
        if (diff.defaultBg !== null) cache.lastDefaultBg = diff.defaultBg;
        const view = cache.frameView(cursor);
        // paint 用 paintBaseline 做行级比对（独立拷贝），所以这里的顺序不再
        // 影响比对正确性；仍保持「先画再记」以免中途抛错留下半帧基线。
        paint(view);
        lastFrame.current = view;
      } catch (err) {
        // 会话已退出后 Rust 侧会报「会话不存在」：这是预期路径（exit 事件
        // 会同步把 ready 置 false 停掉循环），不应刷屏。其它错误（超时、
        // 解码失败）说明快照链路真的坏了，必须让用户看见而不是静默卡住。
        const msg = err instanceof Error ? err.message : String(err);
        if (!msg.includes('会话不存在')) {
          console.error('[terminal] 快照失败:', msg);
        }
        // 失败时把缓存标为失效：下一帧走全量重建，而不是继续在坏状态上增量。
        cache.invalidate();
        // 超时/解码失败的那一帧已把 dirty 清掉：不重新标脏的话，终端静止时
        // 没有任何信号会再触发补拉，画面就永久停在旧帧
        dirty.current = true;
      }
    },
    [paint, refreshKittyImages]
  );

  /**
   * 按宿主实际内容尺寸同步网格（cols/rows）与 PTY 尺寸。
   *
   * 为什么需要这个统一入口：此前只有 ResizeObserver 与 spawn 完成时各算一次，
   * 两者都只在「宿主尺寸发生变化」时触发。而宿主尺寸变化存在observer 收不到的
   * 路径：面板从 display:none 变可见期间伴随的布局未稳定、左右抽屉开关改变
   * 主区宽度、最大化/还原切换高度、窗口 resize 事件比 RO 回调更早落地。
   * 任一漏网都会让 PTY 停在旧网格：prompt 按旧列数折行、光标被挤到第二行行首，
   * 之下的区域永远不出内容（用户看到的「红框区域不渲染」）。
   *
   * 因此渲染循环每帧用一次 O(1) 的尺寸签名比对自愈：offsetWidth/Height 只在
   * 签名变化时才读布局，稳态下零布局抖动。
   *
   * contentRect 已是 `.terminal-canvas-host` 的 content-box 尺寸（16px padding
   * 已扣除），此处**不能**再减一遍。
   */
  const applyHostSize = useCallback((): void => {
    const host = hostRef.current;
    if (!host) return;
    // display:none 时宿主盒子为 0：保持原网格，避免把 PTY 缩到最小行列再弹回
    // （vim 等全屏程序会闪屏重排）
    // `clientWidth/clientHeight` 才是 content-box（padding 已扣除）；
    // `offsetWidth/offsetHeight` 含 padding。用错口径会多算一列/一行，
    // canvas 超出内容区被 overflow 裁掉一列，格内容与格边界错位。
    // 面板 display:none 时两者都是 0：保持原网格，避免把 PTY 缩到最小
    // 行列再弹回（vim 等全屏程序会闪屏重排）。
    if (host.clientWidth < 4 || host.clientHeight < 4) return;
    const next = gridSizeFromHost(
      host.clientWidth,
      host.clientHeight,
      cellW.current,
      cellH.current
    );
    const { cols, rows } = gridRef.current;
    // 与 PTY 实际尺寸比对，而不是与前端 state 比对：见 ptySizeRef 的注释。
    // state 可能在 spawn 之前就被推进一步，此后 next === gridRef 恒成立，
    // resize 永远不再下发，PTY 停在 spawn 时的网格。
    const pty = ptySizeRef.current;
    const changed =
      !pty.synced ||
      pty.cols !== next.cols ||
      pty.rows !== next.rows ||
      pty.cellW !== cellW.current ||
      pty.cellH !== cellH.current;
    if (!changed && next.cols === cols && next.rows === rows) return;
    setCols(next.cols);
    setRows(next.rows);
    // resize 失败必须可见：静默吞掉会让 PTY 停在旧网格，canvas 按旧 rows
    // 画成溢出高度，用户看到「内容只占上半屏、下方空白」。
    // 镜子只在 resolve 后更新：失败时下一轮仍会重试。
    ptyRef.current?.resize(next.cols, next.rows, cellW.current, cellH.current)
      .then(() => {
        ptySizeRef.current = {
          cols: next.cols,
          rows: next.rows,
          cellW: cellW.current,
          cellH: cellH.current,
          synced: true
        };
      })
      .catch((err: unknown) => {
        // 失败不更新镜子：下一轮 applyHostSize 会重试。只记日志：
        // 静默吞掉会让 PTY 停在旧网格，canvas 按旧行数画、下方留空白。
        console.error('[TERM] pty_resize 失败:', err);
      });
    dirty.current = true;
  }, []);

  // 初始化会话
  useEffect(() => {
    let disposed = false;
    let pty: PtyHandle | null = null;
    let unsubscribeExit: (() => void) | null = null;
    let unsubscribePwd: (() => void) | null = null;

    (async () => {
      try {
        measureCell();
        const source = new TerminalSource(sessionId);
        sourceRef.current = source;

        if (disposed) return;

        // 会话退出：shell 结束 / pty_close 后读线程发出真实退出码。
        // 必须停掉渲染循环：否则 `vt_snapshot` 只会报「会话不存在」，
        // 而 refresh 的 catch 把它静默吞掉，rAF 每秒 60 次空转，
        // 光标还在闪 —— 用户以为终端卡死了。
        unsubscribePwd = await subscribePwd(sessionId, value => {
          if (!disposed) setPwd(value ? normalizePwd(value) : cwd);
        });
        if (disposed) {
          unsubscribePwd();
          return;
        }

        unsubscribeExit = await subscribeExit(sessionId, code => {
          if (disposed) return;
          dirty.current = false;
          inFlight.current = false;
          ptyRef.current = null;
          setReady(false);
          setError(
            code === 0
              ? t().terminal.shellExited
              : t().terminal.abnormalExit(String(code))
          );
          // 通知父组件移除死标签：PTY 已终止，留着只占位且无法常规关闭
          onExited?.();
        });
        if (disposed) {
          unsubscribeExit();
          return;
        }

        pty = await spawnPty({ sessionId, cols, rows, cwd, shell });
        // 竞态兜底：cleanup 可能在本 await 期间就已经跑完（彼时 pty 还是
        // null，pty?.close() 是空操作）。此时 spawn 刚刚 resolve，会话已经
        // 在 Rust 侧建好了，必须由这里负责关掉 —— 否则 PtyManager 里的
        // 条目、读线程、底层 fd 全部泄漏，而且没有任何后续代码会碰它。
        if (disposed) {
          pty.close();
          return;
        }
        ptyRef.current = pty;
        // spawn 用的就是此刻的 gridRef（可能已被首次 applyHostSize 推进），
        // 记进镜子：否则 applyHostSize 会以为 PTY 已是另一套尺寸而补发 resize。
        ptySizeRef.current = { cols, rows, cellW: cellW.current, cellH: cellH.current, synced: true };

        // PTY 以默认 100×24 启动。ResizeObserver 的首次回调在 spawn 完成前触发
        // （彼时 ptyRef 还没就绪，resize 被吞掉），之后尺寸不再变化、回调因行列相等而跳过，
        // PTY 行数便一直与视口不符：内容超出一屏时滚轮滚不动，
        // 拖动窗口触发真实 resize 后才恢复。这里在 spawn 完成后立即按宿主实际尺寸同步一次。
        applyHostSize();

        setReady(true);
        dirty.current = true;
      } catch (err) {
        if (!disposed) setError(err instanceof Error ? err.message : String(err));
      }
    })();

    return () => {
      disposed = true;
      unsubscribeExit?.();
      unsubscribePwd?.();
      pty?.close();
      sourceRef.current = null;
      // 图片位图是 GPU 资源，会话结束必须显式释放（不依赖 GC）
      kittyImages.current.forEach(entry => entry.bitmap.close());
      kittyImages.current.clear();
      kittyPlacements.current = [];
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [sessionId]);

  // 面板打开 / 切换会话 / 切换项目后自动聚焦，免去先点击一下才能输入
  useEffect(() => {
    if (active && ready) textareaRef.current?.focus({ preventScroll: true });
  }, [active, ready]);

  // 原生 VT 默认色对齐当前 IDE 主题（--text-primary / --term-bg / --accent）。
  // 终端在浅色 UI 下仍保持深色背景（--term-bg 由变体单独钉死），避免 ANSI
  // 语义色失去对比度。等一帧再读：App 的 data-variant 副作用先于本效果生效。
  useEffect(() => {
    if (!ready) return;
    let disposed = false;
    const frame = requestAnimationFrame(() => {
      if (disposed) return;
      const cs = getComputedStyle(document.body);
      const fg = cssColorToInt(cs.getPropertyValue('--term-fg')) ?? 0xbcbec4;
      const bg = cssColorToInt(cs.getPropertyValue('--term-bg')) ?? 0x282c34;
      const cursor = cssColorToInt(cs.getPropertyValue('--accent')) ?? fg;
      void sourceRef.current?.setColors(fg, bg, cursor).catch(() => {});
    });
    return () => {
      disposed = true;
      cancelAnimationFrame(frame);
    };
  }, [ready, themeKey]);

  // 鼠标跟踪模式可能被 TUI 在任意时刻打开/关闭；轮询缓存 500ms，
  // 点击时无需等待一次 IPC 再决定“选区还是转发”。
  useEffect(() => {
    if (!ready || !active) return;
    let alive = true;
    let timer = 0;
    const poll = () => {
      void sourceRef.current
        ?.mouseTracking()
        .then(v => {
          if (alive) mouseTracking.current = v;
        })
        .catch(() => {})
        .finally(() => {
          if (alive) timer = window.setTimeout(poll, 500);
        });
    };
    poll();
    return () => {
      alive = false;
      window.clearTimeout(timer);
    };
  }, [ready, active]);

  // 面板可见性 / 停靠位置变化：宿主可能刚从 display:none 恢复或换了主轴，
  // 此时必须立即重算网格。等到 ResizeObserver 回调或 rAF 签名比对会慢一帧以上，
  // 而这一帧里 shell 已经按旧网格输出，prompt 會折行、光标错位。
  useEffect(() => {
    if (!ready) return;
    if (!active) {
      // 隐藏时丢掉签名：重新可见时即使尺寸与隐藏前相同也强制重算一次
      lastHostSig.current = '';
      return;
    }
    lastHostSig.current = '';
    applyHostSize();
  }, [active, placement, ready, applyHostSize]);

  // 自适应容器尺寸 (ResizeObserver)
  //
  // 依赖刻意为空数组：回调只经 applyHostSize 读 ref（gridRef/cellW/cellH），
  // 不捕获 state。写 [cols, rows] 会让每次网格变化都重建 observer，而 observer
  // 重建本身又会触发一次尺寸回调，形成「resize → 重建 → 回调 → resize」的放大。
  //
  // observer 只是快路径：真正兜底在渲染循环里的 applyHostSize（见其注释），
  // 那些路径的宿主尺寸变化可以不产生 observer 回调。
  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;

    const ro = new ResizeObserver(() => {
      applyHostSize();
    });

    ro.observe(host);
    return () => ro.disconnect();
  }, [applyHostSize]);

  // 字号变化：重新度量单元格并整屏重绘
  useEffect(() => {
    measureCell();
    // 格宽变了，同一宿主尺寸对应的 cols/rows 也不同：清掉签名逼一次重算
    lastHostSig.current = '';
    applyHostSize();
    dirty.current = true;
  }, [measureCell, applyHostSize]);

  // rAF 渲染循环
  //
  // 依赖刻意只有 `[ready]`：`paint` 依赖 `blinking`、`refresh` 依赖 `paint`，
  // 若把它们写进依赖数组，每次光标闪烁都会 tear down 重建整个 rAF 循环
  // 与 setInterval（一秒两次重建）。改为：循环内只调 `refresh`，
  // 需要的可变状态（blink 相位）读 ref，渲染结果始终基于最新闭包。
  useEffect(() => {
    if (!ready) return;
    let running = true;
    let raf = 0;
    let hiddenTimer = 0;

    const tick = () => {
      if (!running) return;
      // 触控板像素增量会累积出小数行：每次只取整数部分下发，余数留待后续合并
      const scrollDelta = Math.trunc(pendingScroll.current);
      if (scrollDelta !== 0) {
        pendingScroll.current -= scrollDelta;
        // 去路由 Rust 判定：程序开了鼠标跟踪就转发鼠标序列（TUI 自己滚），
        // 备用屏 + DEC 1007 转方向键，其余回滚本地视口 —— 备用屏没有本地
        // scrollback，一律本地回滚会让 omp/opencode 这类 TUI 完全滚不动。
        const at = wheelAt.current;
        void sourceRef.current?.wheel(scrollDelta, at.x, at.y, at.mods).then(() => {
          dirty.current = true;
        });
      }
      // 面板隐藏时暂停快照拉取；保持 dirty 标记，重新可见后下一帧立即补一帧
      const host = hostRef.current;
      const panelHidden = !host || host.offsetWidth === 0 || host.offsetHeight === 0;
      if (panelHidden) {
        // 后台/隐藏会话不按 60Hz 空转：降频到 1Hz 轮询，重新可见后立即恢复 rAF 节奏。
        // 多项目 × 多标签时 4Hz 的 IPC 总量仍然可观，1Hz 足够维持后台输出。
        if (!hiddenTimer) {
          hiddenTimer = window.setTimeout(() => {
            hiddenTimer = 0;
            if (running) raf = requestAnimationFrame(tick);
          }, 1000);
        }
        return;
      }
      // 网格尺寸自愈：宿主尺寸变了但 ResizeObserver 没给回调（面板从 display:none
      // 恢复、左右抽屉开关改变主区宽度、最大化切换高度，都存在这种窗口）时，
      // 这里按尺寸签名兜底重算。签名只在真的变化时才写，稳态下不读布局。
      const hostSig = `${host.offsetWidth}x${host.offsetHeight}`;
      if (hostSig !== lastHostSig.current) {
        lastHostSig.current = hostSig;
        applyHostSize();
      }
      if (dirty.current && !inFlight.current) {
        dirty.current = false;
        // 尺寸变化 / 首帧：拉全量。增量帧的行号是按发送时的网格算的，
        // 与前端缓存尺寸对不上就必须整屏重建，否则脏行会写到错的行上。
        const needsFull = frameCache.current.needsFullRebuild
          || frameCache.current.cols !== gridRef.current.cols
          || frameCache.current.rows !== gridRef.current.rows;
        // in-flight 抢占：一次快照卡住（会话已退出、IPC 排队）时，
        // 若允许后续帧继续入队，渲染循环会被一串永不 settle 的 promise 埋住。
        // 改为只保留一个 in-flight，上一帧超时/失败后立即释放锁重试。
        inFlight.current = true;
        void refresh(needsFull).finally(() => {
          inFlight.current = false;
        });
      }
      raf = requestAnimationFrame(tick);
    };

    raf = requestAnimationFrame(tick);

    // 切回屏上时立即唤醒：隐藏轮询的定时器最多还有 1s，期间会显示旧帧。
    const wake = () => {
      if (!running || !hiddenTimer) return;
      window.clearTimeout(hiddenTimer);
      hiddenTimer = 0;
      cancelAnimationFrame(raf);
      raf = requestAnimationFrame(tick);
    };
    wakeLoop.current = wake;

    // 光标闪烁独立于 paint/refresh：只翻相位 + 标脏，不重建上面的循环
    const timer = window.setInterval(() => {
      blinkOn.current = !blinkOn.current;
      dirty.current = true;
    }, CURSOR_BLINK_MS);

    return () => {
      running = false;
      wakeLoop.current = null;
      cancelAnimationFrame(raf);
      if (hiddenTimer) {
        window.clearTimeout(hiddenTimer);
        hiddenTimer = 0;
      }
      clearInterval(timer);
    };
  }, [ready]);

  const cellFromEvent = (e: React.MouseEvent): { x: number; y: number } | null => {
    const canvas = canvasRef.current;
    const frame = lastFrame.current;
    if (!canvas || !frame) return null;
    const rect = canvas.getBoundingClientRect();
    const x = Math.floor((e.clientX - rect.left) / cellW.current);
    const y = Math.floor((e.clientY - rect.top) / cellH.current);
    if (x < 0 || y < 0 || x >= frame.cols || y >= frame.rows) return null;
    return { x, y };
  };

  /** 把引擎算出的选区端点（视口坐标，闭区间）落到本地高亮与文本缓存上 */
  const applyEngineSelection = (pending?: Promise<[number, number, number, number] | null>) => {
    if (!pending) return;
    dragAnchor.current = null;
    dragging.current = false;
    void pending
      .then(range => {
        if (!range) {
          // 引擎判定此处无可选内容（点在空白上）：保持无选区
          dragStart.current = null;
          dragEnd.current = null;
          selectionText.current = '';
          dirty.current = true;
          return;
        }
        const [sx, sy, ex, ey] = range;
        dragStart.current = { x: sx, y: sy };
        dragEnd.current = { x: ex, y: ey };
        dirty.current = true;
        return sourceRef.current?.selectedText().then(text => {
          selectionText.current = text ?? '';
        });
      })
      .catch(() => {});
  };

  const onMouseDown = (e: React.MouseEvent) => {
    onActivate?.();
    // 阻断 WKWebView 原生文本选择拖拽：画布自绘选区，原生选择层不参与。
    // 不拦的话原生 selection 层会在 canvas 重绘后留下蓝色幽灵条纹。
    // 焦点由下面 focusTextarea 显式接管，不依赖默认行为。
    e.preventDefault();
    focusTextarea();
    // 按下即清空上一次选区（引擎侧 + 本地高亮 + 文本缓存）。点在网格外的
    // 空白处同样生效——否则旧高亮会一直留在画面上，看起来「选不消失」。
    void sourceRef.current?.clearSelection().catch(() => {});
    dragStart.current = null;
    dragEnd.current = null;
    dragAnchor.current = null;
    selectionText.current = '';
    dirty.current = true;

    const cell = cellFromEvent(e);
    const frame = lastFrame.current;
    if (!cell || !frame) return;
    mouseLastCell.current = cell;
    // Cmd/Ctrl+Click 打开终端里的 http(s) 链接（优先于鼠标转发与选区）。
    if (e.button === 0 && (e.metaKey || e.ctrlKey) && !e.altKey) {
      const url = urlAtCell(frame, cell.x, cell.y);
      if (url) {
        void invokeCommand('open_external_url', { url }).catch(() => {});
        return;
      }
    }
    // 程序开了鼠标跟踪：点击/拖动交给 TUI；Shift 强制本地选区（xterm 约定）。
    if (mouseTracking.current && !e.shiftKey) {
      // DOM button 0/1/2 = left/middle/right；Ghostty 鼠标协议 1/3/2。
      const button = e.button === 0 ? 1 : e.button === 1 ? 3 : e.button === 2 ? 2 : null;
      if (button === null) return;
      mouseForwardButton.current = button;
      const mods = (e.ctrlKey ? 2 : 0) | (e.altKey ? 4 : 0) | (e.metaKey ? 8 : 0);
      void sourceRef.current?.mouse(0, button, cell.x, cell.y, mods).catch(() => {});
      return;
    }
    // 本地选区只响应左键；右/中键不开始拖拽。
    if (e.button !== 0) return;
    if (e.detail >= 3) {
      // 三击：选整行 —— 由引擎按行内容（含语义提示边界）算区间
      applyEngineSelection(sourceRef.current?.selectLine(cell.y));
      return;
    }
    if (e.detail === 2) {
      // 双击：选一个词 —— 词边界由引擎判定（Unicode 词边界规则），
      // 中文的「一个词/一整串」与代码里的 foo.bar 都按同一套规则处理
      applyEngineSelection(sourceRef.current?.selectWord(cell.x, cell.y));
      return;
    }
    // 单击先只记锚点：拖出去才形成选区，单击（含点空白）保持无选区
    dragAnchor.current = cell;
    dragging.current = true;
  };

  const onMouseMove = (e: React.MouseEvent) => {
    const cell = cellFromEvent(e);
    if (cell) mouseLastCell.current = cell;
    if (mouseForwardButton.current !== null) {
      if (cell) {
        const mods = (e.ctrlKey ? 2 : 0) | (e.altKey ? 4 : 0) | (e.metaKey ? 8 : 0);
        void sourceRef.current
          ?.mouse(2, mouseForwardButton.current, cell.x, cell.y, mods)
          .catch(() => {});
      }
      return;
    }
    const anchor = dragAnchor.current;
    // 只看「是否处于拖拽中」：mousedown 打开、window 级 mouseup 收尾。
    // 不用 e.buttons 判定 —— WKWebView 下合成/触控板拖拽的 buttons 不总是 1，
    // 一旦被判成 0 就整条拖拽路径失效（选区根本形成不了）
    if (!anchor || !dragging.current) return;
    if (!cell) return;
    const prev = dragEnd.current;
    if (prev && prev.x === cell.x && prev.y === cell.y) return;
    dragStart.current = anchor;
    dragEnd.current = cell;
    dirty.current = true;
  };

  /** finishDrag 的 ref 镜像：全局 mouseup 监听只注册一次，但始终调用最新实现 */
  const finishDragRef = useRef<(e?: React.MouseEvent) => void>(() => {});
  /** 拖拽期间的全局 mouseup：鼠标在画布外松开也要收尾，否则拖拽状态会挂住 */
  useEffect(() => {
    // finishDrag 每次渲染都是新函数；effect 不写依赖数组会让 React 在每次
    // 渲染时 add/remove 一对 listener（refresh→setCursorInfo/setScrollInfo
    // 在 60fps 下每秒 60 次重建）。用 ref 镜像最新实现，effect 只跑一次。
    const onUp = () => {
      if (dragging.current || mouseForwardButton.current !== null) finishDragRef.current();
    };
    window.addEventListener('mouseup', onUp);
    return () => window.removeEventListener('mouseup', onUp);
  }, []);

  const finishDrag = (e?: React.MouseEvent) => {
    if (mouseForwardButton.current !== null) {
      const button = mouseForwardButton.current;
      mouseForwardButton.current = null;
      const cell = e ? cellFromEvent(e) : mouseLastCell.current;
      if (cell) {
        const mods = e
          ? (e.ctrlKey ? 2 : 0) | (e.altKey ? 4 : 0) | (e.metaKey ? 8 : 0)
          : 0;
        void sourceRef.current?.mouse(1, button, cell.x, cell.y, mods).catch(() => {});
      }
      dirty.current = true;
      return;
    }
    dragging.current = false;
    dragAnchor.current = null;
    const s = dragStart.current;
    const endPoint = dragEnd.current;
    if (!s || !endPoint) return;
    const singleCell = s.x === endPoint.x && s.y === endPoint.y;
    if (singleCell) {
      // 落点在同一个格：视为单击，撤销这次「选区」
      dragStart.current = null;
      dragEnd.current = null;
      dirty.current = true;
      return;
    }
    // 选区同步给引擎：Cmd+C 复制时 vt_selected_text 才有内容
    // 同时把文本缓存下来，copy 事件（Cmd+C）能同步取用，不必等 IPC 往返
    const source = sourceRef.current;
    if (!source) return;
    void source
      .setSelection(s.x, s.y, endPoint.x, endPoint.y)
      .then(() => source.selectedText())
      .then(text => {
        selectionText.current = text;
      })
      .catch(() => {
        selectionText.current = '';
      });
  };
  // 渲染期内同步镜像：mouseup 监听用的 finishDragRef 因此始终指向最新实现
  finishDragRef.current = finishDrag;

  // 滚轮：用原生非被动监听接管。
  // React 的合成 wheel 是 passive 注册：preventDefault 无效，事件分发在
  // WKWebView 下也不可靠。这里直接挂在宿主元素上，并按 deltaMode 换算行数——
  // 鼠标滚轮是行增量，触控板是像素增量；累积的增量在 rAF 里交给 vt_wheel，
  // 由 Rust 侧决定「转发给 PTY 程序」还是「回滚本地视口」。
  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;
    const handler = (e: WheelEvent) => {
      e.preventDefault();
      let lines: number;
      if (e.deltaMode === 1) lines = e.deltaY;
      else if (e.deltaMode === 2) lines = e.deltaY * 24;
      else lines = e.deltaY / (cellH.current || 18);
      pendingScroll.current += Math.max(-20, Math.min(20, lines));
      // 记录格坐标与修饰键（转发鼠标协议时要用；坐标换算与取词/选区同一套）
      const canvas = canvasRef.current;
      const frame = lastFrame.current;
      if (canvas && frame) {
        const rect = canvas.getBoundingClientRect();
        const x = Math.floor((e.clientX - rect.left) / cellW.current);
        const y = Math.floor((e.clientY - rect.top) / cellH.current);
        wheelAt.current = {
          x: Math.max(0, Math.min(frame.cols - 1, x)),
          y: Math.max(0, Math.min(frame.rows - 1, y)),
          // GhosttyMods 位域：SHIFT=1 CTRL=2 ALT=4 SUPER=8（与 vt.rs 的 mods 模块同源）
          mods:
            (e.shiftKey ? 1 : 0) | (e.ctrlKey ? 2 : 0) | (e.altKey ? 4 : 0) | (e.metaKey ? 8 : 0)
        };
      }
    };
    host.addEventListener('wheel', handler, { passive: false });
    return () => host.removeEventListener('wheel', handler);
  }, []);


  /** 粘贴：textarea 的原生粘贴路径把文本写入 PTY（换行规整为 \r）。
   *  之前没有 onPaste，Cmd+V / 右键粘贴被整个丢弃。
   *
   *  文件增强：剪贴板里是 Finder 复制的文件/文件夹时（files 非空），
   *  绝对路径不在 clipboardData 里 —— `File` 只有 name/type/size。所以
   *  先同步判定「是不是文件粘贴」（files 非空即 true，不付 IPC 往返），
   *  再异步调 Rust 取绝对路径，包成 bracketed paste 写进 PTY。
   *
   *  **不在 LazeCode 侧渲染附件预览**：omp 的编辑器自己就画 `img #N` chip
   *  （缩略图 + 尺寸，见截图），一切遵循它的规格。LazeCode 再画一遍既是重复，
   *  还会引出「删预览要不要回删 PTY」的同步问题 —— 删掉那一层，问题消失。
   *
   *  浏览器预览模式下 Rust 命令返回 []，此时回落成普通文本粘贴
   *  （至少粘上文件名），不会把粘贴整个吞掉。 */
  const onTextareaPaste = (e: React.ClipboardEvent<HTMLTextAreaElement>) => {
    const text = e.clipboardData.getData('text/plain');
    const hasFiles = pasteHasFiles(e.clipboardData);
    // 既没文本也不是文件：什么都不做（不 preventDefault，让浏览器自行处理）
    if (!text && !hasFiles) return;
    e.preventDefault();
    const pty = ptyRef.current;
    if (!pty) return;

    /** 普通文本粘贴：原样下发，\\n 规整为 \\r（shell 需要回车）。 */
    const writeTextToPty = (payload: string) => {
      const bytes = new TextEncoder().encode(payload.replace(/\r?\n/g, '\r'));
      pty.write(bytes).catch(err => {
        console.error('[terminal] 粘贴下发失败:', err instanceof Error ? err.message : String(err));
      });
      // 清掉暂存内容：textarea 只是 IME/剪贴板中转 buffer，留着会被下次组合输入读走
      if (textareaRef.current) textareaRef.current.value = '';
    };

    /** 附件路径：必须走 bracketed paste。
     *  omp 编辑器只在收到 ESC[200~ 后才可能把「已存在的图片路径」变成
     *  带缩略图的 [Image #N] chip；裸写路径只会得到一串普通字符。
     *  详见 wrapBracketedPaste 的文档（含 omp 18.8.3 二进制的实测证据）。 */
    const writeAttachToPty = (files: readonly TerminalAttachFile[]) => {
      const wrapped = wrapBracketedPaste(attachFilesToPasteText(files));
      if (wrapped === '') return;
      // 不规整 \\r\\n：bracketed paste 的起止序列就是控制字符，replace 会破坏它们
      const bytes = new TextEncoder().encode(wrapped);
      pty.write(bytes).catch(err => {
        console.error('[terminal] 附件路径下发失败:', err instanceof Error ? err.message : String(err));
      });
      if (textareaRef.current) textareaRef.current.value = '';
    };

    if (!hasFiles) {
      writeTextToPty(text);
      return;
    }

    // 文件粘贴：取绝对路径后原样写进 PTY。缩略图/metainfo 完全交给 omp
    // 自己渲染（它认得 bracketed paste 里的图片路径）。Rust 拿不到路径
    // （预览模式/异常）时回落纯文本，绝不让用户白白按一次 Cmd+V。
    void invokeCommand<string[]>('clipboard_file_paths')
      .then(paths => {
        const files = toAttachFiles(paths);
        if (files.length === 0) {
          if (text) writeTextToPty(text);
          return;
        }
        // 不落 state：预览由 omp 自己渲染（见模块注释）。这里只负责把路径
        // 送进 PTY。
        writeAttachToPty(files);
      })
      .catch(err => {
        console.error('[terminal] 读取剪贴板文件路径失败:', err instanceof Error ? err.message : String(err));
        if (text) writeTextToPty(text);
      });
  };
  /** 复制：canvas 自绘选区没有 DOM 文本，浏览器默认复制不到内容。
   *  这里把 mouseup 时缓存的选区文本同步写入剪贴板（clipboardData 同步写，
   *  不依赖 navigator.clipboard 权限，WKWebView 下也可靠）。 */
  const onTextareaCopy = (e: React.ClipboardEvent<HTMLTextAreaElement>) => {
    const text = selectionText.current;
    if (!text) return;
    e.preventDefault();
    e.clipboardData.setData('text/plain', text);
  };

  // 中文/日文等 IME 输入法支持：
  // canvas 元素不接收 composition 事件，需要用隐藏 textarea 中转。
  // textarea 始终持有焦点，compositionend 时把组合结果写入 PTY。
  const runTerminalSearch = useCallback(
    async (select?: 'next' | 'prev') => {
      const source = sourceRef.current;
      if (!source) return;
      const needle = searchQuery;
      if (!needle) {
        setSearchOutcome(null);
        dragStart.current = null;
        dragEnd.current = null;
        selectionText.current = '';
        dirty.current = true;
        void source.search('').catch(() => {});
        return;
      }
      setSearchBusy(true);
      try {
        const outcome = await source.search(needle, select);
        setSearchOutcome(outcome);
        if (outcome.selection) {
          dragStart.current = { x: outcome.selection.startX, y: outcome.selection.startY };
          dragEnd.current = { x: outcome.selection.endX, y: outcome.selection.endY };
          selectionText.current = '';
        } else {
          dragStart.current = null;
          dragEnd.current = null;
        }
        dragAnchor.current = null;
        dragging.current = false;
        dirty.current = true;
      } catch {
        setSearchOutcome(null);
      } finally {
        setSearchBusy(false);
      }
    },
    [searchQuery]
  );

  const closeTerminalSearch = useCallback(() => {
    setSearchOpen(false);
    setSearchOutcome(null);
    dragStart.current = null;
    dragEnd.current = null;
    selectionText.current = '';
    dirty.current = true;
    void sourceRef.current?.clearSelection().catch(() => {});
    textareaRef.current?.focus({ preventScroll: true });
  }, []);

  const onTextareaKeyDown = async (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    // 顶部同步：只有合成中的 keydown 才把 value 镜像成 preedit；非合成态一律
    // 按「无 preedit」收敛 —— 静默取消（删光拼音）时这是第一个页面可见信号，
    // 清残影并记取消时间戳；而非合成的非空 value 是残留垃圾（IME 上屏后回写 /
    // 未映射键默认插入），绝不镜像（镜像了就会画出「光标样式不对」的假笔画）。
    const taTop = textareaRef.current;
    syncPreedit(e.nativeEvent.isComposing && taTop ? taTop.value : '');
    // IME 组合期间不处理
    if (e.nativeEvent.isComposing) return;
    // compositionend 后紧跟的确认键（回车/空格）：已由 compositionend 写入 PTY。
    // 只吞真正的 IME 确认键（isComposing / keyCode 229）：普通按键（如上屏后
    // 立即回车执行命令）不吞，否则会丢一次按键
    if (compositionDoneRef.current) {
      compositionDoneRef.current = false;
      if (e.nativeEvent.isComposing || e.keyCode === 229) return;
    }
    // 取消窗口内的退格/删除：这是「删光拼音」那一下（或其紧随补发），
    // 放行会作为 \x7f 进 PTY 删掉 shell 里已有的字符。窗口过期则正常放行。
    if (
      (e.keyCode === 229 || e.key === 'Backspace' || e.key === 'Delete') &&
      performance.now() - preeditClearedAtRef.current < PREEDIT_CANCEL_WINDOW_MS
    ) {
      return;
    }
    const key = e.key.toLowerCase();
    const isClipboardCombo = (e.metaKey || (e.ctrlKey && e.shiftKey)) && (key === 'c' || key === 'v');

    // 复制/粘贴一律放行，交给系统菜单（Edit → Copy/Paste）路由回 WebView 派发的
    // 原生事件：复制由 onCopy 写入剪贴板、粘贴由 onPaste 写进 PTY。
    //
    // 这里绝不能再 preventDefault —— 拦截后 Cmd+V 会被当成普通按键下发（粘贴
    // 失效），也绝不能自己调系统剪贴板 API：NSPasteboard 必须在主线程访问，
    // 从 IPC 线程调用会 SIGSEGV（实测崩溃，见 clipboard-manager 插件）。
    if (isClipboardCombo) return;
    // Cmd+F：终端内搜索，不发 PTY。Ctrl+F 是 shell/TUI 的 ^F，不能抢。
    if (e.metaKey && !e.ctrlKey && key === 'f') {
      e.preventDefault();
      setSearchOpen(true);
      window.setTimeout(() => searchInputRef.current?.focus(), 0);
      return;
    }

    // 把事件转发给原有的键盘处理逻辑
    const input = domKeyToInput(e.nativeEvent);
    if (!input) {
      // 未映射键（IntlYen/Lang1/媒体键等）：不转发，但必须 preventDefault，
      // 否则默认动作把字符插进隐藏 textarea → onInput 误当 preedit 画出来，
      // 在行尾留下假笔画残影（「光标样式不对」的另一来源）。
      e.preventDefault();
      return;
    }
    e.preventDefault();
    const source = sourceRef.current;
    const pty = ptyRef.current;
    if (!source || !pty) return;
    try {
      const bytes = await source.encodeKey(input);
      if (bytes.length > 0) {
        await pty.write(bytes);
      }
    } catch (err) {
      // 会话已退出后编码/写入都会失败。这里只记日志：exit 事件负责
      // 切到错误态，未处理的 rejection 会变成 unhandledrejection。
      console.error('[terminal] 输入下发失败:', err instanceof Error ? err.message : String(err));
    }
  };

  /**
   * 抬起事件：把 `KEY_ACTION.RELEASE` 交给同一个编码器。
   *
   * 为什么可以无条件转发：引擎只在终端协商了 kitty 事件上报（`CSI > 2 u`
   * 之类的 report event types）时才为抬起产出字节，未协商的普通应用一个
   * 字节都收不到（`vt::key_encoding::release_encodes_nothing_unless_terminal_requests_events`
   * 锁住了这条不变量）。缺失抬起上报时，依赖边沿判定的 TUI 会表现为
   * 「按键卡住」。
   *
   * 这里不做 IME / 剪贴板特判：发布事件不需要 preventDefault，也不该拦
   * 任何默认行为；编码失败按 keydown 同口径记日志。
   */
  const onTextareaKeyUp = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    const input = domKeyToInput(e.nativeEvent, KEY_ACTION.RELEASE);
    if (!input) return;
    const source = sourceRef.current;
    const pty = ptyRef.current;
    if (!source || !pty) return;
    void (async () => {
      try {
        const bytes = await source.encodeKey(input);
        if (bytes.length > 0) await pty.write(bytes);
      } catch (err) {
        console.error('[terminal] 抬起事件下发失败:', err instanceof Error ? err.message : String(err));
      }
    })();
  };

  /**
   * preedit 真值同步：IME 状态只认 textarea.value —— 普通键在 keydown 里
   * 被 preventDefault 不写 value，合成期间的键不拦、由 IME 写入。任何路径
   * 把它镜像到 compositionTextRef 并标脏（rAF 下一帧重绘光标行）；
   * 非空→空跳变记取消时间戳（commit 产生的空跳变由 Done 标记挡掉，不记，
   * 否则上屏后立即退格会被误吞）。
   */
  const syncPreedit = (live: string) => {
    const prev = compositionTextRef.current;
    if (live === prev) return;
    compositionTextRef.current = live;
    dirty.current = true;
    if (!live && prev.length > 0 && !compositionDoneRef.current) {
      preeditClearedAtRef.current = performance.now();
    }
  };

  // 合成 preedit 实时上屏：canvas 只画 PTY 内容，拼音不上画布就全无反馈。
  const onCompositionStart = (e: React.CompositionEvent<HTMLTextAreaElement>) => {
    syncPreedit(e.data);
  };

  // start/update 共用；end 里清空（上屏文本由 PTY 回显接管，取消则直接消失）。
  const onCompositionUpdate = (e: React.CompositionEvent<HTMLTextAreaElement>) => {
    syncPreedit(e.data);
  };

  // value 变化兜底：部分 IME 只发 input 不发 compositionupdate（或顺序不定），
  // value 是真值，任何一侧先到都收敛到同一状态。
  const onTextareaInput = (e: React.FormEvent<HTMLTextAreaElement>) => {
    const ta = e.currentTarget;
    const live = ta.value;
    // React 把 input 的 nativeEvent 声明为 Event；运行时是 InputEvent（带 isComposing）
    if ((e.nativeEvent as InputEvent).isComposing) {
      // 合成中：value 即 preedit（部分 IME 只发 input 不发 compositionupdate）
      syncPreedit(live);
      return;
    }
    if (live) {
      // 非合成的非空 value = 残留（IME 上屏后回写等）：就地清掉，不进 preedit。
      // 直接清不走 sync 的时间戳记账 —— 上屏残留不属「取消」，不能误开退格豁免。
      ta.value = '';
      if (compositionTextRef.current) {
        compositionTextRef.current = '';
        dirty.current = true;
      }
      return;
    }
    // 空：正常收敛（删光拼音的取消路径靠它记时间戳）
    syncPreedit('');
  };
  const onCompositionEnd = (e: React.CompositionEvent<HTMLTextAreaElement>) => {
    const text = e.data;
    // 先标记 Done：挡掉下面 sync/清 value 产生的空跳变被误记为「取消」
    if (text) compositionDoneRef.current = true;
    syncPreedit('');
    if (!text) return;

    const pty = ptyRef.current;
    if (pty) {
      const bytes = new TextEncoder().encode(text);
      pty.write(bytes).catch(err => {
        console.error('[terminal] IME 输入下发失败:', err instanceof Error ? err.message : String(err));
      });
    }
    // 清空 textarea，防止残留文本影响下次输入
    if (textareaRef.current) textareaRef.current.value = '';
    // 标记：下一个 keydown 是中文上屏的确认键，需要跳过
    compositionDoneRef.current = true;
  };

  /** 聚焦终端时，实际聚焦到隐藏 textarea 以接收 IME 输入 */
  const focusTextarea = useCallback(() => {
    textareaRef.current?.focus();
  }, []);

  if (error) {
    return (
      <div className="terminal-error-box">
        <span>{t().terminal.startFailed(error)}</span>
      </div>
    );
  }

  return (
    <div className="single-terminal-view">
      {searchOpen && (
        <div className="terminal-find-bar" onMouseDown={e => e.stopPropagation()}>
          <input
            ref={searchInputRef}
            className="terminal-find-input"
            value={searchQuery}
            placeholder={t().terminal.searchPlaceholder}
            spellCheck={false}
            onChange={e => setSearchQuery(e.target.value)}
            onKeyDown={e => {
              e.stopPropagation();
              if (e.key === 'Escape') {
                e.preventDefault();
                closeTerminalSearch();
              } else if (e.key === 'Enter') {
                e.preventDefault();
                void runTerminalSearch(e.shiftKey ? 'prev' : 'next');
              }
            }}
          />
          <span className="terminal-find-count">
            {searchBusy
              ? '…'
              : searchOutcome
                ? `${searchOutcome.selected !== null ? searchOutcome.selected + 1 : 0}/${searchOutcome.total}`
                : '0/0'}
          </span>
          <button
            type="button"
            className="terminal-find-btn"
            title={t().terminal.searchPrev}
            onClick={() => void runTerminalSearch('prev')}
          >
            ↑
          </button>
          <button
            type="button"
            className="terminal-find-btn"
            title={t().terminal.searchNext}
            onClick={() => void runTerminalSearch('next')}
          >
            ↓
          </button>
          <button
            type="button"
            className="terminal-find-btn"
            title={t().terminal.searchClose}
            onClick={closeTerminalSearch}
          >
            ×
          </button>
        </div>
      )}
      <div
        ref={hostRef}
        className="terminal-canvas-host"
        role="group"
        aria-label={t().terminal.a11yLabel}
        onClick={focusTextarea}
      >
        {/* 隐藏 textarea：接收键盘输入与 IME 组合，canvas 本身不支持
            composition 事件。left/top/width/height 由 paint() 每帧钉到
            光标格（IME 候选框跟随光标）；React 不 diff 这几个键的后续
            变化，初始值仅在首帧 paint 前兜底。 */}
        <textarea
          ref={textareaRef}
          aria-label={t().terminal.a11yInputLabel}
          onKeyDown={onTextareaKeyDown}
          onKeyUp={onTextareaKeyUp}
          onCopy={onTextareaCopy}
          onPaste={onTextareaPaste}
          onInput={onTextareaInput}
          onCompositionStart={onCompositionStart}
          onCompositionUpdate={onCompositionUpdate}
          onCompositionEnd={onCompositionEnd}
          style={{
            position: 'absolute',
            top: 0,
            left: 0,
            width: 0,
            height: 0,
            opacity: 0,
            color: 'transparent',
            background: 'transparent',
            textShadow: 'none',
            textDecoration: 'none',
            userSelect: 'none',
            border: 'none',
            outline: 'none',
            resize: 'none',
            padding: 0,
            caretColor: 'transparent',
            pointerEvents: 'none'
          }}
          autoFocus
        />
        <canvas
          ref={canvasRef}
          aria-hidden="true"
          tabIndex={-1}
          onMouseDown={onMouseDown}
          onMouseMove={onMouseMove}
          onMouseUp={finishDrag}
          style={{ outline: 'none', display: 'block', cursor: 'text' }}
        />
        <TerminalA11yMirror text={a11yText} label={t().terminal.a11yLabel} />
      </div>
      <div className="terminal-statusbar">
        <span className="terminal-status-cwd" title={pwd || cwd || ''}>
          {pwd || cwd || ''}
        </span>
        <span className="terminal-status-shell">{shell ?? ''}</span>
      </div>
    </div>
  );
};

