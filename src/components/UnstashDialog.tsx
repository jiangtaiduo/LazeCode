// Unstash Changes 对话框 · 对齐 PyCharm「Unstash Changes...」
//
// 左侧贮藏列表，右侧选中贮藏的文件清单；
// 底部动作：Apply（恢复并保留）/ Pop（恢复并删除）/ Drop（删除，需确认）。

import React, { useState, useEffect } from 'react';
import { appConfirm } from '../services/confirmDialog';
import { IconWindowClose } from './FileIcons';
import { invokeCommand } from '../services/tauriBridge';
import type { GitStash, GitCommitFile } from '../types';
import { t } from '../services/i18n';

interface UnstashDialogProps {
  open: boolean;
  repoPath: string;
  busy?: boolean;
  onClose: () => void;
  /** 任一操作成功后回调（刷新数据） */
  onChanged: (message: string) => void;
}

export const UnstashDialog: React.FC<UnstashDialogProps> = ({
  open,
  repoPath,
  busy = false,
  onClose,
  onChanged
}) => {
  const [stashes, setStashes] = useState<GitStash[]>([]);
  const [selected, setSelected] = useState<string | null>(null);
  const [files, setFiles] = useState<GitCommitFile[]>([]);
  const [loading, setLoading] = useState(false);
  const [filesLoading, setFilesLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [acting, setActing] = useState(false);

  useEffect(() => {
    if (!open || !repoPath) return;
    let cancelled = false;
    setLoading(true);
    setError(null);
    invokeCommand<GitStash[]>('git_stash_list', { repoPath })
      .then(list => {
        if (cancelled) return;
        setStashes(list);
        setSelected(list[0]?.selector ?? null);
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
  }, [open, repoPath]);

  // 选中贮藏变化时拉取文件清单
  useEffect(() => {
    if (!open || !selected || !repoPath) {
      setFiles([]);
      return;
    }
    let cancelled = false;
    setFilesLoading(true);
    invokeCommand<GitCommitFile[]>('git_stash_files', { repoPath, stash: selected })
      .then(list => {
        if (!cancelled) setFiles(list);
      })
      .catch(() => {
        if (!cancelled) setFiles([]);
      })
      .finally(() => {
        if (!cancelled) setFilesLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [open, selected, repoPath]);

  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape' && !acting && !busy) onClose();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [open, acting, busy, onClose]);

  if (!open) return null;

  const run = async (action: 'apply' | 'pop' | 'drop') => {
    if (!selected) return;
    if (action === 'drop' && !(await appConfirm(t().dialog.deleteStashConfirm(selected), { title: t().dialog.deleteStashTitle, kind: 'warning', okLabel: 'Delete' }))) return;
    setActing(true);
    setError(null);
    try {
      if (action === 'drop') {
        await invokeCommand<string>('git_stash_drop', { repoPath, stash: selected });
        onChanged(t().unstash.dropped(selected));
        return;
      }
      const out = await invokeCommand<string>('git_stash_apply', {
        repoPath,
        stash: selected,
        pop: action === 'pop'
      });
      if (out.includes('CONFLICT')) {
        setError(out);
        return;
      }
      onChanged(action === 'pop' ? t().unstash.popped(selected) : t().unstash.applied(selected));
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err);
      setError(
        message.includes('CONFLICT')
          ? t().unstash.conflictHint(message)
          : message
      );
    } finally {
      setActing(false);
    }
  };

  return (
    <div className="py-dialog-backdrop">
      <div className="py-history-dialog" style={{ height: 'min(520px, calc(100vh - 120px))' }}>
        <div className="py-dialog-header">
          <span className="py-dialog-title">{t().unstash.title}</span>
          <button className="py-dialog-close" onClick={onClose} title={t().common.close}><IconWindowClose size={13} /></button>
        </div>

        <div className="history-body">
          <div className="history-revision-list">
            {error && <div className="history-error">{error}</div>}
            {loading && <div className="history-empty">{t().common.loading}</div>}
            {!loading && stashes.length === 0 && !error && (
              <div className="history-empty">{t().unstash.empty}</div>
            )}
            {stashes.map(s => (
              <div
                key={s.selector}
                className={`history-revision-row ${selected === s.selector ? 'active' : ''}`}
                onClick={() => setSelected(s.selector)}
                title={s.message}
              >
                <div className="history-rev-top">
                  <span className="history-rev-hash">{s.selector}</span>
                  <span className="history-rev-date">{s.date}</span>
                </div>
                <div className="history-rev-msg">{s.message}</div>
                {s.branch && <div className="history-rev-author">{t().unstash.basedOn(s.branch)}</div>}
              </div>
            ))}
          </div>

          <div className="stash-files-pane">
            <div className="compare-section-title">
              {filesLoading ? t().common.loading : t().unstash.fileCount(files.length)}
            </div>
            <div className="stash-files-list">
              {files.map(f => (
                <div key={f.path} className="push-commit-row compare-file-row" title={f.path}>
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
              {!filesLoading && selected && files.length === 0 && (
                <div className="history-empty">{t().unstash.noFileChanges}</div>
              )}
              {!selected && !loading && (
                <div className="history-empty">{t().unstash.selectStash}</div>
              )}
            </div>
          </div>
        </div>

        {error && <div className="push-error" style={{ margin: '0 14px 8px' }}>{error}</div>}

        <div className="py-dialog-footer" style={{ justifyContent: 'space-between' }}>
          <button
            className="py-btn danger"
            disabled={!selected || acting || busy}
            onClick={() => void run('drop')}
            title={t().unstash.dropTooltip}
          >
            Drop
          </button>
          <div style={{ display: 'flex', gap: '8px' }}>
            <button className="py-btn" disabled={!selected || acting || busy} onClick={() => void run('apply')} title={t().unstash.applyTooltip}>
              Apply
            </button>
            <button className="py-btn primary" disabled={!selected || acting || busy} onClick={() => void run('pop')} title={t().unstash.popTooltip}>
              Pop
            </button>
          </div>
        </div>
      </div>
    </div>
  );
};
