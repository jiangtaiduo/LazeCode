/**
 * Compare Branches（PyCharm 对齐版）。
 *
 * 与 Compare Versions / Compare Drawer 的区别是结构性的：
 *   Compare Versions = 两个提交之间的文件 diff；
 *   Compare Branches = 上下两条独立的提交日志：
 *     上：只在 left 分支、不在 right 分支的提交
 *     下：只在 right 分支、不在 left 分支的提交
 *   每条日志右边是当前提交（或 Ctrl/Cmd+A 全选后的整侧）的提交信息与文件树。
 *
 * 组件不自己决定“哪个分支是左/右”：由 App 保存 session 并传入；Swap 也由
 * App 交换 session，保证标签标题、日志顺序与后续刷新使用同一份真相。
 */

import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { invokeCommand } from '../services/tauriBridge';
import type { GitCommit, GitCommitFile, GitCompareBranchesResult } from '../types';
import { computeGraphLayout } from '../services/logFolding';
import { GraphCell } from './GitGraphCell';
import { CtxMenuShell } from './CtxMenuShell';
import { startPointerDrag, startPanelDrag } from '../services/panelDrag';
import { formatCommitDate } from '../services/commitDate';
import { t } from '../services/i18n';
import { useLanguageRefresh } from '../hooks/useLanguageRefresh';
import {
  allCompareDirPaths,
  branchNamesOf,
  buildCompareFileTree,
  collectCompareAuthors,
  collectCompareRefs,
  EMPTY_COMPARE_FILTER,
  filterCompareCommits,
  filterCompareFilesByPath,
  formatCompareDetailsDate
} from '../services/compareBranches';
import type { CompareCommitFilter, CompareDateFilter, CompareFileTreeNode } from '../services/compareBranches';
import {
  FolderIcon,
  GitBranchIcon,
  IconChevronDown,
  IconChevronRight,
  IconCloud,
  IconCollapseAll,
  IconCommitGraph,
  IconDiff,
  IconExpandAll,
  IconMerge,
  IconRefresh,
  IconSearchDropdown,
  IconSwap,
  IconTag,
  IconWarning,
  renderFileIcon
} from './FileIcons';

const ROW_H = 26;
const INITIAL_LIMIT = 1000;
const LOAD_MORE_SIZE = 500;
const SPLIT_KEY = 'ide_compare_branches_split';
const DETAILS_WIDTH_KEY = 'ide_compare_branches_details_width';
/** Git 空树对象：多选范围的最老祖先没有父提交时，用它作为 diff 的旧侧。 */
const EMPTY_TREE_HASH = '4b825dc642cb6eb9a060e54bf8d69288fbee4904';

function readNumber(key: string, fallback: number, min: number, max: number): number {
  try {
    const raw = window.localStorage.getItem(key);
    if (raw === null) return fallback;
    const value = Number(raw);
    return Number.isFinite(value) ? Math.min(max, Math.max(min, value)) : fallback;
  } catch {
    return fallback;
  }
}

function writeNumber(key: string, value: number): void {
  try {
    window.localStorage.setItem(key, String(Math.round(value)));
  } catch {
    // localStorage 不可用（隐私模式/配额）时只丢记忆，不影响比较功能。
  }
}

function statusColor(status: string): string {
  const value = status.toUpperCase();
  if (value.startsWith('A') || value.startsWith('?')) return 'var(--git-green, #3fb950)';
  if (value.startsWith('D')) return 'var(--git-del, #f85149)';
  return 'var(--git-blue, #58a6ff)';
}

interface CompareBranchesViewProps {
  repoPath: string;
  /** 项目树显示名（仓库根可能是子目录时仍显示项目名） */
  repoName?: string;
  leftRef: string;
  rightRef: string;
  onSwap: () => void;
  /** 单个提交的整提交/单文件 diff；path 为空 = 整个提交 */
  onOpenCommitDiff: (commit: string, path?: string) => void;
  /** 全选整侧后的聚合 diff（merge-base..side） */
  onOpenCompareDiff: (from: string, to: string, path?: string) => void;
}

interface CompareBranchPaneProps {
  repoPath: string;
  repoName?: string;
  side: 'left' | 'right';
  refName: string;
  otherRef: string;
  mergeBase: string;
  commits: GitCommit[];
  allFiles: GitCommitFile[];
  truncated: boolean;
  loadingMore: boolean;
  notice: string | null;
  pathFilter: string;
  onPathFilterChange: (path: string) => void;
  onRefresh: () => void;
  onLoadMore: () => void;
  /** 上下两条日志共用同一详情宽度（对齐 PyCharm 的同一列） */
  detailsPct: number;
  onDetailsPctChange: (value: number) => void;
  showSwap: boolean;
  onSwap: () => void;
  onOpenCommitDiff: (commit: string, path?: string) => void;
  onOpenCompareDiff: (from: string, to: string, path?: string) => void;
}

interface FilterMenuState {
  kind: 'ref' | 'author' | 'date' | 'path';
  x: number;
  y: number;
}

const CompareBranchPane: React.FC<CompareBranchPaneProps> = ({
  repoPath,
  repoName: repoNameProp,
  side,
  refName,
  otherRef,
  mergeBase,
  commits,
  allFiles,
  truncated,
  loadingMore,
  notice,
  pathFilter,
  onPathFilterChange,
  onRefresh,
  onLoadMore,
  detailsPct,
  onDetailsPctChange,
  showSwap,
  onSwap,
  onOpenCommitDiff,
  onOpenCompareDiff
}) => {
  useLanguageRefresh();

  const [filter, setFilter] = useState<CompareCommitFilter>({ ...EMPTY_COMPARE_FILTER });
  const [menu, setMenu] = useState<FilterMenuState | null>(null);
  const [pathDraft, setPathDraft] = useState(pathFilter);
  const [showGraph, setShowGraph] = useState(true);
  const [selectedHashes, setSelectedHashes] = useState<string[]>([]);
  const [selectAll, setSelectAll] = useState(false);
  const [selectedFile, setSelectedFile] = useState<string | null>(null);
  const [collapsedDirs, setCollapsedDirs] = useState<Set<string>>(new Set());
  const [rootCollapsed, setRootCollapsed] = useState(false);
  const [scrollTop, setScrollTop] = useState(0);
  const [viewportH, setViewportH] = useState(420);
  const [commitFiles, setCommitFiles] = useState<GitCommitFile[]>([]);
  const [filesLoading, setFilesLoading] = useState(false);
  const [filesError, setFilesError] = useState<string | null>(null);

  const paneRef = useRef<HTMLDivElement>(null);
  const bodyRef = useRef<HTMLDivElement>(null);
  const detailsRef = useRef<HTMLDivElement>(null);
  const scrollRef = useRef<HTMLDivElement>(null);
  const scrollRafRef = useRef<number | null>(null);
  const lastClickedRef = useRef<string | null>(null);

  const byHash = useMemo(() => new Map(commits.map(commit => [commit.hash, commit])), [commits]);
  const repoName = useMemo(
    () =>
      repoNameProp ||
      repoPath.replace(/\\/g, '/').split('/').filter(Boolean).pop() ||
      'Repository',
    [repoNameProp, repoPath]
  );
  const filteredCommits = useMemo(() => filterCompareCommits(commits, filter), [commits, filter]);
  const authors = useMemo(() => collectCompareAuthors(commits), [commits]);
  const refOptions = useMemo(() => collectCompareRefs(commits), [commits]);
  const graphLayout = useMemo(() => computeGraphLayout(commits), [commits]);

  const activeDate = filter.date;
  const activeFilterCount =
    (filter.author ? 1 : 0) +
    (activeDate !== 'all' ? 1 : 0) +
    (filter.refName ? 1 : 0) +
    (pathFilter ? 1 : 0);

  const selectedCommits = useMemo(
    () => (selectAll ? commits : selectedHashes.map(hash => byHash.get(hash)).filter((c): c is GitCommit => !!c)),
    [byHash, commits, selectAll, selectedHashes]
  );
  /** 日志顺序（新→旧）：范围 diff 与详细信息标题都按这个顺序算。 */
  const orderedSelectedCommits = useMemo(() => {
    const order = new Map(commits.map((commit, index) => [commit.hash, index]));
    return [...selectedCommits].sort(
      (a, b) => (order.get(a.hash) ?? 0) - (order.get(b.hash) ?? 0)
    );
  }, [selectedCommits, commits]);
  const selectedKey = orderedSelectedCommits.map(commit => commit.hash).join(',');
  // 详情底部展示第一个被选中的提交（PyCharm 多选时仍显示锚点提交的元数据）。
  const detailsCommit = selectedHashes.length > 0 ? byHash.get(selectedHashes[0]) ?? null : null;

  /**
   * 多选聚合范围：最老选中提交的父提交 → 最新选中提交。
   * 与 PyCharm 的 “Multiple commits selected” 一致：文件树和文件 diff 使用同一
   * 个范围，连续多选时精确等于这些提交的合并结果；不连续时也按首尾范围处理，
   * 不会把中间提交的路径漏掉。根提交用 Git 空树作为旧侧。
   */
  const selectionRange = useMemo(() => {
    if (selectAll) return { from: mergeBase || otherRef, to: refName };
    if (orderedSelectedCommits.length < 2) return null;
    const newest = orderedSelectedCommits[0];
    const oldest = orderedSelectedCommits[orderedSelectedCommits.length - 1];
    return {
      from: oldest.parents?.[0] || EMPTY_TREE_HASH,
      to: newest.hash
    };
  }, [mergeBase, orderedSelectedCommits, otherRef, refName, selectAll]);

  // 初始不默认选中任何提交（对齐 PyCharm：刚打开比较页时日志为空选）。
  // 过滤/刷新只丢弃已经不可见的选中项，不再补选第一条。
  useEffect(() => {
    if (selectAll) return;
    setSelectedHashes(prev => {
      if (prev.length === 0) return prev;
      const visible = new Set(filteredCommits.map(commit => commit.hash));
      const kept = prev.filter(hash => visible.has(hash));
      return kept.length === prev.length ? prev : kept;
    });
  }, [filteredCommits, selectAll]);

  // 选择集合变化时清空文件选择，避免上一组提交的文件路径残留。
  useEffect(() => {
    setSelectedFile(null);
  }, [selectedKey, selectAll]);

  // 单个提交走 git show；多选走父提交→最新提交的范围 diff；Ctrl+A 使用整侧文件。
  useEffect(() => {
    if (selectAll || !detailsCommit) {
      setCommitFiles([]);
      setFilesLoading(false);
      setFilesError(null);
      return;
    }
    if (orderedSelectedCommits.length > 1 && !selectionRange) {
      setCommitFiles([]);
      setFilesLoading(false);
      setFilesError(null);
      return;
    }

    let cancelled = false;
    setCommitFiles([]);
    setFilesLoading(true);
    setFilesError(null);
    const request =
      orderedSelectedCommits.length === 1
        ? invokeCommand<GitCommitFile[]>('git_commit_files', {
            repoPath,
            commit: orderedSelectedCommits[0].hash
          })
        : invokeCommand<GitCommitFile[]>('git_compare_commits', {
            repoPath,
            fromCommit: selectionRange!.from,
            toCommit: selectionRange!.to
          });

    request
      .then(files => {
        if (!cancelled) setCommitFiles(Array.isArray(files) ? files : []);
      })
      .catch(err => {
        if (!cancelled) {
          setCommitFiles([]);
          setFilesError(err instanceof Error ? err.message : String(err));
        }
      })
      .finally(() => {
        if (!cancelled) setFilesLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [detailsCommit, orderedSelectedCommits, repoPath, selectAll, selectionRange]);

  const detailsFiles = useMemo(
    () => filterCompareFilesByPath(selectAll ? allFiles : commitFiles, pathFilter),
    [allFiles, commitFiles, pathFilter, selectAll]
  );
  const tree = useMemo(() => buildCompareFileTree(detailsFiles), [detailsFiles]);
  const allDirPaths = useMemo(() => allCompareDirPaths(tree), [tree]);

  useEffect(() => {
    // 文件选择只由用户点击产生；提交切换后保持空选，文件列表变化时
    // 仅丢弃已不存在的路径，不再自动选中第一项。
    if (!selectedFile) return;
    if (detailsFiles.some(file => file.path === selectedFile)) return;
    setSelectedFile(null);
  }, [detailsFiles, selectedFile]);

  useEffect(() => {
    const el = scrollRef.current;
    if (!el) return;
    const measure = () => setViewportH(el.clientHeight || 420);
    measure();
    const ro = new ResizeObserver(measure);
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  // 搜索/过滤器变化后回到顶部；加载更多不重置位置。
  useEffect(() => {
    setScrollTop(0);
    if (scrollRef.current) scrollRef.current.scrollTop = 0;
  }, [filter.query, filter.useRegex, filter.caseSensitive, filter.author, filter.date, filter.refName, pathFilter]);

  const handleScroll = (e: React.UIEvent<HTMLDivElement>) => {
    if (scrollRafRef.current !== null) return;
    const el = e.currentTarget;
    scrollRafRef.current = requestAnimationFrame(() => {
      scrollRafRef.current = null;
      setScrollTop(el.scrollTop);
    });
  };

  const windowStart = Math.max(0, Math.floor(scrollTop / ROW_H) - 8);
  const windowEnd = Math.min(filteredCommits.length, Math.ceil((scrollTop + viewportH) / ROW_H) + 8);
  const visibleRows = filteredCommits.slice(windowStart, windowEnd);

  const toggleDir = (path: string) => {
    setCollapsedDirs(prev => {
      const next = new Set(prev);
      if (next.has(path)) next.delete(path);
      else next.add(path);
      return next;
    });
  };

  const handleCommitClick = (commit: GitCommit, e: React.MouseEvent) => {
    setSelectAll(false);
    const hash = commit.hash;
    let next: string[];
    if (e.shiftKey && lastClickedRef.current) {
      const anchor = filteredCommits.findIndex(item => item.hash === lastClickedRef.current);
      const target = filteredCommits.findIndex(item => item.hash === hash);
      if (anchor >= 0 && target >= 0) {
        const [lo, hi] = anchor < target ? [anchor, target] : [target, anchor];
        next = filteredCommits.slice(lo, hi + 1).map(item => item.hash);
      } else {
        next = [hash];
      }
    } else if (e.metaKey || e.ctrlKey) {
      next = selectedHashes.includes(hash)
        ? selectedHashes.filter(item => item !== hash)
        : [...selectedHashes, hash];
      if (next.length === 0) next = [hash];
      lastClickedRef.current = hash;
    } else {
      next = [hash];
      lastClickedRef.current = hash;
    }
    setSelectedHashes(next);
  };

  /** 单提交/多选范围/Ctrl+A 整侧统一从这里打开 diff。 */
  const openSelectionDiff = (path?: string) => {
    if (selectAll) {
      onOpenCompareDiff(mergeBase || otherRef, refName, path);
    } else if (orderedSelectedCommits.length > 1 && selectionRange) {
      onOpenCompareDiff(selectionRange.from, selectionRange.to, path);
    } else if (detailsCommit) {
      onOpenCommitDiff(detailsCommit.hash, path);
    }
  };

  const openWhole = () => openSelectionDiff();

  const openFile = (path?: string | null) => {
    const target = path || selectedFile;
    if (!target) return;
    openSelectionDiff(target);
  };

  const handlePaneKeyDown = (e: React.KeyboardEvent<HTMLDivElement>) => {
    const target = e.target as HTMLElement | null;
    if (target && (target.tagName === 'INPUT' || target.tagName === 'TEXTAREA')) return;
    if ((e.metaKey || e.ctrlKey) && (e.key === 'a' || e.key === 'A')) {
      e.preventDefault();
      e.stopPropagation();
      setSelectAll(true);
      setSelectedHashes(commits.map(commit => commit.hash));
      lastClickedRef.current = commits[0]?.hash ?? null;
      return;
    }
    if (e.key === 'Escape') {
      // Esc 回到空选，和打开比较页时的初始状态一致。
      setSelectAll(false);
      setSelectedHashes([]);
      setSelectedFile(null);
      lastClickedRef.current = null;
      return;
    }
    if (e.key !== 'ArrowDown' && e.key !== 'ArrowUp') return;
    if (filteredCommits.length === 0) return;
    e.preventDefault();
    const current = selectedHashes[0] ?? '';
    const index = filteredCommits.findIndex(commit => commit.hash === current);
    const nextIndex =
      index < 0
        ? 0
        : e.key === 'ArrowDown'
          ? Math.min(filteredCommits.length - 1, index + 1)
          : Math.max(0, index - 1);
    setSelectAll(false);
    setSelectedHashes([filteredCommits[nextIndex].hash]);
    lastClickedRef.current = filteredCommits[nextIndex].hash;
    const el = scrollRef.current;
    if (el) {
      const top = nextIndex * ROW_H;
      if (top < el.scrollTop) el.scrollTop = top;
      else if (top + ROW_H > el.scrollTop + el.clientHeight) el.scrollTop = top + ROW_H - el.clientHeight;
    }
  };

  const openFilterMenu = (kind: FilterMenuState['kind']) => (e: React.MouseEvent<HTMLButtonElement>) => {
    const rect = e.currentTarget.getBoundingClientRect();
    setPathDraft(pathFilter);
    setMenu({ kind, x: rect.left, y: rect.bottom + 2 });
  };

  const startDetailsResize = (e: React.PointerEvent<HTMLDivElement>) => {
    const body = bodyRef.current;
    const details = detailsRef.current;
    if (!body || !details) return;
    const rect = body.getBoundingClientRect();
    startPanelDrag(
      e,
      { element: details, axis: 'x' },
      'col-resize',
      clientX => Math.min(62, Math.max(24, ((rect.right - clientX) / rect.width) * 100)),
      value => {
        onDetailsPctChange(value);
      }
    );
  };

  const renderFile = (file: GitCommitFile, depth: number) => {
    const name = file.path.split('/').pop() ?? file.path;
    return (
      <div
        key={file.path}
        className={`cb-file-row ${selectedFile === file.path ? 'selected' : ''}`}
        style={{ paddingLeft: 8 + depth * 12 }}
        onClick={() => setSelectedFile(file.path)}
        onDoubleClick={() => openFile(file.path)}
        title={`${file.path}\n${t().compareBranches.detailsHint}`}
      >
        <span className="cb-file-icon">{renderFileIcon(name, false, false)}</span>
        <span className="cb-file-name" style={{ color: statusColor(file.status) }}>
          {name}
        </span>
        <span className="cb-file-spacer" />
        <span className="cb-numstat">
          <i className="num-add">+{file.additions}</i>
          {file.deletions > 0 && <i className="num-del">-{file.deletions}</i>}
        </span>
      </div>
    );
  };

  const renderDirs = (node: CompareFileTreeNode, depth: number): React.ReactNode => (
    <>
      {node.dirs.map(dir => {
        const collapsed = collapsedDirs.has(dir.path);
        return (
          <React.Fragment key={dir.path}>
            <div
              className="cb-dir-row"
              style={{ paddingLeft: 8 + depth * 12 }}
              onClick={() => toggleDir(dir.path)}
              title={dir.path}
            >
              <span className={`cb-chevron ${collapsed ? 'collapsed' : 'expanded'}`}>
                {collapsed ? (
                  <IconChevronRight size={10} color="var(--text-muted)" />
                ) : (
                  <IconChevronDown size={10} color="var(--text-muted)" />
                )}
              </span>
              <FolderIcon size={13} isOpen={!collapsed} />
              <span className="cb-dir-name">{dir.name}</span>
              <span className="cb-dir-count">{t().compareBranches.fileCount(dir.fileCount)}</span>
            </div>
            {!collapsed && renderDirs(dir, depth + 1)}
          </React.Fragment>
        );
      })}
      {node.files.map(file => renderFile(file, depth))}
    </>
  );

  const bannerText = t().compareBranches.commitsOnlyIn(refName, otherRef);

  return (
    <section
      className={`cb-pane cb-pane-${side}`}
      ref={paneRef}
      tabIndex={0}
      onMouseDown={e => {
        const target = e.target as HTMLElement | null;
        if (target?.closest('input, textarea, button')) return;
        paneRef.current?.focus();
      }}
      onKeyDown={handlePaneKeyDown}
      aria-label={bannerText}
    >
      <div className="cb-banner">
        <IconWarning size={13} color="#e3b341" />
        <span className="cb-banner-text" title={bannerText}>
          {bannerText}
        </span>
        {showSwap && (
          <button type="button" className="cb-swap-btn" onClick={onSwap} title={t().compareBranches.swap}>
            <IconSwap size={12} />
            <span>{t().compareBranches.swap}</span>
          </button>
        )}
      </div>

      <div className="cb-body" ref={bodyRef}>
        <div className="cb-log-col">
          <div className="cb-toolbar">
            <div className="cb-search-box">
              <span className="cb-search-icon" title={t().gitPanel.searchPlaceholder}>
                <IconSearchDropdown size={12} color="var(--text-muted)" />
              </span>
              <input
                type="text"
                className="cb-search-input"
                value={filter.query}
                onChange={e => setFilter(prev => ({ ...prev, query: e.target.value }))}
                placeholder={t().gitPanel.searchPlaceholder}
                aria-label={t().gitPanel.searchPlaceholder}
              />
              <button
                type="button"
                className={`cb-toggle ${filter.useRegex ? 'active' : ''}`}
                onClick={() => setFilter(prev => ({ ...prev, useRegex: !prev.useRegex }))}
                title={t().gitPanel.regexTooltip}
              >
                .*
              </button>
              <button
                type="button"
                className={`cb-toggle ${filter.caseSensitive ? 'active' : ''}`}
                onClick={() => setFilter(prev => ({ ...prev, caseSensitive: !prev.caseSensitive }))}
                title={t().gitPanel.matchCaseTooltip}
              >
                Cc
              </button>
            </div>

            <button
              type="button"
              className={`cb-filter-btn ${filter.refName ? 'active' : ''}`}
              onClick={openFilterMenu('ref')}
              title={t().compareBranches.filterRef}
            >
              <GitBranchIcon size={11} />
            </button>
            <button
              type="button"
              className={`cb-filter-btn ${filter.author ? 'active' : ''}`}
              onClick={openFilterMenu('author')}
              title={t().gitPanel.filterUser}
            >
              <span>{t().gitPanel.filterUser}</span>
            </button>
            <button
              type="button"
              className={`cb-filter-btn ${activeDate !== 'all' ? 'active' : ''}`}
              onClick={openFilterMenu('date')}
              title={t().gitPanel.filterDate}
            >
              <span>{t().gitPanel.filterDate}</span>
            </button>
            <button
              type="button"
              className={`cb-filter-btn ${pathFilter ? 'active' : ''}`}
              onClick={openFilterMenu('path')}
              title={t().gitPanel.filterPaths}
            >
              <span>{t().gitPanel.filterPaths}</span>
            </button>
            {activeFilterCount > 0 && (
              <button
                type="button"
                className="cb-clear-filters"
                title={t().gitPanel.clear}
                onClick={() => {
                  setFilter(prev => ({ ...prev, author: '', date: 'all', refName: '' }));
                  if (pathFilter) onPathFilterChange('');
                }}
              >
                <span>×</span>
              </button>
            )}

            <div className="cb-toolbar-right">
              <button
                type="button"
                className={`cb-icon-btn ${showGraph ? 'active' : ''}`}
                title={t().gitPanel.showGraph}
                onClick={() => setShowGraph(value => !value)}
              >
                <IconCommitGraph size={14} />
              </button>
              <button type="button" className="cb-icon-btn" title={t().gitPanel.refreshTooltip} onClick={onRefresh}>
                <IconRefresh size={14} />
              </button>
            </div>
          </div>

          {notice && <div className="cb-inline-error">{notice}</div>}

          <div className="cb-log-scroll" ref={scrollRef} onScroll={handleScroll}>
            {filteredCommits.length === 0 && (
              <div className="cb-empty">
                {commits.length === 0 ? t().compareBranches.noCommits(refName) : t().compareBranches.noMatches}
              </div>
            )}
            {windowStart > 0 && <div aria-hidden="true" style={{ height: windowStart * ROW_H }} />}
            {visibleRows.map(commit => {
              const active = selectAll || selectedHashes.includes(commit.hash);
              return (
                <div
                  key={commit.hash}
                  className={`log-row cb-log-row ${active ? 'active' : ''}`}
                  data-commit-row={commit.hash}
                  title={t().gitPanel.rowHint}
                  onClick={e => handleCommitClick(commit, e)}
                  onDoubleClick={() => onOpenCommitDiff(commit.hash)}
                >
                  {showGraph && (
                    <span className="log-graph cb-log-graph">
                      <GraphCell commit={commit} layout={graphLayout.get(commit.hash)} />
                    </span>
                  )}
                  <span className="log-subject cb-log-subject">
                    {commit.isMerge && <IconMerge size={12} color="var(--text-secondary)" />}
                    <span className="log-subject-text">{commit.message}</span>
                  </span>
                  {commit.branchTags && commit.branchTags.length > 0 && (
                    <span className="log-refs cb-log-refs">
                      {commit.branchTags.map(tag => (
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
                          ) : (
                            <GitBranchIcon size={11} />
                          )}
                          <span className="log-ref-name">{tag.name}</span>
                        </span>
                      ))}
                    </span>
                  )}
                  <span className="log-spacer" />
                  <span className="log-author" title={commit.authorEmail}>
                    {commit.author}
                  </span>
                  <span className="log-date">{formatCommitDate(commit.date)}</span>
                </div>
              );
            })}
            {windowEnd < filteredCommits.length && (
              <div aria-hidden="true" style={{ height: (filteredCommits.length - windowEnd) * ROW_H }} />
            )}
            {truncated && (
              <div className="cb-load-more-wrap">
                <button type="button" className="cb-load-more" onClick={onLoadMore} disabled={loadingMore}>
                  {loadingMore ? t().common.loading : t().gitPanel.loadMore}
                </button>
                <span className="cb-truncated">{t().compareBranches.truncated(commits.length)}</span>
              </div>
            )}
          </div>
        </div>

        <div
          className="cb-details-resizer"
          onPointerDown={startDetailsResize}
          title={t().compareBranches.resizeDetails}
        />
        <div className="cb-details" ref={detailsRef} style={{ width: `${detailsPct}%` }}>
          <div className="cb-details-header">
            <span className="cb-details-title">
              {filesLoading
                ? t().common.loading
                : `${selectedCommits.length > 1 ? `${t().compareBranches.allSelected(selectedCommits.length)} · ` : ''}${t().gitPanel.filesChanged(detailsFiles.length)}`}
            </span>
            <span className="cb-details-actions">
              <button
                type="button"
                className="cb-icon-btn"
                title={t().compareBranches.expandAll}
                onClick={() => {
                  setRootCollapsed(false);
                  setCollapsedDirs(new Set());
                }}
                disabled={allDirPaths.length === 0}
              >
                <IconExpandAll size={13} />
              </button>
              <button
                type="button"
                className="cb-icon-btn"
                title={t().compareBranches.collapseAll}
                onClick={() => {
                  setRootCollapsed(true);
                  setCollapsedDirs(new Set(allDirPaths));
                }}
                disabled={allDirPaths.length === 0}
              >
                <IconCollapseAll size={13} />
              </button>
              <button
                type="button"
                className="cb-icon-btn"
                title={t().compareBranches.openDiff}
                onClick={() => (selectedFile ? openFile(selectedFile) : openWhole())}
                disabled={!selectedFile && !detailsCommit && !selectAll}
              >
                <IconDiff size={14} />
              </button>
            </span>
          </div>

          <div className="cb-file-tree">
            {filesError && <div className="cb-inline-error">{filesError}</div>}
            {!filesLoading && detailsFiles.length === 0 && (
              <div className="cb-empty cb-details-empty">{t().compareBranches.noFiles}</div>
            )}
            {detailsFiles.length > 0 && (
              <div className="cb-root-row" onClick={() => setRootCollapsed(value => !value)} title={repoPath}>
                <span className={`cb-chevron ${rootCollapsed ? 'collapsed' : 'expanded'}`}>
                  {rootCollapsed ? (
                    <IconChevronRight size={10} color="var(--text-muted)" />
                  ) : (
                    <IconChevronDown size={10} color="var(--text-muted)" />
                  )}
                </span>
                <FolderIcon size={13} isOpen={!rootCollapsed} />
                <span className="cb-root-name">{repoName}</span>
                <span className="cb-root-count">{t().compareBranches.fileCount(detailsFiles.length)}</span>
              </div>
            )}
            {!rootCollapsed && renderDirs(tree, 1)}
          </div>

          <div className="cb-details-meta">
            {selectAll ? (
              <>
                <div className="cb-meta-message">{t().compareBranches.allSelected(commits.length)}</div>
                <div className="cb-meta-line cb-meta-email">{refName}</div>
              </>
            ) : detailsCommit ? (
              <>
                <div className="cb-meta-message">{detailsCommit.message}</div>
                <div className="cb-meta-line">
                  <span className="cb-meta-hash">{detailsCommit.shortHash}</span>
                  <span className="cb-meta-email">{detailsCommit.authorEmail}</span>
                </div>
                <div className="cb-meta-line cb-meta-date">
                  {detailsCommit.authorEmail} on {formatCompareDetailsDate(detailsCommit.date)}
                </div>
                {(() => {
                  const branches = branchNamesOf(detailsCommit);
                  return branches.length > 0 ? (
                    <div className="cb-meta-branches">
                      {t().compareBranches.inBranches(branches.length, branches.join(', '))}
                    </div>
                  ) : null;
                })()}
              </>
            ) : (
              <div className="cb-meta-message muted">{t().compareBranches.noSelection}</div>
            )}
          </div>
        </div>
      </div>

      {menu && (
        <CtxMenuShell
          className="cb-filter-menu"
          style={{ top: menu.y, left: menu.x }}
          onClose={() => setMenu(null)}
        >
          {menu.kind === 'ref' && (
            <>
              <button
                className={`ctx-item ${!filter.refName ? 'selected' : ''}`}
                onClick={() => {
                  setFilter(prev => ({ ...prev, refName: '' }));
                  setMenu(null);
                }}
              >
                {t().compareBranches.allRefs}
              </button>
              {refOptions.map(name => (
                <button
                  key={name}
                  className={`ctx-item ${filter.refName === name ? 'selected' : ''}`}
                  onClick={() => {
                    setFilter(prev => ({ ...prev, refName: name }));
                    setMenu(null);
                  }}
                  title={name}
                >
                  {name}
                </button>
              ))}
            </>
          )}
          {menu.kind === 'author' && (
            <>
              <button
                className={`ctx-item ${!filter.author ? 'selected' : ''}`}
                onClick={() => {
                  setFilter(prev => ({ ...prev, author: '' }));
                  setMenu(null);
                }}
              >
                {t().gitPanel.allAuthors}
              </button>
              {authors.map(name => (
                <button
                  key={name}
                  className={`ctx-item ${filter.author === name ? 'selected' : ''}`}
                  onClick={() => {
                    setFilter(prev => ({ ...prev, author: name }));
                    setMenu(null);
                  }}
                >
                  {name}
                </button>
              ))}
            </>
          )}
          {menu.kind === 'date' && (
            <>
              {([
                ['all', t().gitPanel.dateAll],
                ['today', t().gitPanel.dateToday],
                ['week', t().gitPanel.dateLast7Days],
                ['month', t().gitPanel.dateLast30Days]
              ] as [CompareDateFilter, string][]).map(([id, label]) => (
                <button
                  key={id}
                  className={`ctx-item ${filter.date === id ? 'selected' : ''}`}
                  onClick={() => {
                    setFilter(prev => ({ ...prev, date: id }));
                    setMenu(null);
                  }}
                >
                  {label}
                </button>
              ))}
            </>
          )}
          {menu.kind === 'path' && (
            <div className="cb-path-form">
              <input
                className="cb-path-input"
                value={pathDraft}
                autoFocus
                placeholder={t().gitPanel.pathPlaceholder}
                onChange={e => setPathDraft(e.target.value)}
                onKeyDown={e => {
                  if (e.key === 'Enter') {
                    onPathFilterChange(pathDraft.trim());
                    setMenu(null);
                  }
                }}
              />
              <div className="cb-path-actions">
                <button
                  className="cb-path-btn primary"
                  onClick={() => {
                    onPathFilterChange(pathDraft.trim());
                    setMenu(null);
                  }}
                >
                  {t().gitPanel.apply}
                </button>
                <button
                  className="cb-path-btn"
                  onClick={() => {
                    setPathDraft('');
                    onPathFilterChange('');
                    setMenu(null);
                  }}
                >
                  {t().gitPanel.clear}
                </button>
              </div>
            </div>
          )}
        </CtxMenuShell>
      )}
    </section>
  );
};

/**
 * Compare Branches 顶层：负责两路数据加载、上下分栏与 Swap。
 *
 * 上下比例与详情宽度记忆在 localStorage；两栏的过滤状态在各自 Pane 内部，
 * 互不串扰（顶栏搜索不应该把底栏日志也过滤掉）。
 */
export const CompareBranchesView: React.FC<CompareBranchesViewProps> = ({
  repoPath,
  repoName,
  leftRef,
  rightRef,
  onSwap,
  onOpenCommitDiff,
  onOpenCompareDiff
}) => {
  useLanguageRefresh();

  const [data, setData] = useState<GitCompareBranchesResult | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [leftPath, setLeftPath] = useState('');
  const [rightPath, setRightPath] = useState('');
  const [reloadKey, setReloadKey] = useState(0);
  const [split, setSplit] = useState(() => readNumber(SPLIT_KEY, 40, 20, 80));
  const [detailsPct, setDetailsPct] = useState(() => readNumber(DETAILS_WIDTH_KEY, 36, 24, 62));
  const [loadingMore, setLoadingMore] = useState<{ left: boolean; right: boolean }>({
    left: false,
    right: false
  });
  const [loadMoreNotice, setLoadMoreNotice] = useState<{ left: string | null; right: string | null }>({
    left: null,
    right: null
  });

  const containerRef = useRef<HTMLDivElement>(null);
  const requestSeq = useRef(0);

  // 引用变了（Swap / 重新打开）就把旧数据清掉，避免短暂把左右日志画反。
  useEffect(() => {
    setData(null);
    setError(null);
    setLeftPath('');
    setRightPath('');
    setLoadMoreNotice({ left: null, right: null });
  }, [repoPath, leftRef, rightRef]);

  useEffect(() => {
    const seq = ++requestSeq.current;
    setLoading(true);
    setError(null);
    invokeCommand<GitCompareBranchesResult>('git_compare_branches', {
      repoPath,
      leftRef,
      rightRef,
      maxCount: INITIAL_LIMIT,
      leftPath: leftPath || null,
      rightPath: rightPath || null
    })
      .then(result => {
        if (seq !== requestSeq.current) return;
        setData(result);
      })
      .catch(err => {
        if (seq !== requestSeq.current) return;
        setError(err instanceof Error ? err.message : String(err));
      })
      .finally(() => {
        if (seq === requestSeq.current) setLoading(false);
      });
    return () => {
      requestSeq.current += 1;
    };
  }, [repoPath, leftRef, rightRef, leftPath, rightPath, reloadKey]);

  const refresh = useCallback(() => setReloadKey(value => value + 1), []);
  const changeDetailsPct = useCallback((value: number) => {
    setDetailsPct(value);
    writeNumber(DETAILS_WIDTH_KEY, value);
  }, []);

  const startSplitResize = (e: React.PointerEvent<HTMLDivElement>) => {
    const container = containerRef.current;
    if (!container) return;
    const rect = container.getBoundingClientRect();
    let latest = split;
    const finish = () => writeNumber(SPLIT_KEY, latest);
    startPointerDrag(e, 'row-resize', {
      onMove: (_x, y) => {
        latest = Math.min(80, Math.max(20, ((y - rect.top) / rect.height) * 100));
        setSplit(latest);
      },
      onEnd: finish,
      onCancel: finish
    });
  };

  const loadMore = useCallback(
    async (side: 'left' | 'right') => {
      if (!data) return;
      const current = side === 'left' ? data.leftCommits : data.rightCommits;
      const target = current.length + LOAD_MORE_SIZE;
      const include = side === 'left' ? leftRef : rightRef;
      const exclude = side === 'left' ? rightRef : leftRef;
      const path = side === 'left' ? leftPath : rightPath;

      setLoadingMore(prev => ({ ...prev, [side]: true }));
      setLoadMoreNotice(prev => ({ ...prev, [side]: null }));
      try {
        const commits = await invokeCommand<GitCommit[]>('git_log', {
          repoPath,
          maxCount: target,
          skip: 0,
          allRefs: false,
          branch: `${exclude}..${include}`,
          path: path || null,
          author: null,
          since: null,
          topoOrder: true,
          firstParent: false,
          noMerges: false
        });
        const normalized = (Array.isArray(commits) ? commits : []).map(commit => ({
          ...commit,
          inHead: undefined
        }));
        const stillTruncated = normalized.length >= target;
        setData(prev => {
          if (!prev) return prev;
          return side === 'left'
            ? { ...prev, leftCommits: normalized, leftTruncated: stillTruncated }
            : { ...prev, rightCommits: normalized, rightTruncated: stillTruncated };
        });
      } catch (err) {
        const message = err instanceof Error ? err.message : String(err);
        setLoadMoreNotice(prev => ({ ...prev, [side]: message }));
      } finally {
        setLoadingMore(prev => ({ ...prev, [side]: false }));
      }
    },
    [data, leftRef, rightRef, leftPath, rightPath, repoPath]
  );

  const initialLoading = loading && !data;

  if (error && !data) {
    return (
      <div className="compare-branches-view cb-state">
        <div className="cb-state-title">{t().compareBranches.loadFailed}</div>
        <div className="cb-state-error">{error}</div>
        <button type="button" className="cb-retry-btn" onClick={refresh}>
          {t().compareBranches.retry}
        </button>
      </div>
    );
  }

  if (initialLoading || !data) {
    return (
      <div className="compare-branches-view cb-state">
        <div className="editor-lazy-spinner" />
        <div className="editor-lazy-label">{t().compareBranches.loading}</div>
      </div>
    );
  }

  return (
    <div
      className="compare-branches-view"
      ref={containerRef}
      style={{ gridTemplateRows: `${split}fr 8px ${100 - split}fr` }}
    >
      {error && <div className="cb-global-error">{error}</div>}
      <CompareBranchPane
        key={`left-${leftRef}-${rightRef}`}
        repoPath={repoPath}
        repoName={repoName}
        side="left"
        refName={leftRef}
        otherRef={rightRef}
        mergeBase={data.mergeBase}
        commits={data.leftCommits}
        allFiles={data.leftFiles}
        truncated={data.leftTruncated}
        loadingMore={loadingMore.left}
        notice={loadMoreNotice.left}
        pathFilter={leftPath}
        onPathFilterChange={setLeftPath}
        onRefresh={refresh}
        onLoadMore={() => void loadMore('left')}
        detailsPct={detailsPct}
        onDetailsPctChange={changeDetailsPct}
        showSwap
        onSwap={onSwap}
        onOpenCommitDiff={onOpenCommitDiff}
        onOpenCompareDiff={onOpenCompareDiff}
      />
      <div className="cb-splitter" onPointerDown={startSplitResize} title={t().compareBranches.resizeSplit} />
      <CompareBranchPane
        key={`right-${leftRef}-${rightRef}`}
        repoPath={repoPath}
        repoName={repoName}
        side="right"
        refName={rightRef}
        otherRef={leftRef}
        mergeBase={data.mergeBase}
        commits={data.rightCommits}
        allFiles={data.rightFiles}
        truncated={data.rightTruncated}
        loadingMore={loadingMore.right}
        notice={loadMoreNotice.right}
        pathFilter={rightPath}
        onPathFilterChange={setRightPath}
        onRefresh={refresh}
        onLoadMore={() => void loadMore('right')}
        detailsPct={detailsPct}
        onDetailsPctChange={changeDetailsPct}
        showSwap={false}
        onSwap={onSwap}
        onOpenCommitDiff={onOpenCommitDiff}
        onOpenCompareDiff={onOpenCompareDiff}
      />
    </div>
  );
};
