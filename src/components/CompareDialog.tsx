// Compare with Current 对话框 · 对齐 PyCharm 分支比较
//
// 展示选定分支相对当前 HEAD 独有的提交，以及 HEAD...ref 三点差异的文件清单。

import React, { useState, useEffect } from 'react';
import { t } from '../services/i18n';
import { IconWindowClose } from './FileIcons';
import { invokeCommand } from '../services/tauriBridge';
import type { GitCompareResult } from '../types';
import { formatCommitDate } from '../services/commitDate';

interface CompareDialogProps {
  open: boolean;
  repoPath: string;
  refName: string;
  currentBranch: string;
  /** true = ref 与工作区比较（Show Diff with Working Tree）：只显示文件差异 */
  worktree?: boolean;
  onClose: () => void;
}

export const CompareDialog: React.FC<CompareDialogProps> = ({
  open,
  repoPath,
  refName,
  currentBranch,
  worktree = false,
  onClose
}) => {
  const [result, setResult] = useState<GitCompareResult | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!open || !repoPath || !refName) return;
    let cancelled = false;
    setLoading(true);
    setError(null);
    invokeCommand<GitCompareResult>('git_compare_refs', { repoPath, refName, againstWorktree: worktree })
      .then(r => {
        if (!cancelled) setResult(r);
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
  }, [open, repoPath, refName]);

  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [open, onClose]);

  if (!open) return null;

  return (
    <div className="py-dialog-backdrop">
      <div className="py-compare-dialog">
        <div className="py-dialog-header">
          <span className="py-dialog-title">
            {worktree ? (
              <>{t().compare.worktreeTitle(refName)}</>
            ) : (
              <>Compare <b>{refName}</b> with <b>{currentBranch}</b></>
            )}
          </span>
          <button className="py-dialog-close" onClick={onClose} title={t().common.close}><IconWindowClose size={13} /></button>
        </div>

        {error && <div className="push-error" style={{ margin: '10px 14px 0' }}>{error}</div>}

        <div className="compare-body">
          {!worktree && (
          <div className="compare-section">
            <div className="compare-section-title">
              {loading ? t().common.loading : t().compare.uniqueCommits(result?.commits.length ?? 0, refName)}
            </div>
            <div className="compare-list">
              {(result?.commits ?? []).map(c => (
                <div key={c.hash} className="push-commit-row" title={c.hash}>
                  <span className="push-commit-hash">{c.shortHash}</span>
                  <span className="push-commit-msg">{c.message}</span>
                  <span className="push-commit-meta">{c.author} · {formatCommitDate(c.date)}</span>
                </div>
              ))}
              {!loading && (result?.commits.length ?? 0) === 0 && (
                <div className="push-empty">{t().compare.noUniqueCommits}</div>
              )}
            </div>
          </div>
          )}

          <div className="compare-section">
            <div className="compare-section-title">
              {loading ? t().common.loading : t().compare.diffFileCount(result?.files.length ?? 0)}
            </div>
            <div className="compare-list">
              {(result?.files ?? []).map(f => (
                <div key={f.path} className="push-commit-row compare-file-row">
                  <span className={`det-status ${f.status === 'A' ? 'add' : f.status === 'D' ? 'del' : 'mod'}`}>
                    {f.status}
                  </span>
                  <span className="push-commit-msg" title={f.path}>{f.path}</span>
                  <span className="compare-numstat">
                    <i className="num-add">+{f.additions}</i>
                    {f.deletions > 0 && <i className="num-del">-{f.deletions}</i>}
                  </span>
                </div>
              ))}
              {!loading && (result?.files.length ?? 0) === 0 && (
                <div className="push-empty">{t().compare.noFileDiff}</div>
              )}
            </div>
          </div>
        </div>

        <div className="py-dialog-footer">
          <button className="py-btn" onClick={onClose}>{t().common.close}</button>
        </div>
      </div>
    </div>
  );
};
