// 项目树右键菜单 · 对齐 PyCharm Project 视图右键
//
// 纯文本菜单项（与全局其它右键菜单统一：一律不带图标），悬停展开子菜单。
// 文件与文件夹两套菜单（PyCharm 行为差异）：
// - 文件：New 在同级目录创建；Git 子菜单含 Show Diff / History / Annotate；末尾 Compare With…
// - 文件夹：New 在目录内创建；Git 子菜单为 Add（含子文件）/ Rollback；无 Compare
// 按需求剔除 Run / Debug 及语言相关的 Find Usages / Reformat / Optimize Imports 等。

import React, { useEffect } from 'react';
import { CtxMenuShell } from './CtxMenuShell';
import { IconChevronRight } from './FileIcons';
import { t } from '../services/i18n';

export interface TreeMenuState {
  x: number;
  y: number;
  path: string;
  name: string;
  isDir: boolean;
  /**
   * 本次右键作用的多选集合（含 `path` 自身）。长度 > 1 时菜单是批量形态：
   * Delete 走 deletePathsToTrash（已支持入参数组），其余项按统一的
   * 「批量不支持」禁用。不传或长度为 1 时维持单文件菜单。
   */
  selectedPaths?: string[];
}

interface ProjectTreeContextMenuProps {
  menu: TreeMenuState | null;
  /** 应用级文件剪贴板是否有内容（Paste 置灰） */
  hasClipboard: boolean;
  onClose: () => void;
  onNewFile: (dir: string) => void;
  onNewDirectory: (dir: string) => void;
  onCut: (path: string) => void;
  onCopy: (path: string) => void;
  onPaste: (destDir: string) => void;
  onCopyPath: (path: string, absolute: boolean) => void;
  onRename: (path: string, name: string) => void;
  onDelete: (path: string, name: string, isDir: boolean, selected?: string[]) => void;
  onReveal: (path: string) => void;
  onOpenTerminal: (path: string) => void;
  onAddToVcs: (path: string) => void;
  onRollback: (path: string, isDir: boolean) => void;
  onExpandAll: (dirPath: string) => void;
  onCollapseAll: (dirPath: string) => void;
  onShowDiff: (path: string) => void;
  onShowHistory: (path: string) => void;
  onAnnotate: (path: string) => void;
  onCompareWith: (path: string) => void;
  onReload: (path: string, isDir: boolean) => void;
  /** 「Find in Files...（在此目录）」；不传则不渲染该行 */
  onFindInPath?: (path: string) => void;
  /** 仓库根路径；用于 Copy Path/Reference 的「Path from Repository Root」。
   *  不传或 path 不在其下时，该行降级为禁用态（title 说明原因） */
  repoRoot?: string;
}

const CtxSep: React.FC = () => <div className="ctx-sep" />;

interface RowProps {
  label: string;
  danger?: boolean;
  disabled?: boolean;
  title?: string;
  onClick?: () => void;
}

const Row: React.FC<RowProps> = ({ label, danger, disabled, title, onClick }) => (
  <button
    className={`ctx-item ctx-row ${danger ? 'ctx-danger' : ''}`}
    disabled={disabled}
    title={title}
    onClick={onClick}
  >
    <span className="ctx-label">{label}</span>
  </button>
);

const Sub: React.FC<{ label: string; children: React.ReactNode }> = ({ label, children }) => (
  <div className="ctx-item ctx-row has-sub">
    <span className="ctx-label">{label}</span>
    <span className="ctx-sub-chevron">
      <IconChevronRight size={10} color="var(--text-muted)" />
    </span>
    <div className="ctx-submenu">{children}</div>
  </div>
);

/** 供菜单内部使用：取父目录（根节点时返回自身） */
const parentOf = (path: string) => {
  const trimmed = path.replace(/[\\/]+$/, '');
  const idx = Math.max(trimmed.lastIndexOf('/'), trimmed.lastIndexOf('\\'));
  return idx > 0 ? trimmed.slice(0, idx) : trimmed;
};

export const ProjectTreeContextMenu: React.FC<ProjectTreeContextMenuProps> = ({
  menu,
  hasClipboard,
  onClose,
  onNewFile,
  onNewDirectory,
  onCut,
  onCopy,
  onPaste,
  onCopyPath,
  onRename,
  onDelete,
  onReveal,
  onOpenTerminal,
  onAddToVcs,
  onRollback,
  onExpandAll,
  onCollapseAll,
  onShowDiff,
  onShowHistory,
  onAnnotate,
  onCompareWith,
  onReload,
  onFindInPath,
  repoRoot
}) => {
  useEffect(() => {
    if (!menu) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [menu, onClose]);

  if (!menu) return null;

  const { x, y, path, name, isDir } = menu;
  // PyCharm 语义：文件夹上 New 在目录内创建，文件上 New 在同级创建
  const newDir = isDir ? path : parentOf(path);
  // 菜单很高，防止溢出屏幕底部；470 为估算的菜单最大高度。
  // 这里只做粗略钳制，最终位置由 CtxMenuShell（内部还有一次 clamp）兜底
  const top = Math.min(y, window.innerHeight - 470);
  const left = Math.min(x, window.innerWidth - 280);
  // 「Path from Repository Root」：去掉仓库根前缀得到仓库相对路径；
  // 不在仓库内 / 无仓库根时为 null（禁用态），仓库根本身则为空串
  const repoRelativePath: string | null =
    !repoRoot
      ? null
      : path === repoRoot
        ? ''
        : path.startsWith(`${repoRoot}/`)
          ? path.slice(repoRoot.length + 1)
          : null;

  const act = (fn: () => void) => () => {
    onClose();
    fn();
  };

  const newSubmenu = (
    <Sub label={t().treeMenu.newSubmenu}>
      <Row label={t().treeMenu.newFile} onClick={act(() => onNewFile(newDir))} />
      <Row label={t().treeMenu.newDirectory} onClick={act(() => onNewDirectory(newDir))} />
    </Sub>
  );

  const clipboardRows = (
    <>
      <Row label={t().menu.cut} onClick={act(() => onCut(path))} />
      <Row label={t().menu.copy} onClick={act(() => onCopy(path))} />
      <Sub label={t().treeMenu.copyPathReference}>
        <Row label={t().treeMenu.absolutePath} onClick={act(() => onCopyPath(path, true))} />
        <Row label={t().treeMenu.relativePath} onClick={act(() => onCopyPath(path, false))} />
        <Row
          label={t().treeMenu.pathFromRepoRoot}
          disabled={repoRelativePath === null}
          title={
            repoRelativePath === null
              ? repoRoot
                ? t().treeMenu.notInRepoTitle
                : t().treeMenu.noRepoRootTitle
              : repoRelativePath === ''
                ? t().treeMenu.isRepoRootTitle
                : undefined
          }
          onClick={
            repoRelativePath === null
              ? undefined
              : act(async () => {
                  // 仓库根本身的相对路径记作 '.'，避免复制出空文本
                  const text = repoRelativePath || '.';
                  try {
                    await navigator.clipboard.writeText(text);
                  } catch {
                    // 剪贴板不可用时不弹提示：onCopyPath 兜底也会尝试复制
                  }
                  if (repoRelativePath) {
                    onCopyPath(repoRelativePath, false);
                  }
                })
          }
        />
      </Sub>
      <Row label={t().menu.paste} disabled={!hasClipboard} onClick={act(() => onPaste(newDir))} />
    </>
  );

  const gitSubmenu = isDir ? (
    <Sub label={t().treeMenu.gitSubmenu}>
      <Row label={t().treeMenu.addToVcsDir} onClick={act(() => onAddToVcs(path))} />
      <Row label={t().treeMenu.rollbackDir} onClick={act(() => onRollback(path, true))} />
    </Sub>
  ) : (
    <Sub label={t().treeMenu.gitSubmenu}>
      <Row label={t().treeMenu.addToVcs} onClick={act(() => onAddToVcs(path))} />
      <Row label={t().treeMenu.showDiff} onClick={act(() => onShowDiff(path))} />
      <Row label={t().treeMenu.showHistory} onClick={act(() => onShowHistory(path))} />
      <Row label={t().treeMenu.annotateBlame} onClick={act(() => onAnnotate(path))} />
      <Row label={t().treeMenu.rollbackFile} onClick={act(() => onRollback(path, false))} />
    </Sub>
  );

  const openInSubmenu = (
    <Sub label={t().treeMenu.openInSubmenu}>
      <Row label={t().treeMenu.finder} onClick={act(() => onReveal(path))} />
      <Row label={t().treeMenu.terminal} onClick={act(() => onOpenTerminal(path))} />
    </Sub>
  );

  // 批量形态：多选 >= 2 项时只保留删除（后端 deletePathsToTrash 已入参数组），
  // 其余「一次性作用于单个路径」的项一律不渲染 —— 渲染成禁用态会让用户以为是
  // 临时不可用，干脆不出现更贴近 PyCharm 的多选体验。
  const selection = menu.selectedPaths?.length ? menu.selectedPaths : [path];
  const isBatch = selection.length > 1;

  if (isBatch) {
    return (
      <CtxMenuShell className="project-tree-ctx-menu" style={{ top, left }} onClose={onClose}>
        <Row
          label={t().treeMenu.deleteItems(selection.length)}
          danger
          onClick={act(() => onDelete(path, name, isDir, selection))}
        />
      </CtxMenuShell>
    );
  }

  return (
    <CtxMenuShell className="project-tree-ctx-menu" style={{ top, left }} onClose={onClose}>
      {newSubmenu}
      <CtxSep />
      {clipboardRows}
      <CtxSep />
      <Row label={t().treeMenu.rename} onClick={act(() => onRename(path, name))} />
      {onFindInPath && isDir && (
        <>
          <CtxSep />
          <Row label={t().treeMenu.findInFiles} onClick={act(() => onFindInPath(path))} />
        </>
      )}
      <CtxSep />
      {gitSubmenu}
      <CtxSep />
      <Row label={t().namePrompt.delete} danger onClick={act(() => onDelete(path, name, isDir))} />
      <CtxSep />
      {isDir && (
        <>
          <Row label={t().treeMenu.expandAll} onClick={act(() => onExpandAll(path))} />
          <Row label={t().treeMenu.collapseAll} onClick={act(() => onCollapseAll(path))} />
        </>
      )}
      <Row label={t().treeMenu.reloadFromDisk} onClick={act(() => onReload(path, isDir))} />
      {openInSubmenu}
      {!isDir && (
        <>
          <CtxSep />
          <Row label={t().treeMenu.compareWith} onClick={act(() => onCompareWith(path))} />
        </>
      )}
    </CtxMenuShell>
  );
};
