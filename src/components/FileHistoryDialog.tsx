// 文件历史对话框 · 对齐 PyCharm「Show History for File」
//
// 左侧：该文件的提交列表（--follow 跟踪重命名）；
// 右侧：内嵌 DiffViewer，两种视图 ——
//   此提交的变更（该提交相对其父提交对该文件的改动）
//   与工作区比较（该提交版本 ↔ 当前工作区）

import React, { useState, useEffect } from 'react';
import { IconWindowClose } from './FileIcons';
import { invokeCommand } from '../services/tauriBridge';
import { t } from '../services/i18n';
import { DiffViewer } from './DiffViewer';
import type { GitCommit } from '../types';
import { formatCommitDate } from '../services/commitDate';

interface FileHistoryDialogProps {
  open: boolean;
  repoPath: string;
  filePath: string;
  onClose: () => void;
}

export const FileHistoryDialog: React.FC<FileHistoryDialogProps> = ({
  open,
  repoPath,
  filePath,
  onClose
}) => {
  const [revisions, setRevisions] = useState<GitCommit[]>([]);
  const [selected, setSelected] = useState<string | null>(null);
  const [mode, setMode] = useState<'commit' | 'worktree'>('commit');
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!open || !repoPath || !filePath) return;
    let cancelled = false;
    setLoading(true);
    setError(null);
    setRevisions([]);
    setSelected(null);
    invokeCommand<GitCommit[]>('git_file_history', { repoPath, path: filePath, maxCount: 200 })
      .then(list => {
        if (cancelled) return;
        setRevisions(list);
        setSelected(list[0]?.hash ?? null);
      })
      .catch(err => {
        if (!cancelled) setError(err instanceof Error ? err.message : String(err));
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [open, repoPath, filePath]);

  // Esc 关闭
  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [open, onClose]);

  if (!open) return null;

  const fileName = filePath.split(/[\\/]/).pop() ?? filePath;

  return (
    <div className="py-dialog-backdrop">
      <div className="py-history-dialog">
        <div className="py-dialog-header">
          <span className="py-dialog-title">
            History: <b title={filePath}>{fileName}</b>
          </span>
          <button className="py-dialog-close" onClick={onClose} title={`${t().common.close} (Esc)`}><IconWindowClose size={13} /></button>
        </div>

        <div className="history-body">
          <div className="history-revision-list">
            <div className="history-mode-toggle">
              <button
                className={`history-mode-btn ${mode === 'commit' ? 'active' : ''}`}
                onClick={() => setMode('commit')}
                title={t().fileHistory.commitModeTooltip}
              >
                {t().fileHistory.commitMode}
              </button>
              <button
                className={`history-mode-btn ${mode === 'worktree' ? 'active' : ''}`}
                onClick={() => setMode('worktree')}
                title={t().fileHistory.worktreeModeTooltip}
              >
                {t().fileHistory.worktreeMode}
              </button>
            </div>

            {error && <div className="history-error">{error}</div>}
            {loading && <div className="history-empty">{t().common.loading}</div>}
            {!loading && !error && revisions.length === 0 && (
              <div className="history-empty">{t().fileHistory.noHistory}</div>
            )}

            {revisions.map(c => (
              <div
                key={c.hash}
                className={`history-revision-row ${selected === c.hash ? 'active' : ''}`}
                onClick={() => setSelected(c.hash)}
                title={c.message}
              >
                <div className="history-rev-top">
                  <span className="history-rev-hash">{c.shortHash}</span>
                  <span className="history-rev-date">{formatCommitDate(c.date)}</span>
                </div>
                <div className="history-rev-msg">{c.message}</div>
                <div className="history-rev-author">{c.author}</div>
              </div>
            ))}
          </div>

          <div className="history-diff-pane">
            {selected ? (
              <DiffViewer
                filePath={filePath}
                repoPath={repoPath}
                showCommit={mode === 'commit' ? selected : null}
                againstRef={mode === 'worktree' ? selected : null}
              />
            ) : (
              <div className="history-empty" style={{ flex: 1, display: 'flex', alignItems: 'center', justifyContent: 'center' }}>
                {loading ? t().common.loading : t().fileHistory.selectCommit}
              </div>
            )}
          </div>
        </div>
      </div>
    </div>
  );
};
