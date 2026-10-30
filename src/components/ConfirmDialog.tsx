import React, { useEffect, useState, useRef } from 'react';
import { registerConfirmHandler, deriveActionVerb, type ConfirmRequest } from '../services/confirmDialog';

/**
 * 全局确认弹窗组件（LazeCode 自有实现，交互风格参考常见 IDE）。
 */
export const ConfirmDialog: React.FC = () => {
  const [request, setRequest] = useState<ConfirmRequest | null>(null);
  const resolveRef = useRef<((value: boolean) => void) | null>(null);
  const actionBtnRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    registerConfirmHandler((req: ConfirmRequest) => {
      resolveRef.current?.(false);
      return new Promise<boolean>(resolve => {
        resolveRef.current = resolve;
        setRequest(req);
      });
    });
    return () => {
      registerConfirmHandler(null);
    };
  }, []);

  const handleClose = (result: boolean) => {
    const resolve = resolveRef.current;
    resolveRef.current = null;
    setRequest(null);
    resolve?.(result);
  };

  useEffect(() => {
    if (!request) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.preventDefault();
        e.stopPropagation();
        handleClose(false);
      } else if (e.key === 'Enter') {
        e.preventDefault();
        e.stopPropagation();
        handleClose(true);
      }
    };
    window.addEventListener('keydown', onKey, true);
    requestAnimationFrame(() => {
      actionBtnRef.current?.focus();
    });
    return () => window.removeEventListener('keydown', onKey, true);
  }, [request]);

  if (!request) return null;

  const isZh = /[\u4e00-\u9fa5]/.test(request.title || '') || /[\u4e00-\u9fa5]/.test(request.message);
  const cancelLabel = request.cancelLabel || (isZh ? '取消' : 'Cancel');
  const actionLabel = request.okLabel || deriveActionVerb(request.title, request.message);
  const isDanger =
    request.danger ??
    ((request.title || '').toLowerCase().includes('delete') ||
      (request.title || '').toLowerCase().includes('drop') ||
      (request.title || '').includes('删除') ||
      (request.title || '').includes('丢弃'));

  return (
    <div className="py-dialog-backdrop py-confirm-backdrop">
      <div className="py-confirm-dialog" role="alertdialog" aria-modal="true">
        <div className="py-confirm-body">
          <div className="py-confirm-icon-wrap" aria-hidden="true">
            {/* 中性圆形问号图标：文字渲染，不使用第三方图标资源 */}
            <svg width="28" height="28" viewBox="0 0 28 28" fill="none">
              <circle cx="14" cy="14" r="14" fill="#4c8dff" />
              <text x="14" y="20" textAnchor="middle" fontSize="18" fontWeight="600" fontFamily="system-ui, -apple-system, sans-serif" fill="#ffffff">?</text>
            </svg>
          </div>
          <div className="py-confirm-content">
            {request.title && <div className="py-confirm-title">{request.title}</div>}
            <div className="py-confirm-message">{request.message}</div>
          </div>
        </div>
        <div className="py-confirm-footer">
          <button
            type="button"
            className="py-confirm-btn py-confirm-btn-cancel"
            onClick={() => handleClose(false)}
          >
            {cancelLabel}
          </button>
          <button
            ref={actionBtnRef}
            type="button"
            className={`py-confirm-btn py-confirm-btn-action ${isDanger ? 'danger' : ''}`}
            onClick={() => handleClose(true)}
          >
            {actionLabel}
          </button>
        </div>
      </div>
    </div>
  );
};
