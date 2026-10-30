// 提交右键菜单 · 对齐 PyCharm Log 提交上下文操作
//
// 菜单项：Copy Revision Number / Create Patch... / Cherry-Pick / Checkout Revision
//         / Reset Current Branch to Here... / Revert Commit / Undo Commit（仅 HEAD）
//         / Edit Commit Message（仅 HEAD）/ Drop Commit / New Branch... / New Tag...

import React, { useEffect } from 'react';
import { CtxMenuShell } from './CtxMenuShell';
import type { GitCommit } from '../types';
import { t } from '../services/i18n';

export interface CommitMenuState {
  x: number;
  y: number;
  commit: GitCommit;
  selectedCommits?: GitCommit[];
}

interface CommitContextMenuProps {
  menu: CommitMenuState | null;
  onClose: () => void;
  onCopyHash: (commit: GitCommit) => void;
  onCheckoutRevision: (commit: GitCommit) => void;
  onCherryPick: (commit: GitCommit) => void;
  onNewTag: (commit: GitCommit) => void;
  onNewBranch: (commit: GitCommit) => void;
  onReset: (commit: GitCommit) => void;
  onUndo: (commit: GitCommit) => void;
  onRevert: (commit: GitCommit) => void;
  /** 编辑 HEAD 提交信息（amend） */
  onEditMessage: (commit: GitCommit) => void;
  /** 摘除提交（Drop Commit） */
  onDrop: (commit: GitCommit) => void;
  /** 补丁文本 → 剪贴板 */
  onCopyPatch: (commit: GitCommit) => void;
  /** 补丁文本 → 另存文件 */
  onCreatePatch: (commit: GitCommit) => void;
  /** 交互式变基（以该提交为基点） */
  onInteractiveRebase: (commit: GitCommit) => void;
  /** 跳转到父/子提交 */
  onGoToParent: (commit: GitCommit) => void;
  onGoToChild: (commit: GitCommit) => void;
  canGoParent: boolean;
  canGoChild: boolean;
  onCompareVersions?: (c1: GitCommit, c2: GitCommit) => void;
  /** 多选批量动作；缺省时回落到单选第一条。 */
  onCherryPickMany?: (commits: GitCommit[]) => void;
  onRevertMany?: (commits: GitCommit[]) => void;
  onDropMany?: (commits: GitCommit[]) => void;
  onCreatePatchMany?: (commits: GitCommit[]) => void;
  onSquashCommits?: (commits: GitCommit[]) => void;
}
export const CommitContextMenu: React.FC<CommitContextMenuProps> = ({
  menu,
  onClose,
  onCopyHash,
  onCheckoutRevision,
  onCherryPick,
  onNewTag,
  onNewBranch,
  onReset,
  onUndo,
  onRevert,
  onEditMessage,
  onDrop,
  onCopyPatch,
  onCreatePatch,
  onInteractiveRebase,
  onGoToParent,
  onGoToChild,
  canGoParent,
  canGoChild,
  onCompareVersions,
  onCherryPickMany,
  onRevertMany,
  onDropMany,
  onCreatePatchMany,
  onSquashCommits
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

  const { commit, x, y } = menu;
  const isHead = commit.branchTags?.some(t => t.type === 'head') ?? false;
  const isMerge = commit.isMerge ?? false;
  // 已被远程引用指向的提交视为已推送，禁止改写历史（Drop/Edit Message）
  const pushed = commit.branchTags?.some(t => t.type === 'remote') ?? false;
  const short = commit.shortHash;

  const top = Math.min(y, window.innerHeight - 420);
  const left = Math.min(x, window.innerWidth - 280);

  const renderItem = (
    label: string,
    action: () => void,
    opts?: { disabled?: boolean; danger?: boolean; shortcut?: string }
  ) => (
    <button
      key={label}
      className={`ctx-item ctx-row ${opts?.danger ? 'ctx-danger' : ''}`}
      disabled={opts?.disabled}
      onClick={() => {
        onClose();
        action();
      }}
    >
      <span className="ctx-item-label">{label}</span>
      {opts?.shortcut && (
        <span className="ctx-item-shortcut" style={{ marginLeft: 'auto', paddingLeft: 16, fontSize: 11, color: 'var(--text-muted)' }}>
          {opts.shortcut}
        </span>
      )}
    </button>
  );

  const sep = (key: string) => <div key={key} className="ctx-separator" />;

  const isMultiCommit = menu.selectedCommits && menu.selectedCommits.length > 1;
  const selectedCommits = isMultiCommit ? menu.selectedCommits! : [];
  const isExactlyTwo = selectedCommits.length === 2;
  // 批量 Drop 与单条用同一套护栏：任一选中项是 merge 或已被远程引用，
  // 都不能从菜单发起历史改写（后端仍会独立复核）。
  const batchHasMerge = selectedCommits.some(c => c.isMerge);
  const batchPushed = selectedCommits.some(c => c.branchTags?.some(t => t.type === 'remote'));

  if (isMultiCommit) {
    // 对齐 PyCharm：多选多个提交时的上下文菜单。只有选择两个时 Compare Versions 才可用！
    return (
      <CtxMenuShell style={{ top, left }} onClose={onClose}>
        {renderItem(t().commitMenu.copyRevision, () => {
          const hashes = selectedCommits.map(c => c.hash).join('\n');
          void navigator.clipboard.writeText(hashes);
        }, {
          shortcut: '⌥⇧⌘C'
        })}
        {renderItem(t().commitMenu.createPatch, () => {
          if (onCreatePatchMany) onCreatePatchMany(selectedCommits);
          else onCreatePatch(commit);
        })}
        {renderItem(t().commitMenu.cherryPick(commit.shortHash), () => {
          if (onCherryPickMany) onCherryPickMany(selectedCommits);
          else onCherryPick(commit);
        })}
        {sep('s0')}
        {renderItem(t().commitMenu.compareVersions, () => {
          if (isExactlyTwo) {
            onCompareVersions?.(selectedCommits[0], selectedCommits[1]);
          }
        }, {
          disabled: !isExactlyTwo
        })}
        {sep('s1')}
        {renderItem(t().commitMenu.revertCommits, () => {
          if (onRevertMany) onRevertMany(selectedCommits);
          else onRevert(commit);
        })}
        {sep('s2')}
        {renderItem(t().commitMenu.dropCommits, () => {
          if (onDropMany) onDropMany(selectedCommits);
          else onDrop(commit);
        }, {
          danger: true,
          disabled: batchHasMerge || batchPushed
        })}
        {renderItem(t().commitMenu.squashCommits, () => {
          onSquashCommits?.(selectedCommits);
        }, {
          disabled: batchHasMerge || batchPushed
        })}
      </CtxMenuShell>
    );
  }

  return (
    <CtxMenuShell style={{ top, left }} onClose={onClose}>
      {renderItem(t().commitMenu.copyRevision, () => onCopyHash(commit))}
      {renderItem(t().commitMenu.createPatch, () => onCreatePatch(commit))}
      {sep('s0')}
      {renderItem(t().commitMenu.cherryPick(short), () => onCherryPick(commit), {
        disabled: isHead
      })}
      {renderItem(t().commitMenu.checkoutRevision(short), () => onCheckoutRevision(commit))}
      {sep('s1')}
      {renderItem(t().commitMenu.resetHere, () => onReset(commit))}
      {renderItem(t().commitMenu.revert(short), () => onRevert(commit), { disabled: isHead })}
      {isHead && renderItem(t().commitMenu.undo(short), () => onUndo(commit), { danger: true })}
      {sep('s2')}
      {isHead && !isMerge && renderItem(t().commitMenu.editMessage, () => onEditMessage(commit))}
      {!isMerge && !pushed && renderItem(t().commitMenu.drop(short), () => onDrop(commit), { danger: true })}
      {sep('s3')}
      {renderItem(t().commitMenu.newBranchFrom(short), () => onNewBranch(commit))}
      {renderItem(t().commitMenu.newTagFrom(short), () => onNewTag(commit))}
      {sep('s4')}
      {renderItem(t().commitMenu.copyPatch, () => onCopyPatch(commit))}
      {sep('s5')}
      {renderItem(t().commitMenu.interactiveRebase, () => onInteractiveRebase(commit), { disabled: pushed })}
      {sep('s6')}
      {renderItem(t().commitMenu.goToChild, () => onGoToChild(commit), { disabled: !canGoChild })}
      {renderItem(t().commitMenu.goToParent, () => onGoToParent(commit), { disabled: !canGoParent })}
    </CtxMenuShell>
  );
};
