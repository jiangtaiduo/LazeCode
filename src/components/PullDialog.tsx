// Update Project 对话框 · 对齐 PyCharm Update Project（Ctrl+T）
//
// 选择整合策略后由 App 执行 git pull：
//   Merge  —— git pull（把远程更新合并进当前分支）
//   Rebase —— git pull --rebase（把本地提交变基到远程更新之上）

import React, { useState, useEffect } from 'react';
import { t } from '../services/i18n';
import { IconWindowClose } from './FileIcons';

interface PullDialogProps {
  open: boolean;
  currentBranch: string;
  busy?: boolean;
  /** 打开时预选的策略（分支右键 Pull Using Merge/Rebase 会带值） */
  initialRebase?: boolean;
  onClose: () => void;
  /** 点击 Update：参数为是否 rebase */
  onUpdate: (rebase: boolean) => void;
}

export const PullDialog: React.FC<PullDialogProps> = ({
  open,
  currentBranch,
  busy = false,
  initialRebase = false,
  onClose,
  onUpdate
}) => {
  const [rebase, setRebase] = useState(false);

  useEffect(() => {
    if (open) setRebase(initialRebase);
  }, [open, initialRebase]);

  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape' && !busy) onClose();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [open, busy, onClose]);

  if (!open) return null;

  return (
    <div className="py-dialog-backdrop">
      <div className="py-pull-dialog">
        <div className="py-dialog-header">
          <span className="py-dialog-title">
            {t().pull.title(currentBranch)}
          </span>
          <button className="py-dialog-close" onClick={onClose} title={t().common.close}><IconWindowClose size={13} /></button>
        </div>

        <div className="pull-strategy-group">
          <label className="pull-strategy-option">
            <input
              type="radio"
              name="pull-strategy"
              checked={!rebase}
              onChange={() => setRebase(false)}
            />
            <span>
              <b>Merge</b>{t().pull.mergeDesc}
              <small>{t().pull.mergeHint}</small>
            </span>
          </label>
          <label className="pull-strategy-option">
            <input
              type="radio"
              name="pull-strategy"
              checked={rebase}
              onChange={() => setRebase(true)}
            />
            <span>
              <b>Rebase</b>{t().pull.rebaseDesc}
              <small>{t().pull.rebaseHint}</small>
            </span>
          </label>
        </div>

        <div className="py-dialog-footer">
          <button className="py-btn" onClick={onClose} disabled={busy}>
            {t().dialog.cancel}
          </button>
          <button className="py-btn primary" disabled={busy} onClick={() => onUpdate(rebase)}>
            {busy ? t().pull.updating : 'Update'}
          </button>
        </div>
      </div>
    </div>
  );
};
