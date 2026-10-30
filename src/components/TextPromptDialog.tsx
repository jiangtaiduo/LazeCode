// 通用文本输入对话框 · 替代 window.prompt
//
// 为什么不用 window.prompt：wry 0.55.1 的 WKUIDelegate 没有实现任何 JS 面板
//（无 runJavaScriptAlertPanel / ConfirmPanel / TextInputPanel），prompt 在
// webview 里直接返回 null、连框都不弹 —— 凡走 window.prompt 的菜单项
//（New Branch / Rename / New Tag / Stash 命名 / Amend / Go to Line…）
// 点下去都是"没有任何反应"。见 .audit/10。
//
// DOM 输入框天然命中全局 keydown 的输入域豁免，不需要 nativeModal 那套标记。

import React, { useEffect, useRef, useState } from 'react';
import { t } from '../services/i18n';
import { IconWindowClose } from './FileIcons';

export interface TextPromptRequest {
  message: string;
  defaultValue?: string;
  /** 允许空值确认（默认要求非空，如 tag 描述）；空值确认时 resolve '' */
  allowEmpty?: boolean;
}

interface TextPromptDialogProps {
  request: TextPromptRequest | null;
  onConfirm: (value: string) => void;
  onClose: () => void;
}

export const TextPromptDialog: React.FC<TextPromptDialogProps> = ({ request, onConfirm, onClose }) => {
  const [value, setValue] = useState('');
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (!request) return;
    setValue(request.defaultValue ?? '');
    requestAnimationFrame(() => {
      const el = inputRef.current;
      if (!el) return;
      el.focus();
      el.select();
    });
  }, [request]);

  useEffect(() => {
    if (!request) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [request, onClose]);

  if (!request) return null;

  const trimmed = value.trim();
  const canConfirm = request.allowEmpty === true || trimmed.length > 0;
  const submit = () => {
    if (canConfirm) onConfirm(trimmed);
  };

  return (
    <div className="py-dialog-backdrop">
      <div className="py-name-dialog">
        <div className="py-dialog-header">
          <span className="py-dialog-title">{request.message}</span>
          <button className="py-dialog-close" onClick={onClose} title={t().common.close}>
            <IconWindowClose size={13} />
          </button>
        </div>
        <input
          ref={inputRef}
          className="py-name-input"
          value={value}
          spellCheck={false}
          onChange={e => setValue(e.target.value)}
          onKeyDown={e => {
            if (e.key === 'Enter') submit();
          }}
        />
        <div className="py-dialog-footer">
          <button className="py-name-btn" onClick={onClose}>
            {t().namePrompt.cancel}
          </button>
          <button className="py-name-btn primary" disabled={!canConfirm} onClick={submit}>
            {t().namePrompt.create}
          </button>
        </div>
      </div>
    </div>
  );
};
