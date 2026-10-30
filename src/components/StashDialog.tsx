// Stash Changes 对话框 · 对齐 PyCharm「Stash Changes...」
//
// 输入贮藏说明（可选）、选择是否包含未跟踪文件（-u）后创建贮藏。

import React, { useState, useEffect } from 'react';
import { t } from '../services/i18n';
import { IconWindowClose } from './FileIcons';
import { invokeCommand } from '../services/tauriBridge';
import type { GitChangedFile } from '../types';

interface StashDialogProps {
  open: boolean;
  repoPath: string;
  changedFiles: GitChangedFile[];
  busy?: boolean;
  onClose: () => void;
  /** 贮藏成功后回调（刷新数据并提示） */
  onStashed: (message: string) => void;
}

export const StashDialog: React.FC<StashDialogProps> = ({
  open,
  repoPath,
  changedFiles,
  busy = false,
  onClose,
  onStashed
}) => {
  const [message, setMessage] = useState('');
  const [includeUntracked, setIncludeUntracked] = useState(true);
  const [pushing, setPushing] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (open) {
      setMessage('');
      setError(null);
      setIncludeUntracked(true);
    }
  }, [open]);

  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape' && !pushing) onClose();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [open, pushing, onClose]);

  if (!open) return null;

  const tracked = changedFiles.filter(f => f.status !== 'U');
  const untracked = changedFiles.filter(f => f.status === 'U');

  const handleStash = async () => {
    setPushing(true);
    setError(null);
    try {
      const out = await invokeCommand<string>('git_stash_push', {
        repoPath,
        message: message.trim() || null,
        includeUntracked
      });
      if (out.includes('No local changes')) {
        onStashed(t().stash.noLocalChanges);
      } else {
        onStashed(
          message.trim()
            ? t().stash.stashed(message.trim())
            : t().stash.stashedDefault
        );
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setPushing(false);
    }
  };

  return (
    <div className="py-dialog-backdrop">
      <div className="py-pull-dialog">
        <div className="py-dialog-header">
          <span className="py-dialog-title">{t().stash.title}</span>
          <button className="py-dialog-close" onClick={onClose} title={t().common.close}><IconWindowClose size={13} /></button>
        </div>

        <div className="stash-summary">
          {tracked.length > 0 && <span>{t().stash.trackedCount(tracked.length)}</span>}
          {untracked.length > 0 && <span>{t().stash.untrackedCount(untracked.length)}</span>}
          {changedFiles.length === 0 && <span>{t().stash.cleanTree}</span>}
        </div>

        <div className="stash-message-box">
          <label className="stash-message-label" htmlFor="stash-message-input">{t().stash.messageLabel}</label>
          <input
            id="stash-message-input"
            className="stash-message-input"
            type="text"
            value={message}
            onChange={e => setMessage(e.target.value)}
            placeholder={t().stash.messagePlaceholder}
            autoFocus
          />
        </div>

        <div className="push-options-row" style={{ borderTop: 'none' }}>
          <label className="push-option">
            <input
              type="checkbox"
              checked={includeUntracked}
              onChange={e => setIncludeUntracked(e.target.checked)}
              disabled={untracked.length === 0}
            />
            <span>{t().stash.includeUntracked}</span>
          </label>
        </div>

        {error && <div className="push-error">{error}</div>}

        <div className="py-dialog-footer">
          <button className="py-btn" onClick={onClose} disabled={pushing}>{t().dialog.cancel}</button>
          <button
            className="py-btn primary"
            disabled={pushing || busy || changedFiles.length === 0}
            onClick={() => void handleStash()}
          >
            {pushing ? t().stash.stashing : 'Stash'}
          </button>
        </div>
      </div>
    </div>
  );
};
