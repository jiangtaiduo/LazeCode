import React, { useState, useRef, useEffect, useMemo } from 'react';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { isTauriEnvironment, invokeCommand } from '../services/tauriBridge';
import {
  GitBranchIcon,
  ProjectStripeIcon,
  CommitStripeIcon,
  IconNewWindow,
  IconWindowClose,
  IconWindowMinimize,
  IconWindowMaximize,
  IconPlus,
  IconSearch,
  IconChevronDown,
  IconChevronRight,
  IconTrash,
  IconPull,
  IconPush,
  IconGear,
  IconStar,
  IconTag,
  IconOpContinue,
  IconOpAbort,
  IconArrowDown
} from './FileIcons';
import { startPanelDrag } from '../services/panelDrag';
import { sortLocalBranches } from '../services/branchSort';
import { formatTildePath, getBadgeInitials, getProjectBadgeColor } from '../services/pathUtils';
import type { GitBranch, GitRepoInfo, GitTag } from '../types';
import { t } from '../services/i18n';
import { useLanguageRefresh } from '../hooks/useLanguageRefresh';

interface TitleBarProps {
  projectName: string;
  workspacePath: string;
  recentWorkspaces: string[];
  onOpenProject: () => void;
  onSwitchWorkspace: (path: string) => void;
  /** 本窗口已打开（可切换）的项目：切走的项目保留全部会话状态 */
  sessionProjects?: { path: string; name: string }[];
  /** 其它窗口打开的项目：点击聚焦对应窗口 */
  otherWindows?: { label: string; title: string; path?: string }[];
  onFocusWindow?: (label: string) => void;
  /** 项目菜单展开时刷新其它窗口列表 */
  onProjectMenuOpen?: () => void;
  /** 关闭当前项目（回到欢迎页）；未打开项目时不传 */
  onCloseProject?: () => void;
  currentBranch: string;
  branches: GitBranch[];
  tags?: GitTag[];
  onSelectBranch: (branch: string) => void;
  onCheckoutTag?: (name: string) => void;
  onDeleteTag?: (tag: GitTag) => void;
  onCheckoutRef?: () => void;
  onCreateBranch?: (branchName: string) => void;
  favorites?: string[];
  /**
   * 分支操作子菜单（点击下拉行滑出）：
   * x/y 为视口坐标，side 决定子菜单从锚点哪一侧滑入（默认右侧）。
   */
  onBranchContext?: (branch: GitBranch, x: number, y: number, side?: 'right' | 'left') => void;
  /** 关闭由下拉行滑出的分支子菜单（下拉内部交互使其失效时调用） */
  onCloseBranchContext?: () => void;
  /** 从「最近打开的项目」中移除记录（不删除目录，可再从打开目录找回） */
  onRemoveRecent?: (path: string, e: React.MouseEvent) => void;
  platform: 'mac' | 'win';
  onSearchClick: () => void;
  onNewWindow?: () => void;
  /** 进行中的合并/拣选（顶栏 ⚠ Cherry-picking in <branch> … [»][✕]，对齐 PyCharm） */
  sequencer?: { merging: boolean; cherryPicking: boolean; rebasing: boolean } | null;
  /** 冲突文件数：>0 绿钮=解决冲突；=0 且操作在途=继续当前操作 */
  conflictCount?: number;
  onResolveConflicts?: () => void;
  onContinueCherryPick?: () => void;
  onContinueMerge?: () => void;
  onContinueRebase?: () => void;
  onSkipRebase?: () => void;
  onAbortMerge?: () => void;
  onAbortCherryPick?: () => void;
  onAbortRebase?: () => void;
  onUpdateProject?: () => void;
  onOpenCommit?: () => void;
  onPush?: () => void;
  /**
   * 后台检查发现的新版本（不含前导 v）。
   * 显示在搜索按钮右侧；点击进入现有更新对话框。
   */
  availableUpdateVersion?: string;
  onAvailableUpdateClick?: () => void;
  /** 当前项目/文件是否受 Git 管理：不受 Git 管理时不显示分支名 */
  isRepo?: boolean;
}

/**
 * memo：标题栏含分支过滤 + 排序，App 每次击键/终端输出都会重算；
 * 隔离掉与标题栏无关的重渲染。生效前提：App 侧回调与 branches 数组保持稳定引用。
 */
export const TitleBar = React.memo(function TitleBar({
  projectName,
  workspacePath,
  recentWorkspaces,
  onOpenProject,
  onSwitchWorkspace,
  sessionProjects = [],
  otherWindows = [],
  onFocusWindow,
  onProjectMenuOpen,
  onCloseProject,
  currentBranch,
  branches,
  tags = [],
  onSelectBranch,
  onCheckoutTag,
  onDeleteTag,
  onCheckoutRef,
  onCreateBranch,
  favorites = [],
  onBranchContext,
  onCloseBranchContext,
  onRemoveRecent,
  platform,
  onSearchClick,
  onNewWindow,
  sequencer = null,
  conflictCount = 0,
  onResolveConflicts,
  onContinueCherryPick,
  onContinueMerge,
  onContinueRebase,
  onSkipRebase,
  onAbortMerge,
  onAbortCherryPick,
  onAbortRebase,
  onUpdateProject,
  onOpenCommit,
  onPush,
  availableUpdateVersion,
  onAvailableUpdateClick,
  isRepo = false
}: TitleBarProps) {
  useLanguageRefresh();
  const [isProjectMenuOpen, setIsProjectMenuOpen] = useState(false);
  const [isBranchMenuOpen, setIsBranchMenuOpen] = useState(false);
  const [projectFilter, setProjectFilter] = useState('');
  const [branchFilter, setBranchFilter] = useState('');
  const [isCreatingBranch, setIsCreatingBranch] = useState(false);
  const [newBranchName, setNewBranchName] = useState('');
  /** 分支下拉用户拖出的高度（null = 未拖过，走 CSS 默认 max-height） */
  const [branchMenuHeight, setBranchMenuHeight] = useState<number | null>(null);
  const [sectionsOpen, setSectionsOpen] = useState({
    recent: true,
    local: false,
    remote: false,
    tags: false
  });

  const projectMenuRef = useRef<HTMLDivElement>(null);
  const branchMenuRef = useRef<HTMLDivElement>(null);
  const branchPopupRef = useRef<HTMLDivElement>(null);

  /**
   * 分支下拉展开时给 body 打标记，让分支操作子菜单的遮罩降到标题栏之下。
   *
   * 分支子菜单经 CtxMenuShell portal 到 body，遮罩 z2200 是全屏 fixed；而
   * 标题栏自身（毛玻璃 → 独立层叠上下文）z1800。于是遮罩整块盖住标题栏：
   * 点下拉里的分支行时 pointerdown 命中遮罩而非该行，TitleBar 的
   * outside-click 判定「点在 wrapper 外」→ 关掉整个分支下拉，行的 click
   * 再也不会到达，用户看到的就是「子菜单闪一下就没了」。降到 1700 后
   * 点的就是行本身，行的 onClick 能在同一手势里关闭旧子菜单、展开新子菜单。
   */
  useEffect(() => {
    document.body.classList.toggle('branch-dropdown-open', isBranchMenuOpen);
    return () => document.body.classList.remove('branch-dropdown-open');
  }, [isBranchMenuOpen]);

  // 点击外部自动关闭下拉菜单
  useEffect(() => {
    const handleOutsideClick = (e: PointerEvent) => {
      const target = e.target as Element | null;
      if (projectMenuRef.current && !projectMenuRef.current.contains(target as Node)) {
        setIsProjectMenuOpen(false);
      }
      if (branchMenuRef.current && !branchMenuRef.current.contains(target as Node)) {
        setIsBranchMenuOpen(false);
        setBranchFilter('');
        setIsCreatingBranch(false);
        // 子菜单项上的 pointerdown 先于其 click 到达：此刻关掉子菜单会让随后
        // 的 click 落空（目标已卸载）。点在子菜单内的留给它自己的 onClick 收尾。
        if (!target?.closest?.('.branch-ctx-menu')) onCloseBranchContext?.();
      }
    };
    // capture 阶段拦截：Monaco 等组件会在自身节点 stopPropagation，
    // 冒泡到 window 的 mousedown 永远收不到「点击编辑区」事件
    window.addEventListener('pointerdown', handleOutsideClick, true);
    return () => window.removeEventListener('pointerdown', handleOutsideClick, true);
  }, []);

  const handleWinMinimize = () => {
    if (isTauriEnvironment()) {
      void getCurrentWindow().minimize();
    }
  };

  const handleWinMaximize = () => {
    if (isTauriEnvironment()) {
      void getCurrentWindow().toggleMaximize();
    }
  };

  const handleWinClose = () => {
    if (isTauriEnvironment()) {
      void getCurrentWindow().close();
    }
  };

  const projectInitial = getBadgeInitials(projectName);

  // 打开中项目的 git 分支缓存（path -> branch，对齐 Image #2）
  const [projectBranches, setProjectBranches] = useState<Record<string, string>>({});

  useEffect(() => {
    if (!isProjectMenuOpen || !isTauriEnvironment()) return;
    const pathsToQuery = new Set<string>();
    sessionProjects.forEach(s => {
      if (s.path && s.path !== workspacePath && projectBranches[s.path] === undefined) {
        pathsToQuery.add(s.path);
      }
    });
    otherWindows.forEach(w => {
      if (w.path && w.path !== workspacePath && projectBranches[w.path] === undefined) {
        pathsToQuery.add(w.path);
      }
    });

    if (pathsToQuery.size === 0) return;

    let cancelled = false;
    Promise.all(
      Array.from(pathsToQuery).map(async p => {
        try {
          const info = await invokeCommand<GitRepoInfo>('git_repo_info', { repoPath: p });
          return [p, info?.isRepo && info?.branch ? info.branch : ''] as const;
        } catch {
          return [p, ''] as const;
        }
      })
    ).then(results => {
      if (!cancelled && results.length > 0) {
        setProjectBranches(prev => ({
          ...prev,
          ...Object.fromEntries(results)
        }));
      }
    });

    return () => {
      cancelled = true;
    };
  }, [isProjectMenuOpen, sessionProjects, otherWindows, workspacePath, projectBranches]);

  const getProjectBranch = (p: string) => (p === workspacePath ? (currentBranch || '') : (projectBranches[p] || ''));

  // 项目搜索过滤：名称或路径
  const projectFilterHit = (p: string) => {
    if (!projectFilter) return true;
    const name = p.split(/[\\/]/).filter(Boolean).pop() ?? p;
    const q = projectFilter.toLowerCase();
    return name.toLowerCase().includes(q) || p.toLowerCase().includes(q);
  };

  const openSessionList = sessionProjects.filter(s => projectFilterHit(s.path));
  // 其它窗口的项目：本窗口已打开同一路径时不再单列（同一项目只出现一次）
  const otherWindowList = otherWindows.filter(
    w =>
      (!projectFilter || w.title.toLowerCase().includes(projectFilter.toLowerCase())) &&
      !sessionProjects.some(s => s.path === w.path)
  );

  // 过滤最近项目
  const filteredWorkspaces = recentWorkspaces.filter(p => {
    if (!projectFilter) return true;
    const name = p.split(/[\\/]/).filter(Boolean).pop() ?? p;
    return name.toLowerCase().includes(projectFilter.toLowerCase()) || p.toLowerCase().includes(projectFilter.toLowerCase());
  });

  // 最近项目里剔除已在「打开的项目」中的（本窗口会话 + 其它窗口），避免重复
  const recentList = filteredWorkspaces.filter(
    p => !sessionProjects.some(s => s.path === p) && !otherWindows.some(w => w.path === p)
  );

  // 过滤分支：按 isRemote 分组（远程分支由后端 DWIM 检出，不再手工剥前缀）
  const localBranches = branches.filter(b => !b.isRemote);
  const remoteBranches = branches.filter(b => b.isRemote);

  const favoriteSet = useMemo(() => new Set(favorites), [favorites]);
  const filteredLocalBranches = useMemo(
    () => localBranches.filter(b => !branchFilter || b.name.toLowerCase().includes(branchFilter.toLowerCase())),
    [localBranches, branchFilter]
  );
  const filteredRemoteBranches = useMemo(
    () => remoteBranches.filter(b => !branchFilter || b.name.toLowerCase().includes(branchFilter.toLowerCase())),
    [remoteBranches, branchFilter]
  );
  const filteredTags = useMemo(
    () => tags.filter(tag => !branchFilter || tag.name.toLowerCase().includes(branchFilter.toLowerCase())),
    [tags, branchFilter]
  );

  // 当前分支置顶 → 收藏置顶 → 字典序（与 Git Log 的 Local 分组共用同一排序）
  const sortedLocalBranches = useMemo(
    () => sortLocalBranches(filteredLocalBranches, currentBranch, n => favoriteSet.has(n)),
    [filteredLocalBranches, currentBranch, favoriteSet]
  );

  // 当前分支与上游的领先/落后（真实来自 git for-each-ref）
  const currentBranchInfo = branches.find(b => b.isCurrent || b.name === currentBranch);
  const syncBadge =
    currentBranchInfo && (currentBranchInfo.ahead || currentBranchInfo.behind)
      ? `${currentBranchInfo.ahead ? `↑${currentBranchInfo.ahead}` : ''}${currentBranchInfo.behind ? ` ↓${currentBranchInfo.behind}` : ''}`
      : null;

  const handleCreateBranchSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    if (newBranchName.trim()) {
      if (onCreateBranch) {
        onCreateBranch(newBranchName.trim());
      } else {
        onSelectBranch(newBranchName.trim());
      }
      setNewBranchName('');
      setIsCreatingBranch(false);
      setIsBranchMenuOpen(false);
      onCloseBranchContext?.();
    }
  };

  /**
   * 点击分支行 → 在下拉旁滑出 PyCharm 式操作子菜单（不再直接 checkout，
   * Checkout 是子菜单首项）。子菜单定宽 260（见 .branch-ctx-menu）：
   * 右侧容得下就贴下拉右缘 +4px 滑出，否则贴左缘向左滑出。
   */
  const openBranchSubmenu = (b: GitBranch, e: React.MouseEvent<HTMLDivElement>) => {
    const popupRect = branchPopupRef.current?.getBoundingClientRect();
    const rowRect = e.currentTarget.getBoundingClientRect();
    const spaceRight = popupRect ? window.innerWidth - popupRect.right : Number.POSITIVE_INFINITY;
    const side: 'right' | 'left' = spaceRight >= 260 + 16 ? 'right' : 'left';
    const x = popupRect
      ? side === 'right'
        ? popupRect.right + 4
        : popupRect.left - 260 - 4
      : rowRect.right + 4;
    onBranchContext?.(b, x, rowRect.top, side);
  };
  const recentBranches = useMemo(
    () => sortedLocalBranches.slice(0, 5),
    [sortedLocalBranches]
  );

  const renderBranchRowItem = (b: GitBranch) => {
    const isCurrent = b.isCurrent || b.name === currentBranch;
    const isFavorite = favorites.includes(b.name);
    return (
      <div
        key={b.name}
        className={`pycharm-menu-item branch-menu-item-row ${isCurrent ? 'selected' : ''}`}
        onClick={e => openBranchSubmenu(b, e)}
        onContextMenu={e => e.preventDefault()}
      >
        <span className="branch-row-icon">
          {isFavorite ? (
            <IconStar size={12} color="#e5c07b" />
          ) : isCurrent ? (
            <IconTag size={12} color="var(--text-secondary)" />
          ) : b.isRemote ? (
            <GitBranchIcon size={13} color="var(--text-muted)" />
          ) : (
            <GitBranchIcon size={13} color="currentColor" />
          )}
        </span>
        <span className="menu-item-name">{b.name}</span>
        {b.ahead && b.ahead > 0 ? (
          <span className="branch-row-badge-ahead">↗{b.ahead}</span>
        ) : null}
        {b.tracking && (
          <span className="branch-row-tracking">{b.tracking}</span>
        )}
        <span className="menu-item-sub-chevron">
          <IconChevronRight size={11} color="var(--text-muted)" />
        </span>
      </div>
    );
  };

  /** 分支下拉底部把柄拖高：拖拽中走 panelDrag 直写 DOM，松手一次性提交 state */
  const startBranchMenuResize = (e: React.PointerEvent<HTMLDivElement>) => {
    const popup = branchPopupRef.current;
    if (!popup) return;
    onCloseBranchContext?.();
    const rect = popup.getBoundingClientRect();
    const startY = e.clientY;
    const startH = rect.height;
    // 解除 .pycharm-menu-popup 默认 max-height:480（提交后由 inline style 接管）
    popup.style.maxHeight = 'none';
    startPanelDrag(
      e,
      { element: popup, axis: 'y' },
      'ns-resize',
      (_x, y) => Math.min(Math.max(startH + (y - startY), 200), window.innerHeight - rect.top - 12),
      h => setBranchMenuHeight(Math.round(h))
    );
  };

  // PyCharm 顶栏操作条（🚧 实测 screenshot-20261015-155800.png）：⚠ #f2c55c +
  // 操作文本 #abb2bf 常驻在项目/分支选择器之后；最右绿钮（解决冲突/继续拣选）
  // 与红钮（终止合并/拣选）。
  const opActive = !!sequencer && (sequencer.merging || sequencer.cherryPicking || sequencer.rebasing);
  const opConflicts = (conflictCount ?? 0) > 0;
  const opLabel = !opActive
    ? ''
    : sequencer?.merging
      ? currentBranch ? t().gitPanel.opMerging(currentBranch) : t().gitPanel.opMergingBare
      : sequencer?.rebasing
        ? currentBranch ? t().gitPanel.opRebasing(currentBranch) : t().gitPanel.opRebasingBare
        : currentBranch
          ? t().gitPanel.opCherryPicking(currentBranch)
          : t().gitPanel.opCherryPickingBare;
  // 冲突时绿钮统一进冲突解决；无冲突时继续合并/变基/拣选。
  const opGreenEnabled = !!sequencer && (opConflicts || sequencer.merging || sequencer.cherryPicking || sequencer.rebasing);
  const opGreenTitle = opConflicts ? t().gitPanel.resolveConflicts : t().gitPanel.opContinueTitle;
  const opRedTitle = sequencer?.merging
    ? t().gitPanel.abortMergeTitle
    : sequencer?.rebasing
      ? t().gitPanel.abortRebaseTitle
      : t().gitPanel.abortCherryPickTitle;
  const onOpGreen = () => {
    if (opConflicts) onResolveConflicts?.();
    else if (sequencer?.merging) onContinueMerge?.();
    else if (sequencer?.rebasing) onContinueRebase?.();
    else if (sequencer?.cherryPicking) onContinueCherryPick?.();
  };
  const onOpRed = () => {
    if (sequencer?.merging) onAbortMerge?.();
    else if (sequencer?.rebasing) onAbortRebase?.();
    else onAbortCherryPick?.();
  };

  return (
    <header className="ide-titlebar" data-tauri-drag-region="deep">
      {/* Left: Project Selector + Git Branch Selector (PyCharm New UI Style) */}
      <div className="titlebar-left-cluster" data-tauri-drag-region="deep">
        {/* 1. Project Dropdown Trigger */}
        <div className="pycharm-dropdown-wrapper" ref={projectMenuRef} data-tauri-drag-region="false">
          <button
            className={`pycharm-project-pill ${isProjectMenuOpen ? 'active' : ''}`}
            onClick={() => {
              setIsProjectMenuOpen(prev => {
                if (!prev) onProjectMenuOpen?.();
                return !prev;
              });
              setIsBranchMenuOpen(false);
              setProjectFilter('');
            }}
            title={t().titleBar.projectPillTooltip}
          >
            <span
              className="pycharm-project-badge"
              style={{ background: getProjectBadgeColor(projectName || 'Welcome') }}
            >
              {projectInitial}
            </span>
            <span className="pycharm-project-name">{projectName || 'Welcome'}</span>
            <span className="pycharm-dropdown-arrow"><IconChevronDown size={10} /></span>
          </button>

          {/* Project Dropdown Menu */}
          {isProjectMenuOpen && (
            <div className="pycharm-menu-popup project-menu-popup">
              <div className="pycharm-menu-header">
                <input
                  type="text"
                  className="pycharm-menu-search"
                  placeholder={t().titleBar.searchProjectsPlaceholder}
                  value={projectFilter}
                  onChange={e => setProjectFilter(e.target.value)}
                  autoFocus
                />
              </div>

              <div className="pycharm-menu-actions">
                <button
                  className="pycharm-menu-action-btn"
                  onClick={() => {
                    setIsProjectMenuOpen(false);
                    onOpenProject();
                  }}
                >
                  <span className="action-icon"><ProjectStripeIcon size={14} color="var(--text-secondary)" /></span>
                  <span>{t().titleBar.openNewProject}</span>
                </button>
                {onNewWindow && (
                  <button
                    className="pycharm-menu-action-btn"
                    onClick={() => {
                      setIsProjectMenuOpen(false);
                      onNewWindow();
                    }}
                  >
                    <span className="action-icon"><IconNewWindow size={14} color="var(--text-secondary)" /></span>
                    <span>{t().menu.newWindow}</span>
                    <kbd className="action-shortcut">⌘⇧N</kbd>
                  </button>
                )}
                {onCloseProject && workspacePath && (
                  <button
                    className="pycharm-menu-action-btn"
                    onClick={() => {
                      setIsProjectMenuOpen(false);
                      onCloseProject();
                    }}
                  >
                    <span className="action-icon"><IconWindowClose size={14} color="var(--text-secondary)" /></span>
                    <span>{t().menu.closeProject}</span>
                  </button>
                )}
              </div>

              {/* 打开的项目：本窗口可切换（保留各自会话状态）+ 其它窗口（点击聚焦） */}
              {(openSessionList.length > 0 || otherWindowList.length > 0) && (
                <>
                  <div className="pycharm-menu-section-title">{t().titleBar.openProjectsSection}</div>
                  <div className="pycharm-menu-list">
                    {openSessionList.map(s => {
                      const isCurrent = s.path === workspacePath;
                      const branch = getProjectBranch(s.path);
                      return (
                        <div
                          key={s.path}
                          className={`pycharm-menu-item project-item ${isCurrent ? 'selected' : ''}`}
                          onClick={() => {
                            if (isCurrent) return;
                            setIsProjectMenuOpen(false);
                            onSwitchWorkspace(s.path);
                          }}
                          title={isCurrent ? t().titleBar.currentProjectTitle : t().titleBar.switchProjectTitle}
                        >
                          <span
                            className="menu-item-badge project-badge"
                            style={{ background: getProjectBadgeColor(s.name) }}
                          >
                            {getBadgeInitials(s.name)}
                          </span>
                          <div className="menu-item-info">
                            <div className="menu-item-name">{s.name}</div>
                            <div className="menu-item-path" title={s.path}>
                              {formatTildePath(s.path)}
                            </div>
                            {branch && (
                              <div className="menu-item-branch">
                                <GitBranchIcon size={12} color="var(--text-muted)" />
                                <span className="menu-item-branch-name">{branch}</span>
                              </div>
                            )}
                          </div>
                        </div>
                      );
                    })}
                    {otherWindowList.map(w => {
                      const branch = w.path ? getProjectBranch(w.path) : '';
                      const titleName = w.title || 'IDE';
                      return (
                        <div
                          key={w.label}
                          className="pycharm-menu-item project-item"
                          onClick={() => {
                            setIsProjectMenuOpen(false);
                            onFocusWindow?.(w.label);
                          }}
                          title={t().titleBar.otherWindowTitle}
                        >
                          <span
                            className="menu-item-badge project-badge"
                            style={{ background: getProjectBadgeColor(titleName) }}
                          >
                            {getBadgeInitials(titleName)}
                          </span>
                          <div className="menu-item-info">
                            <div className="menu-item-name">{titleName}</div>
                            <div className="menu-item-path">
                              {w.path ? formatTildePath(w.path) : t().titleBar.openedInOtherWindow}
                            </div>
                            {branch && (
                              <div className="menu-item-branch">
                                <GitBranchIcon size={12} color="var(--text-muted)" />
                                <span className="menu-item-branch-name">{branch}</span>
                              </div>
                            )}
                          </div>
                          <span className="menu-item-window-hint">
                            <IconNewWindow size={13} color="var(--text-muted)" />
                          </span>
                        </div>
                      );
                    })}
                  </div>
                </>
              )}

              <div className={`pycharm-menu-section-title ${(openSessionList.length > 0 || otherWindowList.length > 0) ? 'project-menu-section-divider' : ''}`}>
                {t().titleBar.recentProjectsSection}
              </div>
              <div className="pycharm-menu-list">
                {recentList.length === 0 ? (
                  <div className="pycharm-menu-empty">{t().titleBar.noRecentProjects}</div>
                ) : (
                  recentList.map(p => {
                    const name = p.split(/[\\/]/).filter(Boolean).pop() ?? p;
                    const isCurrent = p === workspacePath;
                    return (
                      <div
                        key={p}
                        className={`pycharm-menu-item project-item ${isCurrent ? 'selected' : ''}`}
                        onClick={() => {
                          setIsProjectMenuOpen(false);
                          onSwitchWorkspace(p);
                        }}
                        title={t().titleBar.openThisProject}
                      >
                        <span
                          className="menu-item-badge project-badge"
                          style={{ background: getProjectBadgeColor(name) }}
                        >
                          {getBadgeInitials(name)}
                        </span>
                        <div className="menu-item-info">
                          <div className="menu-item-name">{name}</div>
                          <div className="menu-item-path" title={p}>
                            {formatTildePath(p)}
                          </div>
                        </div>
                        {onRemoveRecent && (
                          <button
                            className="menu-item-remove"
                            onClick={e => onRemoveRecent(p, e)}
                            title={t().titleBar.removeRecentTitle}
                          >
                            <IconTrash size={13} />
                          </button>
                        )}
                      </div>
                    );
                  })
                )}
              </div>
            </div>
          )}
        </div>

        {/* 2. Git Branch Dropdown Trigger — 非 git 仓库时隐藏 */}
        {isRepo && currentBranch && (
        <div className="pycharm-dropdown-wrapper" ref={branchMenuRef} data-tauri-drag-region="false">
          {opActive ? (
            <div className="titlebar-op-cluster">
              <button
                type="button"
                className={`pycharm-branch-pill pycharm-op-pill ${isBranchMenuOpen ? 'active' : ''}`}
                onClick={() => {
                  setIsBranchMenuOpen(prev => !prev);
                  setIsProjectMenuOpen(false);
                }}
                title={t().titleBar.branchPillTooltip}
              >
                <span className="titlebar-op-warn">⚠</span>
                <span className="pycharm-branch-name">{opLabel}</span>
                <span className="pycharm-dropdown-arrow"><IconChevronDown size={10} /></span>
              </button>
              <div className="titlebar-op-actions">
                <button
                  type="button"
                  className="titlebar-op-btn go"
                  disabled={!opGreenEnabled}
                  onClick={onOpGreen}
                  aria-label={opGreenTitle}
                  title={opGreenTitle}
                >
                  <IconOpContinue size={13} color="#ffffff" />
                </button>
                <button
                  type="button"
                  className="titlebar-op-btn abort"
                  onClick={onOpRed}
                  aria-label={opRedTitle}
                  title={opRedTitle}
                >
                  <IconOpAbort size={13} color="#ffffff" />
                </button>
              </div>
            </div>
          ) : (
            <button
              type="button"
              className={`pycharm-branch-pill ${isBranchMenuOpen ? 'active' : ''}`}
              onClick={() => {
                setIsBranchMenuOpen(prev => !prev);
                setIsProjectMenuOpen(false);
              }}
              title={t().titleBar.branchPillTooltip}
            >
              <GitBranchIcon size={14} color="currentColor" />
              <span className="pycharm-branch-name">{currentBranch}</span>
              {syncBadge && <span className="branch-sync-badge">{syncBadge}</span>}
              <span className="pycharm-dropdown-arrow"><IconChevronDown size={10} /></span>
            </button>
          )}

          {/* Git Branch / VCS Dropdown Menu (对齐 PyCharm Image #1) */}
          {isBranchMenuOpen && (
            <div
              ref={branchPopupRef}
              className="pycharm-menu-popup branch-menu-popup"
              style={
                branchMenuHeight !== null
                  ? { height: branchMenuHeight, maxHeight: 'none' }
                  : undefined
              }
            >
              <div className="pycharm-menu-header git-menu-search-header">
                <span className="menu-search-icon"><IconSearch size={13} color="var(--text-muted)" /></span>
                <input
                  type="text"
                  className="pycharm-menu-search"
                  placeholder={t().titleBar.searchBranchesPlaceholder}
                  value={branchFilter}
                  onChange={e => {
                    setBranchFilter(e.target.value);
                    onCloseBranchContext?.();
                  }}
                  autoFocus
                />
                <span className="menu-header-gear"><IconGear size={14} color="var(--text-muted)" /></span>
              </div>

              {/* Top actions when not filtering */}
              {!branchFilter && (
                <>
                  <div className="git-popup-top-actions">
                    <div
                      className="pycharm-menu-item git-action-item"
                      onClick={() => {
                        setIsBranchMenuOpen(false);
                        onUpdateProject?.();
                      }}
                    >
                      <span className="git-action-icon" style={{ color: 'var(--accent)' }}><IconPull size={14} /></span>
                      <span className="git-action-label">{t().menu.updateProject}</span>
                      <kbd className="git-action-shortcut">⌘T</kbd>
                    </div>
                    <div
                      className="pycharm-menu-item git-action-item"
                      onClick={() => {
                        setIsBranchMenuOpen(false);
                        onOpenCommit?.();
                      }}
                    >
                      <span className="git-action-icon"><CommitStripeIcon size={14} /></span>
                      <span className="git-action-label">{t().menu.commitEllipsis}</span>
                      <kbd className="git-action-shortcut">⌘K</kbd>
                    </div>
                    <div
                      className="pycharm-menu-item git-action-item"
                      onClick={() => {
                        setIsBranchMenuOpen(false);
                        onPush?.();
                      }}
                    >
                      <span className="git-action-icon"><IconPush size={14} /></span>
                      <span className="git-action-label">{t().menu.push}</span>
                      <kbd className="git-action-shortcut">⇧⌘K</kbd>
                    </div>
                  </div>
                  <div className="git-menu-divider" />

                  <div className="git-popup-op-actions">
                    {sequencer?.rebasing && (
                      <>
                        <div
                          className="pycharm-menu-item git-action-item"
                          onClick={() => {
                            setIsBranchMenuOpen(false);
                            onContinueRebase?.();
                          }}
                        >
                          <span className="git-action-label">{t().titleBar.continueRebaseAction}</span>
                        </div>
                        <div
                          className="pycharm-menu-item git-action-item"
                          onClick={() => {
                            setIsBranchMenuOpen(false);
                            onSkipRebase?.();
                          }}
                        >
                          <span className="git-action-label">{t().titleBar.skipRebaseAction}</span>
                        </div>
                        <div
                          className="pycharm-menu-item git-action-item ctx-danger"
                          onClick={() => {
                            setIsBranchMenuOpen(false);
                            onAbortRebase?.();
                          }}
                        >
                          <span className="git-action-label">{t().titleBar.abortRebaseAction}</span>
                        </div>
                      </>
                    )}
                    {sequencer?.cherryPicking && (
                      <div
                        className="pycharm-menu-item git-action-item ctx-danger"
                        onClick={() => {
                          setIsBranchMenuOpen(false);
                          onAbortCherryPick?.();
                        }}
                      >
                        <span className="git-action-label">{t().gitPanel.abortCherryPickTitle}</span>
                      </div>
                    )}
                    {sequencer?.merging && (
                      <div
                        className="pycharm-menu-item git-action-item ctx-danger"
                        onClick={() => {
                          setIsBranchMenuOpen(false);
                          onAbortMerge?.();
                        }}
                      >
                        <span className="git-action-label">{t().gitPanel.abortMergeTitle}</span>
                      </div>
                    )}
                    <div
                      className="pycharm-menu-item git-action-item"
                      onClick={() => {
                        setIsCreatingBranch(true);
                        onCloseBranchContext?.();
                      }}
                    >
                      <span className="git-action-icon"><IconPlus size={13} /></span>
                      <span className="git-action-label">{t().titleBar.newBranchAction}</span>
                      <kbd className="git-action-shortcut">⌥⌘N</kbd>
                    </div>
                    <div
                      className="pycharm-menu-item git-action-item"
                      onClick={() => {
                        setIsBranchMenuOpen(false);
                        onCheckoutRef?.();
                      }}
                    >
                      <span className="git-action-label">{t().titleBar.checkoutTagOrRevision}</span>
                    </div>
                  </div>
                  <div className="git-menu-divider" />
                </>
              )}

              {/* Create new branch form */}
              {isCreatingBranch && (
                <form onSubmit={handleCreateBranchSubmit} className="pycharm-new-branch-form">
                  <input
                    type="text"
                    className="pycharm-new-branch-input"
                    placeholder={t().titleBar.newBranchNamePlaceholder}
                    value={newBranchName}
                    onChange={e => setNewBranchName(e.target.value)}
                    autoFocus
                  />
                  <div className="pycharm-new-branch-btns">
                    <button type="submit" className="pycharm-btn-primary" disabled={!newBranchName.trim()}>
                      {t().titleBar.createAndSwitch}
                    </button>
                    <button
                      type="button"
                      className="pycharm-btn-secondary"
                      onClick={() => {
                        setIsCreatingBranch(false);
                        setNewBranchName('');
                      }}
                    >
                      {t().dialog.cancel}
                    </button>
                  </div>
                </form>
              )}

              {/* 列表区单滚动容器 */}
              <div className="branch-menu-scroll" onScroll={onCloseBranchContext}>
                {/* Recent Branches Section */}
                {!branchFilter && (
                  <>
                    <div
                      className="branch-collapsible-header"
                      onClick={() => setSectionsOpen(prev => ({ ...prev, recent: !prev.recent }))}
                    >
                      <span className="tree-chevron">
                        {sectionsOpen.recent ? <IconChevronDown size={10} /> : <IconChevronRight size={10} />}
                      </span>
                      <span>{t().titleBar.recentSection}</span>
                    </div>
                    {sectionsOpen.recent && (
                      <div className="pycharm-menu-list">
                        {recentBranches.map(b => renderBranchRowItem(b))}
                      </div>
                    )}
                  </>
                )}

                {/* Local Branches Section */}
                {(!branchFilter || sortedLocalBranches.length > 0) && (
                  <>
                    <div
                      className="branch-collapsible-header"
                      onClick={() => setSectionsOpen(prev => ({ ...prev, local: !prev.local }))}
                    >
                      <span className="tree-chevron">
                        {sectionsOpen.local || branchFilter ? <IconChevronDown size={10} /> : <IconChevronRight size={10} />}
                      </span>
                      <span>{t().titleBar.localSection}</span>
                    </div>
                    {(sectionsOpen.local || branchFilter) && (
                      <div className="pycharm-menu-list">
                        {sortedLocalBranches.map(b => renderBranchRowItem(b))}
                      </div>
                    )}
                  </>
                )}

                {/* Remote Branches Section */}
                {(!branchFilter || filteredRemoteBranches.length > 0) && (
                  <>
                    <div
                      className="branch-collapsible-header"
                      onClick={() => setSectionsOpen(prev => ({ ...prev, remote: !prev.remote }))}
                    >
                      <span className="tree-chevron">
                        {sectionsOpen.remote || branchFilter ? <IconChevronDown size={10} /> : <IconChevronRight size={10} />}
                      </span>
                      <span>{t().titleBar.remoteSection}</span>
                    </div>
                    {(sectionsOpen.remote || branchFilter) && (
                      <div className="pycharm-menu-list">
                        {filteredRemoteBranches.map(b => renderBranchRowItem(b))}
                      </div>
                    )}
                  </>
                )}

                {/* Tags Section */}
                {(filteredTags.length > 0 || !branchFilter) && (
                  <>
                    <div
                      className="branch-collapsible-header"
                      onClick={() => setSectionsOpen(prev => ({ ...prev, tags: !prev.tags }))}
                    >
                      <span className="tree-chevron">
                        {sectionsOpen.tags || branchFilter ? <IconChevronDown size={10} /> : <IconChevronRight size={10} />}
                      </span>
                      <span>{t().titleBar.tagsSection}</span>
                    </div>
                    {(sectionsOpen.tags || branchFilter) && (
                      <div className="pycharm-menu-list">
                        {filteredTags.map(tag => (
                          <div
                            key={tag.name}
                            className="pycharm-menu-item branch-menu-item-row"
                            title={`${tag.name} ${tag.shortHash} ${tag.date}\n${tag.message}`}
                            onClick={() => {
                              setIsBranchMenuOpen(false);
                              onCheckoutTag?.(tag.name);
                            }}
                          >
                            <span className="branch-row-icon">
                              <IconTag size={12} color="var(--git-tag, #e3b341)" />
                            </span>
                            <span className="menu-item-name">{tag.name}</span>
                            <button
                              type="button"
                              className="menu-item-remove"
                              title={t().titleBar.deleteTagTitle}
                              onClick={ev => {
                                ev.stopPropagation();
                                setIsBranchMenuOpen(false);
                                onDeleteTag?.(tag);
                              }}
                            >
                              <IconTrash size={11} />
                            </button>
                          </div>
                        ))}
                        {filteredTags.length === 0 && (
                          <div className="pycharm-menu-empty">{t().titleBar.noTags}</div>
                        )}
                      </div>
                    )}
                  </>
                )}
              </div>

              {/* 底部拖动把柄 */}
              <div
                className="branch-menu-resize-grip"
                onPointerDown={startBranchMenuResize}
                title={t().titleBar.dragResizeMenuHeight}
              />
            </div>
          )}
        </div>
      )}
      </div>


      {/* Right: Search & Caption Buttons */}
      <div className="titlebar-actions" data-tauri-drag-region="deep">
        <button
          type="button"
          className="titlebar-search-btn"
          onClick={onSearchClick}
          title={t().titleBar.searchEverywhere}
          aria-label={t().titleBar.searchEverywhere}
          data-tauri-drag-region="false"
        >
          <IconSearch size={15} color="currentColor" />
        </button>

        {availableUpdateVersion && (
          <button
            type="button"
            className="titlebar-update-badge"
            onClick={onAvailableUpdateClick}
            title={t().updater.topbarBadgeTitle(availableUpdateVersion)}
            aria-label={t().updater.topbarBadgeTitle(availableUpdateVersion)}
            data-tauri-drag-region="false"
          >
            <IconArrowDown size={12} color="currentColor" />
            <span>{t().updater.topbarBadge(availableUpdateVersion)}</span>
          </button>
        )}

        {/* Windows Caption Buttons。
            用 <button> 而非 <span>：语义 + 可键盘聚焦 + VoiceOver 可读，
            aria-label 与 title 一致（无障碍树只吃 aria-label，不吃 title）。 */}
        {platform === 'win' && (
          <div className="win-caption-buttons" data-tauri-drag-region="false">
            <button
              type="button"
              className="win-btn win-minimize"
              onClick={handleWinMinimize}
              aria-label={t().common.minimize}
              title={t().common.minimize}
            >
              <IconWindowMinimize size={13} />
            </button>
            <button
              type="button"
              className="win-btn win-maximize"
              onClick={handleWinMaximize}
              aria-label={t().common.maximize}
              title={t().common.maximize}
            >
              <IconWindowMaximize size={12} />
            </button>
            <button
              type="button"
              className="win-btn win-close"
              onClick={handleWinClose}
              aria-label={t().common.close}
              title={t().common.close}
            >
              <IconWindowClose size={13} />
            </button>
          </div>
        )}
      </div>
    </header>
  );
});
