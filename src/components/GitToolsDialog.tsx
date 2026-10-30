// Git 工具窗口：子模块 / 工作树 / LFS / 凭据。
// 所有动作走 Rust 命令，前端只维护列表与表单状态。

import React, { useCallback, useEffect, useState } from 'react';
import { invokeCommand } from '../services/tauriBridge';
import { appConfirm } from '../services/confirmDialog';
import { t } from '../services/i18n';
import type { GitCredentialStatus, GitLfsStatus, GitSubmodule, GitWorktree } from '../types';
import { IconWindowClose } from './FileIcons';

type Tab = 'submodules' | 'worktrees' | 'lfs' | 'credentials';

interface GitToolsDialogProps {
  open: boolean;
  repoPath: string;
  onClose: () => void;
  onChanged: () => void;
}

export const GitToolsDialog: React.FC<GitToolsDialogProps> = ({ open, repoPath, onClose, onChanged }) => {
  const [tab, setTab] = useState<Tab>('submodules');
  const [submodules, setSubmodules] = useState<GitSubmodule[]>([]);
  const [worktrees, setWorktrees] = useState<GitWorktree[]>([]);
  const [lfs, setLfs] = useState<GitLfsStatus | null>(null);
  const [credentials, setCredentials] = useState<GitCredentialStatus | null>(null);
  const [helper, setHelper] = useState('');
  const [loading, setLoading] = useState(false);
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [subUrl, setSubUrl] = useState('');
  const [subPath, setSubPath] = useState('');
  const [subBranch, setSubBranch] = useState('');
  const [wtPath, setWtPath] = useState('');
  const [wtBranch, setWtBranch] = useState('');
  const [wtCreate, setWtCreate] = useState(false);
  const [trackPattern, setTrackPattern] = useState('');

  const load = useCallback(async () => {
    if (!repoPath) return;
    setLoading(true);
    setError(null);
    try {
      if (tab === 'submodules') {
        setSubmodules(await invokeCommand<GitSubmodule[]>('git_submodule_list', { repoPath }));
      } else if (tab === 'worktrees') {
        setWorktrees(await invokeCommand<GitWorktree[]>('git_worktree_list', { repoPath }));
      } else if (tab === 'lfs') {
        setLfs(await invokeCommand<GitLfsStatus>('git_lfs_status', { repoPath }));
      } else {
        const c = await invokeCommand<GitCredentialStatus>('git_credential_status', {});
        setCredentials(c);
        setHelper(c.helper);
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setLoading(false);
    }
  }, [repoPath, tab]);

  useEffect(() => {
    if (open) void load();
  }, [open, load]);

  const run = async (fn: () => Promise<unknown>, message: string) => {
    setBusy(true);
    setError(null);
    setNotice(null);
    try {
      await fn();
      setNotice(t().gitTools.done(message));
      await load();
      onChanged();
    } catch (err) {
      setError(t().gitTools.error(err instanceof Error ? err.message : String(err)));
    } finally {
      setBusy(false);
    }
  };

  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape' && !busy) onClose();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [open, busy, onClose]);

  if (!open) return null;

  const pickWorktreePath = async () => {
    const picked = await invokeCommand<string | null>('pick_path', {
      title: t().gitTools.path,
      defaultPath: null
    });
    if (typeof picked === 'string' && picked) setWtPath(picked);
  };

  return (
    <div className="py-dialog-backdrop">
      <div className="git-tools-dialog">
        <div className="py-dialog-header">
          <span className="py-dialog-title">{t().gitTools.title}</span>
          <button className="py-dialog-close" onClick={onClose} disabled={busy} title={t().gitTools.close}>
            <IconWindowClose size={13} />
          </button>
        </div>
        <div className="git-tools-tabs">
          {(['submodules', 'worktrees', 'lfs', 'credentials'] as Tab[]).map(id => (
            <button
              key={id}
              className={`git-tools-tab ${tab === id ? 'active' : ''}`}
              onClick={() => { setTab(id); setNotice(null); setError(null); }}
            >
              {t().gitTools[id]}
            </button>
          ))}
          <button className="py-btn sm" style={{ marginLeft: 'auto' }} onClick={() => void load()} disabled={loading}>
            {t().gitTools.refresh}
          </button>
        </div>

        <div className="git-tools-body">
          {loading && <div className="git-tools-empty">{t().common.loading}</div>}
          {error && <div className="push-error">{error}</div>}
          {notice && <div className="git-tools-notice">{notice}</div>}

          {!loading && tab === 'submodules' && (
            <>
              <div className="git-tools-list">
                {submodules.length === 0 && <div className="git-tools-empty">{t().gitTools.empty}</div>}
                {submodules.map(sm => (
                  <div key={sm.path} className="git-tools-row">
                    <span className="git-tools-status">{sm.status}</span>
                    <span className="git-tools-path" title={sm.path}>{sm.path}</span>
                    <span className="git-tools-url" title={sm.url}>{sm.url}</span>
                    <span className="git-tools-actions">
                      <button className="py-btn sm" disabled={busy} onClick={() => void run(() => invokeCommand('git_submodule_update', { repoPath, path: sm.path }), sm.path)}>{t().gitTools.update}</button>
                    </span>
                  </div>
                ))}
              </div>
              <div className="git-tools-form">
                <input className="py-name-input" placeholder={t().gitTools.url} value={subUrl} onChange={e => setSubUrl(e.target.value)} />
                <input className="py-name-input" placeholder={t().gitTools.path} value={subPath} onChange={e => setSubPath(e.target.value)} />
                <input className="py-name-input" placeholder={t().gitTools.branch} value={subBranch} onChange={e => setSubBranch(e.target.value)} />
                <button
                  className="py-btn primary sm"
                  disabled={busy || !subUrl.trim() || !subPath.trim()}
                  onClick={() => void run(
                    () => invokeCommand('git_submodule_add', { repoPath, url: subUrl.trim(), path: subPath.trim(), branch: subBranch.trim() || null }),
                    subPath
                  )}
                >
                  {t().gitTools.addSubmodule}
                </button>
              </div>
              <div className="git-tools-footer-actions">
                <button className="py-btn sm" disabled={busy} onClick={() => void run(() => invokeCommand('git_submodule_update', { repoPath, path: null }), 'update')}>{t().gitTools.update}</button>
                <button className="py-btn sm" disabled={busy} onClick={() => void run(() => invokeCommand('git_submodule_sync', { repoPath }), 'sync')}>{t().gitTools.sync}</button>
              </div>
            </>
          )}

          {!loading && tab === 'worktrees' && (
            <>
              <div className="git-tools-list">
                {worktrees.length === 0 && <div className="git-tools-empty">{t().gitTools.empty}</div>}
                {worktrees.map(wt => (
                  <div key={wt.path} className="git-tools-row">
                    <span className="git-tools-path" title={wt.path}>{wt.path}</span>
                    <span className="git-tools-url">{wt.branch || wt.head.slice(0, 8)}{wt.detached ? ' (detached)' : ''}</span>
                    <span className="git-tools-actions">
                      <button
                        className="py-btn sm danger"
                        disabled={busy}
                        onClick={async () => {
                          if (await appConfirm(`Remove worktree ${wt.path}?`, { title: t().gitTools.worktrees, kind: 'warning', okLabel: t().gitTools.remove })) {
                            void run(() => invokeCommand('git_worktree_remove', { repoPath, path: wt.path, force: false }), wt.path);
                          }
                        }}
                      >
                        {t().gitTools.remove}
                      </button>
                    </span>
                  </div>
                ))}
              </div>
              <div className="git-tools-form">
                <div className="clone-dest-row">
                  <input className="py-name-input" placeholder={t().gitTools.path} value={wtPath} onChange={e => setWtPath(e.target.value)} />
                  <button className="py-btn sm" onClick={() => void pickWorktreePath()}>{t().clone.browse}</button>
                </div>
                <input className="py-name-input" placeholder={t().gitTools.branch} value={wtBranch} onChange={e => setWtBranch(e.target.value)} />
                <label className="push-option">
                  <input type="checkbox" checked={wtCreate} onChange={e => setWtCreate(e.target.checked)} />
                  <span>{t().gitTools.createBranch}</span>
                </label>
                <button
                  className="py-btn primary sm"
                  disabled={busy || !wtPath.trim() || (wtCreate && !wtBranch.trim())}
                  onClick={() => void run(
                    () => invokeCommand('git_worktree_add', {
                      repoPath,
                      path: wtPath.trim(),
                      branch: wtBranch.trim() || null,
                      createBranch: wtCreate
                    }),
                    wtPath
                  )}
                >
                  {t().gitTools.addWorktree}
                </button>
              </div>
            </>
          )}

          {!loading && tab === 'lfs' && (
            <>
              {!lfs?.installed ? (
                <div className="git-tools-empty">{t().gitTools.lfsUnavailable}</div>
              ) : (
                <>
                  <div className="git-tools-meta">{t().gitTools.lfsVersion(lfs.version)} · {t().gitTools.lfsFiles(lfs.files.length)}</div>
                  {lfs.error && <div className="push-error">{lfs.error}</div>}
                  <div className="git-tools-list">
                    {lfs.files.map(f => <div key={f} className="git-tools-row"><span className="git-tools-path" title={f}>{f}</span></div>)}
                    {lfs.files.length === 0 && <div className="git-tools-empty">{t().gitTools.empty}</div>}
                  </div>
                  <div className="git-tools-form">
                    <button className="py-btn sm" disabled={busy} onClick={() => void run(() => invokeCommand('git_lfs_pull', { repoPath }), 'lfs pull')}>{t().gitTools.pull}</button>
                    <input className="py-name-input" placeholder={t().gitTools.trackPlaceholder} value={trackPattern} onChange={e => setTrackPattern(e.target.value)} />
                    <button
                      className="py-btn sm"
                      disabled={busy || !trackPattern.trim()}
                      onClick={() => void run(() => invokeCommand('git_lfs_track', { repoPath, pattern: trackPattern.trim() }), trackPattern)}
                    >
                      {t().gitTools.track}
                    </button>
                  </div>
                </>
              )}
            </>
          )}

          {!loading && tab === 'credentials' && (
            <>
              {credentials && (
                <div className="git-tools-meta">
                  {credentials.osxKeychainAvailable ? t().gitTools.keychainAvailable : t().gitTools.keychainMissing}
                </div>
              )}
              <div className="git-tools-form">
                <label className="clone-label" htmlFor="cred-helper">{t().gitTools.helperLabel}</label>
                <input
                  id="cred-helper"
                  className="py-name-input"
                  placeholder={t().gitTools.helperPlaceholder}
                  value={helper}
                  onChange={e => setHelper(e.target.value)}
                />
                <button
                  className="py-btn primary sm"
                  disabled={busy || !helper.trim()}
                  onClick={() => void run(() => invokeCommand('git_credential_set_helper', { helper: helper.trim() }), helper)}
                >
                  {t().gitTools.saveHelper}
                </button>
              </div>
            </>
          )}
        </div>
      </div>
    </div>
  );
};
