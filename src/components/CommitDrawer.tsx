// Commit 工具窗口 · 对齐 PyCharm Commit / Shelf 双页签
//
// 页签：Commit（提交）/ Shelf（贮藏架），工具栏对齐 PyCharm：
//   Commit：刷新 / Rollback / Shelve / 按文件夹结构查看 / 展开所有 / 折叠所有
//   Shelf ：Unshelve / 按文件夹结构查看 / 展开所有 / 折叠所有
// 变更列表按本地 changelist 分组（默认 Changes + 自定义列表，存 .git/ide-changelists.json），
// 未跟踪文件固定在 Unversioned Files 组；「按文件夹结构查看」切换为目录树。
// Shelve 作用于勾选文件；Unshelve 勾选文件（或整组），目标 changelist 可选/可新建。

import React, { useState, useMemo, useCallback, useEffect, useRef } from 'react';
import { save as saveDialog } from '@tauri-apps/plugin-dialog';
import { appConfirm } from '../services/confirmDialog';
import type { GitChangedFile, Shelf } from '../types';
import { invokeCommand } from '../services/tauriBridge';
import { getCommitDraft, saveCommitDraft } from '../services/commitDraft';
import { t } from '../services/i18n';
import { errText } from '../services/errorText';
import { useLanguageRefresh } from '../hooks/useLanguageRefresh';
import { CtxMenuShell } from './CtxMenuShell';
import {
  renderFileIcon,
  IconRefresh,
  IconRollback,
  IconViewOptions,
  IconMoreVert,
  IconShelveIn,
  IconShelveOut,
  IconExpandAll,
  IconCollapseAll,
  IconHistory,
  IconWindowMinimize,
  IconWindowClose,
  IconChevronDown,
  IconChevronRight,
  IconDiffHorizontal,
  IconDiffTab,
  IconCreatePatch
} from './FileIcons';

interface CommitDrawerProps {
  changedFiles: GitChangedFile[];
  selectedFile: string;
  onSelectFileForDiff: (filePath: string, staged?: boolean) => void;
  /** 提交；第三个参数为勾选的文件路径列表 */
  onCommit: (message: string, isAmend: boolean, paths: string[]) => boolean | void | Promise<boolean | void>;
  onCommitAndPush: (message: string, isAmend: boolean, paths: string[]) => boolean | void | Promise<boolean | void>;
  /** 回滚单个文件的未提交更改 */
  onDiscardFile?: (filePath: string) => void;
  /** 将未跟踪文件加入版本控制（git add，对齐 PyCharm Add to VCS） */
  onAddToVcs?: (paths: string[]) => void;
  /** 删除未跟踪文件（移入系统废纸篓，对齐 PyCharm 对 Unversioned 的 Delete） */
  onDeleteUntracked?: (paths: string[]) => void;
  /** 仅提交这一个文件（提示输入信息后提交） */
  onCommitFile?: (path: string) => void;
  /** Commit Files...：对当前选择一次性提交（App 负责提示提交信息）。 */
  onCommitPaths?: (paths: string[]) => void;
  /** 在编辑器中打开该文件（Jump to Source） */
  onJumpToSource?: (path: string) => void;
  /** Compare Before with Local：该文件 HEAD 版本 ↔ 工作区 */
  onCompareBeforeLocal?: (path: string) => void;
  /** History Up to Here：打开该文件历史 */
  onFileHistory?: (path: string) => void;
  /** 该文件本地更改的补丁 → 剪贴板 / 另存 */
  onCopyLocalPatch?: (path: string) => void;
  onCreateLocalPatch?: (path: string) => void;
  /** Shelve Changes...：贮藏勾选的文件（对齐 PyCharm，可多选） */
  onShelve?: (paths: string[]) => void;
  /** 变更列表名集合（首项为默认 Changes） */
  changelists?: string[];
  /** 路径 → changelist 名（缺省归 Changes） */
  fileListMap?: Record<string, string>;
  onCreateChangelist?: (name: string) => void;
  /** 删除 changelist；rollback=true 还原其下文件，否则移回 Changes */
  onDeleteChangelist?: (name: string, rollback: boolean) => void;
  /** 移动文件到指定 changelist */
  onAssignToChangelist?: (paths: string[], list: string) => void;
  /** Unshelve：应用贮藏的（勾选）文件并放入目标 changelist */
  onUnshelve?: (shelf: string, paths: string[], target: string) => void;
  /** 仓库根目录绝对路径（Shelf 页读写 .git/ide-shelves） */
  repoPath?: string;
  /** Shelf 页操作完成后的回调（提示 + 刷新 Git 状态） */
  onShelfChanged?: (message: string) => void;
  /** 合并冲突文件数，>0 显示冲突横幅（解决入口在顶栏绿色 » 按钮） */
  conflictCount?: number;
  /** 解决冲突入口（打开冲突解决对话框） */
  onResolveConflicts?: () => void;
  /** 搁置条目差异预览（Show Diff / Compare with Local） */
  onShelfDiff?: (shelfName: string, filePath: string, inNewTab?: boolean, compareWithLocal?: boolean) => void;
  busy?: boolean;
  /** refreshGit 在途：刷新按钮转圈，空列表时显示加载中而非「工作区干净」 */
  refreshing?: boolean;
  onRefresh?: () => void;
  onClose?: () => void;
  /** 面板宽度（可拖拽调节，由 App 持久化） */
  width?: number;
  /** 当前目录不是 git 仓库时为 false：显示提示并禁用提交 */
  isRepo?: boolean;
  /** 本机真实用过的提交信息（来自 localStorage，由提交动作记录） */
  recentMessages?: string[];
  /** 清除本机提交信息历史（隐私：message 里可能粘贴过 token） */
  onClearHistory?: () => void;
  /**
   * 通用文本输入（App 的 appPrompt bridge：window.prompt 在 wry 里直接回 null）。
   * 可选：不传时新建 changelist 入口静默不可用（绝不回退到 window.prompt）。
   */
  onPromptName?: (message: string, defaultValue?: string) => Promise<string | null>;
}

/** 文件行/分组头上的右键菜单状态 */
interface DrawerMenuState {
  x: number;
  y: number;
  /** 受影响的文件路径列表 */
  paths: string[];
  /** true = 未跟踪文件（提供 Add to VCS / Delete） */
  untracked: boolean;
  /** 右键发生在该 changelist 的分组头上（提供 Delete Changelist） */
  listName?: string;
  /** 非空时表示右键的是 Shelf 页的某个贮藏条目 */
  shelfName?: string;
  shelfPath?: string;
  isDeletedShelf?: boolean;
  isGroup?: boolean;
  /** 右键文件在索引中的那份改动（Show Diff 用） */
  staged?: boolean;
}

const DEFAULT_LIST = 'Changes';

/** 归一化比较：变更清单是仓库相对路径，diff 导航传来的是绝对路径，二者需转绝对后再比 */
const isSamePath = (a: string, b: string, repo?: string): boolean => {
  if (!a || !b) return false;
  if (a === b) return true;
  const toAbs = (p: string) =>
    !p || /^([A-Za-z]:[\\/]|\/)/.test(p) || !repo ? p : `${repo.replace(/[\\/]+$/, '')}/${p}`;
  return toAbs(a) === toAbs(b);
};

const dirHintOf = (path: string): string => {
  const parts = path.split(/[\\/]/);
  parts.pop();
  return parts.join('/');
};

const statusClassOf = (status: GitChangedFile['status']): string => {
  if (status === 'A') return 'status-added';
  if (status === 'D') return 'status-deleted';
  if (status === 'C') return 'status-conflict';
  if (status === 'U') return 'status-unversioned';
  return 'status-modified';
};

// ============ 目录树构建（按文件夹结构查看） ============

interface DirNode<T> {
  seg: string;
  /** 从仓库根起的目录路径，作为折叠状态键 */
  path: string;
  sub: DirNode<T>[];
  files: T[];
  totalFileCount: number;
}

/** 按路径的分隔段建项目目录树（对齐 PyCharm Image #2 规范） */
function buildProjectDirTree<T extends { path: string }>(files: T[], repoName?: string): DirNode<T>[] {
  if (files.length === 0) return [];
  const hasRootFiles = files.some(f => !f.path.includes('/') && !f.path.includes('\\'));
  const effectiveFiles = hasRootFiles && repoName
    ? files.map(f => ({ ...f, _displayPath: `${repoName}/${f.path}` }))
    : files.map(f => ({ ...f, _displayPath: f.path }));

  const root: DirNode<T> = { seg: '', path: '', sub: [], files: [], totalFileCount: 0 };
  for (const f of effectiveFiles) {
    const segs = f._displayPath.split(/[\\/]/);
    segs.pop();
    let cur = root;
    let acc = '';
    for (const s of segs) {
      acc = acc ? `${acc}/${s}` : s;
      let next = cur.sub.find(n => n.seg === s);
      if (!next) {
        next = { seg: s, path: acc, sub: [], files: [], totalFileCount: 0 };
        cur.sub.push(next);
      }
      cur = next;
    }
    cur.files.push(f);
  }

  const calcAndSort = (n: DirNode<T>): number => {
    let count = n.files.length;
    for (const s of n.sub) {
      count += calcAndSort(s);
    }
    n.totalFileCount = count;
    n.sub.sort((a, b) => a.seg.localeCompare(b.seg));
    n.files.sort((a, b) => a.path.localeCompare(b.path));
    return count;
  };
  calcAndSort(root);

  return compactDirNodes(root.sub);
}

/** 压缩空单子目录（Compact Empty Middle Packages：对齐 PyCharm 目录树规范） */
function compactDirNodes<T extends { path: string }>(nodes: DirNode<T>[]): DirNode<T>[] {
  return nodes.map(node => {
    let compactedSub = compactDirNodes(node.sub);
    let cur: DirNode<T> = { ...node, sub: compactedSub };
    while (cur.files.length === 0 && cur.sub.length === 1) {
      const only = cur.sub[0];
      cur = {
        seg: `${cur.seg}/${only.seg}`,
        path: only.path,
        sub: only.sub,
        files: only.files,
        totalFileCount: only.totalFileCount
      };
    }
    return cur;
  });
}

function collectAllDirFilePaths<T extends { path: string }>(node: DirNode<T>): string[] {
  const result: string[] = node.files.map(f => f.path);
  for (const s of node.sub) {
    result.push(...collectAllDirFilePaths(s));
  }
  return result;
}

function buildDirTree<T extends { path: string }>(files: T[], repoName?: string): DirNode<T> {
  const nodes = buildProjectDirTree(files, repoName);
  return { seg: '', path: '', sub: nodes, files: [], totalFileCount: files.length };
}
/** 收集树里全部目录路径（折叠所有用） */
const collectDirPaths = <T,>(node: DirNode<T>): string[] => {
  const acc: string[] = [];
  const walk = (n: DirNode<T>) => {
    n.sub.forEach(d => {
      acc.push(d.path);
      walk(d);
    });
  };
  walk(node);
  return acc;
};

/** 目录折叠状态的作用域键，避免不同分组（如 Merge Conflicts、Changes、Unversioned）同名目录相互影响 */
const dirScopeKey = (scope: string, dirPath: string) => (scope ? `${scope}:${dirPath}` : dirPath);

// ============ 「按目录查看」开关的持久化：重启 app 后保持上次选择 ============
const COMMIT_TREE_VIEW_KEY = 'ide_commit_tree_view';
const SHELF_TREE_VIEW_KEY = 'ide_shelf_tree_view';
const readViewFlag = (key: string): boolean => {
  try {
    return localStorage.getItem(key) === '1';
  } catch {
    // 隐私模式下读取失败：回到默认平铺视图
    return false;
  }
};

// ============ 文件行（平铺分组 / 目录树 / Shelf 清单共用） ============

interface FileRowProps {
  /** 文件完整路径（渲染名取末段） */
  path: string;
  /** 状态色：不传则文件名用普通色（Shelf 清单） */
  status?: GitChangedFile['status'];
  showCheckbox?: boolean;
  checked?: boolean;
  selected?: boolean;
  onToggle?: (e: React.MouseEvent) => void;
  onSelect?: (e: React.MouseEvent) => void;
  onDoubleClick?: () => void;
  onRollback?: () => void;
  onMenu?: (e: React.MouseEvent) => void;
  depth?: number;
  /** 平铺行显示右侧目录提示；目录树内由目录行表达层级，不再重复 */
  showHint?: boolean;
  inTree?: boolean;
}
const paddingForDepth = (depth = 0, inTree = false, showCheckbox = true): number => {
  if (inTree) {
    // 树模式下的文件行：无 chevron(12px + 4px gap)，同层级目录行对齐
    return 6 + depth * 14 + 16;
  }
  // 平铺模式：常规变更文件 depth=1 -> 36px；冲突文件 depth=2 -> 50px
  if (depth === 2) return 50;
  if (!showCheckbox) return 22;
  return 36;
};


const FileRow: React.FC<FileRowProps> = ({
  path,
  status,
  showCheckbox = false,
  checked = false,
  selected = false,
  onToggle,
  onSelect,
  onDoubleClick,
  onRollback,
  onMenu,
  depth,
  inTree = false,
  showHint = true
}) => {
  const name = path.split(/[\\/]/).pop() ?? path;
  const hint = showHint ? dirHintOf(path) : '';
  const nameCls = status ? statusClassOf(status) : '';
  return (
    <div
      className={`change-file-row ${selected ? 'selected' : ''}`}
      data-path={path}
      style={{ paddingLeft: `${paddingForDepth(depth, inTree, showCheckbox)}px` }}
      onClick={onSelect}
      onDoubleClick={onDoubleClick}
      onContextMenu={onMenu}
      title={onSelect ? t().commitDrawer.fileRowHint(path) : t().commitDrawer.fileRowHintPlain(path)}
    >
      {showCheckbox && (
        <input
          type="checkbox"
          className="pycharm-checkbox"
          checked={checked}
          onChange={() => {}}
          onClick={e => onToggle?.(e)}
        />
      )}
      <span className="git-file-icon">{renderFileIcon(name, false, false)}</span>
      <span className={`git-file-name ${nameCls}`}>{name}</span>
      {hint && <span className="file-dir-hint">{hint}</span>}
      {onRollback && (
        <button
          className="file-rollback-btn"
          title={t().commitDrawer.rollbackFileTooltip}
          onClick={e => {
            e.stopPropagation();
            onRollback();
          }}
        >
          <IconRollback size={12} />
        </button>
      )}
    </div>
  );
};

interface CommitGroupProps {
  title: string;
  /** 该组所属 changelist 名（分组头右键提供 Delete Changelist） */
  listName?: string;
  files: GitChangedFile[];
  unversioned?: boolean;
  isIgnored?: boolean;
  collapsed: boolean;
  repoName?: string;
  isSelectedPath: (path: string) => boolean;
  isCheckedPath: (path: string) => boolean;
  onToggleCollapse: () => void;
  onToggleGroup: (e: React.MouseEvent) => void;
  onToggleFile: (path: string, e?: React.MouseEvent) => void;
  onToggleDirFiles?: (paths: string[], check: boolean) => void;
  onSelectFile: (path: string, e: React.MouseEvent) => void;
  onSelectForDiff: (path: string, staged?: boolean) => void;
  onFileMenu: (
    path: string,
    e: React.MouseEvent,
    options: { untracked: boolean; staged?: boolean }
  ) => void;
  onRollback?: (path: string) => void;
  /** 文件行/分组头右键菜单 */
  onMenu?: (menu: DrawerMenuState) => void;
  conflictFiles?: GitChangedFile[];
  onResolveConflicts?: () => void;
  treeView?: boolean;
  collapsedDirs?: Record<string, boolean>;
  onToggleDir?: (dirPath: string) => void;
}

/** 一个 changelist 分组：标题行（chevron + 三态勾选框 + 名称 + 数量）+ 扁平文件列表 */
const CommitGroup: React.FC<CommitGroupProps> = ({
  title,
  listName,
  files,
  unversioned = false,
  isIgnored = false,
  collapsed,
  repoName,
  isSelectedPath,
  isCheckedPath,
  onToggleCollapse,
  onToggleGroup,
  onToggleFile,
  onToggleDirFiles,
  onSelectFile,
  onSelectForDiff,
  onFileMenu,
  onRollback,
  onMenu,
  conflictFiles = [],
  onResolveConflicts,
  treeView = false,
  collapsedDirs = {},
  onToggleDir
}) => {
  const [conflictsCollapsed, setConflictsCollapsed] = useState(false);
  const regularFiles = useMemo(
    () => (conflictFiles.length > 0 ? files.filter(f => f.status !== 'C') : files),
    [conflictFiles, files]
  );
  const allGroupFiles = useMemo(
    () => [...conflictFiles, ...regularFiles],
    [conflictFiles, regularFiles]
  );
  const checkedCount = allGroupFiles.filter(f => isCheckedPath(f.path)).length;
  const allChecked = allGroupFiles.length > 0 && checkedCount === allGroupFiles.length;
  const indeterminate = checkedCount > 0 && checkedCount < allGroupFiles.length;

  const conflictsCheckedCount = conflictFiles.filter(f => isCheckedPath(f.path)).length;
  const allConflictsChecked = conflictFiles.length > 0 && conflictsCheckedCount === conflictFiles.length;
  const someConflictsChecked = conflictsCheckedCount > 0 && conflictsCheckedCount < conflictFiles.length;

  const conflictTreeNodes = useMemo(
    () => (treeView && conflictFiles.length > 0 ? buildProjectDirTree(conflictFiles, repoName) : []),
    [treeView, conflictFiles, repoName]
  );
  const regularTreeNodes = useMemo(
    () => (treeView && regularFiles.length > 0 ? buildProjectDirTree(regularFiles, repoName) : []),
    [treeView, regularFiles, repoName]
  );

  const handleGroupContext = (e: React.MouseEvent) => {
    if (!onMenu) return;
    e.preventDefault();
    e.stopPropagation();
    onMenu({ x: e.clientX, y: e.clientY, paths: allGroupFiles.map(f => f.path), untracked: unversioned, listName });
  };

  const renderProjectDirNode = (
    d: DirNode<GitChangedFile>,
    depth: number,
    isUntracked = false,
    isIgnoredFile = false,
    scope = ''
  ): React.ReactNode => {
    const nodeKey = dirScopeKey(scope, d.path);
    const isDirCollapsed = !!collapsedDirs[nodeKey];
    const dirFilePaths = collectAllDirFilePaths(d);
    const allDirChecked = !isIgnoredFile && dirFilePaths.length > 0 && dirFilePaths.every(p => isCheckedPath(p));
    const someDirChecked = !isIgnoredFile && !allDirChecked && dirFilePaths.some(p => isCheckedPath(p));

    const handleToggleDir = (e: React.MouseEvent) => {
      e.stopPropagation();
      onToggleDirFiles?.(dirFilePaths, !allDirChecked);
    };

    return (
      <div key={`dir:${nodeKey}`}>
        <div
          className="commit-dir-row"
          style={{ paddingLeft: `${6 + depth * 14}px` }}
          onClick={() => onToggleDir?.(nodeKey)}
          title={d.path}
        >
          <span className={`tree-chevron ${isDirCollapsed ? 'collapsed' : 'expanded'}`}>
            {isDirCollapsed ? <IconChevronRight size={11} color="var(--text-muted)" /> : <IconChevronDown size={11} color="var(--text-muted)" />}
          </span>
          {!isIgnoredFile && (
            <input
              type="checkbox"
              className="pycharm-checkbox"
              checked={allDirChecked}
              ref={el => {
                if (el) el.indeterminate = someDirChecked;
              }}
              onChange={() => {}}
              onClick={handleToggleDir}
            />
          )}
          <span className="git-file-icon">{renderFileIcon(d.seg, true, !isDirCollapsed)}</span>
          <span className="git-file-name">{d.seg}</span>
          <span className="commit-dir-count">
            {d.totalFileCount} {d.totalFileCount === 1 ? 'file' : 'files'}
          </span>
        </div>
        {!isDirCollapsed && (
          <div>
            {d.sub.map(s => renderProjectDirNode(s, depth + 1, isUntracked, isIgnoredFile, scope))}
            {d.files.map(f => (
              <FileRow
                key={f.path}
                path={f.path}
                status={f.status}
                showCheckbox={!isIgnoredFile}
                checked={isCheckedPath(f.path)}
                selected={isSelectedPath(f.path)}
                depth={depth + 1}
                inTree={true}
                showHint={false}
                onToggle={e => onToggleFile(f.path, e)}
                onSelect={e => onSelectFile(f.path, e)}
                onDoubleClick={() => onSelectForDiff(f.path, f.isStaged && !f.isUnstaged)}
                onRollback={onRollback && !isUntracked && !isIgnoredFile ? () => onRollback(f.path) : undefined}
                onMenu={e =>
                  onFileMenu(f.path, e, {
                    untracked: isUntracked,
                    staged: f.isStaged && !f.isUnstaged
                  })
                }
              />
            ))}
          </div>
        )}
      </div>
    );
  };
  return (
    <div className="commit-group">
      <div className="commit-group-row" onClick={onToggleCollapse} onContextMenu={handleGroupContext}>
        <span className={`tree-chevron ${collapsed ? 'collapsed' : 'expanded'}`}>
          {collapsed ? <IconChevronRight size={11} color="var(--text-muted)" /> : <IconChevronDown size={11} color="var(--text-muted)" />}
        </span>
        {!isIgnored && (
          <input
            type="checkbox"
            className="pycharm-checkbox"
            checked={allChecked}
            ref={el => {
              if (el) el.indeterminate = indeterminate;
            }}
            onChange={() => {}}
            onClick={onToggleGroup}
          />
        )}
        <span className="commit-group-name">{title}</span>
        <span className="commit-group-count">{t().commitDrawer.fileCount(allGroupFiles.length)}</span>
      </div>

      {!collapsed && (
        <div className="commit-group-files">
          {conflictFiles.length > 0 && (
            <div className="commit-conflict-subgroup">
              <div
                className="commit-conflict-row"
                onClick={() => setConflictsCollapsed(v => !v)}
              >
                <span className={`tree-chevron ${conflictsCollapsed ? 'collapsed' : 'expanded'}`}>
                  {conflictsCollapsed ? <IconChevronRight size={11} color="var(--text-muted)" /> : <IconChevronDown size={11} color="var(--text-muted)" />}
                </span>
                <input
                  type="checkbox"
                  className="pycharm-checkbox"
                  checked={allConflictsChecked}
                  ref={el => {
                    if (el) el.indeterminate = someConflictsChecked;
                  }}
                  onChange={() => {}}
                  onClick={e => {
                    e.stopPropagation();
                    const target = !allConflictsChecked;
                    for (const f of conflictFiles) {
                      if (isCheckedPath(f.path) !== target) onToggleFile(f.path);
                    }
                  }}
                />
                <span className="commit-conflict-title">{t().commitDrawer.mergeConflictsTitle}</span>
                {onResolveConflicts && (
                  <button
                    type="button"
                    className="commit-resolve-link"
                    onClick={e => {
                      e.stopPropagation();
                      onResolveConflicts();
                    }}
                    title={t().commitDrawer.resolveConflicts}
                  >
                    {t().commitDrawer.resolveLink}
                  </button>
                )}
              </div>
              {!conflictsCollapsed && (
                <div className="commit-conflict-files">
                  {treeView
                    ? conflictTreeNodes.map(d => renderProjectDirNode(d, 2, false, false, 'conflicts'))
                    : conflictFiles.map(f => (
                        <FileRow
                          key={f.path}
                          path={f.path}
                          status={f.status}
                          showCheckbox
                          checked={isCheckedPath(f.path)}
                          selected={isSelectedPath(f.path)}
                          onToggle={e => onToggleFile(f.path, e)}
                          onSelect={e => onSelectFile(f.path, e)}
                          onDoubleClick={() => onSelectForDiff(f.path, f.isStaged && !f.isUnstaged)}
                          onRollback={onRollback ? () => onRollback(f.path) : undefined}
                          onMenu={e =>
                            onFileMenu(f.path, e, {
                              untracked: false,
                              staged: false
                            })
                          }
                          depth={2}
                        />
                      ))}
                </div>
              )}
            </div>
          )}

          {treeView
            ? regularTreeNodes.map(d => renderProjectDirNode(d, 1, unversioned, isIgnored, listName || title))
            : regularFiles.map(f => (
                <FileRow
                  key={f.path}
                  path={f.path}
                  status={f.status}
                  showCheckbox={!isIgnored}
                  checked={isCheckedPath(f.path)}
                  selected={isSelectedPath(f.path)}
                  onToggle={e => onToggleFile(f.path, e)}
                  onSelect={e => onSelectFile(f.path, e)}
                  onDoubleClick={() => onSelectForDiff(f.path, f.isStaged && !f.isUnstaged)}
                  onRollback={onRollback && !unversioned && !isIgnored ? () => onRollback(f.path) : undefined}
                  onMenu={e =>
                    onFileMenu(f.path, e, {
                      untracked: unversioned,
                      staged: f.isStaged && !f.isUnstaged
                    })
                  }
                  depth={1}
                />
              ))}
        </div>
      )}
    </div>
  );
};

// ============ Shelf 贮藏架条目（平铺视图，支持勾选文件） ============

interface ShelfGroupProps {
  shelf: Shelf;
  selected: boolean;
  collapsed: boolean;
  onSelect: () => void;
  onToggleCollapse: () => void;
  onMenu: (e: React.MouseEvent) => void;
  onFileMenu: (path: string, e: React.MouseEvent) => void;
  onSelectFile: (path: string, e?: React.MouseEvent) => void;
  selectedPaths: string[];
  onDoubleClickFile?: (path: string) => void;
}

const ShelfGroup: React.FC<ShelfGroupProps> = ({
  shelf,
  selected,
  collapsed,
  onSelect,
  onToggleCollapse,
  onMenu,
  onFileMenu,
  onSelectFile,
  selectedPaths,
  onDoubleClickFile,
}) => {
  return (
    <div className="commit-group">
      <div
        className={`commit-group-row ${selected ? 'selected' : ''}`}
        onClick={onSelect}
        onContextMenu={onMenu}
        title={t().commitDrawer.shelfGroupHint(shelf.name)}
      >
        <span
          className={`tree-chevron ${collapsed ? 'collapsed' : 'expanded'}`}
          onClick={e => {
            e.stopPropagation();
            onToggleCollapse();
          }}
        >
          {collapsed ? <IconChevronRight size={11} color="var(--text-muted)" /> : <IconChevronDown size={11} color="var(--text-muted)" />}
        </span>
        <span className="commit-group-name">{shelf.name}</span>
        <span className="commit-group-count">
          {t().commitDrawer.shelfCount(shelf.files.length, shelf.date ?? '')}
        </span>
      </div>
      {!collapsed && (
        <div className="commit-group-files">
          {shelf.files.map(p => (
            <FileRow
              key={p}
              path={p}
              showCheckbox={false}
              showHint={true}
              inTree={false}
              depth={1}
              selected={selectedPaths.includes(p)}
              onSelect={e => onSelectFile(p, e)}
              onDoubleClick={() => onDoubleClickFile?.(p)}
              onMenu={e => {
                e.preventDefault();
                e.stopPropagation();
                onFileMenu(p, e);
              }}
            />
          ))}
        </div>
      )}
    </div>
  );
};

// ============ 主组件 ============

/**
 * memo：App 是无 memo 的单壳层，终端输出 / 编辑器击键都会重渲染整棵树；
 * 抽屉里的 changelist 分组与目录树与这些高频 state 无关。
 * 生效前提：App 侧回调保持稳定引用（见交付说明）。
 */
export const CommitDrawer = React.memo(function CommitDrawer({
  changedFiles,
  selectedFile,
  onSelectFileForDiff,
  onCommit,
  onCommitAndPush,
  onDiscardFile,
  onAddToVcs,
  onDeleteUntracked,
  onCommitFile,
  onCommitPaths,
  onJumpToSource,
  onCompareBeforeLocal,
  onFileHistory,
  onCopyLocalPatch,
  onCreateLocalPatch,
  onShelve,
  changelists = ['Changes'],
  fileListMap = {},
  onCreateChangelist,
  onDeleteChangelist,
  onAssignToChangelist,
  onUnshelve,
  repoPath,
  onShelfChanged,
  conflictCount: _conflictCount = 0,
  onResolveConflicts,
  onShelfDiff,
  busy = false,
  refreshing = false,
  onRefresh,
  onClose,
  width,
  isRepo,
  recentMessages = [],
  onClearHistory,
  onPromptName
}: CommitDrawerProps) {
  useLanguageRefresh();
  const [tab, setTab] = useState<'commit' | 'shelf'>('commit');
  const [commitMessage, setCommitMessage] = useState(() => (repoPath ? getCommitDraft(repoPath) : ''));

  // 抽屉卸载后草稿仍在模块表里；切回仓库/重新打开时恢复。
  // 子目录项目会先以 workspacePath 挂载，refreshGit 后才把 repoPath 换成
  // 仓库根；此时用户可能已经输入，不能拿新 key 的空草稿覆盖，必须迁移。
  const messageRef = useRef(commitMessage);
  messageRef.current = commitMessage;
  const draftRepoRef = useRef(repoPath);
  useEffect(() => {
    const prev = draftRepoRef.current;
    draftRepoRef.current = repoPath;
    if (!repoPath || prev === repoPath) return;
    if (messageRef.current) {
      saveCommitDraft(repoPath, messageRef.current);
    } else {
      setCommitMessage(getCommitDraft(repoPath));
    }
  }, [repoPath]);

  const updateCommitMessage = useCallback(
    (value: string) => {
      setCommitMessage(value);
      if (repoPath) saveCommitDraft(repoPath, value);
    },
    [repoPath]
  );
  const [fileChecked, setFileChecked] = useState<Record<string, boolean>>({});
  /** 变更列表的行选择（与“勾选提交”独立；支持 Cmd/Ctrl 与 Shift 多选）。 */
  const [selectedPaths, setSelectedPaths] = useState<string[]>([]);
  const lastClickedFileRef = useRef<string | null>(null);
  const [collapsedGroups, setCollapsedGroups] = useState<Record<string, boolean>>({});
  const [isHistoryOpen, setIsHistoryOpen] = useState(false);
  const [menu, setMenu] = useState<DrawerMenuState | null>(null);

  // 「按文件夹结构查看」：平铺 ↔ 目录树（两个页签各自记忆，跨重启持久化）
  const [commitTreeView, setCommitTreeView] = useState(() => readViewFlag(COMMIT_TREE_VIEW_KEY));
  const [shelfTreeView, setShelfTreeView] = useState(() => readViewFlag(SHELF_TREE_VIEW_KEY));
  const [collapsedDirs, setCollapsedDirs] = useState<Record<string, boolean>>({});
  const [shelfCollapsedDirs, setShelfCollapsedDirs] = useState<Record<string, boolean>>({});
  // 持久化：用户重新打开 app 后仍按上次的查看方式展开
  useEffect(() => {
    try {
      localStorage.setItem(COMMIT_TREE_VIEW_KEY, commitTreeView ? '1' : '0');
    } catch {
      /* 隐私模式 / 配额耗尽：内存状态仍然有效 */
    }
  }, [commitTreeView]);
  useEffect(() => {
    try {
      localStorage.setItem(SHELF_TREE_VIEW_KEY, shelfTreeView ? '1' : '0');
    } catch {
      /* 隐私模式 / 配额耗尽：内存状态仍然有效 */
    }
  }, [shelfTreeView]);

  // Shelf 页状态
  const [shelves, setShelves] = useState<Shelf[]>([]);
  const [deletedShelves, setDeletedShelves] = useState<Shelf[]>([]);
  const [deletedCollapsed, setDeletedCollapsed] = useState(false);
  const [selectedShelf, setSelectedShelf] = useState<string | null>(null);
  const [selectedShelfPaths, setSelectedShelfPaths] = useState<string[]>([]);
  const [lastFocusedShelfFile, setLastFocusedShelfFile] = useState<string | null>(null);
  const [shelfActing, setShelfActing] = useState(false);
  const [shelfCollapsed, setShelfCollapsed] = useState<Record<string, boolean>>({});
  const [shelfViewMenuOpen, setShelfViewMenuOpen] = useState(false);
  const [showAlreadyUnshelved, setShowAlreadyUnshelved] = useState(true);
  const shelfViewMenuRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!shelfViewMenuOpen) return;
    const onClick = (e: MouseEvent) => {
      if (shelfViewMenuRef.current && !shelfViewMenuRef.current.contains(e.target as Node)) {
        setShelfViewMenuOpen(false);
      }
    };
    window.addEventListener('mousedown', onClick);
    return () => window.removeEventListener('mousedown', onClick);
  }, [shelfViewMenuOpen]);

  const allLists = changelists.length > 0 ? changelists : ['Changes'];

  // Shelf 专用全局快捷键（⌥⌘U 静默应用 / ⇧⌘U 弹窗 / ⌃P 切换目录树 / ⌘D 查差异 / F2 重命名 / ⌫ 删除）
  useEffect(() => {
    if (tab !== 'shelf') return;
    const onKey = (e: KeyboardEvent) => {
      if (e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement) return;

      // ⌥⌘U: Unshelve Silently (Image #3)
      if (e.altKey && (e.metaKey || e.ctrlKey) && (e.key === 'u' || e.key === 'U')) {
        e.preventDefault();
        handleUnshelveSilently();
        return;
      }
      // ⇧⌘U: Unshelve... (Image #1)
      if (e.shiftKey && (e.metaKey || e.ctrlKey) && (e.key === 'u' || e.key === 'U')) {
        e.preventDefault();
        if (selectedShelf) {
          const s = shelves.find(item => item.name === selectedShelf);
          if (s) openUnshelveDialog(s, selectedShelfPaths.length > 0 ? selectedShelfPaths : undefined);
        }
        return;
      }
      // ⌃P: Toggle Directory View (Image #4)
      if (tab === 'shelf' && e.ctrlKey && !e.metaKey && (e.key === 'p' || e.key === 'P')) {
        if ((document.activeElement as HTMLElement | null)?.closest('#pycharmBottomGit')) return;
        e.preventDefault();
        setShelfTreeView(v => !v);
        return;
      }
      // ⌘D: Show Diff (Image #1)
      if ((e.metaKey || e.ctrlKey) && !e.shiftKey && (e.key === 'd' || e.key === 'D')) {
        if (selectedShelf && selectedShelfPaths.length > 0) {
          e.preventDefault();
          onShelfDiff?.(selectedShelf, selectedShelfPaths[0], false, false);
          return;
        }
      }
      // F2: Rename shelf
      if (e.key === 'F2') {
        if (selectedShelf && selectedShelfPaths.length === 0) {
          e.preventDefault();
          void handleShelfRename(selectedShelf);
          return;
        }
      }
      // Backspace: Delete
      if (e.key === 'Backspace' && !e.metaKey && !e.ctrlKey) {
        if (selectedShelf) {
          e.preventDefault();
          if (selectedShelfPaths.length > 0) {
            void handleShelfDeleteFiles(selectedShelf, selectedShelfPaths);
          } else {
            void handleShelfDelete(selectedShelf, false);
          }
        }
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [tab, selectedShelf, selectedShelfPaths, shelves, allLists]);
  // 删除 changelist 确认弹窗（移回 Changes / Rollback）
  const [deleteListDialog, setDeleteListDialog] = useState<{ name: string; files: number } | null>(null);
  const [deleteMode, setDeleteMode] = useState<'move' | 'rollback'>('move');
  // Unshelve 目标 changelist 弹窗
  const [unshelveDialog, setUnshelveDialog] = useState<{ shelf: Shelf; paths: string[] } | null>(null);
  const [unshelveTarget, setUnshelveTarget] = useState<string>('');
  const [unshelveMode, setUnshelveMode] = useState<'existing' | 'new'>('existing');
  const [unshelveNewName, setUnshelveNewName] = useState('');


  useEffect(() => {
    if (!deleteListDialog && !unshelveDialog) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        setDeleteListDialog(null);
        setUnshelveDialog(null);
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [deleteListDialog, unshelveDialog]);

  const repoName = useMemo(() => {
    return repoPath ? repoPath.split(/[\\/]/).pop() || '' : '';
  }, [repoPath]);

  const [viewMenuOpen, setViewMenuOpen] = useState(false);
  const [showIgnoredFiles, setShowIgnoredFiles] = useState(() => {
    try {
      return localStorage.getItem('ide_commit_show_ignored') === '1';
    } catch {
      return false;
    }
  });
  const viewMenuRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    try {
      localStorage.setItem('ide_commit_show_ignored', showIgnoredFiles ? '1' : '0');
    } catch {}
  }, [showIgnoredFiles]);

  useEffect(() => {
    if (!viewMenuOpen) return;
    const onDown = (e: MouseEvent) => {
      if (viewMenuRef.current && !viewMenuRef.current.contains(e.target as Node)) {
        setViewMenuOpen(false);
      }
    };
    window.addEventListener('mousedown', onDown);
    return () => window.removeEventListener('mousedown', onDown);
  }, [viewMenuOpen]);

  // Ctrl+P / ⌘P 快捷键切换 Group By Directory。
  // 抽屉被 App 隐藏时组件仍挂载，且 Git Log 右侧文件清单也绑定了 ^P：
  // 只在抽屉可见、焦点不在 Log 面板内时响应，避免两边互相切换。
  useEffect(() => {
    if (tab !== 'commit') return;
    const onKey = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'p' && !e.shiftKey && !e.altKey) {
        const tag = (e.target as HTMLElement)?.tagName;
        if (tag === 'INPUT' || tag === 'TEXTAREA') return;
        const root = document.getElementById('pycharmCommitDrawer');
        // App 用祖先 display:none 隐藏抽屉（组件不卸载）：此时不响应。
        if (!root || root.closest('[style*="display: none"]')) return;
        const active = document.activeElement as HTMLElement | null;
        if (active?.closest('#pycharmBottomGit')) return;
        e.preventDefault();
        setCommitTreeView(prev => !prev);
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [tab]);

  // PyCharm 语义：未跟踪固定在 Unversioned Files 组，被忽略文件在 Ignored Files 组
  const trackedFiles = useMemo(() => changedFiles.filter(f => f.status !== 'U' && f.status !== 'I'), [changedFiles]);
  const untrackedFiles = useMemo(() => changedFiles.filter(f => f.status === 'U'), [changedFiles]);
  const ignoredFiles = useMemo(() => changedFiles.filter(f => f.status === 'I'), [changedFiles]);
  const conflictFiles = useMemo(() => changedFiles.filter(f => f.status === 'C'), [changedFiles]);

  const handleToggleDirFiles = useCallback((paths: string[], check: boolean) => {
    setFileChecked(prev => {
      const next = { ...prev };
      paths.forEach(p => {
        next[p] = check;
      });
      return next;
    });
  }, []);

  const handleToggleDir = useCallback((dirPath: string) => {
    setCollapsedDirs(prev => ({ ...prev, [dirPath]: !prev[dirPath] }));
  }, []);
  // 按 changelist 分组（缺省归 Changes）
  const listGroups = useMemo(() => {
    const map = new Map<string, GitChangedFile[]>();
    for (const f of trackedFiles) {
      const l = fileListMap[f.path] ?? DEFAULT_LIST;
      const arr = map.get(l);
      if (arr) arr.push(f);
      else map.set(l, [f]);
    }
    return allLists.map(name => ({ name, files: map.get(name) ?? [] }));
  }, [trackedFiles, fileListMap, allLists]);

  // 默认不勾选任何变更文件；提交范围完全由用户显式勾选决定。
  // （未跟踪文件原本就不默认勾选，这里统一为 false。）
  const isChecked = useCallback(
    (path: string) => fileChecked[path] ?? false,
    [fileChecked]
  );

  const toggleFileCheck = (path: string, e?: React.MouseEvent) => {
    e?.stopPropagation();
    setFileChecked(prev => ({ ...prev, [path]: !isChecked(path) }));
  };

  const toggleGroupCheck = (files: GitChangedFile[]) => (e: React.MouseEvent) => {
    e.stopPropagation();
    const someUnchecked = files.some(f => !isChecked(f.path));
    setFileChecked(prev => {
      const next = { ...prev };
      files.forEach(f => {
        next[f.path] = someUnchecked;
      });
      return next;
    });
  };

  const toggleGroupCollapse = (title: string) => () => {
    setCollapsedGroups(prev => ({ ...prev, [title]: !prev[title] }));
  };

  const checkedPaths = useMemo(
    () => changedFiles.filter(f => f.status !== 'I' && isChecked(f.path)).map(f => f.path),
    [changedFiles, isChecked]
  );
  const selectedCount = checkedPaths.length;

  /** 行选中：本地多选优先；没有本地选择时沿用 App 当前 diff/编辑文件。 */
  const isFileSelected = useCallback(
    (path: string) => {
      if (selectedPaths.length > 0) {
        return selectedPaths.some(p => isSamePath(p, path, repoPath));
      }
      return isSamePath(selectedFile, path, repoPath);
    },
    [selectedPaths, selectedFile, repoPath]
  );

  // Diff 是从别处打开/跨文件跳转过来时，把对应行同步进列表选择。
  useEffect(() => {
    if (!selectedFile) return;
    const matched = changedFiles.find(f => isSamePath(f.path, selectedFile, repoPath));
    if (!matched) return;
    setSelectedPaths(prev =>
      prev.some(p => isSamePath(p, matched.path, repoPath)) ? prev : [matched.path]
    );
  }, [selectedFile, changedFiles, repoPath]);

  const handleFileSelect = useCallback(
    (path: string, e: React.MouseEvent) => {
      const additive = e.metaKey || e.ctrlKey;
      const range = e.shiftKey;
      let next: string[];
      if (range && lastClickedFileRef.current) {
        const visible = Array.from(
          document.querySelectorAll<HTMLElement>('.pycharm-commit-drawer .change-file-row[data-path]')
        )
          .map(row => row.dataset.path ?? '')
          .filter(p => changedFiles.some(f => f.path === p));
        const anchor = visible.indexOf(lastClickedFileRef.current);
        const target = visible.indexOf(path);
        if (anchor >= 0 && target >= 0) {
          const [lo, hi] = anchor < target ? [anchor, target] : [target, anchor];
          next = visible.slice(lo, hi + 1);
        } else {
          next = [path];
        }
      } else if (additive) {
        next = selectedPaths.some(p => isSamePath(p, path, repoPath))
          ? selectedPaths.filter(p => !isSamePath(p, path, repoPath))
          : [...selectedPaths, path];
        if (next.length === 0) next = [path];
        lastClickedFileRef.current = path;
      } else {
        next = [path];
        lastClickedFileRef.current = path;
      }
      setSelectedPaths(next);
      // 单击只选中行；双击或右键 Show Diff 才打开 diff（对齐 PyCharm 变更列表）。
    },
    [changedFiles, onSelectFileForDiff, repoPath, selectedPaths]
  );

  /** 文件行右键：右键不在当前选择里时只选中该行；在则对整个选择操作。 */
  const handleFileMenu = useCallback(
    (
      path: string,
      e: React.MouseEvent,
      options: { untracked: boolean; staged?: boolean }
    ) => {
      e.preventDefault();
      e.stopPropagation();
      const inSelection = selectedPaths.some(p => isSamePath(p, path, repoPath));
      const paths = inSelection ? selectedPaths : [path];
      if (!inSelection) {
        setSelectedPaths([path]);
        lastClickedFileRef.current = path;
      }
      const allUntracked =
        paths.length > 0 &&
        paths.every(p => changedFiles.some(f => f.path === p && f.status === 'U'));
      setMenu({
        x: e.clientX,
        y: e.clientY,
        paths,
        untracked: allUntracked || (paths.length === 1 && options.untracked),
        staged: paths.length === 1 ? options.staged : undefined
      });
    },
    [changedFiles, repoPath, selectedPaths]
  );

  /** 分组/目录右键：操作整组文件，并把这些行设为当前选择。 */
  const handleGroupMenu = useCallback((state: DrawerMenuState) => {
    if (state.paths.length > 0) {
      setSelectedPaths(state.paths);
      lastClickedFileRef.current = state.paths[0] ?? null;
    }
    setMenu(state);
  }, []);

  // ---- 目录树（按文件夹结构查看） ----

  // 目录树里的折叠键带上 Shelf 名，避免不同贮藏条目的同名目录互相影响
  const shelvesTree = useMemo(
    () => shelves.map(s => ({ shelf: s, tree: buildDirTree(s.files.map(p => ({ path: p })), repoName) })),
    [shelves, repoName]
  );
  const shelfDirPaths = useMemo(
    () => shelvesTree.flatMap(({ shelf, tree }) => collectDirPaths(tree).map(p => `${shelf.name}:${p}`)),
    [shelvesTree]
  );

  const expandAllCommit = () => {
    setCollapsedGroups({});
    setCollapsedDirs({});
  };
  const collapseAllCommit = () => {
    setCollapsedGroups(
      Object.fromEntries(
        allLists.map(l => [l, true]).concat([['Unversioned Files', true], ['Ignored Files', true]])
      )
    );
    const allDirKeys: string[] = [];
    const walk = (d: DirNode<any>, scope: string) => {
      allDirKeys.push(dirScopeKey(scope, d.path));
      d.sub.forEach(child => walk(child, scope));
    };
    buildProjectDirTree(conflictFiles, repoName).forEach(d => walk(d, 'conflicts'));
    listGroups.forEach(g => {
      buildProjectDirTree(g.files, repoName).forEach(d => walk(d, g.name));
    });
    buildProjectDirTree(untrackedFiles, repoName).forEach(d => walk(d, 'Unversioned Files'));
    buildProjectDirTree(ignoredFiles, repoName).forEach(d => walk(d, 'Ignored Files'));
    setCollapsedDirs(Object.fromEntries(allDirKeys.map(k => [k, true])));
  };
  const expandAllShelf = () => {
    if (shelfTreeView) setShelfCollapsedDirs({});
    else setShelfCollapsed({});
  };
  const collapseAllShelf = () => {
    if (shelfTreeView) setShelfCollapsedDirs(Object.fromEntries(shelfDirPaths.map(p => [p, true])));
    else setShelfCollapsed(Object.fromEntries(shelves.map(s => [s.name, true])));
  };

  const shelfDirKey = (shelfName: string, dirPath: string) => `${shelfName}:${dirPath}`;

  const renderShelfDirNode = (shelfName: string, d: DirNode<{ path: string }>, depth: number): React.ReactNode => {
    const key = shelfDirKey(shelfName, d.path);
    return (
      <div key={`dir:${key}`}>
        <div
          className="commit-dir-row"
          style={{ paddingLeft: `${6 + depth * 14}px` }}
          onClick={() => setShelfCollapsedDirs(prev => ({ ...prev, [key]: !prev[key] }))}
          title={d.path}
        >
          <span className={`tree-chevron ${shelfCollapsedDirs[key] ? 'collapsed' : 'expanded'}`}>
            {shelfCollapsedDirs[key] ? <IconChevronRight size={11} color="var(--text-muted)" /> : <IconChevronDown size={11} color="var(--text-muted)" />}
          </span>
          <span className="git-file-icon">{renderFileIcon(d.seg, true, !shelfCollapsedDirs[key])}</span>
          <span className="git-file-name">{d.seg}</span>
          <span className="commit-dir-count">{d.totalFileCount === 1 ? '1 file' : `${d.totalFileCount} files`}</span>
        </div>
        {!shelfCollapsedDirs[key] && (
          <div>
            {d.sub.map(s => renderShelfDirNode(shelfName, s, depth + 1))}
            {d.files.map(f => (
              <FileRow
                key={f.path}
                path={f.path}
                depth={depth}
                inTree={true}
                showHint={false}
                showCheckbox={false}
                selected={selectedShelfPaths.includes(f.path)}
                onSelect={e => handleShelfFileClick(shelfName, f.path, e)}
                onDoubleClick={() => onShelfDiff?.(shelfName, f.path, false, false)}
                onMenu={e => handleShelfFileMenu(shelfName, f.path, e)}
              />
            ))}
          </div>
        )}
      </div>
    );
  };

  // ---- Shelf 读写 ----

  const loadShelves = useCallback(async () => {
    if (!repoPath) return;
    try {
      const [list, deletedList] = await Promise.all([
        invokeCommand<Shelf[]>('git_list_shelves', { repoPath }),
        invokeCommand<Shelf[]>('git_list_deleted_shelves', { repoPath }).catch(() => [])
      ]);
      setShelves(list);
      setDeletedShelves(deletedList);
      setSelectedShelf(prev => (list.some(s => s.name === prev) ? prev : list[0]?.name ?? null));
    } catch {
      setShelves([]);
      setDeletedShelves([]);
    }
  }, [repoPath]);

  // diff 导航切换文件后，把 Changes 列表里的选中行滚动到可见位置
  useEffect(() => {
    if (!selectedFile) return;
    document.querySelector('.change-file-row.selected')?.scrollIntoView({ block: 'nearest' });
  }, [selectedFile, changedFiles, commitTreeView]);

  useEffect(() => {
    if (tab === 'shelf') void loadShelves();
  }, [tab, loadShelves]);

  const handleShelfRestore = async (name: string) => {
    if (!repoPath) return;
    setShelfActing(true);
    try {
      await invokeCommand('git_restore_shelf', { repoPath, name });
      await loadShelves();
      setSelectedShelf(name);
      onShelfChanged?.(t().shelfMenu.restored(name));
    } catch (err) {
      onShelfChanged?.(`恢复失败: ${errText(err)}`);
    } finally {
      setShelfActing(false);
    }
  };

  const handleShelfRename = async (name: string) => {
    if (!repoPath || !onPromptName) return;
    const newName = await onPromptName(t().shelfMenu.renamePrompt, name);
    if (!newName || !newName.trim() || newName.trim() === name) return;
    setShelfActing(true);
    try {
      await invokeCommand('git_rename_shelf', { repoPath, oldName: name, newName: newName.trim() });
      await loadShelves();
      setSelectedShelf(newName.trim());
      onShelfChanged?.(`已重命名为 "${newName.trim()}"`);
    } catch (err) {
      onShelfChanged?.(`重命名失败: ${errText(err)}`);
    } finally {
      setShelfActing(false);
    }
  };

  const handleShelfDelete = async (name: string, isDeleted = false) => {
    if (!repoPath) return;
    const ok = await appConfirm(
      isDeleted
        ? `彻底删除搁置 "${name}"？此操作无法撤销。`
        : t().shelfMenu.deleteShelfConfirm(name),
      {
        title: t().shelfMenu.delete,
        kind: 'warning',
        okLabel: 'Delete',
        danger: true
      }
    );
    if (!ok) return;
    setShelfActing(true);
    try {
      await invokeCommand('git_delete_shelf', { repoPath, name });
      await loadShelves();
      onShelfChanged?.(t().commitDrawer.shelfDeleted(name));
    } catch (err) {
      onShelfChanged?.(t().commitDrawer.shelfDeleteFailed(errText(err)));
    } finally {
      setShelfActing(false);
    }
  };

  const handleShelfDeleteFiles = async (shelfName: string, paths: string[]) => {
    if (!repoPath || paths.length === 0) return;
    const msg = paths.length === 1
      ? t().shelfMenu.deleteFileConfirm(paths[0].split(/[\\/]/).pop() ?? paths[0], shelfName)
      : `从搁置 "${shelfName}" 中删除选中的 ${paths.length} 个文件？`;
    const ok = await appConfirm(msg, {
      title: t().shelfMenu.delete,
      kind: 'warning',
      okLabel: 'Delete',
      danger: true
    });
    if (!ok) return;
    setShelfActing(true);
    try {
      await invokeCommand('git_delete_shelf_files', { repoPath, name: shelfName, paths });
      await loadShelves();
      setSelectedShelfPaths([]);
      onShelfChanged?.(`已从搁置中移除 ${paths.length} 个文件`);
    } catch (err) {
      onShelfChanged?.(`删除文件失败: ${errText(err)}`);
    } finally {
      setShelfActing(false);
    }
  };

  const handleShelfCopyPatch = async (shelfName: string, paths?: string[]) => {
    if (!repoPath) return;
    try {
      const patch = await invokeCommand<string>('git_read_shelf_patch', {
        repoPath,
        name: shelfName,
        path: null,
        paths: paths && paths.length > 0 ? paths : null,
      });
      await navigator.clipboard.writeText(patch);
      onShelfChanged?.(t().shelfMenu.patchCopied);
    } catch (err) {
      onShelfChanged?.(`复制失败: ${errText(err)}`);
    }
  };

  const handleShelfCreatePatch = async (shelfName: string, paths?: string[]) => {
    if (!repoPath) return;
    try {
      const patch = await invokeCommand<string>('git_read_shelf_patch', {
        repoPath,
        name: shelfName,
        path: null,
        paths: paths && paths.length > 0 ? paths : null,
      });
      const defName = paths && paths.length === 1
        ? `${paths[0].split(/[\\/]/).pop() ?? 'patch'}.patch`
        : `${shelfName}.patch`;
      const savePath = await saveDialog({
        title: t().shelfMenu.createPatch,
        defaultPath: defName,
        filters: [{ name: 'Patch', extensions: ['patch', 'diff'] }]
      });
      if (savePath) {
        await invokeCommand('write_file_content', { path: savePath, content: patch });
        onShelfChanged?.(t().git.patchSaved(savePath));
      }
    } catch (err) {
      onShelfChanged?.(`创建补丁失败: ${errText(err)}`);
    }
  };

  const handleShelfImportPatches = async () => {
    if (!repoPath) return;
    try {
      const selected = await invokeCommand<string | null>('pick_path', {
        title: t().shelfMenu.importPatches,
        defaultPath: null
      });
      if (selected && typeof selected === 'string') {
        const msg = await invokeCommand<string>('git_import_shelf_patch', {
          repoPath,
          patchPath: selected,
          name: null,
        });
        await loadShelves();
        onShelfChanged?.(msg || t().shelfMenu.imported(selected));
      }
    } catch (err) {
      onShelfChanged?.(`导入补丁失败: ${errText(err)}`);
    }
  };
  // ---- 右键菜单动作 ----

  const promptCreateChangelist = async () => {
    if (!onPromptName) return;
    const name = await onPromptName(t().commitDrawer.newChangelistPrompt);
    if (!name || !name.trim()) return;
    onCreateChangelist?.(name.trim());
  };

  const openDeleteListDialog = (name: string) => {
    const count = Object.values(fileListMap).filter(l => l === name).length;
    setDeleteMode('move');
    setDeleteListDialog({ name, files: count });
  };

  /** 打开 Unshelve 弹窗：若传入 specificPaths 则只选这些，否则取已选文件（无选择则整组） */
  const openUnshelveDialog = (shelf: Shelf, specificPaths?: string[]) => {
    if (specificPaths && specificPaths.length > 0) {
      setUnshelveDialog({ shelf, paths: specificPaths });
    } else {
      const paths = selectedShelfPaths.length > 0 ? selectedShelfPaths : [...shelf.files];
      setUnshelveDialog({ shelf, paths });
    }
    setUnshelveTarget(shelf.name);
    setUnshelveMode('existing');
    setUnshelveNewName('');
  };

  /** 静默 Unshelve（⌥⌘U / 工具栏第 1 键：对齐 PyCharm 规范） */
  const handleUnshelveSilently = () => {
    if (!selectedShelf || shelfActing || busy) return;
    const currentShelf = shelves.find(s => s.name === selectedShelf);
    if (!currentShelf) return;
    const paths = selectedShelfPaths.length > 0 ? selectedShelfPaths : currentShelf.files;
    const target = allLists[0] || 'Changes';
    onUnshelve?.(currentShelf.name, paths, target);
  };

  const confirmUnshelve = () => {
    if (!unshelveDialog) return;
    const target = unshelveMode === 'new' ? unshelveNewName.trim() : unshelveTarget.trim();
    if (!target) return;
    onUnshelve?.(unshelveDialog.shelf.name, unshelveDialog.paths, target);
    setSelectedShelfPaths([]);
    setUnshelveDialog(null);
  };

  const handleShelfFileClick = (shelfName: string, filePath: string, e?: React.MouseEvent) => {
    setSelectedShelf(shelfName);
    setLastFocusedShelfFile(filePath);

    if (e?.metaKey || e?.ctrlKey) {
      setSelectedShelfPaths(prev => {
        if (prev.includes(filePath)) {
          return prev.filter(p => p !== filePath);
        } else {
          return [...prev, filePath];
        }
      });
    } else if (e?.shiftKey && lastFocusedShelfFile) {
      const currentShelf = shelves.find(s => s.name === shelfName) || deletedShelves.find(s => s.name === shelfName);
      if (currentShelf) {
        const allFiles = currentShelf.files;
        const idxA = allFiles.indexOf(lastFocusedShelfFile);
        const idxB = allFiles.indexOf(filePath);
        if (idxA !== -1 && idxB !== -1) {
          const start = Math.min(idxA, idxB);
          const end = Math.max(idxA, idxB);
          setSelectedShelfPaths(allFiles.slice(start, end + 1));
          return;
        }
      }
      setSelectedShelfPaths([filePath]);
    } else {
      setSelectedShelfPaths([filePath]);
    }
  };

  const handleShelfFileMenu = (shelfName: string, filePath: string, e: React.MouseEvent) => {
    e.preventDefault();
    e.stopPropagation();
    setSelectedShelf(shelfName);
    let pathsToUse = selectedShelfPaths;
    if (!selectedShelfPaths.includes(filePath)) {
      pathsToUse = [filePath];
      setSelectedShelfPaths([filePath]);
      setLastFocusedShelfFile(filePath);
    }
    setMenu({
      x: e.clientX,
      y: e.clientY,
      paths: pathsToUse,
      shelfName,
      shelfPath: filePath,
      isGroup: false,
      untracked: false,
    });
  };

  const handleShelfGroupMenu = (shelf: Shelf, e: React.MouseEvent, isDeleted = false) => {
    e.preventDefault();
    setSelectedShelf(shelf.name);
    setSelectedShelfPaths([]);
    setLastFocusedShelfFile(null);
    setMenu({
      x: e.clientX,
      y: e.clientY,
      paths: shelf.files,
      shelfName: shelf.name,
      isDeletedShelf: isDeleted,
      isGroup: true,
      untracked: false,
    });
  };

  const handleCommitSubmit = async () => {
    if (!commitMessage.trim()) return;
    const ok = await onCommit(commitMessage.trim(), false, checkedPaths);
    if (ok !== false) updateCommitMessage('');
  };

  const handleCommitAndPushSubmit = async () => {
    if (!commitMessage.trim()) return;
    const ok = await onCommitAndPush(commitMessage.trim(), false, checkedPaths);
    if (ok !== false) updateCommitMessage('');
  };

  return (
    <aside className="pycharm-commit-drawer" id="pycharmCommitDrawer" style={width ? { width, flexShrink: 0 } : undefined}>
      {/* 右键菜单：changelist 新建/删除；未跟踪文件加入版本控制 / 删除；已跟踪文件移动、回滚等；Shelf 条目 Unshelve / Delete。
          portal 到 body：抽屉自身有 backdrop-filter（毛玻璃主题），fixed 的菜单/遮罩
          会以抽屉为 containing block，导致菜单偏离鼠标、遮罩盖不到编辑器。 */}
      {menu && (
        <CtxMenuShell
          style={{
            top: Math.min(menu.y, window.innerHeight - 340),
            left: Math.min(menu.x, window.innerWidth - 300)
          }}
          onClose={() => setMenu(null)}
        >
          {menu.shelfName ? (
            <>
              {/* 1. Unshelve & Restore */}
              <button
                className="ctx-item ctx-row"
                disabled={shelfActing || busy}
                onClick={() => {
                  const name = menu.shelfName!;
                  const shelf = (menu.isDeletedShelf ? deletedShelves : shelves).find(s => s.name === name);
                  setMenu(null);
                  if (shelf) openUnshelveDialog(shelf, menu.paths.length > 0 ? menu.paths : undefined);
                }}
              >
                <span className="ctx-item-icon" style={{ display: 'inline-flex', alignItems: 'center' }}><IconShelveOut size={14} /></span>
                <span className="ctx-item-label">{t().shelfMenu.unshelve}</span>
                <span className="ctx-item-shortcut" style={{ marginLeft: 'auto', paddingLeft: 16, fontSize: 11, color: 'var(--text-muted)' }}>⇧⌘U</span>
              </button>
              <button
                className="ctx-item ctx-row"
                disabled={!menu.isDeletedShelf || shelfActing || busy}
                onClick={() => {
                  const name = menu.shelfName!;
                  setMenu(null);
                  void handleShelfRestore(name);
                }}
              >
                <span className="ctx-item-label" style={{ paddingLeft: 22 }}>{t().shelfMenu.restore}</span>
              </button>
              {/* 2. Diff & Compare (直接紧跟，对齐 Image #1) */}
              <button
                className="ctx-item ctx-row"
                disabled={shelfActing || busy || menu.paths.length === 0}
                onClick={() => {
                  const name = menu.shelfName!;
                  const path = menu.paths[0] || (shelves.find(s => s.name === name)?.files[0] ?? '');
                  setMenu(null);
                  if (path) onShelfDiff?.(name, path, false, false);
                }}
              >
                <span className="ctx-item-icon" style={{ display: 'inline-flex', alignItems: 'center' }}><IconDiffHorizontal size={14} /></span>
                <span className="ctx-item-label">{t().shelfMenu.showDiff}</span>
                <span className="ctx-item-shortcut" style={{ marginLeft: 'auto', paddingLeft: 16, fontSize: 11, color: 'var(--text-muted)' }}>⌘D</span>
              </button>
              <button
                className="ctx-item ctx-row"
                disabled={shelfActing || busy || menu.paths.length === 0}
                onClick={() => {
                  const name = menu.shelfName!;
                  const path = menu.paths[0] || (shelves.find(s => s.name === name)?.files[0] ?? '');
                  setMenu(null);
                  if (path) onShelfDiff?.(name, path, true, false);
                }}
              >
                <span className="ctx-item-icon" style={{ display: 'inline-flex', alignItems: 'center' }}><IconDiffTab size={14} /></span>
                <span className="ctx-item-label">{t().shelfMenu.showDiffInNewTab}</span>
              </button>
              <button
                className="ctx-item ctx-row"
                disabled={shelfActing || busy || menu.paths.length === 0}
                onClick={() => {
                  const name = menu.shelfName!;
                  const path = menu.paths[0];
                  setMenu(null);
                  if (path) onShelfDiff?.(name, path, true, true);
                }}
              >
                <span className="ctx-item-label" style={{ paddingLeft: 22 }}>{t().shelfMenu.compareWithLocal}</span>
              </button>

              {/* 3. Patches (直接紧跟，对齐 Image #1) */}
              <button
                className="ctx-item ctx-row"
                disabled={shelfActing || busy}
                onClick={() => {
                  const name = menu.shelfName!;
                  setMenu(null);
                  void handleShelfCreatePatch(name, menu.paths);
                }}
              >
                <span className="ctx-item-icon" style={{ display: 'inline-flex', alignItems: 'center' }}><IconCreatePatch size={14} /></span>
                <span className="ctx-item-label">{t().shelfMenu.createPatch}</span>
              </button>
              <button
                className="ctx-item ctx-row"
                disabled={shelfActing || busy}
                onClick={() => {
                  const name = menu.shelfName!;
                  setMenu(null);
                  void handleShelfCopyPatch(name, menu.paths);
                }}
              >
                <span className="ctx-item-label" style={{ paddingLeft: 22 }}>{t().shelfMenu.copyPatch}</span>
              </button>
              <button
                className="ctx-item ctx-row"
                disabled={shelfActing || busy}
                onClick={() => {
                  setMenu(null);
                  void handleShelfImportPatches();
                }}
              >
                <span className="ctx-item-label" style={{ paddingLeft: 22 }}>{t().shelfMenu.importPatches}</span>
              </button>

              {/* 仅在此处保留单条分割线（Image #1 规范） */}
              <div className="ctx-separator" />

              {/* 4. Rename & Delete */}
              <button
                className="ctx-item ctx-row"
                disabled={menu.isDeletedShelf || !menu.isGroup || shelfActing || busy}
                onClick={() => {
                  const name = menu.shelfName!;
                  setMenu(null);
                  void handleShelfRename(name);
                }}
              >
                <span className="ctx-item-label" style={{ paddingLeft: 22 }}>{t().shelfMenu.rename}</span>
                <span className="ctx-item-shortcut" style={{ marginLeft: 'auto', paddingLeft: 16, fontSize: 11, color: 'var(--text-muted)' }}>F2</span>
              </button>
              <button
                className="ctx-item ctx-row ctx-danger"
                disabled={shelfActing || busy}
                onClick={() => {
                  const name = menu.shelfName!;
                  const paths = menu.paths;
                  setMenu(null);
                  if (paths.length > 0 && !menu.isGroup) {
                    void handleShelfDeleteFiles(name, paths);
                  } else {
                    void handleShelfDelete(name, menu.isDeletedShelf);
                  }
                }}
              >
                <span className="ctx-item-label" style={{ paddingLeft: 22 }}>{t().shelfMenu.delete}</span>
                <span className="ctx-item-shortcut" style={{ marginLeft: 'auto', paddingLeft: 16, fontSize: 11, color: 'var(--text-muted)' }}>⌫</span>
              </button>
            </>
          ) : (
              <>
                <button
                  className="ctx-item"
                  onClick={() => {
                    setMenu(null);
                    promptCreateChangelist();
                  }}
                >
                  {t().commitDrawer.newChangelist}
                </button>
                {menu.listName && menu.listName !== DEFAULT_LIST && (
                  <button
                    className="ctx-item ctx-danger"
                    onClick={() => {
                      const name = menu.listName!;
                      setMenu(null);
                      openDeleteListDialog(name);
                    }}
                  >
                    {t().commitDrawer.deleteChangelist}
                  </button>
                )}
                <div className="ctx-separator" />
                {menu.untracked ? (
                  <>
                    <button
                      className="ctx-item"
                      onClick={() => {
                        setMenu(null);
                        onAddToVcs?.(menu.paths);
                      }}
                    >
                      {menu.paths.length > 1
                        ? t().commitDrawer.addFilesToVcs(menu.paths.length)
                        : t().commitDrawer.addToVcs}
                    </button>
                    {menu.paths.length === 1 && (
                      <button className="ctx-item" onClick={() => { const p = menu.paths[0]; setMenu(null); onJumpToSource?.(p); }}>
                        {t().commitDrawer.jumpToSource}
                      </button>
                    )}
                    <div className="ctx-separator" />
                    <button
                      className="ctx-item ctx-danger"
                      onClick={() => {
                        const paths = menu.paths;
                        setMenu(null);
                        onDeleteUntracked?.(paths);
                      }}
                    >
                      {menu.paths.length > 1
                        ? t().commitDrawer.deleteFilesAction(menu.paths.length)
                        : t().commitDrawer.deleteAction}
                    </button>
                  </>
                ) : (
                  <>
                    {onCommitPaths && (
                      <button
                        className="ctx-item"
                        onClick={() => {
                          const paths = menu.paths;
                          setMenu(null);
                          if (paths.length === 1) onCommitFile?.(paths[0]);
                          else onCommitPaths(paths);
                        }}
                      >
                        {menu.paths.length === 1
                          ? t().commitDrawer.commitFileOnly
                          : t().commitDrawer.commitFilesAction}
                      </button>
                    )}
                    {menu.paths.length === 1 && (
                      <button className="ctx-item" onClick={() => { const p = menu.paths[0]; setMenu(null); onSelectFileForDiff(p, menu.staged); }}>
                        {t().commitDrawer.showDiffAction}
                      </button>
                    )}
                    {menu.paths.length === 1 && (
                      <button className="ctx-item" disabled title={t().commitDrawer.showDiffNewTab}>
                        {t().commitDrawer.showDiffNewTab}
                      </button>
                    )}
                    {menu.paths.length === 1 && (
                      <button className="ctx-item" disabled>
                        {t().commitDrawer.compareWithLocal}
                      </button>
                    )}
                    {menu.paths.length === 1 && (
                      <button className="ctx-item" onClick={() => { const p = menu.paths[0]; setMenu(null); onCompareBeforeLocal?.(p); }}>
                        {t().commitDrawer.compareBeforeLocal}
                      </button>
                    )}
                    {menu.paths.length === 1 && (
                      <button className="ctx-item" onClick={() => { const p = menu.paths[0]; setMenu(null); onJumpToSource?.(p); }}>
                        {t().commitDrawer.jumpToSource}
                      </button>
                    )}
                    {menu.paths.length === 1 && (
                      <button className="ctx-item" disabled>
                        {t().commitDrawer.openRepositoryVersion}
                      </button>
                    )}
                    <div className="ctx-separator" />
                    <button
                      className="ctx-item ctx-danger"
                      onClick={() => {
                        setMenu(null);
                        menu.paths.forEach(p => onDiscardFile?.(p));
                      }}
                    >
                      {t().commitDrawer.revertSelectedChanges}
                    </button>
                    <button className="ctx-item" disabled>{t().commitDrawer.cherryPickSelectedChanges}</button>
                    <button className="ctx-item" disabled>{t().commitDrawer.extractSelectedChanges}</button>
                    <button className="ctx-item" disabled>{t().commitDrawer.dropSelectedChanges}</button>
                    {menu.paths.length === 1 && (
                      <button className="ctx-item" onClick={() => {
                        const p = menu.paths[0];
                        setMenu(null);
                        onCreateLocalPatch?.(p);
                      }}>
                        {t().commitDrawer.createPatch}
                      </button>
                    )}
                    <button className="ctx-item" disabled>{t().commitDrawer.getFromRevision}</button>
                    {menu.paths.length === 1 && (
                      <button className="ctx-item" onClick={() => {
                        const p = menu.paths[0];
                        setMenu(null);
                        onFileHistory?.(p);
                      }}>
                        {t().commitDrawer.historyUpToHere}
                      </button>
                    )}
                    <button className="ctx-item" disabled>{t().commitDrawer.showChangesToParents}</button>
                    <div className="ctx-separator" />
                    {menu.paths.length > 0 && (
                      <div className="ctx-item ctx-row has-sub">
                        <span className="ctx-label">{t().commitDrawer.moveToChangelist}</span>
                        <span className="ctx-sub-chevron">
                          <IconChevronRight size={10} color="var(--text-muted)" />
                        </span>
                        <div className="ctx-submenu">
                          {allLists.map(l => (
                            <button
                              key={l}
                              className="ctx-item"
                              disabled={menu.paths.every(
                                p => (fileListMap[p] ?? DEFAULT_LIST) === l
                              )}
                              onClick={() => {
                                setMenu(null);
                                onAssignToChangelist?.(menu.paths, l);
                              }}
                            >
                              {l}
                            </button>
                          ))}
                        </div>
                      </div>
                    )}
                    {menu.paths.length === 1 && (
                      <button className="ctx-item" onClick={() => { const p = menu.paths[0]; setMenu(null); onCopyLocalPatch?.(p); }}>
                        {t().commitDrawer.copyPatch}
                      </button>
                    )}
                    {menu.paths.length > 0 && (
                      <button className="ctx-item" onClick={() => { const paths = menu.paths; setMenu(null); onShelve?.(paths); }}>
                        {t().commitDrawer.shelveAction}
                      </button>
                    )}
                    {onRefresh && (
                      <button
                        className="ctx-item"
                        onClick={() => {
                          setMenu(null);
                          onRefresh();
                        }}
                      >
                        {t().commitDrawer.refreshChanges}
                      </button>
                    )}
                  </>
                )}
              </>
            )}
        </CtxMenuShell>
      )}

      {/* 删除 changelist 确认弹窗：默认移入 Changes，可选 Rollback */}
      {deleteListDialog && (
        <div className="py-dialog-backdrop">
          <div className="py-name-dialog">
            <div className="py-dialog-header">
              <span className="py-dialog-title">{t().commitDrawer.deleteChangelistTitle}</span>
              <button className="py-dialog-close" onClick={() => setDeleteListDialog(null)} title={t().common.close}>
                <IconWindowClose size={13} />
              </button>
            </div>
            <div className="py-name-subtitle">{deleteListDialog.name}</div>
            <div className="py-name-message">
              {t().commitDrawer.deleteChangelistMsg(deleteListDialog.name, deleteListDialog.files)}
            </div>
            <div className="py-choice-list">
              <label className="py-choice-row">
                <input type="radio" checked={deleteMode === 'move'} onChange={() => setDeleteMode('move')} />
                <span>{t().commitDrawer.deleteChangelistMove}</span>
              </label>
              <label className="py-choice-row">
                <input type="radio" checked={deleteMode === 'rollback'} onChange={() => setDeleteMode('rollback')} />
                <span>{t().commitDrawer.deleteChangelistRollback}</span>
              </label>
            </div>
            <div className="py-dialog-footer">
              <button className="py-name-btn" onClick={() => setDeleteListDialog(null)}>
                {t().dialog.cancel}
              </button>
              <button
                className="py-name-btn danger"
                onClick={() => {
                  const { name } = deleteListDialog;
                  setDeleteListDialog(null);
                  onDeleteChangelist?.(name, deleteMode === 'rollback');
                }}
              >
                {t().namePrompt.delete}
              </button>
            </div>
          </div>
        </div>
      )}

      {/* Unshelve 弹窗：选择目标 changelist（默认为贮藏名，可新建） */}
      {unshelveDialog && (
        <div className="py-dialog-backdrop">
          <div className="py-name-dialog">
            <div className="py-dialog-header">
              <span className="py-dialog-title">{t().commitDrawer.unshelveTitle}</span>
              <button className="py-dialog-close" onClick={() => setUnshelveDialog(null)} title={t().common.close}>
                <IconWindowClose size={13} />
              </button>
            </div>
            <div className="py-name-subtitle">
              {unshelveDialog.shelf.name} · {t().commitDrawer.fileCount(unshelveDialog.paths.length)}
            </div>
            <div className="py-name-message">{t().commitDrawer.unshelveMsg}</div>
            <div className="py-choice-list">
              {(() => {
                const opts = [...allLists];
                const s = unshelveDialog.shelf.name;
                if (!opts.includes(s)) opts.push(s);
                return (
                  <>
                    {opts.map(o => (
                      <label key={o} className="py-choice-row">
                        <input
                          type="radio"
                          checked={unshelveMode === 'existing' && unshelveTarget === o}
                          onChange={() => {
                            setUnshelveMode('existing');
                            setUnshelveTarget(o);
                          }}
                        />
                        <span>
                          {o}
                          {o === s && !allLists.includes(s) ? t().commitDrawer.unshelveNewTag : ''}
                        </span>
                      </label>
                    ))}
                  </>
                );
              })()}
              <label className="py-choice-row">
                <input type="radio" checked={unshelveMode === 'new'} onChange={() => setUnshelveMode('new')} />
                <span>{t().commitDrawer.newChangelistLabel}</span>
                <input
                  className="py-name-input"
                  style={{ flex: 1, margin: 0 }}
                  value={unshelveNewName}
                  placeholder={t().commitDrawer.newChangelistPlaceholder}
                  onChange={e => setUnshelveNewName(e.target.value)}
                />
              </label>
            </div>
            <div className="py-dialog-footer">
              <button className="py-name-btn" onClick={() => setUnshelveDialog(null)}>
                {t().dialog.cancel}
              </button>
              <button
                className="py-name-btn primary"
                disabled={unshelveMode === 'new' && !unshelveNewName.trim()}
                onClick={confirmUnshelve}
              >
                {t().commitDrawer.unshelve}
              </button>
            </div>
          </div>
        </div>
      )}

      {/* 1. Header：Commit | Shelf 双页签（对齐 PyCharm） */}
      <div className="commit-drawer-header">
        <div className="commit-header-tabs">
          <button className={`commit-tab-title ${tab === 'commit' ? 'active' : ''}`} onClick={() => setTab('commit')}>
            {t().commitDrawer.tabCommit}
          </button>
          <button className={`commit-tab-title ${tab === 'shelf' ? 'active' : ''}`} onClick={() => setTab('shelf')}>
            {t().commitDrawer.tabShelf}
          </button>
        </div>
        <div className="commit-header-actions">
          <button type="button" className="tb-icon-btn" title="Options">
            <IconMoreVert size={14} />
          </button>
          {onClose && (
            <button type="button" className="tb-icon-btn" onClick={onClose} title={t().commitDrawer.hidePanel}>
              <IconWindowMinimize size={13} />
            </button>
          )}
        </div>
      </div>

      {tab === 'commit' ? (
        <>
          {/* 2. Commit 工具栏：刷新 / Rollback / Shelve / 按文件夹结构查看 / 展开所有 / 折叠所有 */}
          <div className="commit-toolbar">
            <button
              className={`tb-icon-btn${refreshing ? ' is-refreshing' : ''}`}
              onClick={onRefresh}
              title={refreshing ? t().commitDrawer.refreshing : t().commitDrawer.refreshTooltip}
              disabled={refreshing}
            >
              <IconRefresh size={14} />
            </button>
            <button
              className="tb-icon-btn"
              disabled={!selectedFile || busy}
              onClick={() => selectedFile && onDiscardFile?.(selectedFile)}
              title={t().commitDrawer.rollbackSelectedTooltip}
            >
              <IconRollback size={14} />
            </button>
            <button
              className="tb-icon-btn"
              disabled={busy || isRepo === false || selectedCount === 0}
              onClick={() => onShelve?.(checkedPaths)}
              title={t().commitDrawer.shelveSelectedTooltip}
            >
              <IconShelveIn size={14} />
            </button>
            <div className="commit-view-menu-wrap" ref={viewMenuRef}>
              <button
                type="button"
                className={`tb-icon-btn ${viewMenuOpen ? 'active' : ''}`}
                onClick={() => setViewMenuOpen(v => !v)}
                title={t().commitDrawer.viewOptions}
              >
                <IconViewOptions size={14} />
              </button>
              {viewMenuOpen && (
                <div className="commit-view-menu">
                  <div className="commit-view-menu-header">{t().commitDrawer.groupBy}</div>
                  <div
                    className={`commit-view-menu-item ${commitTreeView ? 'active' : ''}`}
                    onClick={() => {
                      setCommitTreeView(prev => !prev);
                      setViewMenuOpen(false);
                    }}
                  >
                    <span className="commit-view-item-label">
                      <span className="commit-view-check">{commitTreeView ? '✓' : ''}</span>
                      <span>{t().commitDrawer.groupByDirectory}</span>
                    </span>
                    <span className="commit-view-shortcut">^P</span>
                  </div>
                  <div className="commit-view-menu-sep" />
                  <div className="commit-view-menu-header">{t().commitDrawer.show}</div>
                  <div
                    className={`commit-view-menu-item ${showIgnoredFiles ? 'active' : ''}`}
                    onClick={() => {
                      setShowIgnoredFiles(prev => !prev);
                      setViewMenuOpen(false);
                    }}
                  >
                    <span className="commit-view-item-label">
                      <span className="commit-view-check">{showIgnoredFiles ? '✓' : ''}</span>
                      <span>{t().commitDrawer.showIgnoredFiles}</span>
                    </span>
                  </div>
                </div>
              )}
            </div>
            <button className="tb-icon-btn" onClick={expandAllCommit} title={t().commitDrawer.expandAll}>
              <IconExpandAll size={14} />
            </button>
            <button className="tb-icon-btn" onClick={collapseAllCommit} title={t().commitDrawer.collapseAll}>
              <IconCollapseAll size={14} />
            </button>
          </div>


          {/* 3. Changelists：平铺分组或目录树 */}
          <div className="commit-tree-container">
            {isRepo === false ? (
              <div className="commit-empty-notice">{t().commitDrawer.notRepoNotice}</div>
            ) : changedFiles.length === 0 ? (
              <div className="commit-empty-notice">
                {refreshing ? t().commitDrawer.readingChanges : t().commitDrawer.worktreeClean}
              </div>
            ) : (
              <>
                {listGroups.map(g => (
                  <CommitGroup
                    key={g.name}
                    title={g.name}
                    listName={g.name}
                    conflictFiles={g.name === DEFAULT_LIST ? conflictFiles : []}
                    onResolveConflicts={onResolveConflicts}
                    files={g.files}
                    collapsed={!!collapsedGroups[g.name]}
                    repoName={repoName}
                    isSelectedPath={isFileSelected}
                    isCheckedPath={isChecked}
                    onToggleCollapse={toggleGroupCollapse(g.name)}
                    onToggleGroup={toggleGroupCheck(g.files)}
                    onToggleFile={toggleFileCheck}
                    onToggleDirFiles={handleToggleDirFiles}
                    onSelectFile={handleFileSelect}
                    onSelectForDiff={onSelectFileForDiff}
                    onFileMenu={handleFileMenu}
                    onRollback={onDiscardFile}
                    onMenu={handleGroupMenu}
                    treeView={commitTreeView}
                    collapsedDirs={collapsedDirs}
                    onToggleDir={handleToggleDir}
                  />
                ))}

                {untrackedFiles.length > 0 && (
                  <CommitGroup
                    title={t().commitDrawer.unversionedFiles}
                    listName="Unversioned Files"
                    files={untrackedFiles}
                    unversioned
                    collapsed={!!collapsedGroups['Unversioned Files']}
                    repoName={repoName}
                    isSelectedPath={isFileSelected}
                    isCheckedPath={isChecked}
                    onToggleCollapse={toggleGroupCollapse('Unversioned Files')}
                    onToggleGroup={toggleGroupCheck(untrackedFiles)}
                    onToggleFile={toggleFileCheck}
                    onToggleDirFiles={handleToggleDirFiles}
                    onSelectFile={handleFileSelect}
                    onSelectForDiff={onSelectFileForDiff}
                    onFileMenu={handleFileMenu}
                    onRollback={onDiscardFile}
                    onMenu={handleGroupMenu}
                    treeView={commitTreeView}
                    collapsedDirs={collapsedDirs}
                    onToggleDir={handleToggleDir}
                  />
                )}

                {showIgnoredFiles && ignoredFiles.length > 0 && (
                  <CommitGroup
                    title={t().commitDrawer.ignoredFiles}
                    listName="Ignored Files"
                    files={ignoredFiles}
                    isIgnored
                    collapsed={!!collapsedGroups['Ignored Files']}
                    repoName={repoName}
                    isSelectedPath={isFileSelected}
                    isCheckedPath={isChecked}
                    onToggleCollapse={toggleGroupCollapse('Ignored Files')}
                    onToggleGroup={() => {}}
                    onToggleFile={() => {}}
                    onToggleDirFiles={() => {}}
                    onSelectFile={handleFileSelect}
                    onSelectForDiff={onSelectFileForDiff}
                    onFileMenu={handleFileMenu}
                    onMenu={handleGroupMenu}
                    treeView={commitTreeView}
                    collapsedDirs={collapsedDirs}
                    onToggleDir={handleToggleDir}
                  />
                )}
              </>
            )}
          </div>

          {/* 4. Commit Message & Actions */}
          <div className="commit-bottom-box">
            <div className="commit-options-row">
              <div className="commit-history-dropdown-wrapper">
                <button
                  className="tb-icon-btn history-btn"
                  onClick={() => setIsHistoryOpen(prev => !prev)}
                  title={recentMessages.length > 0 ? t().commitDrawer.historyTooltip : t().commitDrawer.noHistoryTooltip}
                  disabled={recentMessages.length === 0}
                >
                  <IconHistory size={14} />
                </button>
                {isHistoryOpen && recentMessages.length > 0 && (
                  <div className="commit-history-popup">
                    <div className="history-header">{t().commitDrawer.historyHeader}</div>
                    {recentMessages.map((msg, i) => (
                      <div
                        key={i}
                        className="history-item"
                        onClick={() => {
                          updateCommitMessage(msg);
                          setIsHistoryOpen(false);
                        }}
                      >
                        {msg}
                      </div>
                    ))}
                    {onClearHistory && (
                      <div
                        className="history-item history-clear"
                        role="button"
                        tabIndex={0}
                        title={t().commitDrawer.clearHistory}
                        onClick={() => {
                          onClearHistory();
                          setIsHistoryOpen(false);
                        }}
                        onKeyDown={e => {
                          if (e.key === 'Enter' || e.key === ' ') {
                            e.preventDefault();
                            onClearHistory();
                            setIsHistoryOpen(false);
                          }
                        }}
                      >
                        {t().commitDrawer.clearHistory}
                      </div>
                    )}
                  </div>
                )}
              </div>
            </div>

            <textarea
              className="commit-textarea"
              placeholder={t().commitDrawer.messagePlaceholder}
              value={commitMessage}
              onChange={e => updateCommitMessage(e.target.value)}
              rows={4}
              wrap="soft"
              spellCheck={false}
            />

            <div className="commit-actions-row">
              <button
                className="pycharm-commit-btn primary"
                disabled={busy || isRepo === false || selectedCount === 0 || !commitMessage.trim()}
                onClick={handleCommitSubmit}
              >
                {busy ? t().commitDrawer.committing : t().commitDrawer.commit}
              </button>

              <button
                className="pycharm-commit-btn secondary"
                disabled={busy || isRepo === false || selectedCount === 0 || !commitMessage.trim()}
                onClick={handleCommitAndPushSubmit}
              >
                {t().commitDrawer.commitAndPush}
              </button>
            </div>
          </div>
        </>
      ) : (
        <>
          {/* 2. Shelf 工具栏（对齐 PyCharm Image #3 & Image #4 规范） */}
          <div className="commit-toolbar">
            {/* 键 1：Unshelve Silently ⌥⌘U (Image #3) */}
            <button
              className="tb-icon-btn"
              disabled={!selectedShelf || shelfActing || busy}
              onClick={handleUnshelveSilently}
              title="Unshelve Silently  ⌥⌘U"
            >
              <IconShelveOut size={14} />
            </button>

            {/* 键 2：View Options 眼睛下拉菜单 (Image #4) */}
            <div className="commit-view-menu-wrap" ref={shelfViewMenuRef}>
              <button
                type="button"
                className={`tb-icon-btn ${shelfViewMenuOpen ? 'active' : ''}`}
                onClick={() => setShelfViewMenuOpen(v => !v)}
                title={t().commitDrawer.viewOptions}
              >
                <IconViewOptions size={14} />
              </button>
              {shelfViewMenuOpen && (
                <div className="commit-view-menu" style={{ minWidth: 175 }}>
                  <div className="commit-view-menu-header">{t().commitDrawer.groupBy}</div>
                  <div
                    className={`commit-view-menu-item ${shelfTreeView ? 'active' : ''}`}
                    onClick={() => {
                      setShelfTreeView(prev => !prev);
                      setShelfViewMenuOpen(false);
                    }}
                  >
                    <span className="commit-view-item-label">
                      <span className="commit-view-check">{shelfTreeView ? '✓' : ''}</span>
                      <span>{t().commitDrawer.groupByDirectory}</span>
                    </span>
                    <span className="commit-view-shortcut">^P</span>
                  </div>
                  <div style={{ height: 6 }} />
                  <div className="commit-view-menu-header">{t().commitDrawer.show}</div>
                  <div
                    className={`commit-view-menu-item ${showAlreadyUnshelved ? 'active' : ''}`}
                    onClick={() => {
                      setShowAlreadyUnshelved(prev => !prev);
                      setShelfViewMenuOpen(false);
                    }}
                  >
                    <span className="commit-view-item-label">
                      <span className="commit-view-check">{showAlreadyUnshelved ? '✓' : ''}</span>
                      <span>Already Unshelved</span>
                    </span>
                  </div>
                </div>
              )}
            </div>

            {/* 键 3：Expand All */}
            <button className="tb-icon-btn" onClick={expandAllShelf} title={t().commitDrawer.expandAll}>
              <IconExpandAll size={14} />
            </button>

            {/* 键 4：Collapse All */}
            <button className="tb-icon-btn" onClick={collapseAllShelf} title={t().commitDrawer.collapseAll}>
              <IconCollapseAll size={14} />
            </button>
          </div>

          {/* 3. 贮藏架清单（完全无多选框，对齐 PyCharm 规范） */}
          <div className="commit-tree-container">
            {shelves.length === 0 && deletedShelves.length === 0 ? (
              <div className="commit-empty-notice">{t().commitDrawer.shelfEmptyHint}</div>
            ) : shelfTreeView ? (
              shelvesTree.map(({ shelf, tree }) => (
                <div key={shelf.name} className="commit-group">
                  <div
                    className={`commit-group-row ${selectedShelf === shelf.name && selectedShelfPaths.length === 0 ? 'selected' : ''}`}
                    onClick={() => {
                      setSelectedShelf(shelf.name);
                      setSelectedShelfPaths([]);
                      setLastFocusedShelfFile(null);
                    }}
                    onContextMenu={e => handleShelfGroupMenu(shelf, e, false)}
                    title={t().commitDrawer.shelfTreeHint(shelf.name)}
                  >
                    <span
                      className={`tree-chevron ${shelfCollapsed[shelf.name] ? 'collapsed' : 'expanded'}`}
                      onClick={e => {
                        e.stopPropagation();
                        setShelfCollapsed(prev => ({ ...prev, [shelf.name]: !prev[shelf.name] }));
                      }}
                    >
                      {shelfCollapsed[shelf.name] ? <IconChevronRight size={11} color="var(--text-muted)" /> : <IconChevronDown size={11} color="var(--text-muted)" />}
                    </span>
                    <span className="commit-group-name">{shelf.name}</span>
                    <span className="commit-group-count">
                      {t().commitDrawer.shelfCount(shelf.files.length, shelf.date ?? '')}
                    </span>
                  </div>
                  {!shelfCollapsed[shelf.name] && (
                    <div className="commit-group-files">
                      {tree.sub.map(d => renderShelfDirNode(shelf.name, d, 0))}
                      {tree.files.map(f => (
                        <FileRow
                          key={f.path}
                          path={f.path}
                          depth={0}
                          inTree={true}
                          showHint={false}
                          showCheckbox={false}
                          selected={selectedShelfPaths.includes(f.path)}
                          onSelect={e => handleShelfFileClick(shelf.name, f.path, e)}
                          onDoubleClick={() => onShelfDiff?.(shelf.name, f.path, false, false)}
                          onMenu={e => handleShelfFileMenu(shelf.name, f.path, e)}
                        />
                      ))}
                    </div>
                  )}
                </div>
              ))
            ) : (
              shelves.map(s => (
                <ShelfGroup
                  key={s.name}
                  shelf={s}
                  selected={selectedShelf === s.name && selectedShelfPaths.length === 0}
                  collapsed={!!shelfCollapsed[s.name]}
                  onSelect={() => {
                    setSelectedShelf(s.name);
                    setSelectedShelfPaths([]);
                    setLastFocusedShelfFile(null);
                  }}
                  onToggleCollapse={() => setShelfCollapsed(prev => ({ ...prev, [s.name]: !prev[s.name] }))}
                  onMenu={e => handleShelfGroupMenu(s, e, false)}
                  onFileMenu={(p, e) => handleShelfFileMenu(s.name, p, e)}
                  onSelectFile={(p, e) => handleShelfFileClick(s.name, p, e)}
                  selectedPaths={selectedShelf === s.name ? selectedShelfPaths : []}
                  onDoubleClickFile={p => onShelfDiff?.(s.name, p, false, false)}
                />
              ))
            )}

            {/* 4. 最近删除（Recently Deleted） */}
            {deletedShelves.length > 0 && (
              <div className="commit-group recently-deleted-group" style={{ marginTop: 8, borderTop: '1px solid var(--border-subtle)', paddingTop: 4 }}>
                <div
                  className="commit-group-row"
                  onClick={() => setDeletedCollapsed(v => !v)}
                  style={{ opacity: 0.85 }}
                  title={t().shelfMenu.recentlyDeleted}
                >
                  <span className={`tree-chevron ${deletedCollapsed ? 'collapsed' : 'expanded'}`}>
                    {deletedCollapsed ? <IconChevronRight size={11} color="var(--text-muted)" /> : <IconChevronDown size={11} color="var(--text-muted)" />}
                  </span>
                  <span className="commit-group-name" style={{ fontWeight: 500 }}>{t().shelfMenu.recentlyDeleted}</span>
                  <span className="commit-group-count">{deletedShelves.length}</span>
                </div>
                {!deletedCollapsed && (
                  <div className="commit-group-files">
                    {deletedShelves.map(s => (
                      <div
                        key={s.name}
                        className={`shelf-deleted-row commit-group-row ${selectedShelf === s.name ? 'selected' : ''}`}
                        style={{ paddingLeft: 22, opacity: 0.75 }}
                        onClick={() => {
                          setSelectedShelf(s.name);
                          setSelectedShelfPaths([]);
                          setLastFocusedShelfFile(null);
                        }}
                        onContextMenu={e => handleShelfGroupMenu(s, e, true)}
                      >
                        <span className="commit-group-name" style={{ textDecoration: 'line-through' }}>{s.name}</span>
                        <span className="commit-group-count">
                          {t().commitDrawer.shelfCount(s.files.length, s.date ?? '')}
                        </span>
                      </div>
                    ))}
                  </div>
                )}
              </div>
            )}
          </div>
        </>
      )}
    </aside>
  );
});
