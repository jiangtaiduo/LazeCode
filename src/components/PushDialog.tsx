// Push 对话框 · 对齐 PyCharm Push（Ctrl+Shift+K）
//
// 打开时拉取当前分支的待推送提交（git_pending_commits，上游解析：
// 配置的上游 → 同名远程分支 → 所有未推送提交）。
// 支持选择远程、Force Push（--force-with-lease）、Push Tags（--tags）；
// 分支无上游时自动以 git push -u 推送并建立跟踪。

import React, { useState, useEffect, useRef } from 'react';
import { IconWindowClose } from './FileIcons';
import { invokeCommand } from '../services/tauriBridge';
import { t } from '../services/i18n';
import type { GitCommit, GitBranch, GitRemote } from '../types';
import { formatCommitDate } from '../services/commitDate';

interface PushDialogProps {
  open: boolean;
  repoPath: string;
  currentBranch: string;
  branches: GitBranch[];
  onClose: () => void;
  /** 推送成功（参数为 git 输出文本），由 App 刷新数据并提示 */
  onPushed: (output: string) => void;
  /** 打开远程管理（App 的 RemotesDialog；不传则不显示入口） */
  onManageRemotes?: () => void;
  /** 远程增删改后递增：触发 remote 下拉重读 */
  remotesNonce?: number;
}

export const PushDialog: React.FC<PushDialogProps> = ({
  open,
  repoPath,
  currentBranch,
  branches,
  onClose,
  onPushed,
  onManageRemotes,
  remotesNonce = 0
}) => {
  const [pending, setPending] = useState<GitCommit[]>([]);
  const [remotes, setRemotes] = useState<GitRemote[]>([]);
  const [remote, setRemote] = useState('origin');
  const [loading, setLoading] = useState(false);
  const [pushing, setPushing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [force, setForce] = useState(false);
  const [pushTags, setPushTags] = useState(false);
  const [elapsed, setElapsed] = useState(0);
  const wasOpenRef = useRef(false);

  const current = branches.find(b => b.isCurrent || b.name === currentBranch);
  const needsUpstream = !current?.tracking;

  useEffect(() => {
    if (!open || !repoPath) return;
    let cancelled = false;
    setLoading(true);
    setError(null);
    // 只在新打开时重置选项：远程管理回来触发的重读（remotesNonce）不能
    // 顺手把用户的 Force/Tags 勾选清掉。
    if (!wasOpenRef.current) {
      setForce(false);
      setPushTags(false);
    }
    wasOpenRef.current = true;
    Promise.all([
      invokeCommand<GitCommit[]>('git_pending_commits', { repoPath, maxCount: 200 }),
      invokeCommand<GitRemote[]>('git_remote_list', { repoPath })
    ])
      .then(([p, r]) => {
        if (cancelled) return;
        setPending(p);
        setRemotes(r);
        if (r.length) {
          setRemote(r.some(x => x.name === 'origin') ? 'origin' : r[0].name);
        }
      })
      .catch(err => {
        if (!cancelled) setError(err instanceof Error ? err.message : String(err));
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [open, repoPath, remotesNonce]);

  // 推送进行时显示已等待秒数：让网络/凭据等待过程可感知，而不是“像卡住”
  useEffect(() => {
    if (!pushing) {
      setElapsed(0);
      return;
    }
    const timer = setInterval(() => setElapsed(v => v + 1), 1000);
    return () => clearInterval(timer);
  }, [pushing]);

  // Esc 关闭（推送中不可关）
  useEffect(() => {
    if (!open) {
      wasOpenRef.current = false;
      return;
    }
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape' && !pushing) onClose();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [open, pushing, onClose]);

  const handlePush = async () => {
    setPushing(true);
    setError(null);
    try {
      const out = await invokeCommand<string>('git_push', {
        repoPath,
        remote,
        branch: currentBranch,
        setUpstream: needsUpstream,
        force,
        pushTags
      });
      onPushed(out);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setPushing(false);
    }
  };

  if (!open) return null;

  return (
    <div className="py-dialog-backdrop">
      <div className="py-push-dialog">
        <div className="py-dialog-header">
          <span className="py-dialog-title">
            {t().push.title(currentBranch, `${remote}/${currentBranch}`)}
          </span>
          <button className="py-dialog-close" onClick={onClose} title={t().common.close}><IconWindowClose size={13} /></button>
        </div>

        <div className="push-commits-header">
          <span>{loading ? t().push.readingPending : t().push.pendingCount(pending.length)}</span>
          {needsUpstream && !loading && (
            <span className="push-upstream-note">{t().push.noUpstreamNote}</span>
          )}
        </div>

        <div className="push-commits-list">
          {error && loading && null}
          {!loading && pending.length === 0 && !error && (
            <div className="push-empty">{t().push.allPushed}</div>
          )}
          {pending.map(c => (
            <div key={c.hash} className="push-commit-row" title={c.hash}>
              <span className="push-commit-hash">{c.shortHash}</span>
              <span className="push-commit-msg">{c.message}</span>
              <span className="push-commit-meta">{c.author} · {formatCommitDate(c.date)}</span>
            </div>
          ))}
        </div>

        <div className="push-options-row">
          <label className="push-option">
            <input type="checkbox" checked={force} onChange={e => setForce(e.target.checked)} />
            <span>{t().push.forceLabel}</span>
          </label>
          <label className="push-option">
            <input type="checkbox" checked={pushTags} onChange={e => setPushTags(e.target.checked)} />
            <span>{t().push.tagsLabel}</span>
          </label>
          <label className="push-remote-select">
            {t().push.remote}
            <select value={remote} onChange={e => setRemote(e.target.value)}>
              {remotes.map(r => (
                <option key={r.name} value={r.name}>{r.name}</option>
              ))}
              {remotes.length === 0 && <option value="origin">origin</option>}
            </select>
          </label>
          {onManageRemotes && (
            <button className="py-name-btn" onClick={onManageRemotes} disabled={pushing}>
              {t().push.manageRemotes}
            </button>
          )}
        </div>

        {(pushing || error) && (
          <div className="push-status-area">
            {pushing && (
              <>
                <div className="push-progress-track">
                  <div className="push-progress-bar" />
                </div>
                <div className="push-status-line">
                  <span className="push-spinner" />
                  <span>
                    {t().push.pushingTo(remote + '/' + currentBranch, elapsed)}
                  </span>
                </div>
                {elapsed >= 5 && (
                  <div className="push-waiting-hint">
                    {t().push.waitingHint(elapsed)}
                  </div>
                )}
              </>
            )}
            {error && <div className="push-error">{error}</div>}
          </div>
        )}

        <div className="py-dialog-footer">
          <button className="py-btn" onClick={onClose} disabled={pushing}>
            {t().push.cancel}
          </button>
          <button
            className="py-btn primary"
            disabled={pushing || loading || pending.length === 0 || remotes.length === 0}
            onClick={() => void handlePush()}
          >
            {pushing ? (
              <>
                <span className="btn-spinner" aria-hidden="true" />
                {t().push.pushing}
              </>
            ) : (
              t().push.pushButton
            )}
          </button>
        </div>
      </div>
    </div>
  );
};
