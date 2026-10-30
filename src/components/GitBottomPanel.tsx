import React, { useState, useEffect, useRef, useMemo, useCallback } from 'react';
import { createPortal } from 'react-dom';
import { invokeCommand } from '../services/tauriBridge';
import { CtxMenuShell } from './CtxMenuShell';
import { getIpcLog, subscribeIpcLog } from '../services/ipcLog';
import type { GitCommit, GitBranch, GitTag } from '../types';
import {
  IconRefresh, IconDiff, IconFetch, IconUpdateArrow,
  IconRollback, IconSearch, IconChevronDown, IconChevronRight, IconWindowMinimize, IconWindowMaximize,
  IconWindowClose, IconCloud, IconStar, IconTag, IconShelf, GitBranchIcon, TextFileIcon, IconMerge, IconGear,
  IconPlus, IconTrash, IconCompareVersions,
  IconDoubleChevronLeft, IconDoubleChevronRight, FolderIcon,
  IconSearchDropdown, IconSortCommits, IconCommitGraph, IconViewOptions,
  IconExpandAll, IconCollapseAll, renderFileIcon
} from './FileIcons';
import type { CompareFileTreeNode } from '../services/compareBranches';
import { buildCompareFileTree, allCompareDirPaths } from '../services/compareBranches';
import { formatCommitDate } from '../services/commitDate';
import {
  computeGraphBreaks,
  computeGraphLayout,
  mergeGraphBreaks
} from '../services/logFolding';
import type { GraphBreak } from '../services/logFolding';
import { GraphCell, foldArrowHit } from './GitGraphCell';
import { startPanelDrag } from '../services/panelDrag';
import { sortLocalBranches } from '../services/branchSort';
import { buildRefTree, flattenRefTree, refKey, visibleRefKeys } from '../services/refTree';
import type { RefTreeNode } from '../services/refTree';
import { t } from '../services/i18n';
import { useLanguageRefresh } from '../hooks/useLanguageRefresh';

/** 与 Rust `CommitFileDto` 对应 */
interface CommitFile {
  path: string;
  status: string;
  additions: number;
  deletions: number;
}

interface GitBottomPanelProps {
  /** 仓库根目录：提交详情等查询必须带上；为 null 时后端退化为进程 cwd（打包后会错） */
  repoPath?: string;
  /** 项目/仓库显示名；repoPath 为 "." 或临时路径时仍显示真实项目名 */
  repoName?: string;
  commits: GitCommit[];
  branches: GitBranch[];
  tags?: GitTag[];
  currentBranch: string;
  onSelectCommit: (commit: GitCommit) => void;
  onSelectBranch?: (branch: string) => void;
  onCheckoutTag?: (name: string) => void;
  /** 两个 ref 的 Compare Branches（两条独立日志，不是文件抽屉） */
  onCompareBranches?: (a: GitBranch, b: GitBranch) => void;
  /** 单个 ref 与工作区比较（分支树纵向工具栏） */
  /** Update Selected：fetch 后刷新 */
  onUpdateSelected?: (branches: GitBranch[]) => void;
  /** Delete Selected：一次确认后批量删除 */
  onDeleteSelected?: (branches: GitBranch[]) => void;
  /** 单个标签动作 */
  onDeleteTag?: (name: string) => void;
  /** 新建分支（无起点） */
  onCreateBranch?: () => void;
  onToggleFavorite?: (name: string) => void;
  /** 日志排序与过滤选项（由 App 重新查询 git_log） */
  sortMode?: 'date' | 'topo';
  onSortModeChange?: (mode: 'date' | 'topo') => void;
  firstParent?: boolean;
  onFirstParentChange?: (value: boolean) => void;
  noMerges?: boolean;
  onNoMergesChange?: (value: boolean) => void;
  onClose: () => void;
  /** 最大化/还原底部面板（由 App 调整面板高度） */
  maximized?: boolean;
  onToggleMaximize?: () => void;
  /** 重新拉取 Git 数据 */
  onRefresh?: () => void;
  /** 打开整个提交的 diff */
  onOpenCommitDiff: (hash: string) => void;
  /** 打开提交内单个文件的 diff */
  onOpenFileDiff: (hash: string, path: string) => void;
  /** 多选提交时只合并这些提交的文件与 diff（不包含中间未选提交） */
  onOpenCommitsDiff: (commits: string[], path?: string) => void;
  /** 提交内单个文件 ↔ 工作区比较（Compare with Local） */
  onCompareWithLocal?: (hash: string, path: string) => void;
  /** 把该提交版本的文件检出到工作区（Get from Revision） */
  onGetFromRevision?: (hash: string, path: string) => void;
  /** 当前 diff 正在查看的文件路径（绝对或相对均可，用于在提交文件清单中高亮） */
  activeFile?: string;
  /** 该提交的补丁 → 剪贴板 / 另存 */
  onCopyCommitPatch?: (hash: string) => void;
  onCreateCommitPatch?: (hash: string) => void;
  /** 手动回滚选中的提交（git revert，生成反向提交） */
  onRevertCommit?: (hash: string) => void;
  /** Git 操作进行中（回滚/提交等），按钮进入禁用态 */
  busy?: boolean;
  /** refreshGit 在途：刷新按钮转圈，列表可显示加载提示 */
  refreshing?: boolean;
  /** 当前目录不是 git 仓库时为 false，面板显示初始化入口 */
  isRepo?: boolean;
  /** 打开 Git 工具（子模块/工作树/LFS/凭据） */
  onOpenTools?: () => void;
  /** 初始化仓库（git init） */
  onInitRepo?: () => void;
  /** 分支右键菜单 */
  favorites?: string[];
  onBranchContext?: (branch: GitBranch, x: number, y: number, selected?: GitBranch[]) => void;
  /** 提交右键菜单 */
  onCommitContext?: (commit: GitCommit, x: number, y: number, selectedCommits?: GitCommit[]) => void;
  logBranch?: string;
  onLogBranchChange?: (branch: string) => void;
  /** 日志路径过滤：只看改动过该路径的提交 */
  logPath?: string;
  onLogPathChange?: (path: string) => void;
  /** 日志作者/日期过滤（由 App 持有，透传给后端 git log） */
  userFilter?: string;
  onUserFilterChange?: (author: string) => void;
  dateFilter?: 'all' | 'today' | 'week' | 'month';
  onDateFilterChange?: (date: 'all' | 'today' | 'week' | 'month') => void;
  /** Push 对话框（Ctrl+Shift+K） */
  onPush?: () => void;
  /** Update Project 对话框（Ctrl+T） */
  onPull?: () => void;
  /** 直接执行 git fetch --all */
  onFetch?: () => void;
  /** Stash Changes 对话框（贮藏更改） */
  onStash?: () => void;
  /** Unstash Changes 对话框（恢复贮藏） */
  onUnstash?: () => void;
  /** Shelf 对话框（贮藏架） */
  onShelf?: () => void;
  /** 合并冲突文件数，>0 显示解决横幅 */
  conflictCount?: number;
  /** 打开冲突解决对话框 */
  onResolveConflicts?: () => void;
  /** 加载更早的提交（滚动到底自动触发） */
  onLoadMore?: () => void;
  hasMore?: boolean;
  loadingMore?: boolean;
  /**
   * 外部驱动的提交跳转（Go to Parent/Child 菜单，App.focusCommit）：
   * 收到后走内部 jumpToCommit（选中 + 滚动 + 闪烁）。seq 保证同 hash 连点
   * 可重复触发；null = 无请求。App 曾用"写 ref + document.querySelector"
   * 直跳，够不着虚拟化未渲染的行且从不移动选中态 —— 体感为零（.audit/11）。
   */
  focusCommit?: { hash: string; seq: number } | null;
}

/** 分栏宽度持久化键（拖动边缘后记住） */
const GIT_COL_W_KEY = {
  branches: 'ide_git_branches_width',
  details: 'ide_git_details_width',
  /** 详情列上半区（变更文件清单）高度 */
  detailsTop: 'ide_git_details_top_height'
} as const;
/** 提交详情文件树的视图选项（与 PyCharm 的 Eye 菜单一致） */
const DETAILS_VIEW_KEY = {
  groupByDirectory: 'ide_git_details_group_by_directory',
  showDetails: 'ide_git_details_show_details'
} as const;

function readStoredFlag(key: string, fallback: boolean): boolean {
  try {
    const raw = localStorage.getItem(key);
    return raw === null ? fallback : raw === '1';
  } catch {
    return fallback;
  }
}

function writeStoredFlag(key: string, value: boolean): void {
  try {
    localStorage.setItem(key, value ? '1' : '0');
  } catch {
    /* 隐私模式写入失败可忽略 */
  }
}

/**
 * 浏览器预览用 CSS zoom 放大 UI（见 appSettings），此时 getBoundingClientRect
 * 给出视觉坐标，而 position:fixed 的 top/left 仍按 zoom 前的 CSS 单位解释。
 * 原生 Tauri 走 webview page zoom，getComputedStyle(html).zoom 不是数字，
 * 回落 1 —— 两种环境用同一段换算都成立。
 */
function uiZoom(): number {
  const raw = getComputedStyle(document.documentElement).zoom;
  const value = typeof raw === 'string' ? parseFloat(raw) : NaN;
  return Number.isFinite(value) && value > 0 ? value : 1;
}
/** 详情列上下分割的边界（px） */
const DETAILS_TOP_MIN = 60;
const DETAILS_TOP_MAX = 500;
const DETAILS_BOTTOM_MIN = 60;

/** 归一化比较：清单相对路径 vs diff 导航绝对路径 */
const isSamePath = (a: string, b: string, repo?: string): boolean => {
  if (!a || !b) return false;
  if (a === b) return true;
  const toAbs = (p: string) =>
    !p || /^([A-Za-z]:[\\/]|\/)/.test(p) || !repo ? p : `${repo.replace(/[\\/]+$/, '')}/${p}`;
  return toAbs(a) === toAbs(b);
};

/** 文件状态颜色（与 Compare Branches 的文件树一致）。 */
const detailsStatusColor = (status: string): string => {
  const value = status.toUpperCase();
  if (value.startsWith('A') || value.startsWith('?')) return 'var(--git-green, #3fb950)';
  if (value.startsWith('D')) return 'var(--git-del, #f85149)';
  return 'var(--text-primary)';
};

/** PyCharm 日志工具栏的下拉过滤按钮：按钮 + 弹出层（点外部关闭） */
const LogFilterMenu: React.FC<{
  label: string;
  active?: boolean;
  width?: number;
  children: (close: () => void) => React.ReactNode;
}> = ({ label, active, width = 220, children }) => {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    // capture 阶段拦截：面板内其它控件也会 stopPropagation
    const onDown = (e: PointerEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false);
    };
    window.addEventListener('pointerdown', onDown, true);
    return () => window.removeEventListener('pointerdown', onDown, true);
  }, [open]);

  return (
    <div className="log-filter" ref={ref}>
      <button
        className={`log-filter-btn ${active ? 'active' : ''}`}
        onClick={() => setOpen(v => !v)}
        title={t().gitPanel.filterTitle(label)}
      >
        {label}
        <IconChevronDown size={9} color="currentColor" />
      </button>
      {open && (
        <div className="log-filter-popup" style={{ minWidth: width }}>
          {children(() => setOpen(false))}
        </div>
      )}
    </div>
  );
};

/**
 * memo：App 是无 memo 的单壳层，终端输出 / 编辑器击键 / 光标移动都会重渲染整棵树，
 * 而这个面板自带虚拟化与本地过滤状态，与那些高频 state 无关。
 * 生效前提：App 侧传下来的回调保持稳定引用（见交付说明里的清单）。
 */
export const GitBottomPanel = React.memo(function GitBottomPanel({
  repoPath,
  repoName: repoNameProp,
  commits,
  branches,
  tags = [],
  currentBranch,
  onSelectCommit,
  onClose,
  maximized = false,
  onToggleMaximize,
  onRefresh,
  onOpenCommitDiff,
  onOpenFileDiff,
  onOpenCommitsDiff,
  onCompareWithLocal,
  onGetFromRevision,
  activeFile,
  onCopyCommitPatch,
  onCreateCommitPatch,
  onRevertCommit,
  busy = false,
  refreshing = false,
  isRepo,
  onInitRepo,
  onSelectBranch,
  onCheckoutTag,
  onCompareBranches,
  onUpdateSelected,
  onDeleteSelected,
  onDeleteTag,
  onCreateBranch,
  onToggleFavorite,
  sortMode = 'date',
  onSortModeChange,
  firstParent = false,
  onFirstParentChange,
  noMerges = false,
  onNoMergesChange,
  favorites = [],
  onBranchContext,
  onCommitContext,
  logBranch = '',
  onLogBranchChange,
  logPath = '',
  onLogPathChange,
  userFilter = '',
  onUserFilterChange,
  dateFilter = 'all',
  onDateFilterChange,
  onPush: _onPush,
  onPull: _onPull,
  onFetch,
  onStash: _onStash,
  onUnstash: _onUnstash,
  onShelf: _onShelf,
  conflictCount: _conflictCount = 0,
  onResolveConflicts: _onResolveConflicts,
  onLoadMore,
  hasMore = false,
  loadingMore = false,
  focusCommit = null,
  onOpenTools
}: GitBottomPanelProps) {
  useLanguageRefresh();
  const [fileMenu, setFileMenu] = useState<{ x: number; y: number; path: string } | null>(null);
  // 左键只做行选中（对齐 PyCharm 分支树）；切换分支一律走右键菜单的 Checkout，防误点
  const [selectedRefKeys, setSelectedRefKeys] = useState<string[]>([]);
  /** 目录行被右键/组合选择时的高亮（动作作用于其全部叶子）。 */
  const [selectedFolderKeys, setSelectedFolderKeys] = useState<string[]>([]);
  const lastRefKeyRef = useRef<string | null>(null);
  /** 目录/分组折叠状态：sec:local / sec:remote / sec:tags / dir:local:feature */
  const [collapsedSections, setCollapsedSections] = useState<Set<string>>(new Set());
  const [collapsedRefDirs, setCollapsedRefDirs] = useState<Set<string>>(new Set());
  const [branchFilter, setBranchFilter] = useState('');
  /** `<<` 隐藏分支列表（保留左侧动作条）；`>>` 恢复。不是折叠分组。 */
  const [branchListHidden, setBranchListHidden] = useState(false);
  const [branchSearchOpen, setBranchSearchOpen] = useState(false);
  const [groupByDirectory, setGroupByDirectory] = useState(true);
  const [branchViewMenu, setBranchViewMenu] = useState<{ x: number; y: number } | null>(null);
  const [sortMenuOpen, setSortMenuOpen] = useState(false);
  const [sortMenuPos, setSortMenuPos] = useState<{ x: number; y: number }>({ x: 0, y: 0 });
  const [showGraph, setShowGraph] = useState(true);
  const filterInputRef = useRef<HTMLInputElement>(null);
  const branchFilterRef = useRef<HTMLInputElement>(null);
  const sortBtnRef = useRef<HTMLButtonElement>(null);
  const [activeSubtab, setActiveSubtab] = useState<'log' | 'console'>('log');
  const [selectedCommits, setSelectedCommits] = useState<GitCommit[]>(commits[0] ? [commits[0]] : []);
  const selectedCommit = selectedCommits[0] ?? null;

  /**
   * 日志顺序（新→旧）里的选中提交。多选文件清单与 diff 只合并这里的提交：
   * 不能取最老父提交到最新提交的范围，否则会带入未被选中的中间提交。
   */
  const orderedSelectedCommits = useMemo(() => {
    const order = new Map(commits.map((commit, index) => [commit.hash, index]));
    return [...selectedCommits]
      .filter(commit => order.has(commit.hash))
      .sort((a, b) => (order.get(a.hash) ?? 0) - (order.get(b.hash) ?? 0));
  }, [commits, selectedCommits]);
  const selectionKey = orderedSelectedCommits.map(commit => commit.hash).join(',');
  /** 顺序与后端统计口径一致：新提交在前，同名文件状态取最新一次。 */
  const selectedHashes = useMemo(
    () => orderedSelectedCommits.map(commit => commit.hash),
    [orderedSelectedCommits]
  );
  const isMultiSelection = selectedHashes.length > 1;

  const [searchFilter, setSearchFilter] = useState('');
  /** 搜索开关（对齐 PyCharm 搜索框右侧两个按钮）：正则匹配 / 区分大小写 */
  const [useRegex, setUseRegex] = useState(false);
  const [caseSensitive, setCaseSensitive] = useState(false);
  /** Paths 过滤的输入草稿（回车应用，服务端过滤） */
  const [pathDraft, setPathDraft] = useState(logPath);
  /** 分栏宽度：左（分支树）与右（提交详情），拖动边缘调整并持久化 */
  const [branchesWidth, setBranchesWidth] = useState(() => {
    const saved = Number(localStorage.getItem(GIT_COL_W_KEY.branches));
    return Number.isFinite(saved) && saved > 0 ? Math.max(140, Math.min(460, saved)) : 200;
  });
  const [detailsWidth, setDetailsWidth] = useState(() => {
    const saved = Number(localStorage.getItem(GIT_COL_W_KEY.details));
    return Number.isFinite(saved) && saved > 0 ? Math.max(180, Math.min(620, saved)) : 290;
  });
  /** 详情列上半区（变更文件清单）高度：上下分割条拖动调整并持久化 */
  const [detailsTopH, setDetailsTopH] = useState(() => {
    const saved = Number(localStorage.getItem(GIT_COL_W_KEY.detailsTop));
    return Number.isFinite(saved) && saved > 0
      ? Math.max(DETAILS_TOP_MIN, Math.min(DETAILS_TOP_MAX, saved))
      : 150;
  });
  /** 当前选中的变更文件（高亮，指示正在查看哪个文件的 diff） */
  const [selectedFilePath, setSelectedFilePath] = useState<string | null>(null);
  /** 跳转高亮的提交哈希（淡入淡出强调动画） */
  const [highlightHash, setHighlightHash] = useState<string | null>(null);
  const highlightTimerRef = useRef<number | null>(null);
  useEffect(() => {
    return () => {
      if (highlightTimerRef.current !== null) {
        window.clearTimeout(highlightTimerRef.current);
      }
    };
  }, []);
  const branchesColRef = useRef<HTMLDivElement>(null);
  const detailsColRef = useRef<HTMLDivElement>(null);
  const detailsTopRef = useRef<HTMLDivElement>(null);

  /** 拖动左/右边缘调整对应分栏宽度（与项目抽屉同一套指针拖拽实现） */
  const startColumnResize = (which: 'branches' | 'details', e: React.PointerEvent) => {
    const startX = e.clientX;
    const startW = which === 'branches' ? branchesWidth : detailsWidth;
    const element = (which === 'branches' ? branchesColRef : detailsColRef).current;
    startPanelDrag(
      e,
      { element, axis: 'x' },
      'col-resize',
      x =>
        which === 'branches'
          ? Math.max(140, Math.min(460, startW + (x - startX)))
          : Math.max(180, Math.min(620, startW - (x - startX))),
      w => {
        if (which === 'branches') setBranchesWidth(w);
        else setDetailsWidth(w);
        try {
          localStorage.setItem(GIT_COL_W_KEY[which], String(Math.round(w)));
        } catch {
          /* 隐私模式下写入失败可忽略 */
        }
      }
    );
  };

  /** 拖动详情列上下分割条：文件清单区（上）↔ 提交信息区（下） */
  const startDetailsSplitResize = (e: React.PointerEvent) => {
    const startY = e.clientY;
    const startH = detailsTopH;
    // 下方提交信息区至少留 DETAILS_BOTTOM_MIN px
    const maxTop = () =>
      Math.max(DETAILS_TOP_MIN, (detailsColRef.current?.clientHeight ?? 0) - DETAILS_BOTTOM_MIN);
    startPanelDrag(
      e,
      { element: detailsTopRef.current, axis: 'y' },
      'row-resize',
      // startPanelDrag 以 (clientX, clientY) 传参：垂直拖拽必须取第二个参数
      (_clientX, clientY) =>
        Math.max(DETAILS_TOP_MIN, Math.min(maxTop(), startH + (clientY - startY))),
      h => {
        const v = Math.round(h);
        setDetailsTopH(v);
        try {
          localStorage.setItem(GIT_COL_W_KEY.detailsTop, String(v));
        } catch {
          /* 隐私模式下写入失败可忽略 */
        }
      }
    );
  };
  const [commitFiles, setCommitFiles] = useState<CommitFile[]>([]);
  const [filesLoading, setFilesLoading] = useState(false);
  const [detailsGroupByDirectory, setDetailsGroupByDirectory] = useState(() =>
    readStoredFlag(DETAILS_VIEW_KEY.groupByDirectory, true)
  );
  const [showDetails, setShowDetails] = useState(() =>
    readStoredFlag(DETAILS_VIEW_KEY.showDetails, true)
  );
  const [detailsViewMenuOpen, setDetailsViewMenuOpen] = useState(false);
  const [detailsViewMenuPos, setDetailsViewMenuPos] = useState({ top: 0, left: 0 });
  const detailsViewMenuRef = useRef<HTMLDivElement>(null);
  const detailsViewMenuPanelRef = useRef<HTMLDivElement>(null);
  const [collapsedDetailsDirs, setCollapsedDetailsDirs] = useState<Set<string>>(new Set());
  const [detailsRootCollapsed, setDetailsRootCollapsed] = useState(false);
  const detailsTree = useMemo(() => buildCompareFileTree(commitFiles), [commitFiles]);
  const detailsRootName = useMemo(
    () =>
      repoNameProp?.trim() ||
      (repoPath || '').replace(/[\\/]+$/, '').split(/[\\/]/).pop() ||
      'Repository',
    [repoNameProp, repoPath]
  );

  // 视图选项持久化：与 PyCharm 一致，下次打开仓库沿用。
  useEffect(() => {
    writeStoredFlag(DETAILS_VIEW_KEY.groupByDirectory, detailsGroupByDirectory);
  }, [detailsGroupByDirectory]);
  useEffect(() => {
    writeStoredFlag(DETAILS_VIEW_KEY.showDetails, showDetails);
  }, [showDetails]);

  useEffect(() => {
    if (!detailsViewMenuOpen) return;
    const onDown = (e: MouseEvent) => {
      const target = e.target as Node;
      if (
        detailsViewMenuRef.current?.contains(target) ||
        detailsViewMenuPanelRef.current?.contains(target)
      ) {
        return;
      }
      setDetailsViewMenuOpen(false);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') setDetailsViewMenuOpen(false);
    };
    window.addEventListener('mousedown', onDown);
    window.addEventListener('keydown', onKey);
    return () => {
      window.removeEventListener('mousedown', onDown);
      window.removeEventListener('keydown', onKey);
    };
  }, [detailsViewMenuOpen]);

  const toggleDetailsDir = (path: string) => {
    setCollapsedDetailsDirs(prev => {
      const next = new Set(prev);
      if (next.has(path)) next.delete(path);
      else next.add(path);
      return next;
    });
  };
  const expandAllDetailsDirs = () => {
    setCollapsedDetailsDirs(new Set());
    setDetailsRootCollapsed(false);
  };
  const collapseAllDetailsDirs = () => {
    setCollapsedDetailsDirs(new Set(allCompareDirPaths(detailsTree)));
    setDetailsRootCollapsed(true);
  };
  const toggleDetailsViewMenu = () => {
    if (detailsViewMenuOpen) {
      setDetailsViewMenuOpen(false);
      return;
    }
    const rect = detailsViewMenuRef.current?.getBoundingClientRect();
    if (rect) {
      const zoom = uiZoom();
      const width = 210 * zoom;
      const height = 150 * zoom;
      const leftVisual = Math.max(
        8,
        Math.min(rect.right - width, window.innerWidth - width - 8)
      );
      // 下方放不下时翻到眼睛按钮上方（与 PyCharm 弹出菜单一致）。
      const topVisual =
        rect.bottom + 4 + height > window.innerHeight - 8
          ? Math.max(8, rect.top - height - 4)
          : rect.bottom + 4;
      setDetailsViewMenuPos({ top: topVisual / zoom, left: leftVisual / zoom });
    }
    setDetailsViewMenuOpen(true);
  };
  const [logEntries, setLogEntries] = useState(getIpcLog());
  const consoleRef = useRef<HTMLDivElement>(null);

  // 提交日志窗口化渲染。
  // 分页每页 50 条，反复「加载更早」后总数可达数千，全量渲染会让每次
  // 滚动都重建几千个 <tr>。这里只渲染可视窗口 + 上下 overscan，
  // 用两个等高 spacer 行把 table 的总高度撑回真实值，滚动条与
  // 「加载更早」触底判定因此保持原样。
  const [logScrollTop, setLogScrollTop] = useState(0);
  const [logViewportH, setLogViewportH] = useState(0);
  const logScrollRef = useRef<HTMLDivElement>(null);
  /** 稳定的外层面板节点：只观察它，不观察会被日志内容撑变的滚动容器。 */
  const logPanelRef = useRef<HTMLDivElement>(null);

  // ⌃P（PyCharm 默认键位）在本面板内切换 Group By Directory。
  // 只在面板可见、焦点在面板内时响应：Commit/Compare 抽屉也有同名快捷键，
  // 面板隐藏但组件常驻，若不按焦点分流会互相切换对方的目录树。
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!(e.ctrlKey || e.metaKey) || e.shiftKey || e.altKey) return;
      if (e.key.toLowerCase() !== 'p') return;
      const panel = logPanelRef.current;
      if (!panel) return;
      // 隐藏时焦点不可能留在面板内（面板由祖先 display:none 隐藏），
      // 因此这一步同时承担「面板可见」判定，避免用 jsdom 拿不到的布局信息。
      const active = document.activeElement;
      if (!active || !panel.contains(active)) return;
      const target = e.target as HTMLElement | null;
      if (target) {
        const tag = target.tagName;
        if (tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT' || target.isContentEditable) {
          return;
        }
      }
      e.preventDefault();
      setDetailsGroupByDirectory(prev => !prev);
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, []);

  /**
   * 滚动事件一帧内可触发多次，每次 setState 都会让整个面板重渲染（分支树 + 日志 + 详情）。
   * 按帧合并——只保留最后一个值，每帧最多提交一次；挂起的值在任何收尾路径上都要落值，
   * 否则最后一次滚动位置会丢（窗口边界停在旧值上）。
   */
  const logScrollPendingRef = useRef<{ top: number; height: number } | null>(null);
  const logScrollFrameRef = useRef<number | null>(null);
  const commitPendingLogScroll = useCallback(() => {
    if (logScrollFrameRef.current !== null) {
      cancelAnimationFrame(logScrollFrameRef.current);
      logScrollFrameRef.current = null;
    }
    const pending = logScrollPendingRef.current;
    if (!pending) return;
    logScrollPendingRef.current = null;
    // 只提交滚动位置。视口高度由外层面板的 ResizeObserver 负责：
    // 在滚动回调里改高度会触发「窗口行数变化 → 容器尺寸变化 → 再滚动」
    // 的反馈环（浏览器预览实测 Maximum update depth）。
    setLogScrollTop(pending.top);
  }, []);

  // Console 页签订阅真实的原生调用日志。
  // 只在页签可见时订阅：pty_write 每次按键都产生日志，不可见时setState纯属浪费
  useEffect(() => {
    if (activeSubtab !== 'console') return;
    return subscribeIpcLog(setLogEntries);
  }, [activeSubtab]);

  useEffect(() => {
    if (activeSubtab === 'console' && consoleRef.current) {
      consoleRef.current.scrollTop = consoleRef.current.scrollHeight;
    }
  }, [logEntries, activeSubtab]);

  // diff 导航切换文件后，同步提交文件清单里的高亮（activeFile 可能是绝对路径）
  useEffect(() => {
    if (!activeFile) return;
    const hit = commitFiles.find(f => isSamePath(activeFile, f.path, repoPath));
    if (hit) {
      setSelectedFilePath(hit.path);
      requestAnimationFrame(() => {
        document
          .querySelector('.details-file-item.active')
          ?.scrollIntoView({ block: 'nearest' });
      });
    }
  }, [activeFile, commitFiles, repoPath]);

  // 选中提交变化时拉取真实变更文件清单：单选取该提交，多选调用
  // git_commits_files 只合并显式选中的提交，状态取最新提交、+/− 累加。
  useEffect(() => {
    const first = orderedSelectedCommits[0];
    setSelectedFilePath(null);
    if (!first) {
      setCommitFiles([]);
      setFilesLoading(false);
      return;
    }
    let cancelled = false;
    setCommitFiles([]);
    setFilesLoading(true);
    const request = isMultiSelection
      ? invokeCommand<CommitFile[]>('git_commits_files', {
          repoPath: repoPath || null,
          commits: selectedHashes
        })
      : invokeCommand<CommitFile[]>('git_commit_files', {
          repoPath: repoPath || null,
          commit: first.hash
        });
    request
      .then(files => {
        if (!cancelled) setCommitFiles(Array.isArray(files) ? files : []);
      })
      .catch(() => {
        if (!cancelled) setCommitFiles([]);
      })
      .finally(() => {
        if (!cancelled) setFilesLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [selectionKey, repoPath, selectedHashes, isMultiSelection]);

  /** 整个选中范围的 diff：多选走所选提交合并 diff，单选取提交本身。 */
  const openSelectionDiff = () => {
    if (isMultiSelection) {
      setSelectedFilePath(null);
      onOpenCommitsDiff(selectedHashes);
      return;
    }
    if (selectedCommit) {
      setSelectedFilePath(null);
      onOpenCommitDiff(selectedCommit.hash);
    }
  };

  /** 选中范围里单个文件的 diff：多选同样只合并所选提交。 */
  const openSelectionFileDiff = (path: string) => {
    setSelectedFilePath(path);
    if (isMultiSelection) {
      onOpenCommitsDiff(selectedHashes, path);
      return;
    }
    if (selectedCommit) onOpenFileDiff(selectedCommit.hash, path);
  };

  const handleRowClick = (c: GitCommit, e?: React.MouseEvent) => {
    // 断开箭头的跳转不走 SVG 的 onClick：WKWebView 上那块命中区收不到点击
    //（实测 fill="transparent" 与 pointer-events="all" 都无效，事件直接落到行上，
    //  表现为「点箭头 = 只选中所在行」，跳转静默失效）。改为在行级按坐标判定：
    //  落在哪个箭头的矩形里就跳到配对端，不依赖 WebKit 的 SVG 命中测试实现。
    //  graphBreaks / jumpToCommit 定义在下方，但只在点击时才读到，无 TDZ 问题。
    if (e) {
      try {
        const g = e.currentTarget.querySelector('.log-graph');
        if (g) {
          const r = g.getBoundingClientRect();
          const x = e.clientX - r.left;
          const y = e.clientY - r.top;
          const brk = graphBreaks.get(c.hash);
          for (const side of ['below', 'above'] as const) {
            for (const b of brk?.[side] ?? []) {
              const h = foldArrowHit(b.lane, side === 'below' ? 'down' : 'up');
              if (x >= h.x0 && x <= h.x1 && y >= h.y0 && y <= h.y1) {
                if (b.pending) {
                  // 段延伸到分页底、父提交未加载：记下待跳请求并触发续页，
                  // 下方的续页闭环 effect 在箭头解析成真父提交后完成跳转。
                  // 旧实现只滚到底 + 续一页、从不跳转（注释声称「再跳」但没有
                  // 对应代码），点击看起来像「跳到了列表末尾的某一行」。
                  // !hasMore 时父提交永远进不了列表，不留悬空请求。
                  pendingJumpRef.current = hasMore
                    ? { hash: c.hash, lane: b.lane, side, lastLen: commits.length, tries: 0 }
                    : null;
                  const el = logScrollRef.current;
                  el?.scrollTo({ top: el.scrollHeight, behavior: 'smooth' });
                  if (hasMore && !loadingMore) onLoadMore?.();
                } else {
                  jumpToCommit(b.peer);
                }
                return;
              }
            }
          }
        }
      } catch {
        // 判定失败就按普通行点击处理，绝不能让异常吞掉整次点击
      }
    }
    if (e && (e.metaKey || e.ctrlKey)) {
      setSelectedCommits(prev => {
        const exists = prev.some(item => item.hash === c.hash);
        if (exists) {
          if (prev.length > 1) {
            const next = prev.filter(item => item.hash !== c.hash);
            onSelectCommit(next[0]);
            return next;
          }
          return prev;
        } else {
          // 不限制多选提交数量
          return [...prev, c];
        }
      });
      return;
    }
    setSelectedCommits([c]);
    onSelectCommit(c);
  };

  // commits 整体替换（切项目 / 切分支 / 刷新）后 selectedCommit 会停在旧列表里的提交：
  // 提交详情区显示的是上一个仓库/分支的提交与文件清单，Revert 也会拿旧 hash 打到新仓库。
  // 只在该 hash 确实不在当前列表时才回落到首条，用户在当前列表里的显式选择不动。
  useEffect(() => {
    if (selectedCommits.length > 0 && selectedCommits.some(sc => commits.some(c => c.hash === sc.hash))) return;
    // 关键：empty 分支也要返回同一个 prev。旧实现 `setSelectedCommits([])`
    // 每次都创建新数组，而 effect 依赖 selectedCommits —— 欢迎页 commits 为空时
    // 形成 setState → 新 [] → effect → setState 的更新深度死循环。
    setSelectedCommits(prev => {
      const next = commits[0] ? [commits[0]] : [];
      if (
        prev.length === next.length &&
        prev.every((item, index) => item.hash === next[index].hash)
      ) {
        return prev;
      }
      return next;
    });
  }, [commits, selectedCommits]);

  /** 日志里出现过的作者（User 过滤菜单数据源）：单次遍历避免中间数组分配 */
  const logAuthors = useMemo(() => {
    const set = new Set<string>();
    for (let i = 0; i < commits.length; i++) {
      set.add(commits[i].author);
    }
    return Array.from(set).sort();
  }, [commits]);
  const favoriteSet = useMemo(() => new Set(favorites), [favorites]);

  // 分支树数据源：分支集合与当前分支不变时没必要每次 render 重跑 filter + sort
  const localBranches = useMemo(
    () => sortLocalBranches(branches.filter(b => !b.isRemote), currentBranch, n => favoriteSet.has(n)),
    [branches, currentBranch, favoriteSet]
  );
  const remoteBranches = useMemo(() => branches.filter(b => b.isRemote), [branches]);
  /** HEAD 组展示的分支：当前本地分支；detached 等拿不到对象时兜底构造一行 */
  const headBranch: GitBranch | undefined =
    localBranches.find(b => b.isCurrent || b.name === currentBranch) ??
    (currentBranch
      ? { name: currentBranch, isCurrent: true, isRemote: false }
      : undefined);

  /** 标签在分支树里以 GitBranch 形态渲染（isTag=true），后端 git_branches 不返回它们。 */
  const tagBranches = useMemo<GitBranch[]>(
    () => tags.map(tag => ({ name: tag.name, isCurrent: false, isRemote: false, isTag: true })),
    [tags]
  );
  const allRefs = useMemo(() => {
    const map = new Map<string, GitBranch>();
    for (const ref of [headBranch, ...localBranches, ...remoteBranches, ...tagBranches]) {
      if (ref) map.set(refKey(ref), ref);
    }
    return map;
  }, [headBranch, localBranches, remoteBranches, tagBranches]);

  const branchNeedle = branchFilter.trim().toLowerCase();
  const matchesBranchFilter = useCallback(
    (ref: GitBranch) => !branchNeedle || ref.name.toLowerCase().includes(branchNeedle),
    [branchNeedle]
  );
  const filteredLocal = useMemo(
    () => localBranches.filter(matchesBranchFilter),
    [localBranches, matchesBranchFilter]
  );
  const filteredRemote = useMemo(
    () => remoteBranches.filter(matchesBranchFilter),
    [remoteBranches, matchesBranchFilter]
  );
  const filteredTags = useMemo(
    () => tagBranches.filter(matchesBranchFilter),
    [tagBranches, matchesBranchFilter]
  );

  const collapsedPathsFor = useCallback(
    (kind: 'local' | 'remote' | 'tag') => {
      // 搜索时自动展开，否则命中项可能藏在折叠目录里，用户会以为没搜到
      if (branchNeedle) return new Set<string>();
      const prefix = `dir:${kind}:`;
      return new Set(
        [...collapsedRefDirs]
          .filter(k => k.startsWith(prefix))
          .map(k => k.slice(prefix.length))
      );
    },
    [branchNeedle, collapsedRefDirs]
  );

  const localRows = useMemo(() => {
    if (!groupByDirectory) {
      return filteredLocal.map(ref => ({ type: 'ref' as const, depth: 0, path: ref.name, ref }));
    }
    return flattenRefTree(buildRefTree(filteredLocal), collapsedPathsFor('local'));
  }, [filteredLocal, groupByDirectory, collapsedPathsFor]);
  const remoteRows = useMemo(() => {
    if (!groupByDirectory) {
      return filteredRemote.map(ref => ({ type: 'ref' as const, depth: 0, path: ref.name, ref }));
    }
    return flattenRefTree(buildRefTree(filteredRemote), collapsedPathsFor('remote'));
  }, [filteredRemote, groupByDirectory, collapsedPathsFor]);
  const tagRows = useMemo(() => {
    if (!groupByDirectory) {
      return filteredTags.map(ref => ({ type: 'ref' as const, depth: 0, path: ref.name, ref }));
    }
    return flattenRefTree(buildRefTree(filteredTags), collapsedPathsFor('tag'));
  }, [filteredTags, groupByDirectory, collapsedPathsFor]);

  const sectionCollapsed = useCallback(
    (section: 'local' | 'remote' | 'tags') => collapsedSections.has(`sec:${section}`),
    [collapsedSections]
  );
  /** 当前可见叶子 ref 的展示顺序：Shift 连选只在这个序列上工作。 */
  const visibleRefOrder = useMemo(() => {
    const out: string[] = [];
    if (branchNeedle || !sectionCollapsed('local')) out.push(...visibleRefKeys(localRows));
    if (branchNeedle || !sectionCollapsed('remote')) out.push(...visibleRefKeys(remoteRows));
    if (branchNeedle || !sectionCollapsed('tags')) out.push(...visibleRefKeys(tagRows));
    return out;
  }, [branchNeedle, sectionCollapsed, localRows, remoteRows, tagRows]);

  const selectedBranches = useMemo(
    () => selectedRefKeys.map(k => allRefs.get(k)).filter((b): b is GitBranch => !!b),
    [selectedRefKeys, allRefs]
  );

  /** 目录行右键：把该目录下所有叶子当作多选。 */
  const refsUnderNode = (node: RefTreeNode): GitBranch[] => {
    const out: GitBranch[] = [];
    const walk = (n: RefTreeNode) => {
      for (const e of n.entries) {
        if (e.type === 'dir') walk(e.node);
        else out.push(e.ref);
      }
    };
    walk(node);
    return out;
  };

  const selectRef = (ref: GitBranch, e: React.MouseEvent) => {
    const key = refKey(ref);
    const additive = e.metaKey || e.ctrlKey;
    const range = e.shiftKey;
    let next: string[];
    if (range && lastRefKeyRef.current && lastRefKeyRef.current !== key) {
      const a = visibleRefOrder.indexOf(lastRefKeyRef.current);
      const b = visibleRefOrder.indexOf(key);
      if (a >= 0 && b >= 0) {
        const [lo, hi] = a < b ? [a, b] : [b, a];
        next = visibleRefOrder.slice(lo, hi + 1);
      } else {
        next = [key];
      }
    } else if (additive) {
      next = selectedRefKeys.includes(key)
        ? selectedRefKeys.filter(k => k !== key)
        : [...selectedRefKeys, key];
      if (next.length === 0) next = [key];
    } else {
      next = [key];
    }
    setSelectedRefKeys(next);
    setSelectedFolderKeys([]);
    lastRefKeyRef.current = key;
  };

  const selectionForContext = (ref: GitBranch): GitBranch[] => {
    const key = refKey(ref);
    if (selectedRefKeys.includes(key)) {
      return selectedRefKeys.map(k => allRefs.get(k)).filter((b): b is GitBranch => !!b);
    }
    setSelectedRefKeys([key]);
    lastRefKeyRef.current = key;
    return [ref];
  };

  const filteredCommits = useMemo(() => {
    const needle = caseSensitive ? searchFilter : searchFilter.toLowerCase();
    // 正则模式：编译失败（用户还在输入中）时视为无过滤，不打断列表
    let re: RegExp | null = null;
    if (useRegex && searchFilter) {
      try {
        re = new RegExp(searchFilter, caseSensitive ? '' : 'i');
      } catch {
        re = null;
      }
    }
    const matchText = (c: GitCommit) => {
      if (!searchFilter) return true;
      const fields = [c.message, c.shortHash, c.author];
      if (re) return fields.some(f => re!.test(f));
      const msg = caseSensitive ? c.message : c.message.toLowerCase();
      const hash = caseSensitive ? c.shortHash : c.shortHash.toLowerCase();
      return msg.includes(needle) || hash.includes(needle) || (caseSensitive ? c.author : c.author.toLowerCase()).includes(needle);
    };
    const since = (() => {
      if (dateFilter === 'all') return 0;
      const d = new Date();
      d.setHours(0, 0, 0, 0);
      const days = dateFilter === 'today' ? 0 : dateFilter === 'week' ? 7 : 30;
      return d.getTime() / 1000 - days * 86400 + (dateFilter === 'today' ? 0 : 0);
    })();
    return commits.filter(c => {
      if (!matchText(c)) return false;
      if (userFilter && c.author !== userFilter) return false;
      if (dateFilter !== 'all' && /^\d{9,}$/.test(c.date)) {
        if (Number(c.date) < since) return false;
      }
      return true;
    });
  }, [commits, searchFilter, caseSensitive, useRegex, userFilter, dateFilter]);

  // 本地搜索/服务端过滤改变可见集合后，绝不能把不可见的提交留在多选里：
  // 批量 Drop/Revert/Cherry-pick 的确认框只报数量，用户看不到被隐藏的对象。
  useEffect(() => {
    const visible = new Set(filteredCommits.map(c => c.hash));
    setSelectedCommits(prev => {
      const kept = prev.filter(c => visible.has(c.hash));
      if (kept.length === prev.length) return prev;
      if (kept.length > 0) return kept;
      return filteredCommits[0] ? [filteredCommits[0]] : [];
    });
  }, [filteredCommits]);



  // 窗口边界。行高从 DOM 量一次（首行渲染后），拿不到就退化为 26px 的
  // 近似值；overscan 取两屏，保证快速滚动时有内容可看。
  const LOG_ROW_OVERSCAN = 24;
  const [logRowH, setLogRowH] = useState(26);
  useEffect(() => {
    const el = logScrollRef.current;
    const first = el?.querySelector<HTMLElement>('[data-commit-row]');
    if (first) setLogRowH(first.getBoundingClientRect().height || 26);
  }, [filteredCommits.length]);
  const logWindow = useMemo(() => {
    const start = Math.max(0, Math.floor(logScrollTop / logRowH) - LOG_ROW_OVERSCAN);
    const visible = Math.ceil((logViewportH || 400) / logRowH) + LOG_ROW_OVERSCAN * 2;
    const end = Math.min(filteredCommits.length, start + visible);
    return { start, end };
  }, [logScrollTop, logViewportH, logRowH, filteredCommits.length]);
  /**
   * 长跨度折叠：判定与端点箭头全部下沉到 services/logFolding.ts
   * （computeGraphLayout + computeGraphBreaks），那边以「前缀单调」
   * 为硬不变式写死 —— 滚动 loadMore 追加提交后重算，已画好的
   * 断开/省略标记不会被撤销（旧实现按整段 interaction 二元判定，
   * 远端 merge 一加载进来整段放弃折叠，蓝线从断开变回连接）。
   */
  const graphLayoutMap = useMemo(() => computeGraphLayout(commits), [commits]);

  // 上一轮折叠结果：loadMore 后重算 fresh，再把旧标记合并进来（只加不撤）。
  // 渲染期写 ref 是幂等的（同前缀合并收敛到同一结果，仓库无 StrictMode，见 main.tsx）。
  const prevBreaksRef = useRef<{ commits: GitCommit[]; breaks: Map<string, GraphBreak> } | null>(null);
  const graphBreaks = useMemo(() => {
    const fresh = computeGraphBreaks(commits, graphLayoutMap);
    const prev = prevBreaksRef.current;
    const merged = prev ? mergeGraphBreaks(prev.commits, prev.breaks, commits, fresh) : fresh;
    prevBreaksRef.current = { commits, breaks: merged };
    return merged;
  }, [commits, graphLayoutMap]);

  /** 点击 pending ▼/▲ 时记录的待跳请求（行 hash + 轨道 + 方向），
   *  由下方 pendingJumpEffect 在箭头解析后消费。lastLen/tries 用于区分
   *  「续页在推进」（计数清零、继续等）与「git_log 连续失败」（连续 3 次
   *  列表没变长就放弃，避免对着坏仓库无限重试）。 */
  const pendingJumpRef = useRef<{
    hash: string;
    lane: number;
    side: 'below' | 'above';
    lastLen: number;
    tries: number;
  } | null>(null);

  /** 跳到 `c` 那一行：选中它、平滑滚动到视口中间，并闪烁高亮强调目标行 */
  const jumpToCommit = useCallback(
    (c: GitCommit) => {
      setSelectedCommits([c]);
      onSelectCommit(c);
      const idx = filteredCommits.findIndex(x => x.hash === c.hash);
      const el = logScrollRef.current;
      if (idx >= 0 && el) {
        el.scrollTo({
          top: Math.max(0, idx * logRowH - el.clientHeight / 2 + logRowH / 2),
          behavior: 'smooth',
        });
        setHighlightHash(c.hash);
        if (highlightTimerRef.current !== null) {
          window.clearTimeout(highlightTimerRef.current);
        }
        highlightTimerRef.current = window.setTimeout(() => {
          setHighlightHash(null);
          highlightTimerRef.current = null;
        }, 1500);
      } else if (hasMore && onLoadMore) {
        el?.scrollTo({ top: el.scrollHeight, behavior: 'smooth' });
        onLoadMore();
      }
    },
    [filteredCommits, logRowH, hasMore, onLoadMore, onSelectCommit]
  );

  /**
   * 外部跳转请求（focusCommit prop）的消费：找到提交就走 jumpToCommit
   * （选中 + 滚动 + 闪烁三件套，与 ▼/▲ 箭头跳转同通道）。
   * 找不到（过滤/分页外）则静默忽略 —— canGoParent/Child 在菜单侧已按
   * 未过滤列表禁用了可达性，过滤导致的不可达是可接受的降级。
   */
  const lastFocusSeqRef = useRef(0);
  useEffect(() => {
    if (!focusCommit || focusCommit.seq <= lastFocusSeqRef.current) return;
    lastFocusSeqRef.current = focusCommit.seq;
    const found = commits.find(c => c.hash === focusCommit.hash);
    if (!found) return;
    // 目标被搜索/用户/日期过滤藏住时：只移动选中态（详情区跟过去），
    // 不走 jumpToCommit —— 否则它会误触"滚到底 + 加载更多"。
    if (filteredCommits.some(c => c.hash === found.hash)) jumpToCommit(found);
    else {
      setSelectedCommits([found]);
      onSelectCommit(found);
    }
  }, [focusCommit, commits, filteredCommits, jumpToCommit, onSelectCommit]);

  /**
   * pending 箭头跳转的续页闭环：点击 ▼ 时父提交尚未加载（箭头 pending），
   * 点击先记下请求并触发续页；此后每次分页重算 graphBreaks 都来检查：
   * - 解析成真父提交 → 完成这次点击承诺的跳转；
   * - 仍 pending → 有下一页就继续加载（loadingMore 在途时等下一轮）；
   * - 没有更多提交 → 放弃（父提交永远进不了列表）。
   * 必须循环续页，补一次不够：真实仓库里该箭头 N=200 仍 pending、
   * N=250 才解析（scripts/verify-log-folding.mjs 的多页夹具锁住该形态）。
   */
  useEffect(() => {
    const req = pendingJumpRef.current;
    if (!req) return;
    const m = graphBreaks.get(req.hash)?.[req.side]?.find(a => a.lane === req.lane);
    if (m?.pending) {
      if (!hasMore) {
        pendingJumpRef.current = null;
        return;
      }
      if (loadingMore) return;
      // 每次发起续页前记账：列表长了 = 分页在推进（清零重计）；
      // 没长 = 上一次 git_log 失败或被去重吞掉，连续 3 次就放弃。
      if (commits.length > req.lastLen) {
        req.lastLen = commits.length;
        req.tries = 0;
      } else if (++req.tries > 3) {
        pendingJumpRef.current = null;
        return;
      }
      onLoadMore?.();
      return;
    }
    pendingJumpRef.current = null;
    if (m) jumpToCommit(m.peer);
  }, [graphBreaks, commits.length, hasMore, loadingMore, onLoadMore, jumpToCommit]);

  const visibleCommits = filteredCommits.slice(logWindow.start, logWindow.end);

  /** 分支行（HEAD / Local 两组共用）：名字过长省略号截断，title 悬停显全名 */
  const toggleSection = (section: 'local' | 'remote' | 'tags') => {
    setCollapsedSections(prev => {
      const next = new Set(prev);
      const key = `sec:${section}`;
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });
  };
  const toggleDir = (kind: 'local' | 'remote' | 'tag', path: string) => {
    setCollapsedRefDirs(prev => {
      const next = new Set(prev);
      const key = `dir:${kind}:${path}`;
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });
  };
  const expandAllRefs = () => {
    setCollapsedSections(new Set());
    setCollapsedRefDirs(new Set());
  };
  const collapseAllRefs = () => {
    setCollapsedSections(new Set(['sec:local', 'sec:remote', 'sec:tags']));
    setCollapsedRefDirs(new Set());
  };

  const isFavoriteRef = (b: GitBranch) =>
    favorites.includes(b.name) ||
    (b.isRemote && favorites.includes(b.name.split('/').slice(1).join('/')));
  const renderRefRow = (b: GitBranch, depth: number, hint: string) => {
    const key = refKey(b);
    const leaf = depth > 0 ? b.name.split('/').filter(Boolean).pop() ?? b.name : b.name;
    return (
      <div
        key={`br-${key}`}
        onClick={e => selectRef(b, e)}
        onDoubleClick={() => {
          if (b.isCurrent) return;
          if (b.isTag) onCheckoutTag?.(b.name);
          else onSelectBranch?.(b.name);
        }}
        onContextMenu={e => {
          e.preventDefault();
          const selected = selectionForContext(b);
          onBranchContext?.(b, e.clientX, e.clientY, selected);
        }}
        className={`b-item ${!b.isTag && b.name === currentBranch ? 'active' : ''} ${selectedRefKeys.includes(key) ? 'selected' : ''}`}
        style={{ paddingLeft: `${8 + depth * 12}px` }}
        title={`${b.name}\n${hint}`}
      >
        <span className="b-icon">
          {isFavoriteRef(b) && !b.isTag ? (
            <IconStar size={12} color="var(--git-yellow, #e3b341)" />
          ) : b.isTag || b.isCurrent ? (
            <IconTag size={12} color="var(--git-yellow, #e3b341)" />
          ) : b.isRemote ? (
            <IconCloud size={13} color="var(--text-muted)" />
          ) : (
            <GitBranchIcon size={13} color="var(--git-green)" />
          )}
        </span>
        <span className="b-name">{leaf}</span>
        {(b.ahead || b.behind) ? (
          <span className="b-up-arrow">
            {b.ahead ? `↑${b.ahead}` : ''}
            {b.behind ? ` ↓${b.behind}` : ''}
          </span>
        ) : null}
      </div>
    );
  };

  const renderFolderRow = (kind: 'local' | 'remote' | 'tag', node: RefTreeNode, depth: number) => {
    const key = `dir:${kind}:${node.path}`;
    const closed = !branchNeedle && collapsedRefDirs.has(key);
    return (
      <div
        key={`dir-${kind}-${node.path}`}
        className={`b-item b-folder-row ${selectedFolderKeys.includes(key) ? 'selected' : ''}`}
        style={{ paddingLeft: `${8 + depth * 12}px` }}
        onClick={e => {
          if (e.metaKey || e.ctrlKey) {
            const refs = refsUnderNode(node);
            if (selectedFolderKeys.includes(key)) {
              setSelectedFolderKeys([]);
              setSelectedRefKeys([]);
            } else {
              setSelectedRefKeys(refs.map(refKey));
              setSelectedFolderKeys([key]);
            }
            return;
          }
          setSelectedFolderKeys([]);
          toggleDir(kind, node.path);
        }}
        onDoubleClick={() => toggleDir(kind, node.path)}
        onContextMenu={e => {
          e.preventDefault();
          const refs = refsUnderNode(node);
          if (refs.length === 0) return;
          setSelectedRefKeys(refs.map(refKey));
          setSelectedFolderKeys([key]);
          onBranchContext?.(refs[0], e.clientX, e.clientY, refs);
        }}
        title={node.path}
      >
        <span className="b-chevron">
          {closed ? (
            <IconChevronRight size={10} color="var(--text-muted)" />
          ) : (
            <IconChevronDown size={10} color="var(--text-muted)" />
          )}
        </span>
        <FolderIcon size={12} color="var(--text-muted)" />
        <span className="b-name">{node.name}</span>
      </div>
    );
  };

  const renderRefRows = (kind: 'local' | 'remote' | 'tag', rows: ReturnType<typeof flattenRefTree>) =>
    rows.map(row =>
      row.type === 'dir'
        ? renderFolderRow(kind, row.node!, row.depth)
        : renderRefRow(row.ref!, row.depth, refHint(row.ref!))
    );

  const refHint = (b: GitBranch) =>
    b.isTag
      ? t().gitPanel.refTag(b.name)
      : b.isRemote
        ? t().gitPanel.branchCheckoutTrackHint
        : t().gitPanel.branchCheckoutHint;

  const renderSection = (section: 'local' | 'remote' | 'tags', label: string, rows: ReturnType<typeof flattenRefTree>) => {
    const collapsed = !branchNeedle && sectionCollapsed(section);
    return (
      <React.Fragment key={`sec-${section}`}>
        <div className="b-row b-group-row" onClick={() => toggleSection(section)}>
          {collapsed ? (
            <IconChevronRight size={11} color="var(--text-muted)" />
          ) : (
            <IconChevronDown size={11} color="var(--text-muted)" />
          )}
          <span className="b-title">{label}</span>
        </div>
        {!collapsed && <div className="b-nested">{renderRefRows(section === 'tags' ? 'tag' : section, rows)}</div>}
      </React.Fragment>
    );
  };

  const singleSelected = selectedBranches.length === 1 ? selectedBranches[0] : null;
  const checkoutSelected = () => {
    if (!singleSelected || singleSelected.isCurrent) return;
    if (singleSelected.isTag) onCheckoutTag?.(singleSelected.name);
    else onSelectBranch?.(singleSelected.name);
  };
  const deleteSelected = () => {
    if (singleSelected?.isTag && onDeleteTag) onDeleteTag(singleSelected.name);
    else if (onDeleteSelected) onDeleteSelected(selectedBranches);
  };
  const updateSelected = () => {
    if (onUpdateSelected) onUpdateSelected(selectedBranches);
  };
  const compareSelected = () => {
    if (selectedBranches.length === 2) onCompareBranches?.(selectedBranches[0], selectedBranches[1]);
  };
  const toggleFavoriteSelected = () => {
    if (singleSelected) onToggleFavorite?.(singleSelected.name);
  };
  const openBranchViewMenu = (e: React.MouseEvent<HTMLButtonElement>) => {
    const r = e.currentTarget.getBoundingClientRect();
    setBranchViewMenu({ x: r.right, y: r.bottom + 2 });
  };

  /** 滚动到底部附近时自动加载更早的提交 */
  const handleLogScroll = (e: React.UIEvent<HTMLDivElement>) => {
    const el = e.currentTarget;
    logScrollPendingRef.current = { top: el.scrollTop, height: el.clientHeight };
    if (logScrollFrameRef.current === null) {
      logScrollFrameRef.current = requestAnimationFrame(() => {
        logScrollFrameRef.current = null;
        commitPendingLogScroll();
      });
    }
    if (!hasMore || loadingMore) return;
    if (el.scrollTop + el.clientHeight >= el.scrollHeight - 80) {
      onLoadMore?.();
    }
  };

  // 切到 Console 页签会卸载日志滚动容器（DOM 回到 scrollTop 0），离开前把挂起值提交给
  // state：否则切回 Log 时窗口边界停在上次提交前的偏移上，列表可能整片空白。
  useEffect(() => {
    if (activeSubtab === 'log') return;
    commitPendingLogScroll();
  }, [activeSubtab, commitPendingLogScroll]);

  // 容器尺寸变化时重算窗口：面板被拖高、切回 Log 页签都要重新取一次
  // clientHeight。只观察外层面板，不观察 .git-log-scroll：后者的尺寸由
  // 窗口化行数间接影响，观察它会形成 ResizeObserver 反馈环。
  useEffect(() => {
    const el = logScrollRef.current;
    const panel = logPanelRef.current;
    if (!el || !panel || activeSubtab !== 'log') return;
    const measure = () => {
      const h = el.clientHeight;
      if (h <= 0) return; // 面板隐藏 / 尚未布局；可见时 panel RO 会再触发
      setLogViewportH(prev => (Math.abs(prev - h) < 1 ? prev : h));
    };
    measure();
    const ro = new ResizeObserver(measure);
    ro.observe(panel);
    return () => ro.disconnect();
  }, [activeSubtab, filteredCommits.length]);
  const renderDetailsFile = (file: CommitFile, depth: number): React.ReactNode => {
    const name = file.path.split('/').pop() ?? file.path;
    const active = isSamePath(selectedFilePath || '', file.path, repoPath) ||
      (activeFile ? isSamePath(activeFile, file.path, repoPath) : false);
    return (
      <div
        key={file.path}
        className={`details-file-item ${active ? 'active' : ''}`}
        style={{ paddingLeft: 8 + depth * 12, cursor: 'pointer' }}
        title={t().gitPanel.fileDiffHint(file.path)}
        onClick={() => setSelectedFilePath(file.path)}
        onDoubleClick={() => openSelectionFileDiff(file.path)}
        onContextMenu={e => {
          if (!selectedCommit) return;
          e.preventDefault();
          setSelectedFilePath(file.path);
          setFileMenu({ x: e.clientX, y: e.clientY, path: file.path });
        }}
      >
        <span className="details-file-icon">{renderFileIcon(name, false, false)}</span>
        <span
          className="file-item-name"
          style={{
            flex: 1,
            minWidth: 0,
            overflow: 'hidden',
            textOverflow: 'ellipsis',
            whiteSpace: 'nowrap',
            color: detailsStatusColor(file.status)
          }}
        >
          {name}
        </span>
        <span className="details-file-stats">
          <span className="det-lines green">+{file.additions}</span>
          {file.deletions > 0 && <span className="det-lines red">-{file.deletions}</span>}
        </span>
      </div>
    );
  };

  const renderDetailsDirs = (node: CompareFileTreeNode, depth: number): React.ReactNode => (
    <>
      {node.dirs.map(dir => {
        const collapsed = collapsedDetailsDirs.has(dir.path);
        return (
          <React.Fragment key={dir.path}>
            <div
              className="details-dir-row"
              style={{ paddingLeft: 8 + depth * 12 }}
              onClick={() => toggleDetailsDir(dir.path)}
              title={dir.path}
            >
              <span className="details-chevron">
                {collapsed ? (
                  <IconChevronRight size={10} color="var(--text-muted)" />
                ) : (
                  <IconChevronDown size={10} color="var(--text-muted)" />
                )}
              </span>
              <FolderIcon size={13} isOpen={!collapsed} />
              <span className="details-dir-name">{dir.name}</span>
              <span className="details-dir-count">{t().compareBranches.fileCount(dir.fileCount)}</span>
            </div>
            {!collapsed && renderDetailsDirs(dir, depth + 1)}
          </React.Fragment>
        );
      })}
      {node.files.map(file => renderDetailsFile(file, depth))}
    </>
  );

  return (
    <div
      className="pycharm-bottom-git"
      id="pycharmBottomGit"
      ref={logPanelRef}
      tabIndex={-1}
      onPointerDown={e => {
        // 快捷键按焦点分流：点击面板任意空白处先让面板拿到焦点。
        // 按钮/输入框保持自身焦点（它们同样位于面板内）。
        const target = e.target as HTMLElement;
        if (target.closest('button, input, textarea, select, [contenteditable="true"]')) return;
        e.currentTarget.focus({ preventScroll: true });
      }}
      style={{
        height: '100%',
        flex: 1,
        minHeight: 0
      }}
    >
      <div className="bottom-git-header">
        <div className="git-header-tabs">
          <span className="git-tab-title">{t().gitPanel.tabTitle}</span>
          <div className="git-subtabs">
            <button
              onClick={() => setActiveSubtab('log')}
              className={`subtab-btn ${activeSubtab === 'log' ? 'active' : ''}`}
            >
              {t().gitPanel.tabLog}
            </button>
            <button
              onClick={() => setActiveSubtab('console')}
              className={`subtab-btn ${activeSubtab === 'console' ? 'active' : ''}`}
            >
              {t().gitPanel.tabConsole}
            </button>
          </div>
        </div>

        {/* Window actions (⤢ / —, ✕) */}
        <div className="git-header-window-actions">
          {onOpenTools && (
            <button
              onClick={onOpenTools}
              className="win-action-btn"
              title={t().gitTools.title}
            >
              <IconGear size={13} />
            </button>
          )}
          <button
            onClick={() => onToggleMaximize?.()}
            className="win-action-btn"
            title={t().gitPanel.maximizeRestore}
          >
            {maximized ? <IconWindowMinimize size={12} /> : <IconWindowMaximize size={12} />}
          </button>
          <button
            onClick={onClose}
            className="win-action-btn"
            title={t().common.close}
          >
            <IconWindowClose size={13} />
          </button>
        </div>
      </div>

      {/* 2. Content */}
      {activeSubtab === 'log' ? (
        isRepo === false ? (
            /* 非 git 仓库：提示并提供初始化入口 */
            <div className="git-no-repo-banner">
              <span className="no-repo-text">{t().gitPanel.notRepoText}</span>
              {onInitRepo && (
                <button className="no-repo-init-btn" onClick={onInitRepo} disabled={busy}>
                  {t().gitPanel.initRepo}
                </button>
              )}
            </div>
          ) : (
          <div className="git-four-column-body" id="gitLogFourColumn">
            {/* Col 2: Branches Tree */}
            <div
              className={`git-col-branches ${branchListHidden ? 'branches-hidden' : ''}`}
              id="gitColBranches"
              ref={branchesColRef}
              style={{ width: branchListHidden ? '30px' : `${branchesWidth}px` }}
            >
              {/* 纵向工具栏：PyCharm 分支树左侧的窄条，全部动作针对当前多选。 */}
              <div className="branch-vertical-toolbar" role="toolbar" aria-label={t().gitPanel.local}>
                <button
                  className={`branch-tool-btn ${branchListHidden ? 'active' : ''}`}
                  onClick={() => setBranchListHidden(v => !v)}
                  title={branchListHidden ? t().gitPanel.branchToolbar.showBranches : t().gitPanel.branchToolbar.hideBranches}
                  aria-expanded={!branchListHidden}
                >
                  {branchListHidden ? <IconDoubleChevronRight size={13} /> : <IconDoubleChevronLeft size={13} />}
                </button>
                <div className="branch-tool-sep" />
                <button className="branch-tool-btn" onClick={() => onCreateBranch?.()} title={t().gitPanel.branchToolbar.newBranch}>
                  <IconPlus size={14} />
                </button>
                <button
                  className="branch-tool-btn"
                  onClick={updateSelected}
                  disabled={selectedBranches.length === 0 || selectedBranches.some(b => b.isRemote || b.isTag)}
                  title={t().gitPanel.branchToolbar.update}
                >
                  <IconUpdateArrow size={14} />
                </button>
                <button
                  className="branch-tool-btn"
                  onClick={deleteSelected}
                  disabled={selectedBranches.length === 0}
                  title={t().gitPanel.branchToolbar.delete}
                >
                  <IconTrash size={14} />
                </button>
                <button
                  className="branch-tool-btn"
                  onClick={checkoutSelected}
                  disabled={!singleSelected || !!singleSelected.isCurrent}
                  title={t().gitPanel.branchToolbar.checkout}
                >
                  <GitBranchIcon size={14} color="var(--text-secondary)" />
                </button>
                <div className="branch-tool-sep" />
                <button
                  className={`branch-tool-btn ${branchSearchOpen ? 'active' : ''}`}
                  onClick={() => {
                    setBranchSearchOpen(v => !v);
                    if (branchSearchOpen) setBranchFilter('');
                    else requestAnimationFrame(() => branchFilterRef.current?.focus());
                  }}
                  title={t().gitPanel.branchToolbar.search}
                >
                  <IconSearch size={13} />
                </button>
                <button
                  className="branch-tool-btn"
                  onClick={() => onFetch?.()}
                  disabled={!onFetch}
                  title={t().gitPanel.branchToolbar.fetch}
                >
                  <IconFetch size={14} />
                </button>
                <button
                  className="branch-tool-btn"
                  onClick={toggleFavoriteSelected}
                  disabled={!singleSelected || !!singleSelected.isTag}
                  title={t().gitPanel.branchToolbar.favorite}
                >
                  <IconStar size={13} />
                </button>
                <button
                  className="branch-tool-btn"
                  onClick={compareSelected}
                  disabled={selectedBranches.length !== 2}
                  title={t().gitPanel.branchToolbar.compare}
                >
                  <IconCompareVersions size={14} />
                </button>
                <div className="branch-tool-sep" />
                <button
                  className={`branch-tool-btn ${branchViewMenu ? 'active' : ''}`}
                  onClick={openBranchViewMenu}
                  title={t().gitPanel.branchToolbar.viewOptions}
                >
                  <IconGear size={13} />
                </button>
              </div>

              {!branchListHidden && <div className="branch-pane">
                {branchSearchOpen && (
                  <div className="branch-search-wrap">
                    <IconSearch size={11} color="var(--text-muted)" />
                    <input
                      ref={branchFilterRef}
                      className="branch-search-input"
                      value={branchFilter}
                      onChange={e => setBranchFilter(e.target.value)}
                      placeholder={t().gitPanel.branchSearchPlaceholder}
                      autoFocus
                    />
                  </div>
                )}
                <div className="branch-tree-scroll">
                  <div className="b-row b-head-group b-head-label">
                    <span className="b-title">{t().gitPanel.headCurrentBranch}</span>
                  </div>
                  {renderSection('local', t().gitPanel.local, localRows)}
                  {renderSection('remote', t().gitPanel.remote, remoteRows)}
                  {tagBranches.length > 0 && renderSection('tags', t().gitPanel.tags, tagRows)}
                  {branchFilter && visibleRefOrder.length === 0 && (
                    <div className="branch-empty">{t().gitPanel.noMatchingBranches}</div>
                  )}
                </div>
              </div>}
            </div>

            {/* 左边缘：拖动调整分支树宽度 */}
            {!branchListHidden && (
              <div
                className="git-col-resizer"
                onPointerDown={e => startColumnResize('branches', e)}
                title={t().gitPanel.resizeBranches}
              />
            )}

            {/* Col 3: Commits Table with Branch DAG */}
            <div className="git-col-commits">
              {/* Filter Bar */}
              <div className="git-filter-bar">
                <div className="git-log-search-box">
                  <span className="git-log-search-icon" title={t().gitPanel.searchPlaceholder}>
                    <IconSearchDropdown size={12} color="var(--text-muted)" />
                  </span>
                  <input
                    ref={filterInputRef}
                    type="text"
                    value={searchFilter}
                    onChange={e => setSearchFilter(e.target.value)}
                    placeholder={t().gitPanel.searchPlaceholder}
                    className="git-log-search-input"
                  />
                  <button
                    type="button"
                    className={`git-log-search-toggle ${useRegex ? 'active' : ''}`}
                    onClick={() => setUseRegex(v => !v)}
                    title={t().gitPanel.regexTooltip}
                  >
                    .*
                  </button>
                  <button
                    type="button"
                    className={`git-log-search-toggle ${caseSensitive ? 'active' : ''}`}
                    onClick={() => setCaseSensitive(v => !v)}
                    title={t().gitPanel.matchCaseTooltip}
                  >
                    Cc
                  </button>
                </div>

                {/* Branch：服务端过滤（切分支/全部分支） */}
                <LogFilterMenu label={t().gitPanel.filterBranch} active={!!logBranch} width={260}>
                  {close => (
                    <>
                      <div
                        className={`log-filter-item ${!logBranch ? 'selected' : ''}`}
                        onClick={() => {
                          onLogBranchChange?.('');
                          close();
                        }}
                      >
                        {t().gitPanel.allBranches}
                      </div>
                      {branches.map(b => (
                        <div
                          key={b.name}
                          className={`log-filter-item ${logBranch === b.name ? 'selected' : ''}`}
                          onClick={() => {
                            onLogBranchChange?.(b.name);
                            close();
                          }}
                          title={b.isRemote ? t().gitPanel.branchKindRemote : t().gitPanel.branchKindLocal}
                        >
                          {b.isRemote ? <IconCloud size={11} /> : <GitBranchIcon size={11} />}
                          <span className="log-filter-label">{b.name}</span>
                          {b.isCurrent && <span className="log-filter-mark">{t().gitPanel.refHead}</span>}
                        </div>
                      ))}
                    </>
                  )}
                </LogFilterMenu>

                {/* User：服务端 git log --author 过滤 */}
                <LogFilterMenu label={t().gitPanel.filterUser} active={!!userFilter} width={220}>
                  {close => (
                    <>
                      <div
                        className={`log-filter-item ${!userFilter ? 'selected' : ''}`}
                        onClick={() => {
                          onUserFilterChange?.('');
                          close();
                        }}
                      >
                        {t().gitPanel.allAuthors}
                      </div>
                      {logAuthors.map(a => (
                        <div
                          key={a}
                          className={`log-filter-item ${userFilter === a ? 'selected' : ''}`}
                          onClick={() => {
                            onUserFilterChange?.(a);
                            close();
                          }}
                        >
                          <span className="log-filter-label">{a}</span>
                        </div>
                      ))}
                    </>
                  )}
                </LogFilterMenu>

                {/* Date：服务端 git log --since 过滤 */}
                <LogFilterMenu label={t().gitPanel.filterDate} active={dateFilter !== 'all'} width={180}>
                  {close => (
                    <>
                      {([
                        ['all', t().gitPanel.dateAll],
                        ['today', t().gitPanel.dateToday],
                        ['week', t().gitPanel.dateLast7Days],
                        ['month', t().gitPanel.dateLast30Days]
                      ] as const).map(([id, text]) => (
                        <div
                          key={id}
                          className={`log-filter-item ${dateFilter === id ? 'selected' : ''}`}
                          onClick={() => {
                            onDateFilterChange?.(id);
                            close();
                          }}
                        >
                          {text}
                        </div>
                      ))}
                    </>
                  )}
                </LogFilterMenu>

                {/* Paths：服务端过滤（只看改动过该路径的提交） */}
                <LogFilterMenu label={t().gitPanel.filterPaths} active={!!logPath} width={280}>
                  {close => (
                    <div className="log-path-form">
                      <input
                        className="log-path-input"
                        placeholder={t().gitPanel.pathPlaceholder}
                        value={pathDraft}
                        autoFocus
                        onChange={e => setPathDraft(e.target.value)}
                        onKeyDown={e => {
                          if (e.key === 'Enter') {
                            onLogPathChange?.(pathDraft.trim());
                            close();
                          }
                        }}
                      />
                      <div className="log-path-actions">
                        <button
                          className="log-path-btn primary"
                          onClick={() => {
                            onLogPathChange?.(pathDraft.trim());
                            close();
                          }}
                        >
                          {t().gitPanel.apply}
                        </button>
                        <button
                          className="log-path-btn"
                          onClick={() => {
                            setPathDraft('');
                            onLogPathChange?.('');
                            close();
                          }}
                        >
                          {t().gitPanel.clear}
                        </button>
                      </div>
                    </div>
                  )}
                </LogFilterMenu>

                <div className="filter-actions-right">
                  <button
                    ref={sortBtnRef}
                    className={`f-act-btn ${sortMenuOpen ? 'active' : ''}`}
                    title={t().gitPanel.sortTooltip}
                    onClick={e => {
                      const r = e.currentTarget.getBoundingClientRect();
                      setSortMenuPos({ x: r.left, y: r.bottom + 3 });
                      setSortMenuOpen(v => !v);
                    }}
                  >
                    <IconSortCommits size={14} />
                  </button>
                  <button
                    className={`f-act-btn ${showGraph ? 'active' : ''}`}
                    title={t().gitPanel.showGraph}
                    onClick={() => setShowGraph(v => !v)}
                  >
                    <IconCommitGraph size={14} />
                  </button>
                  <button
                    className={`f-act-btn${refreshing ? ' is-refreshing' : ''}`}
                    title={refreshing ? t().gitPanel.refreshingTooltip : t().gitPanel.refreshTooltip}
                    onClick={onRefresh}
                    disabled={refreshing}
                  >
                    <IconRefresh size={14} />
                  </button>
                </div>
              </div>

              {/* 提交列表：PyCharm 单行列表（无表头）——轨道 | 主题 | 引用 | 作者 | 时间 */}
              <div className="git-log-scroll" ref={logScrollRef} onScroll={handleLogScroll}>
                {logWindow.start > 0 && (
                  <div aria-hidden="true" style={{ height: logWindow.start * logRowH }} />
                )}
                {visibleCommits.map(c => {
                  const isSelected = selectedCommits.some(item => item.hash === c.hash);
                  return (
                    <div
                      key={c.hash}
                      data-commit-row={c.hash}
                      onClick={e => handleRowClick(c, e)}
                      onContextMenu={e => {
                        e.preventDefault();
                        if (!selectedCommits.some(item => item.hash === c.hash)) {
                          setSelectedCommits([c]);
                          onSelectCommit(c);
                          onCommitContext?.(c, e.clientX, e.clientY, [c]);
                        } else {
                          onCommitContext?.(c, e.clientX, e.clientY, selectedCommits);
                        }
                      }}
                      className={`log-row ${isSelected ? 'active' : ''} ${c.inHead === false ? 'outside-head' : ''} ${highlightHash === c.hash ? 'log-jump-highlight' : ''}`}
                      title={t().gitPanel.rowHint}
                    >
                      {showGraph && (
                        <span className="log-graph">
                          <GraphCell
                            commit={c}
                            layout={graphLayoutMap.get(c.hash)}
                            brk={graphBreaks.get(c.hash)}
                          />
                        </span>
                      )}
                      <span className="log-subject">
                        {c.isMerge && <IconMerge size={12} color="var(--text-secondary)" />}
                        <span className="log-subject-text">{c.message}</span>
                      </span>
                      {c.branchTags && c.branchTags.length > 0 && (
                        <span className="log-refs">
                          {c.branchTags.map(tag => (
                            <span
                              key={tag.name}
                              className={`log-ref ${tag.type}`}
                              title={
                                tag.type === 'tag'
                                  ? t().gitPanel.refTag(tag.name)
                                  : tag.type === 'remote'
                                    ? t().gitPanel.refRemoteBranch(tag.name)
                                    : tag.type === 'stash'
                                      ? t().gitPanel.refStash(tag.name)
                                      : t().gitPanel.refBranch(tag.name)
                              }
                            >
                              {tag.type === 'tag' ? (
                                <IconTag size={10} />
                              ) : tag.type === 'remote' ? (
                                <IconCloud size={11} />
                              ) : tag.type === 'stash' ? (
                                <IconShelf size={11} />
                              ) : (
                                <GitBranchIcon size={11} />
                              )}
                              <span className="log-ref-name">{tag.name}</span>
                            </span>
                          ))}
                        </span>
                      )}
                      <span className="log-spacer" />
                      <span className="log-author" title={c.authorEmail}>
                        {c.author}
                      </span>
                      <span className="log-date">{formatCommitDate(c.date)}</span>
                    </div>
                  );
                })}
                {logWindow.end < filteredCommits.length && (
                  <div
                    aria-hidden="true"
                    style={{ height: (filteredCommits.length - logWindow.end) * logRowH }}
                  />
                )}
                {hasMore && (
                  <div className="git-load-more-wrap">
                    <button className="git-load-more" onClick={onLoadMore} disabled={loadingMore}>
                      {loadingMore ? t().common.loading : t().gitPanel.loadMore}
                    </button>
                  </div>
                )}
              </div>
            </div>

            {/* 右边缘：拖动调整提交详情宽度 */}
            <div
              className="git-col-resizer"
              onPointerDown={e => startColumnResize('details', e)}
              title={t().gitPanel.resizeDetails}
            />

            {/* Col 4: 提交详情（真实变更文件清单） */}
            <div
              className="git-col-details"
              id="gitColDetails"
              ref={detailsColRef}
              style={{ width: `${detailsWidth}px` }}
            >
              <div
                className="details-top-files"
                ref={detailsTopRef}
                style={
                  showDetails
                    ? { height: `${detailsTopH}px`, flex: '0 0 auto' }
                    : { flex: '1 1 auto' }
                }
              >
                <div
                  className="details-files-header"
                  style={{ justifyContent: 'space-between', paddingRight: '4px' }}
                >
                  <span style={{ display: 'flex', alignItems: 'center', gap: '5px' }}>
                    <TextFileIcon size={13} color="var(--text-muted)" />
                    <span>
                      {filesLoading ? t().gitPanel.filesLoading : t().gitPanel.filesChanged(commitFiles.length)}
                    </span>
                  </span>
                  <span style={{ display: 'flex', alignItems: 'center', gap: '2px' }}>
                    <button
                      className="f-act-btn"
                      title={t().gitPanel.revertTooltip}
                      disabled={busy || !selectedCommit || isMultiSelection}
                      onClick={() => selectedCommit && onRevertCommit?.(selectedCommit.hash)}
                    >
                      <IconRollback size={14} />
                    </button>
                    <button
                      className="f-act-btn"
                      title={t().gitPanel.showCommitDiffTooltip}
                      onClick={openSelectionDiff}
                    >
                      <IconDiff size={14} />
                    </button>
                    <div className="commit-view-menu-wrap" ref={detailsViewMenuRef}>
                      <button
                        className={`f-act-btn ${detailsViewMenuOpen ? 'active' : ''}`}
                        title={t().commitDrawer.viewOptions}
                        onClick={toggleDetailsViewMenu}
                      >
                        <IconViewOptions size={14} />
                      </button>
                      {detailsViewMenuOpen && createPortal(
                        <div
                          ref={detailsViewMenuPanelRef}
                          className="commit-view-menu details-view-menu"
                          style={{
                            position: 'fixed',
                            top: detailsViewMenuPos.top,
                            left: detailsViewMenuPos.left,
                            minWidth: 200
                          }}
                        >
                          <div className="commit-view-menu-header">{t().commitDrawer.groupBy}</div>
                          <div
                            className="commit-view-menu-item"
                            onClick={() => {
                              setDetailsGroupByDirectory(prev => !prev);
                              setDetailsViewMenuOpen(false);
                            }}
                          >
                            <span className="commit-view-item-label">
                              <span className="commit-view-check">{detailsGroupByDirectory ? '✓' : ''}</span>
                              <span>{t().commitDrawer.groupByDirectory}</span>
                            </span>
                            <span className="commit-view-shortcut">^P</span>
                          </div>
                          <div className="commit-view-menu-sep" />
                          <div className="commit-view-menu-header">{t().commitDrawer.layout}</div>
                          <div
                            className="commit-view-menu-item"
                            onClick={() => {
                              setShowDetails(prev => !prev);
                              setDetailsViewMenuOpen(false);
                            }}
                          >
                            <span className="commit-view-item-label">
                              <span className="commit-view-check">{showDetails ? '✓' : ''}</span>
                              <span>{t().commitDrawer.showDetails}</span>
                            </span>
                          </div>
                        </div>,
                        document.body
                      )}
                    </div>
                    <button
                      className="f-act-btn"
                      title={t().commitDrawer.expandAll}
                      disabled={!detailsGroupByDirectory}
                      onClick={expandAllDetailsDirs}
                    >
                      <IconExpandAll size={14} />
                    </button>
                    <button
                      className="f-act-btn"
                      title={t().commitDrawer.collapseAll}
                      disabled={!detailsGroupByDirectory}
                      onClick={collapseAllDetailsDirs}
                    >
                      <IconCollapseAll size={14} />
                    </button>
                  </span>
                </div>

                {!filesLoading && commitFiles.length === 0 && (
                  <div style={{ padding: '6px 10px', fontSize: '10px', color: 'var(--text-muted)' }}>
                    {selectedCommit ? t().gitPanel.noFileChanges : t().gitPanel.noCommitSelected}
                  </div>
                )}

                {commitFiles.length > 0 && detailsGroupByDirectory ? (
                  <>
                    <div
                      className="details-tree-root"
                      title={repoPath || detailsRootName}
                      onClick={() => setDetailsRootCollapsed(prev => !prev)}
                    >
                      <span className="details-chevron">
                        {detailsRootCollapsed ? (
                          <IconChevronRight size={10} color="var(--text-muted)" />
                        ) : (
                          <IconChevronDown size={10} color="var(--text-muted)" />
                        )}
                      </span>
                      <FolderIcon size={14} isOpen={!detailsRootCollapsed} />
                      <span className="details-root-name">{detailsRootName}</span>
                      <span className="details-root-count">
                        {t().compareBranches.fileCount(commitFiles.length)}
                      </span>
                    </div>
                    {!detailsRootCollapsed && (
                      <div className="details-tree-children">
                        {renderDetailsDirs(detailsTree, 0)}
                      </div>
                    )}
                  </>
                ) : (
                  commitFiles.map(f => renderDetailsFile(f, 0))
                )}
              </div>

              {/* 上下分割条：拖动调整文件清单 / 提交信息两块高度 */}
              {showDetails && (
                <>
                  <div
                    className="details-split-resizer"
                    onPointerDown={startDetailsSplitResize}
                    title={t().gitPanel.resizeDetailsSplit}
                  />

                  <div className="details-bottom-meta">
                    <div className="meta-commit-title">{selectedCommit?.message}</div>
                    <div className="meta-commit-author">
                      <span className="hash-text">{selectedCommit?.shortHash}</span>{' '}
                      <span className="author-link">
                        {selectedCommit?.author} &lt;{selectedCommit?.authorEmail}&gt;
                      </span>{' '}
                      <span className="meta-date">{formatCommitDate(selectedCommit?.date)}</span>
                    </div>
                  </div>
                </>
              )}
            </div>
          </div>
          )
        ) : (
          /* Console Subpane */
          <div className="git-console-subpane" id="gitConsoleSubpane" ref={consoleRef}>
            {logEntries.length === 0 ? (
              <div className="console-line console-empty">{t().gitPanel.consoleEmpty}</div>
            ) : (
              logEntries.map(entry => (
                <div
                  key={entry.id}
                  className={`console-line ${entry.level === 'error' ? 'console-error' : ''}`}
                >
                  {entry.time}: [lazecode] {entry.text}
                </div>
              ))
            )}
          </div>
        )}
      {/* 日志排序 / 选项菜单（对应截图中的 Sort / Options / Branch Actions） */}
      {sortMenuOpen && (
        <CtxMenuShell
          style={{
            top: Math.min(sortMenuPos.y, window.innerHeight - 330),
            left: Math.max(4, Math.min(sortMenuPos.x, window.innerWidth - 270))
          }}
          onClose={() => setSortMenuOpen(false)}
        >
          <div className="ctx-menu-header">{t().gitPanel.sortBy}</div>
          <button
            className={`ctx-item ctx-item-check ${sortMode === 'date' ? 'selected' : ''}`}
            onClick={() => { onSortModeChange?.('date'); setSortMenuOpen(false); }}
          >
            <span className="ctx-check">{sortMode === 'date' ? '✓' : ''}</span>
            {t().gitPanel.byCommitDate}
          </button>
          <button
            className={`ctx-item ctx-item-check ${sortMode === 'topo' ? 'selected' : ''}`}
            onClick={() => { onSortModeChange?.('topo'); setSortMenuOpen(false); }}
          >
            <span className="ctx-check">{sortMode === 'topo' ? '✓' : ''}</span>
            {t().gitPanel.topologically}
          </button>
          <div className="ctx-separator" />
          <div className="ctx-menu-header">{t().gitPanel.options}</div>
          <button
            className={`ctx-item ctx-item-check ${firstParent ? 'selected' : ''}`}
            onClick={() => { onFirstParentChange?.(!firstParent); setSortMenuOpen(false); }}
          >
            <span className="ctx-check">{firstParent ? '✓' : ''}</span>
            {t().gitPanel.firstParent}
          </button>
          <button
            className={`ctx-item ctx-item-check ${noMerges ? 'selected' : ''}`}
            onClick={() => { onNoMergesChange?.(!noMerges); setSortMenuOpen(false); }}
          >
            <span className="ctx-check">{noMerges ? '✓' : ''}</span>
            {t().gitPanel.noMerges}
          </button>
          <div className="ctx-separator" />
          <div className="ctx-menu-header">{t().gitPanel.branchActions}</div>
          <button className="ctx-item" disabled>{t().gitPanel.collapseLinear}</button>
          <button className="ctx-item" disabled>{t().gitPanel.expandLinear}</button>
        </CtxMenuShell>
      )}


      {/* 分支树视图选项（纵向工具栏齿轮） */}
      {branchViewMenu && (
        <CtxMenuShell
          style={{
            top: Math.min(branchViewMenu.y, window.innerHeight - 150),
            left: Math.min(branchViewMenu.x, window.innerWidth - 240)
          }}
          onClose={() => setBranchViewMenu(null)}
        >
          <button
            className="ctx-item"
            onClick={() => {
              setGroupByDirectory(v => !v);
              setBranchViewMenu(null);
            }}
          >
            <span className="ctx-check">{groupByDirectory ? '✓' : ''}</span>
            {t().gitPanel.branchToolbar.groupByDirectory}
          </button>
          <div className="ctx-separator" />
          <button className="ctx-item" onClick={() => { expandAllRefs(); setBranchViewMenu(null); }}>
            {t().gitPanel.branchToolbar.expandAll}
          </button>
          <button className="ctx-item" onClick={() => { collapseAllRefs(); setBranchViewMenu(null); }}>
            {t().gitPanel.branchToolbar.collapseAll}
          </button>
        </CtxMenuShell>
      )}

      {/* 提交详情文件右键菜单。portal 到 body：底部面板在毛玻璃主题下有
          backdrop-filter，会成为 fixed 菜单/遮罩的 containing block，导致菜单
          偏离鼠标、遮罩盖不到编辑器点击、菜单被 overflow:hidden 裁剪。 */}
      {fileMenu && selectedCommit && (
        <CtxMenuShell
          style={{ top: Math.min(fileMenu.y, window.innerHeight - 220), left: Math.min(fileMenu.x, window.innerWidth - 280) }}
          onClose={() => setFileMenu(null)}
        >
          <button className="ctx-item" onClick={() => { const path = fileMenu.path; setFileMenu(null); openSelectionFileDiff(path); }}>
            {t().gitPanel.ctxShowDiff}
          </button>
          {!isMultiSelection && (
            <>
              <button className="ctx-item" onClick={() => { const c = selectedCommit; setFileMenu(null); onCompareWithLocal?.(c.hash, fileMenu.path); }}>
                {t().gitPanel.ctxCompareWithLocal}
              </button>
              <button className="ctx-item" onClick={() => { const c = selectedCommit; setFileMenu(null); onGetFromRevision?.(c.hash, fileMenu.path); }}>
                {t().gitPanel.ctxGetFromRevision}
              </button>
              <div className="ctx-separator" />
              <button className="ctx-item" onClick={() => { const c = selectedCommit; setFileMenu(null); onCopyCommitPatch?.(c.hash); }}>
                {t().gitPanel.ctxCopyPatch}
              </button>
              <button className="ctx-item" onClick={() => { const c = selectedCommit; setFileMenu(null); onCreateCommitPatch?.(c.hash); }}>
                {t().gitPanel.ctxCreatePatch}
              </button>
            </>
          )}
        </CtxMenuShell>
      )}
    </div>
  );
});
