// 分支右键菜单 · 对齐 PyCharm 分支树
//
// 单选本地分支：Checkout / New Branch / Checkout and Rebase / Compare /
//               Show Diff with Working Tree / Rebase / Merge / Update / Push /
//               Rename / Delete
// 单选远程分支：Checkout / New Branch / Checkout and Rebase / Compare /
//               Show Diff / Rebase / Merge / Pull Using Rebase/Merge / Delete
// 单选标签：    Checkout / Show Diff / Merge / Push to origin / Delete
// 多选（分支/远程/标签混合）：Compare Branches / Show Files Diff /
//               Update Selected / Delete Branches

import React, { useEffect } from 'react';
import { CtxMenuShell } from './CtxMenuShell';
import type { GitBranch } from '../types';
import { t } from '../services/i18n';

export interface BranchMenuState {
  x: number;
  y: number;
  branch: GitBranch;
  /** 右键时分支树的完整多选；缺省 = 只有 branch */
  branches?: GitBranch[];
  /**
   * 菜单出现在锚点哪一侧（分支下拉点击滑出子菜单用）：
   * right = 锚点右侧滑出（默认，右键菜单恒为此值），left = 左侧滑出。
   * 只决定入场动画方向 —— 定位一律由 x/y 决定。
   */
  side?: 'right' | 'left';
}

interface BranchContextMenuProps {
  menu: BranchMenuState | null;
  currentBranch: string;
  onClose: () => void;
  onCheckout: (name: string) => void;
  onCreateBranchFrom: (refName: string) => void;
  onRename: (oldName: string) => void;
  onDelete: (branch: GitBranch) => void;
  onCompare: (refName: string) => void;
  /** Show Diff with Working Tree：分支 ↔ 工作区差异浏览器 */
  onShowDiffWithWorktree: (refName: string) => void;
  /** Update：当前分支弹 Pull 对话框，其余分支执行 fetch */
  onUpdate: (branch: GitBranch) => void;
  onPush: () => void;
  /** Merge ref 到当前分支 */
  onMergeIntoCurrent: (refName: string) => void;
  /** 当前分支变基到 ref 之上 */
  onRebaseOntoCurrent: (refName: string) => void;
  /** Pull into Current Using Merge/Rebase */
  onPullInto: (rebase: boolean) => void;
  /** 检出 ref 后把新分支变基到当前分支（PyCharm: Checkout and Rebase onto） */
  onCheckoutAndRebase: (refName: string, current: string) => void;
  /** 多选 Compare Branches：打开两条独立提交日志（不是文件差异抽屉） */
  onCompareBranches: (a: GitBranch, b: GitBranch) => void;
  /** 多选 Show Files Diff：两个 ref 的文件差异浏览器 */
  onShowFilesDiff: (a: GitBranch, b: GitBranch) => void;
  /** 多选 Update Selected：fetch 远端后刷新 */
  onUpdateSelected: (branches: GitBranch[]) => void;
  /** 多选 Delete Branches：一次确认后逐项删除 */
  onDeleteSelected: (branches: GitBranch[]) => void;
  /** 标签专用动作 */
  onCheckoutTag: (name: string) => void;
  onPushTag: (name: string) => void;
  onDeleteTag: (name: string) => void;
}

export const BranchContextMenu: React.FC<BranchContextMenuProps> = ({
  menu,
  currentBranch,
  onClose,
  onCheckout,
  onCreateBranchFrom,
  onRename,
  onDelete,
  onCompare,
  onShowDiffWithWorktree,
  onUpdate,
  onPush,
  onMergeIntoCurrent,
  onRebaseOntoCurrent,
  onPullInto,
  onCheckoutAndRebase,
  onCompareBranches,
  onShowFilesDiff,
  onUpdateSelected,
  onDeleteSelected,
  onCheckoutTag,
  onPushTag,
  onDeleteTag
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

  const { branch, x, y } = menu;
  const selected = menu.branches && menu.branches.length > 0 ? menu.branches : [branch];
  const multiple = selected.length > 1;
  const isTag = !!branch.isTag;
  const isRemote = branch.isRemote;
  const isCurrent = !isTag && (branch.isCurrent || branch.name === currentBranch);

  const top = Math.min(y, Math.max(8, window.innerHeight - (multiple ? 190 : 470)));
  // 菜单定宽 260（见 .branch-ctx-menu）：右缘不得超出视口留 4px，
  // 左缘（下拉左侧滑出的 x 已在 TitleBar 算好）不低于 4px
  const left = Math.max(4, Math.min(x, window.innerWidth - 264));
  const side = menu.side ?? 'right';

  const renderItem = (
    label: string,
    action: () => void,
    opts?: { disabled?: boolean; danger?: boolean; shortcut?: string }
  ) => (
    <button
      key={label}
      className={`ctx-item ${opts?.danger ? 'ctx-danger' : ''}`}
      disabled={opts?.disabled}
      title={label}
      onClick={() => {
        onClose();
        action();
      }}
    >
      {label}
      {opts?.shortcut && <span className="ctx-shortcut">{opts.shortcut}</span>}
    </button>
  );

  const sep = (key: string) => <div key={key} className="ctx-separator" />;

  if (multiple) {
    const canUpdate = selected.every(b => !b.isRemote && !b.isTag);
    // PyCharm：多选包含远程/标签/当前分支时不允许批量删除；比较只需 ≥2 个叶子。
    const canDelete = selected.every(b => !b.isRemote && !b.isTag && !b.isCurrent);
    const pair = selected.length === 2 ? selected : null;
    return (
      <CtxMenuShell
        className={`side-${side}`}
        style={{ top, left }}
        overlayClassName="ctx-overlay--under-titlebar"
        onClose={onClose}
      >
        {renderItem(t().branchMenu.compareBranches, () => pair && onCompareBranches(pair[0], pair[1]), {
          disabled: !pair
        })}
        {renderItem(t().branchMenu.showFilesDiff, () => pair && onShowFilesDiff(pair[0], pair[1]), {
          disabled: !pair
        })}
        {renderItem(t().branchMenu.updateSelected, () => onUpdateSelected(selected), {
          disabled: !canUpdate
        })}
        {renderItem(t().branchMenu.deleteBranches, () => onDeleteSelected(selected), {
          disabled: !canDelete,
          shortcut: '⌘⌫'
        })}
      </CtxMenuShell>
    );
  }

  return (
    <CtxMenuShell
      className={`side-${side}`}
      style={{ top, left }}
      overlayClassName="ctx-overlay--under-titlebar"
      onClose={onClose}
    >
      {isTag ? (
        <>
          {renderItem(t().branchMenu.checkoutPlain, () => onCheckoutTag(branch.name))}
          {sep('tag-sep1')}
          {renderItem(t().branchMenu.showDiff, () => onShowDiffWithWorktree(branch.name))}
          {sep('tag-sep2')}
          {renderItem(t().branchMenu.mergeInto(branch.name, currentBranch), () => onMergeIntoCurrent(branch.name))}
          {sep('tag-sep3')}
          {renderItem(t().branchMenu.pushTag, () => onPushTag(branch.name))}
          {sep('tag-sep4')}
          {renderItem(t().branchMenu.deletePlain, () => onDeleteTag(branch.name))}
        </>
      ) : isRemote ? (
        <>
          {renderItem(t().branchMenu.checkoutPlain, () => onCheckout(branch.name))}
          {renderItem(t().branchMenu.newBranchFrom(branch.name), () => onCreateBranchFrom(branch.name))}
          {renderItem(t().branchMenu.checkoutAndRebase(currentBranch, branch.name), () =>
            onCheckoutAndRebase(branch.name, currentBranch)
          )}
          {sep('remote-sep1')}
          {renderItem(t().branchMenu.compare, () => onCompare(branch.name))}
          {renderItem(t().branchMenu.showDiff, () => onShowDiffWithWorktree(branch.name))}
          {sep('remote-sep2')}
          {renderItem(t().branchMenu.rebaseOnto(currentBranch, branch.name), () => onRebaseOntoCurrent(branch.name))}
          {renderItem(t().branchMenu.mergeInto(branch.name, currentBranch), () => onMergeIntoCurrent(branch.name))}
          {sep('remote-sep3')}
          {renderItem(t().branchMenu.pullRebase(currentBranch), () => onPullInto(true))}
          {renderItem(t().branchMenu.pullMerge(currentBranch), () => onPullInto(false))}
          {sep('remote-sep4')}
          {renderItem(t().branchMenu.deletePlain, () => onDelete(branch))}
        </>
      ) : isCurrent ? (
        <>
          {renderItem(t().branchMenu.newBranchFrom(branch.name), () => onCreateBranchFrom(branch.name))}
          {renderItem(t().branchMenu.showDiff, () => onShowDiffWithWorktree(branch.name))}
          {sep('current-sep1')}
          {renderItem(t().branchMenu.updatePlain, () => onUpdate(branch))}
          {renderItem(t().branchMenu.push, () => onPush())}
          {sep('current-sep2')}
          {renderItem(t().branchMenu.rename, () => onRename(branch.name), { shortcut: 'F2' })}
        </>
      ) : (
        <>
          {renderItem(t().branchMenu.checkout(branch.name), () => onCheckout(branch.name))}
          {renderItem(t().branchMenu.newBranchFrom(branch.name), () => onCreateBranchFrom(branch.name))}
          {renderItem(t().branchMenu.checkoutAndRebase(currentBranch, branch.name), () =>
            onCheckoutAndRebase(branch.name, currentBranch)
          )}
          {sep('local-sep1')}
          {renderItem(t().branchMenu.compare, () => onCompare(branch.name))}
          {renderItem(t().branchMenu.showDiff, () => onShowDiffWithWorktree(branch.name))}
          {sep('local-sep2')}
          {renderItem(t().branchMenu.rebaseOnto(currentBranch, branch.name), () => onRebaseOntoCurrent(branch.name))}
          {renderItem(t().branchMenu.mergeInto(branch.name, currentBranch), () => onMergeIntoCurrent(branch.name))}
          {sep('local-sep3')}
          {renderItem(t().branchMenu.updatePlain, () => onUpdate(branch))}
          {renderItem(t().branchMenu.push, () => onPush())}
          {sep('local-sep4')}
          {renderItem(t().branchMenu.rename, () => onRename(branch.name), { shortcut: 'F2' })}
          {renderItem(t().branchMenu.deletePlain, () => onDelete(branch))}
        </>
      )}
    </CtxMenuShell>
  );
};
