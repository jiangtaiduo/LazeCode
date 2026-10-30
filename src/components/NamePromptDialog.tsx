// 项目树「新建 / 重命名 / 删除确认」轻量对话框 · 对齐 PyCharm 的输入式弹窗

import React, { useEffect, useRef, useState } from 'react';
import { t } from '../services/i18n';
import { IconWindowClose } from './FileIcons';

export interface NameDialogState {
  mode: 'new-file' | 'new-dir' | 'rename' | 'delete';
  /** new-* 模式：创建目标目录；rename/delete：原路径 */
  targetDir?: string;
  path?: string;
  name?: string;
  isDir?: boolean;
  /**
   * delete 的批量形态：多选 >= 2 项时给出全部待删路径（含 `path` 自身）。
   * 对话框据此把确认文案从单个文件名换成「N 项」，确认后入参给
   * deletePathsToTrash（已支持数组）。
   */
  paths?: string[];
}

interface NamePromptDialogProps {
  state: NameDialogState | null;
  onConfirm: (value: string) => void;
  onClose: () => void;
}

export const NamePromptDialog: React.FC<NamePromptDialogProps> = ({ state, onConfirm, onClose }) => {
  const [value, setValue] = useState('');
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (!state) return;
    const initial = state.mode === 'rename' ? state.name ?? '' : '';
    setValue(initial);
    // 聚焦并选中主名（不含扩展名，对齐 PyCharm 重命名行为）
    requestAnimationFrame(() => {
      const el = inputRef.current;
      if (!el) return;
      el.focus();
      if (state.mode === 'rename') {
        const dot = initial.lastIndexOf('.');
        el.setSelectionRange(0, dot > 0 ? dot : initial.length);
      }
    });
  }, [state]);

  useEffect(() => {
    if (!state) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [state, onClose]);

  if (!state) return null;

  const isDelete = state.mode === 'delete';
  /** 批量删除的项数（含自身）；< 2 走单文件文案 */
  const batchCount = isDelete && state.paths ? state.paths.length : 0;
  const title =
    state.mode === 'new-file'
      ? t().namePrompt.titleNewFile
      : state.mode === 'new-dir'
        ? t().namePrompt.titleNewDir
        : state.mode === 'rename'
          ? t().namePrompt.titleRename
          : t().namePrompt.titleDelete;
  const subtitle = isDelete || state.mode === 'rename' ? state.path : state.targetDir;
  const confirmText = isDelete
    ? t().namePrompt.delete
    : state.mode === 'rename'
      ? t().namePrompt.rename
      : t().namePrompt.create;
  const trimmed = value.trim();
  const canConfirm = isDelete ? true : trimmed.length > 0;

  const submit = () => {
    if (canConfirm) onConfirm(trimmed);
  };

  return (
    <div className="py-dialog-backdrop">
      <div className="py-name-dialog">
        <div className="py-dialog-header">
          <span className="py-dialog-title">{title}</span>
          <button className="py-dialog-close" onClick={onClose} title={t().common.close}>
            <IconWindowClose size={13} />
          </button>
        </div>
        {subtitle && <div className="py-name-subtitle">{subtitle}</div>}
        {isDelete ? (
          batchCount > 1 ? (
            <div className="py-name-message">
              {t().namePrompt.batchDelete(batchCount)}
            </div>
          ) : (
            <div className="py-name-message">
              {state.isDir
                ? t().namePrompt.deleteDirMsg(state.name ?? '')
                : t().namePrompt.deleteFileMsg(state.name ?? '')}
            </div>
          )
        ) : (
          <input
            ref={inputRef}
            className="py-name-input"
            value={value}
            spellCheck={false}
            placeholder={
              state.mode === 'new-file'
                ? t().namePrompt.fileNamePlaceholder
                : state.mode === 'new-dir'
                  ? t().namePrompt.dirNamePlaceholder
                  : t().namePrompt.newNamePlaceholder
            }
            onChange={e => setValue(e.target.value)}
            onKeyDown={e => {
              if (e.key === 'Enter') submit();
            }}
          />
        )}
        <div className="py-dialog-footer">
          <button className="py-name-btn" onClick={onClose}>
            {t().namePrompt.cancel}
          </button>
          <button className={`py-name-btn ${isDelete ? 'danger' : 'primary'}`} disabled={!canConfirm} onClick={submit}>
            {confirmText}
          </button>
        </div>
      </div>
    </div>
  );
};
