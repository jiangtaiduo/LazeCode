// 交互式变基对话框 · 对齐 PyCharm「Interactively Rebase from Here...」
//
// 以选中提交为 base，对 base..HEAD 的提交逐条设置动作（Pick/Squash/Fixup/Drop），
// 支持上下调整顺序；确认后由后端通过 GIT_SEQUENCE_EDITOR 注入 todo 执行 git rebase -i。
// 注：Reword 需要逐提交打开编辑器，此处未提供（HEAD 提交可用右键 Edit Commit Message）。

import React, { useState, useEffect, useMemo } from 'react';
import { appConfirm } from '../services/confirmDialog';
import { IconWindowClose, IconChevronUp, IconChevronDown } from './FileIcons';
import { invokeCommand } from '../services/tauriBridge';
import type { GitCommit } from '../types';
import { t } from '../services/i18n';

export type RebaseAction = 'pick' | 'squash' | 'fixup' | 'drop';

interface InteractiveRebaseDialogProps {
  open: boolean;
  repoPath: string;
  /** 作为变基基点（不含在 todo 内）的提交 */
  baseCommit: GitCommit;
  /** 当前已加载的提交列表（含 baseCommit 与其后代） */
  commits: GitCommit[];
  busy?: boolean;
  onClose: () => void;
  onDone: (message: string) => void;
}

interface TodoRow {
  commit: GitCommit;
  action: RebaseAction;
}

export const InteractiveRebaseDialog: React.FC<InteractiveRebaseDialogProps> = ({
  open,
  repoPath,
  baseCommit,
  commits,
  busy = false,
  onClose,
  onDone
}) => {
  const [rows, setRows] = useState<TodoRow[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [running, setRunning] = useState(false);

  // 打开时构建 todo：base 之后（更新）的提交按 git log 顺序
  useEffect(() => {
    if (!open) return;
    setError(null);
    const idx = commits.findIndex(c => c.hash === baseCommit.hash);
    const rest = idx >= 0 ? commits.slice(0, idx) : [];
    setRows(rest.map(c => ({ commit: c, action: 'pick' as RebaseAction })));
  }, [open, baseCommit.hash, commits]);

  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape' && !running && !busy) onClose();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [open, running, busy, onClose]);

  const update = (idx: number, patch: Partial<TodoRow>) => {
    setRows(prev => prev.map((r, i) => (i === idx ? { ...r, ...patch } : r)));
  };

  const move = (idx: number, dir: -1 | 1) => {
    setRows(prev => {
      const next = [...prev];
      const target = idx + dir;
      if (target < 0 || target >= next.length) return prev;
      [next[idx], next[target]] = [next[target], next[idx]];
      return next;
    });
  };

  const firstNonDrop = useMemo(() => rows.find(r => r.action !== 'drop'), [rows]);

  const canRun = rows.length > 0 && firstNonDrop !== undefined && !running && !busy;

  const handleRun = async () => {
    if (!firstNonDrop) return;
    if (!(await appConfirm(t().dialog.interactiveRebaseConfirm(baseCommit.shortHash, rows.length), { title: t().dialog.interactiveRebaseTitle, kind: 'warning', okLabel: 'Start Rebase' }))) return;
    setRunning(true);
    setError(null);
    const todo = rows
      .filter(r => r.action !== 'drop')
      .map(r => `${r.action} ${r.commit.hash} ${r.commit.message}`);
    // drop 行也写进去，语义等价（git 跳过）
    const full = rows.map(r => `${r.action} ${r.commit.hash} ${r.commit.message}`);
    try {
      await invokeCommand('git_rebase_interactive', { repoPath, base: baseCommit.hash, todo: full.length ? full : todo });
      onDone(t().rebase.done(rows.length));
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err);
      setError(
        message.includes('CONFLICT') || message.includes('could not apply')
          ? t().rebase.conflictHint(message)
          : message
      );
    } finally {
      setRunning(false);
    }
  };

  if (!open) return null;

  const short = (c: GitCommit) => c.shortHash;

  return (
    <div className="py-dialog-backdrop">
      <div className="py-history-dialog" style={{ height: 'min(600px, calc(100vh - 110px))' }}>
        <div className="py-dialog-header">
          <span className="py-dialog-title">
            Interactively Rebase from <b>{short(baseCommit)}</b>
          </span>
          <button className="py-dialog-close" onClick={onClose} title={t().common.close}><IconWindowClose size={13} /></button>
        </div>

        {error && <div className="push-error" style={{ margin: '10px 14px 0' }}>{error}</div>}

        <div className="rebase-hint">{t().rebase.hint}</div>

        <div className="rebase-rows">
          {rows.length === 0 && <div className="push-empty">{t().rebase.noCommits}</div>}
          {rows.map((row, idx) => (
            <div key={row.commit.hash} className="rebase-row">
              <select
                className="rebase-action-select"
                value={row.action}
                onChange={e => update(idx, { action: e.target.value as RebaseAction })}
              >
                <option value="pick">pick</option>
                <option value="squash">squash</option>
                <option value="fixup">fixup</option>
                <option value="drop">drop</option>
              </select>
              <span className={`rebase-hash ${row.action === 'drop' ? 'dropped' : ''}`}>{short(row.commit)}</span>
              <span className={`rebase-msg ${row.action === 'drop' ? 'dropped' : ''}`} title={row.commit.message}>
                {row.commit.message}
              </span>
              <span className="rebase-move-btns">
                <button className="f-act-btn" disabled={idx === 0} onClick={() => move(idx, -1)} title={t().rebase.moveUp}><IconChevronUp size={12} /></button>
                <button className="f-act-btn" disabled={idx === rows.length - 1} onClick={() => move(idx, 1)} title={t().rebase.moveDown}><IconChevronDown size={12} /></button>
              </span>
            </div>
          ))}
        </div>

        <div className="py-dialog-footer">
          <button className="py-btn" onClick={onClose} disabled={running || busy}>{t().dialog.cancel}</button>
          <button className="py-btn primary" disabled={!canRun} onClick={() => void handleRun()}>
            {running ? t().rebase.running : 'Start Rebase'}
          </button>
        </div>
      </div>
    </div>
  );
};
