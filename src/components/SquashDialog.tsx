import React, { useEffect, useRef, useState } from 'react';
import type { GitCommit } from '../types';
import { t } from '../services/i18n';
import { IconWindowClose } from './FileIcons';

interface SquashDialogProps {
  commits: GitCommit[] | null;
  onConfirm: (message: string) => void;
  onClose: () => void;
  busy?: boolean;
}

export const SquashDialog: React.FC<SquashDialogProps> = ({
  commits,
  onConfirm,
  onClose,
  busy = false
}) => {
  const [message, setMessage] = useState('');
  const textareaRef = useRef<HTMLTextAreaElement>(null);

  useEffect(() => {
    if (commits && commits.length > 0) {
      // 提交信息按时间从旧到新合并，或者直接按选中的提交信息拼装
      // 过滤空消息，以双换行分隔（对齐 PyCharm 默认合并行为）
      const initial = commits
        .map(c => c.message.trim())
        .filter(Boolean)
        .join('\n\n');
      setMessage(initial);
      setTimeout(() => {
        if (textareaRef.current) {
          textareaRef.current.focus();
          textareaRef.current.select();
        }
      }, 50);
    }
  }, [commits]);

  useEffect(() => {
    if (!commits) return;
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.preventDefault();
        onClose();
      } else if ((e.metaKey || e.ctrlKey) && e.key === 'Enter') {
        e.preventDefault();
        if (message.trim() && !busy) {
          onConfirm(message.trim());
        }
      }
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [commits, message, busy, onConfirm, onClose]);

  if (!commits || commits.length < 2) return null;

  const trimmed = message.trim();
  const canSubmit = trimmed.length > 0 && !busy;

  const handleSubmit = () => {
    if (canSubmit) {
      onConfirm(trimmed);
    }
  };

  return (
    <div className="py-dialog-backdrop">
      <div className="py-name-dialog py-squash-dialog">
        <div className="py-dialog-header">
          <span className="py-dialog-title">{t().squashDialog.title}</span>
          <button className="py-dialog-close" onClick={onClose} title={t().common.close}>
            <IconWindowClose size={13} />
          </button>
        </div>

        <div className="py-squash-summary">
          {t().squashDialog.summary(commits.length)}
        </div>

        <div className="py-squash-commits-list">
          {commits.map(c => (
            <div key={c.hash} className="py-squash-commit-item">
              <span className="py-squash-hash">{c.shortHash}</span>
              <span className="py-squash-msg">{c.message.split('\n')[0]}</span>
            </div>
          ))}
        </div>

        <div className="py-squash-label">
          {t().squashDialog.messageLabel}
        </div>

        <textarea
          ref={textareaRef}
          className="py-squash-textarea"
          value={message}
          onChange={e => setMessage(e.target.value)}
          rows={6}
          placeholder={t().commitDrawer.messagePlaceholder}
          spellCheck={false}
        />

        <div className="py-dialog-footer">
          <button className="py-name-btn" onClick={onClose} disabled={busy}>
            {t().squashDialog.cancelBtn}
          </button>
          <button
            className="py-name-btn primary"
            disabled={!canSubmit}
            onClick={handleSubmit}
          >
            {busy ? t().common.loading : t().squashDialog.squashBtn}
          </button>
        </div>
      </div>
    </div>
  );
};
