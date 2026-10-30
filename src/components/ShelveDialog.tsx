// Shelve 贮藏架对话框 · 对齐 PyCharm Shelve/Unshelve
//
// 补丁持久化在仓库 .git/ide-shelves/ 下。Unshelve 应用补丁到工作区（可选择是否删除条目）。

import React, { useState, useEffect } from 'react';
import { appConfirm } from '../services/confirmDialog';
import { IconWindowClose } from './FileIcons';
import { invokeCommand } from '../services/tauriBridge';
import type { Shelf } from '../types';
import { t } from '../services/i18n';

interface ShelveDialogProps {
  open: boolean;
  repoPath: string;
  busy?: boolean;
  onClose: () => void;
  onChanged: (message: string) => void;
}

export const ShelveDialog: React.FC<ShelveDialogProps> = ({
  open,
  repoPath,
  busy = false,
  onClose,
  onChanged
}) => {
  const [shelves, setShelves] = useState<Shelf[]>([]);
  const [selected, setSelected] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [acting, setActing] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const load = async () => {
    setLoading(true);
    setError(null);
    try {
      const list = await invokeCommand<Shelf[]>('git_list_shelves', { repoPath });
      setShelves(list);
      setSelected(prev => (list.some(s => s.name === prev) ? prev : list[0]?.name ?? null));
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    if (open && repoPath) void load();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open, repoPath]);

  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape' && !acting && !busy) onClose();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [open, acting, busy, onClose]);

  const unshelve = async (keep: boolean) => {
    if (!selected) return;
    setActing(true);
    setError(null);
    try {
      await invokeCommand('git_apply_shelf', { repoPath, name: selected });
      if (!keep) {
        await invokeCommand('git_delete_shelf', { repoPath, name: selected });
      }
      onChanged(keep ? t().shelve.unshelvedKeep(selected) : t().shelve.unshelvedDeleted(selected));
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err);
      setError(
        message.includes('does not apply') || message.includes('conflict')
          ? t().shelve.applyFailed(message)
          : message
      );
    } finally {
      setActing(false);
      await load();
    }
  };

  const remove = async () => {
    if (!selected) return;
    if (!(await appConfirm(t().dialog.deleteShelfConfirm(selected), { title: t().dialog.deleteShelfTitle, kind: 'warning', okLabel: 'Delete' }))) return;
    setActing(true);
    try {
      await invokeCommand('git_delete_shelf', { repoPath, name: selected });
      await load();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setActing(false);
    }
  };

  if (!open) return null;

  return (
    <div className="py-dialog-backdrop">
      <div className="py-pull-dialog">
        <div className="py-dialog-header">
          <span className="py-dialog-title">{t().shelve.title}</span>
          <button className="py-dialog-close" onClick={onClose} title={t().common.close}><IconWindowClose size={13} /></button>
        </div>

        {error && <div className="push-error" style={{ margin: '10px 14px 0' }}>{error}</div>}

        <div className="push-commits-list" style={{ minHeight: 180, maxHeight: 320 }}>
          {loading && <div className="push-empty">{t().common.loading}</div>}
          {!loading && shelves.length === 0 && !error && (
            <div className="push-empty">{t().shelve.emptyHint}</div>
          )}
          {shelves.map(s => (
            <div
              key={s.name}
              className={`push-commit-row ${selected === s.name ? 'selected' : ''}`}
              onClick={() => setSelected(s.name)}
              style={{ cursor: 'pointer' }}
            >
              <span className="push-commit-msg" title={s.name}>{s.name}</span>
              <span className="push-commit-meta">{s.date}</span>
            </div>
          ))}
        </div>

        <div className="py-dialog-footer" style={{ justifyContent: 'space-between' }}>
          <button className="py-btn danger" disabled={!selected || acting || busy} onClick={() => void remove()}>
            Delete
          </button>
          <div style={{ display: 'flex', gap: '8px' }}>
            <button className="py-btn" disabled={!selected || acting || busy} onClick={() => void unshelve(true)} title={t().shelve.unshelveKeepTooltip}>
              {t().shelve.unshelveKeepLabel}
            </button>
            <button className="py-btn primary" disabled={!selected || acting || busy} onClick={() => void unshelve(false)} title={t().shelve.unshelveDeleteTooltip}>
              Unshelve and Delete
            </button>
          </div>
        </div>
      </div>
    </div>
  );
};
