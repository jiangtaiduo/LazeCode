import React, { useEffect, useState } from 'react';
import { invokeCommand, isTauriEnvironment } from '../services/tauriBridge';
import { t } from '../services/i18n';
import { IconWindowClose } from './FileIcons';

interface CloneDialogProps {
  open: boolean;
  onClose: () => void;
  onCloned: (path: string) => void;
}

export const CloneDialog: React.FC<CloneDialogProps> = ({ open, onClose, onCloned }) => {
  const [url, setUrl] = useState('');
  const [destination, setDestination] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!open) return;
    setUrl('');
    setDestination('');
    setError(null);
    setBusy(false);
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

  const pickDestination = async () => {
    if (!isTauriEnvironment()) return;
    const picked = await invokeCommand<string | null>('pick_path', {
      title: t().clone.destinationLabel,
      defaultPath: null
    });
    if (typeof picked === 'string' && picked) setDestination(picked);
  };

  const clone = async () => {
    if (!url.trim() || !destination.trim() || busy) return;
    setBusy(true);
    setError(null);
    try {
      // 目标目录 = 父目录 + 仓库名（去掉 .git）
      const name = url.trim().replace(/[/\\]+$/, '').split(/[/\\]/).pop()?.replace(/\.git$/, '') || 'repository';
      const sep = destination.includes('\\') && !destination.includes('/') ? '\\' : '/';
      const dest = `${destination.replace(/[/\\]+$/, '')}${sep}${name}`;
      await invokeCommand<string>('git_clone', { url: url.trim(), destination: dest });
      onCloned(dest);
      onClose();
    } catch (err) {
      setError(t().clone.failed(err instanceof Error ? err.message : String(err)));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="py-dialog-backdrop">
      <div className="py-pull-dialog">
        <div className="py-dialog-header">
          <span className="py-dialog-title">{t().clone.title}</span>
          <button className="py-dialog-close" onClick={onClose} disabled={busy} title={t().common.close}>
            <IconWindowClose size={13} />
          </button>
        </div>

        <div className="clone-form">
          <label className="clone-label" htmlFor="clone-url">{t().clone.urlLabel}</label>
          <input
            id="clone-url"
            className="py-name-input"
            value={url}
            spellCheck={false}
            autoFocus
            placeholder={t().clone.urlPlaceholder}
            onChange={e => setUrl(e.target.value)}
          />
          <label className="clone-label" htmlFor="clone-dest">{t().clone.destinationLabel}</label>
          <div className="clone-dest-row">
            <input
              id="clone-dest"
              className="py-name-input"
              value={destination}
              spellCheck={false}
              placeholder={t().clone.destinationPlaceholder}
              onChange={e => setDestination(e.target.value)}
            />
            <button type="button" className="py-btn" onClick={() => void pickDestination()}>
              {t().clone.browse}
            </button>
          </div>
          {error && <div className="push-error">{error}</div>}
        </div>

        <div className="py-dialog-footer">
          <button className="py-btn" onClick={onClose} disabled={busy}>{t().push.cancel}</button>
          <button
            className="py-btn primary"
            disabled={busy || !url.trim() || !destination.trim()}
            onClick={() => void clone()}
          >
            {busy ? t().clone.cloning : t().clone.cloneButton}
          </button>
        </div>
      </div>
    </div>
  );
};
