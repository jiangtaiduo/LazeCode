// 远程管理对话框 · 对齐 PyCharm Git → Manage Remotes…
//
// PyCharm 版式：标题 Git Remotes + 远端列表（Name / URL）+ 右侧 +/−/编辑；
// 点 Add/Edit 进第二步 Define Remote（Name + URL 两个字段），Remove 二次确认。
// 这里把两步收进同一个 modal（mode 切换），少一层窗口。

import React, { useCallback, useEffect, useState } from 'react';
import { appConfirm } from '../services/confirmDialog';
import { IconWindowClose } from './FileIcons';
import { invokeCommand } from '../services/tauriBridge';
import { t } from '../services/i18n';
import type { GitRemote } from '../types';

interface RemotesDialogProps {
  open: boolean;
  repoPath: string;
  onClose: () => void;
  /** 增删改成功后通知 App（Push 对话框的 remote 下拉需要重读） */
  onChanged: () => void;
}

export const RemotesDialog: React.FC<RemotesDialogProps> = ({ open, repoPath, onClose, onChanged }) => {
  const [remotes, setRemotes] = useState<GitRemote[]>([]);
  const [selected, setSelected] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  /** null = 列表态；否则为 Define Remote 表单态 */
  const [form, setForm] = useState<{ editName: string | null; name: string; url: string } | null>(null);
  const [saving, setSaving] = useState(false);

  const load = useCallback(async () => {
    if (!repoPath) return;
    setLoading(true);
    setError(null);
    try {
      const list = await invokeCommand<GitRemote[]>('git_remote_list', { repoPath });
      setRemotes(list);
      setSelected(prev => (prev && list.some(r => r.name === prev) ? prev : (list[0]?.name ?? null)));
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setLoading(false);
    }
  }, [repoPath]);

  useEffect(() => {
    if (!open) return;
    setForm(null);
    setError(null);
    void load();
  }, [open, load]);

  // Esc 关闭（保存中不可关；表单态先退回列表态，对齐子对话框的 Esc 语义）
  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== 'Escape' || saving) return;
      if (form) setForm(null);
      else onClose();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [open, saving, form, onClose]);

  if (!open) return null;

  const selectedRemote = remotes.find(r => r.name === selected) ?? null;

  const handleSave = async () => {
    if (!form || saving) return;
    const name = form.name.trim();
    const url = form.url.trim();
    if (!name || !url) return;
    setSaving(true);
    setError(null);
    try {
      if (form.editName === null) {
        await invokeCommand('git_remote_add', { repoPath, name, url });
      } else {
        if (name !== form.editName) {
          await invokeCommand('git_remote_rename', { repoPath, oldName: form.editName, newName: name });
        }
        if (url !== (selectedRemote?.url ?? '')) {
          await invokeCommand('git_remote_set_url', { repoPath, name, url });
        }
      }
      setSelected(name);
      setForm(null);
      await load();
      onChanged();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setSaving(false);
    }
  };

  const handleRemove = async () => {
    if (!selectedRemote || saving) return;
    if (!(await appConfirm(t().remotes.removeConfirm(selectedRemote.name), { title: t().remotes.removeTitle, kind: 'warning', okLabel: 'Remove' }))) return;
    setSaving(true);
    setError(null);
    try {
      await invokeCommand('git_remote_remove', { repoPath, name: selectedRemote.name });
      setSelected(null);
      await load();
      onChanged();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setSaving(false);
    }
  };

  const canSave = !!form && form.name.trim().length > 0 && form.url.trim().length > 0 && !saving;

  return (
    <div className="py-dialog-backdrop">
      <div className="py-remotes-dialog">
        <div className="py-dialog-header">
          <span className="py-dialog-title">
            {form ? (form.editName === null ? t().remotes.defineNew : t().remotes.defineEdit(form.editName)) : t().remotes.title}
          </span>
          <button className="py-dialog-close" onClick={onClose} title={t().common.close}>
            <IconWindowClose size={13} />
          </button>
        </div>
        <div className="py-remotes-repo">{repoPath}</div>

        {form ? (
          <>
            <label className="py-remotes-field">
              <span>{t().remotes.nameLabel}</span>
              <input
                className="py-name-input"
                value={form.name}
                spellCheck={false}
                placeholder={t().remotes.namePlaceholder}
                onChange={e => setForm({ ...form, name: e.target.value })}
                onKeyDown={e => {
                  if (e.key === 'Enter' && canSave) void handleSave();
                }}
              />
            </label>
            <label className="py-remotes-field">
              <span>{t().remotes.urlLabel}</span>
              <input
                className="py-name-input"
                value={form.url}
                spellCheck={false}
                placeholder={t().remotes.urlPlaceholder}
                onChange={e => setForm({ ...form, url: e.target.value })}
                onKeyDown={e => {
                  if (e.key === 'Enter' && canSave) void handleSave();
                }}
              />
            </label>
            {error && <div className="remotes-error">{error}</div>}
            <div className="py-dialog-footer">
              <button className="py-name-btn" onClick={() => setForm(null)} disabled={saving}>
                {t().remotes.cancel}
              </button>
              <button className="py-name-btn primary" disabled={!canSave} onClick={() => void handleSave()}>
                {t().remotes.save}
              </button>
            </div>
          </>
        ) : (
          <>
            <div className="py-remotes-list" role="listbox" aria-label={t().remotes.title}>
              {loading && remotes.length === 0 && <div className="py-remotes-empty">{t().push.readingPending}</div>}
              {!loading && remotes.length === 0 && <div className="py-remotes-empty">{t().remotes.empty}</div>}
              {remotes.map(r => (
                <div
                  key={r.name}
                  role="option"
                  aria-selected={r.name === selected}
                  className={`py-remotes-row ${r.name === selected ? 'selected' : ''}`}
                  onClick={() => setSelected(r.name)}
                  onDoubleClick={() => setForm({ editName: r.name, name: r.name, url: r.url })}
                >
                  <span className="py-remotes-name">{r.name}</span>
                  <span className="py-remotes-url" title={r.url}>{r.url}</span>
                </div>
              ))}
            </div>
            {error && <div className="remotes-error">{error}</div>}
            <div className="py-dialog-footer remotes-footer">
              <span className="remotes-actions">
                <button
                  className="py-name-btn"
                  disabled={saving}
                  onClick={() => {
                    setError(null);
                    setForm({ editName: null, name: remotes.some(r => r.name === 'origin') ? '' : 'origin', url: '' });
                  }}
                >
                  {t().remotes.add}
                </button>
                <button
                  className="py-name-btn"
                  disabled={!selectedRemote || saving}
                  onClick={() => {
                    if (!selectedRemote) return;
                    setError(null);
                    setForm({ editName: selectedRemote.name, name: selectedRemote.name, url: selectedRemote.url });
                  }}
                >
                  {t().remotes.edit}
                </button>
                <button className="py-name-btn danger" disabled={!selectedRemote || saving} onClick={() => void handleRemove()}>
                  {t().remotes.remove}
                </button>
              </span>
              <button className="py-name-btn primary" onClick={onClose}>
                {t().remotes.close}
              </button>
            </div>
          </>
        )}
      </div>
    </div>
  );
};
