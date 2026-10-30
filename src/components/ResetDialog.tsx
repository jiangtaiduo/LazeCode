// Reset 对话框 · 对齐 PyCharm「Reset Current Branch to Here」
//
// 选择重置模式后由 App 执行 git reset：
//   Soft  —— 移动 HEAD，更改保留在暂存区
//   Mixed —— 移动 HEAD，更改保留在工作区（默认）
//   Hard  —— 丢弃所有更改（危险）
//   Keep  —— 移动 HEAD，尽量保留工作区，冲突文件回滚

import React, { useEffect, useState } from 'react';
import { t } from '../services/i18n';
import { IconWindowClose } from './FileIcons';

export type ResetMode = 'soft' | 'mixed' | 'hard' | 'keep';

interface ResetDialogProps {
  open: boolean;
  commitHash: string;
  commitMessage: string;
  currentBranch: string;
  busy?: boolean;
  onClose: () => void;
  onConfirm: (mode: ResetMode) => void;
}

const MODE_OPTIONS: { mode: ResetMode; label: string; descKey: 'softDesc' | 'mixedDesc' | 'hardDesc' | 'keepDesc' }[] = [
  { mode: 'soft', label: 'Soft', descKey: 'softDesc' },
  { mode: 'mixed', label: 'Mixed', descKey: 'mixedDesc' },
  { mode: 'hard', label: 'Hard', descKey: 'hardDesc' },
  { mode: 'keep', label: 'Keep', descKey: 'keepDesc' }
];

export const ResetDialog: React.FC<ResetDialogProps> = ({
  open,
  commitHash,
  commitMessage,
  currentBranch,
  busy = false,
  onClose,
  onConfirm
}) => {
  const [mode, setMode] = useState<ResetMode>('mixed');

  useEffect(() => {
    if (open) setMode('mixed');
  }, [open]);

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
            Reset <b>{currentBranch}</b> to <b>{commitHash.slice(0, 8)}</b>
          </span>
          <button className="py-dialog-close" onClick={onClose} title={t().common.close}><IconWindowClose size={13} /></button>
        </div>

        <div className="reset-commit-info" title={commitMessage}>
          {commitMessage || t().reset.noCommitMessage}
        </div>

        <div className="pull-strategy-group">
          {MODE_OPTIONS.map(opt => (
            <label key={opt.mode} className="pull-strategy-option">
              <input
                type="radio"
                name="reset-mode"
                checked={mode === opt.mode}
                onChange={() => setMode(opt.mode)}
              />
              <span>
                <b>{opt.label}</b>
                <small>{t().reset[opt.descKey]}</small>
              </span>
            </label>
          ))}
        </div>

        {mode === 'hard' && (
          <div className="reset-hard-warning">{t().reset.hardWarning}</div>
        )}

        <div className="py-dialog-footer">
          <button className="py-btn" onClick={onClose} disabled={busy}>
            {t().dialog.cancel}
          </button>
          <button className={`py-btn ${mode === 'hard' ? 'danger' : 'primary'}`} disabled={busy} onClick={() => onConfirm(mode)}>
            {busy ? t().reset.resetting : 'Reset'}
          </button>
        </div>
      </div>
    </div>
  );
};
