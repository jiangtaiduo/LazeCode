// 应用更新对话框。
//
// 交互状态参考常见 IDE 的更新流程，LazeCode 自有实现：
// 1. Checking：检测中进度与取消
// 2. Up to date：已是最新版本提示
// 3. Available：新版本详情、Markdown 更新日志、忽略此版本选项、更新并重启
// 4. Downloading：实时百分比与字节进度条
// 5. Ready：更新已就绪，提示重启生效

import React, { useState, useEffect, useRef } from 'react';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import { IconWindowClose } from './FileIcons';
import {
  checkForAppUpdate,
  downloadAndInstallUpdate,
  relaunchApp,
  formatBytes,
  type UpdateInfo,
  type DownloadProgress,
} from '../services/updaterService';
import { APP_VERSION } from '../services/appVersion';
import { t } from '../services/i18n';

export type UpdateDialogState = 'checking' | 'up-to-date' | 'available' | 'downloading' | 'ready' | 'error';

export interface UpdateDialogProps {
  open: boolean;
  onClose: () => void;
  initialState?: UpdateDialogState;
  initialUpdateInfo?: UpdateInfo | null;
  onIgnoreVersion?: (version: string) => void;
  hasDirtyBuffers?: () => boolean;
  confirmDiscardDirty?: () => Promise<boolean>;
}

export const UpdateDialog: React.FC<UpdateDialogProps> = ({
  open,
  onClose,
  initialState = 'checking',
  initialUpdateInfo = null,
  onIgnoreVersion,
  hasDirtyBuffers,
  confirmDiscardDirty,
}) => {
  const [state, setState] = useState<UpdateDialogState>(initialState);
  const [updateInfo, setUpdateInfo] = useState<UpdateInfo | null>(initialUpdateInfo);
  const [progress, setProgress] = useState<DownloadProgress>({ downloadedBytes: 0, totalBytes: 0, percent: 0 });
  const [errorMsg, setErrorMsg] = useState<string | null>(null);
  const [ignoreVersionChecked, setIgnoreVersionChecked] = useState(false);
  const okBtnRef = useRef<HTMLButtonElement>(null);

  // 打开时重置或执行检查
  useEffect(() => {
    if (!open) return;
    if (initialUpdateInfo) {
      setUpdateInfo(initialUpdateInfo);
      setState('available');
      return;
    }

    if (initialState === 'checking') {
      doCheck();
    } else {
      setState(initialState);
    }
  }, [open, initialState, initialUpdateInfo]);

  // 'up-to-date' 时自动聚焦 OK 按钮
  useEffect(() => {
    if (open && (state === 'up-to-date' || state === 'ready')) {
      requestAnimationFrame(() => {
        okBtnRef.current?.focus();
      });
    }
  }, [open, state]);

  // Esc 键关闭
  useEffect(() => {
    if (!open) return;
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape' && state !== 'downloading') {
        e.stopPropagation();
        onClose();
      }
    };
    window.addEventListener('keydown', handleKeyDown, true);
    return () => window.removeEventListener('keydown', handleKeyDown, true);
  }, [open, state, onClose]);

  const doCheck = async () => {
    setState('checking');
    setErrorMsg(null);
    try {
      const res = await checkForAppUpdate();
      if (res) {
        setUpdateInfo(res);
        setState('available');
      } else {
        setUpdateInfo(null);
        setState('up-to-date');
      }
    } catch (err) {
      setErrorMsg(err instanceof Error ? err.message : String(err));
      setState('error');
    }
  };

  const handleStartUpdate = async () => {
    if (!updateInfo) return;
    setState('downloading');
    setProgress({ downloadedBytes: 0, totalBytes: 0, percent: 0 });
    setErrorMsg(null);

    try {
      await downloadAndInstallUpdate(updateInfo.rawUpdate, p => {
        setProgress(p);
      });
      setState('ready');
    } catch (err) {
      setErrorMsg(err instanceof Error ? err.message : String(err));
      setState('error');
    }
  };

  const handleRestartNow = async () => {
    if (hasDirtyBuffers?.()) {
      const confirmed = await confirmDiscardDirty?.();
      if (!confirmed) return;
    }
    await relaunchApp();
  };

  const handleRemindLater = () => {
    if (ignoreVersionChecked && updateInfo) {
      onIgnoreVersion?.(updateInfo.version);
    }
    onClose();
  };

  if (!open) return null;

  return (
    <div
      className="py-dialog-backdrop"
      style={{ display: 'flex', alignItems: 'center', justifyContent: 'center' }}
    >
      <div
        className="py-dialog-window"
        style={{
          width: state === 'available' ? '540px' : '440px',
          maxWidth: '92vw',
          maxHeight: '88vh',
          display: 'flex',
          flexDirection: 'column',
          background: 'var(--bg-dialog, #2b2d30)',
          border: '1px solid var(--border-color)',
          boxShadow: '0 18px 48px var(--scrim-strong, rgba(0, 0, 0, 0.65))',
          borderRadius: '8px',
          overflow: 'hidden',
        }}
        onClick={e => e.stopPropagation()}
      >
        {/* Header */}
        <div className="py-dialog-header" style={{ padding: '10px 16px', borderBottom: '1px solid var(--border-subtle)', background: 'var(--bg-dialog, #2b2d30)' }}>
          <div className="py-dialog-title" style={{ fontWeight: 600, fontSize: '13px' }}>
            {state === 'checking' && t().updater.checkingTitle}
            {state === 'up-to-date' && t().updater.upToDateTitle}
            {state === 'available' && t().updater.updateAvailableTitle}
            {state === 'downloading' && t().updater.downloadingTitle}
            {state === 'ready' && t().updater.updateReadyTitle}
            {state === 'error' && t().updater.checkFailedTitle}
          </div>
          {state !== 'downloading' && (
            <button
              type="button"
              className="py-dialog-close"
              onClick={onClose}
              title={t().common.close}
            >
              <IconWindowClose size={12} />
            </button>
          )}
        </div>

        {/* Content Body */}
        <div style={{ padding: '18px 20px', flex: 1, overflowY: 'auto', fontSize: '12px', color: 'var(--text-primary)', background: 'var(--bg-dialog, #2b2d30)' }}>
          {state === 'checking' && (
            <div style={{ display: 'flex', alignItems: 'center', gap: '14px', padding: '12px 0' }}>
              <div
                style={{
                  width: '18px',
                  height: '18px',
                  border: '2px solid var(--border-color)',
                  borderTopColor: 'var(--accent-color)',
                  borderRadius: '50%',
                  animation: 'spin 0.8s linear infinite',
                }}
              />
              <span style={{ fontSize: '13px' }}>{t().updater.checkingDesc}</span>
            </div>
          )}

          {/* 2. Up to date */}
          {state === 'up-to-date' && (
            <div style={{ padding: '6px 0 10px' }}>
              <div style={{ fontSize: '13px', fontWeight: 600, color: 'var(--text-bright)', marginBottom: '8px' }}>
                {t().updater.upToDateTitle}
              </div>
              <div style={{ color: 'var(--text-secondary)', lineHeight: '20px' }}>
                {t().updater.upToDateDesc(APP_VERSION)}
              </div>
              <div style={{ color: 'var(--text-muted)', fontSize: '11px', marginTop: '10px' }}>
                {t().updater.upToDateCheckedAt(new Date().toLocaleTimeString())}
              </div>
            </div>
          )}

          {/* 3. Available */}
          {state === 'available' && updateInfo && (
            <div>
              <div style={{ fontSize: '14px', fontWeight: 600, color: 'var(--text-bright)', marginBottom: '6px' }}>
                {t().updater.newVersionAvailable(updateInfo.version, updateInfo.currentVersion)}
              </div>
              {updateInfo.date && (
                <div style={{ color: 'var(--text-muted)', fontSize: '11px', marginBottom: '14px' }}>
                  {t().updater.releaseDate(new Date(updateInfo.date).toLocaleDateString())}
                </div>
              )}

              {/* Release Notes */}
              <div style={{ marginBottom: '12px' }}>
                <div style={{ fontWeight: 600, fontSize: '12px', color: 'var(--text-secondary)', marginBottom: '6px' }}>
                  {t().updater.releaseNotes}
                </div>
                <div
                  style={{
                    maxHeight: '220px',
                    overflowY: 'auto',
                    padding: '12px 14px',
                    background: 'var(--bg-dialog-input, #1e1f22)',
                    borderRadius: '6px',
                    fontSize: '12px',
                    lineHeight: '1.6',
                    color: 'var(--text-primary)',
                  }}
                  className="markdown-body"
                >
                  {updateInfo.body ? (
                    <ReactMarkdown remarkPlugins={[remarkGfm]}>{updateInfo.body}</ReactMarkdown>
                  ) : (
                    <span style={{ color: 'var(--text-muted)' }}>{t().common.none}</span>
                  )}
                </div>
              </div>

              {/* Ignore Checkbox */}
              <label style={{ display: 'inline-flex', alignItems: 'center', gap: '8px', cursor: 'pointer', marginTop: '4px' }}>
                <input
                  type="checkbox"
                  checked={ignoreVersionChecked}
                  onChange={e => setIgnoreVersionChecked(e.target.checked)}
                  style={{ accentColor: 'var(--accent-color)', cursor: 'pointer' }}
                />
                <span style={{ color: 'var(--text-secondary)', fontSize: '11px' }}>
                  {t().updater.ignoreThisVersion}
                </span>
              </label>
            </div>
          )}

          {/* 4. Downloading */}
          {state === 'downloading' && (
            <div style={{ padding: '8px 0' }}>
              <div style={{ fontSize: '13px', fontWeight: 600, marginBottom: '12px' }}>
                {t().updater.downloadingTitle}
              </div>
              <div
                style={{
                  height: '8px',
                  width: '100%',
                  background: 'var(--bg-app)',
                  border: '1px solid var(--border-subtle)',
                  borderRadius: '4px',
                  overflow: 'hidden',
                  marginBottom: '10px',
                }}
              >
                <div
                  style={{
                    height: '100%',
                    width: `${progress.percent}%`,
                    background: 'var(--accent-color)',
                    transition: 'width 0.2s ease',
                  }}
                />
              </div>
              <div style={{ display: 'flex', justifyContent: 'space-between', color: 'var(--text-muted)', fontSize: '11px' }}>
                <span>{progress.percent}%</span>
                <span>
                  {formatBytes(progress.downloadedBytes)}
                  {progress.totalBytes > 0 ? ` / ${formatBytes(progress.totalBytes)}` : ''}
                </span>
              </div>
            </div>
          )}

          {/* 5. Ready */}
          {state === 'ready' && (
            <div style={{ padding: '6px 0 10px' }}>
              <div style={{ fontSize: '13px', fontWeight: 600, color: 'var(--text-bright)', marginBottom: '8px' }}>
                {t().updater.updateReadyTitle}
              </div>
              <div style={{ color: 'var(--text-secondary)', lineHeight: '20px' }}>
                {t().updater.updateReadyDesc}
              </div>
            </div>
          )}

          {/* 6. Error */}
          {state === 'error' && (
            <div style={{ padding: '6px 0 10px' }}>
              <div style={{ fontSize: '13px', fontWeight: 600, color: 'var(--git-del-strong, #c94f4f)', marginBottom: '8px' }}>
                {t().updater.checkFailedTitle}
              </div>
              <div style={{ color: 'var(--text-secondary)', lineHeight: '20px' }}>
                {t().updater.checkFailedDesc(errorMsg || 'Unknown error')}
              </div>
            </div>
          )}
        </div>

        {/* Footer Actions */}
        <div
          className="py-dialog-footer"
          style={{
            padding: '10px 16px',
            borderTop: '1px solid var(--border-subtle)',
            background: 'var(--bg-dialog-footer, #212225)',
            justifyContent: 'flex-end',
            gap: '8px',
          }}
        >
          {state === 'checking' && (
            <button type="button" className="py-btn" onClick={onClose}>
              {t().common.close}
            </button>
          )}

          {state === 'up-to-date' && (
            <button ref={okBtnRef} type="button" className="py-btn primary" onClick={onClose}>
              OK
            </button>
          )}

          {state === 'available' && (
            <>
              <button type="button" className="py-btn" onClick={handleRemindLater}>
                {t().updater.remindMeLater}
              </button>
              <button type="button" className="py-btn primary" onClick={handleStartUpdate}>
                {t().updater.updateAndRestart}
              </button>
            </>
          )}

          {state === 'downloading' && (
            <button type="button" className="py-btn" disabled>
              {t().updater.downloadingTitle}…
            </button>
          )}

          {state === 'ready' && (
            <>
              <button type="button" className="py-btn" onClick={onClose}>
                {t().updater.restartLater}
              </button>
              <button ref={okBtnRef} type="button" className="py-btn primary" onClick={handleRestartNow}>
                {t().updater.restartNow}
              </button>
            </>
          )}

          {state === 'error' && (
            <>
              <button type="button" className="py-btn" onClick={onClose}>
                {t().common.close}
              </button>
              <button type="button" className="py-btn primary" onClick={doCheck}>
                {t().common.retry}
              </button>
            </>
          )}
        </div>
      </div>
    </div>
  );
};
