// Conflicts 对话框 · 对齐 PyCharm 的 Conflicts 列表框
//
// PyCharm 把解决冲突拆成两步：这个框只管"文件级二选一" —— Name / Yours /
// Theirs 三列表 + 右侧 Accept Yours / Accept Theirs / Merge…；正文冲突点
// Merge… 才进 MergeToolDialog 逐块合；删改冲突没有三方，直接选一边
// （Accept Yours/Theirs 即确认删除/保留修改，与 git_use_merge_side 的
// 缺 stage 走 git rm 语义对齐）。
//
// 这个框里只有 Close（对齐 PyCharm）：Abort/Continue 归 Git 面板顶部的操作
// pill，关框即得 —— 对话框是模态的，框里放 Abort 只是重复入口。

import React, { useState, useEffect, useMemo, useRef } from 'react';
import { renderFileIcon } from './FileIcons';
import { MergeToolDialog } from './MergeToolDialog';
import { MacTrafficLights } from './MacTrafficLights';
import { ModalResizeHandles } from './ModalResizeHandles';
import { useDraggableModal } from '../hooks/useDraggableModal';
import { invokeCommand } from '../services/tauriBridge';
import { t } from '../services/i18n';
import type { GitChangedFile, SequencerState, ConflictKind } from '../types';

interface ConflictDialogProps {
  open: boolean;
  repoPath: string;
  /** 处于冲突状态（status === 'C'）的文件清单 */
  files: GitChangedFile[];
  /** 当前分支：Yours 列头与合并副标题用 */
  currentBranch?: string;
  /** sequencer：副标题与 Theirs 列头的上下文（mergeHead / cherryPickHead） */
  sequencer?: SequencerState | null;
  /** Monaco 主题名（透传给合并工具） */
  monacoTheme?: string;
  busy?: boolean;
  onClose: () => void;
  /** 全部解决后回调 */
  onAllResolved: (message: string) => void;
  /**
   * 单个文件解决后回调（App 刷新 git 状态 + toast）：列表实时缩短。
   * 最后一个文件仍走 onAllResolved（关框），避免双刷新/双 toast。
   */
  onFileResolved?: (path: string, message: string) => void;
  isNativeWindow?: boolean;
  onMergeOpenChange?: (isOpen: boolean) => void;
  onOpenMergeExternal?: (params: { path: string; kind: ConflictKind; theirsTitle: string; localDetails?: string; theirsDetails?: string }) => void;
}

const dirOf = (p: string) => {
  const i = p.lastIndexOf('/');
  return i < 0 ? '' : p.slice(0, i);
};
const fileNameOf = (p: string) => p.split(/[\\/]/).pop() ?? p;

export const ConflictDialog: React.FC<ConflictDialogProps> = ({
  open,
  repoPath,
  files,
  currentBranch = '',
  sequencer = null,
  monacoTheme = 'ide-jetbrains-dark',
  busy = false,
  onClose,
  onAllResolved,
  onFileResolved,
  isNativeWindow = false,
  onMergeOpenChange,
  onOpenMergeExternal
}) => {
  const [selectedPath, setSelectedPath] = useState<string | null>(null);
  const [kinds, setKinds] = useState<Record<string, ConflictKind>>({});
  const [groupByDir, setGroupByDir] = useState(false);
  const [mergePath, setMergePath] = useState<string | null>(null);
  const [acting, setActing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  const {
    geom,
    isMaximized,
    handleTitlePointerDown,
    handleResizePointerDown,
    toggleMaximize
  } = useDraggableModal({
    defaultWidth: 760,
    defaultHeight: 480,
    minWidth: 520,
    minHeight: 320
  });
  const remaining = useMemo(() => files.filter(f => f.status === 'C'), [files]);

  // files 是 App 每次渲染现 filter 的新数组：effect 只认内容键，
  // 避免无关渲染重置选中。列表用 ref 读最新。
  const filesKey = files.map(f => `${f.path}:${f.status}`).join('\n');
  const filesRef = useRef(files);
  filesRef.current = files;

  // 打开 / 列表变化时：批量拉 kinds（一单次 ls-files），选中首行；
  // 已解决的行（外部 add 过）自动跳过。
  useEffect(() => {
    if (!open) return;
    setError(null);
    setNotice(null);
    let cancelled = false;
    invokeCommand<Record<string, ConflictKind>>('git_conflict_kinds', { repoPath })
      .then(k => {
        if (cancelled) return;
        setKinds(k ?? {});
        const list = filesRef.current.filter(f => f.status === 'C');
        const first = list[0]?.path ?? null;
        setSelectedPath(prev => (prev && list.some(f => f.path === prev) ? prev : first));
      })
      .catch(err => {
        if (!cancelled) setError(err instanceof Error ? err.message : String(err));
      });
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open, filesKey, repoPath]);

  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape' && !acting && !busy && !mergePath) onClose();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [open, acting, busy, mergePath, onClose]);

  /** 单个动作成功后：最后一个走关框（onAllResolved），否则通知 App 刷新。 */
  const afterAction = (path: string, message: string) => {
    if (remaining.length <= 1) {
      onAllResolved(message);
      return;
    }
    if (onFileResolved) {
      onFileResolved(path, message);
    } else {
      const idx = remaining.findIndex(f => f.path === path);
      const next = remaining[idx + 1] ?? remaining[0];
      setSelectedPath(next?.path ?? null);
    }
  };

  const applySide = async (side: 'ours' | 'theirs') => {
    if (!selectedPath) return;
    const path = selectedPath;
    setActing(true);
    setError(null);
    try {
      await invokeCommand('git_use_merge_side', { repoPath, path, side });
      const message = t().conflict.usedSide(side === 'ours' ? t().conflict.colYours : t().conflict.colTheirs, path);
      afterAction(path, message);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setActing(false);
    }
  };

  // —— 列表：分组模式按目录聚拢（组头为目录），平铺模式按路径排序 ——
  // 必须在 early return 之前：open 翻转时 hook 数不能变（React #310）。
  const sorted = [...remaining].sort((a, b) => a.path.localeCompare(b.path));
  const groups = useMemo(() => {
    if (!groupByDir) return null;
    const map = new Map<string, GitChangedFile[]>();
    for (const f of sorted) {
      const d = dirOf(f.path);
      if (!map.has(d)) map.set(d, []);
      map.get(d)!.push(f);
    }
    return [...map.entries()].sort((a, b) => a[0].localeCompare(b[0]));
    // sorted 由 remaining/files 派生，filesKey 覆盖它
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [groupByDir, filesKey]);

  if (!open) return null;
  const picking = sequencer?.cherryPicking ?? false;
  const merging = sequencer?.merging ?? false;
  const rebasing = sequencer?.rebasing ?? false;
  const head = picking ? sequencer?.cherryPickHead : sequencer?.mergeHead;
  const subtitle = picking && sequencer?.cherryPickHead
    ? sequencer.cherryPickAuthor
      ? t().conflict.subtitleCherryPick(sequencer.cherryPickHead, sequencer.cherryPickAuthor)
      : t().conflict.subtitleCherryPickBare(sequencer.cherryPickHead)
    : merging && sequencer?.mergeHead
      ? currentBranch
        ? t().conflict.subtitleMerge(sequencer.mergeHead, currentBranch)
        : t().conflict.subtitleMergeBare
      : rebasing
        ? t().conflict.subtitleRebase
        : t().conflict.subtitleGeneric;
  const yoursHead = currentBranch
    ? t().conflict.yoursCtx(currentBranch)
    : t().conflict.colYours;
  const theirsLabel = picking ? 'cherry-pick' : rebasing ? 'rebase' : (head || '');
  const theirsHead = theirsLabel
    ? t().conflict.theirsCtx(theirsLabel)
    : t().conflict.colTheirs;
  const kindOf = (p: string): ConflictKind => kinds[p] ?? 'content';
  const yoursStatus = (k: ConflictKind) => k === 'deleted_by_us' ? t().conflict.statusDeleted : t().conflict.statusModified;
  const theirsStatus = (k: ConflictKind) => k === 'deleted_by_them' ? t().conflict.statusDeleted : t().conflict.statusModified;

  const selectedKind = selectedPath ? kindOf(selectedPath) : null;
  const selectedFile = remaining.find(f => f.path === selectedPath) ?? null;

  const renderRow = (f: GitChangedFile) => {
    const k = kindOf(f.path);
    const active = selectedPath === f.path;
    const fileName = fileNameOf(f.path);
    const dir = dirOf(f.path);
    const isConflictColor = k === 'content' || k === 'deleted_by_us' || k === 'deleted_by_them';
    return (
      <div
        key={f.path}
        className={`conflict-row ${active ? 'active' : ''}`}
        onClick={() => setSelectedPath(f.path)}
        onDoubleClick={() => openMerge(f.path)}
        title={f.path}
      >
        <span className="conflict-cell name">
          <span className="conflict-file-icon">{renderFileIcon(fileName, false, false)}</span>
          <span className={`conflict-file-name ${isConflictColor && !active ? 'is-conflict' : ''}`}>
            {fileName}
          </span>
          {dir && <span className="conflict-file-dir">{dir}</span>}
        </span>
        <span className={`conflict-cell ${k === 'deleted_by_us' ? 'deleted' : ''}`}>{yoursStatus(k)}</span>
        <span className={`conflict-cell ${k === 'deleted_by_them' ? 'deleted' : ''}`}>{theirsStatus(k)}</span>
      </div>
    );
  };

  const getMergeDetails = (p: string) => {
    const picking = !!sequencer?.cherryPicking;
    const head = sequencer?.cherryPickHead || sequencer?.mergeHead || '';
    const theirsTitle = head
      ? picking
        ? t().conflict.mergeTool.remotePaneCherry(head)
        : t().conflict.mergeTool.remotePane(head)
      : t().conflict.mergeTool.remotePane('');
    const localDetails = currentBranch || undefined;
    const theirsDetails = picking
      ? [sequencer?.cherryPickAuthor, sequencer?.cherryPickSubject].filter(Boolean).join(' · ') ||
        head ||
        undefined
      : head || undefined;
    return {
      path: p,
      kind: kinds[p] ?? 'content',
      theirsTitle,
      localDetails,
      theirsDetails
    };
  };

  const openMerge = (p: string) => {
    if (onOpenMergeExternal) {
      onOpenMergeExternal(getMergeDetails(p));
      return;
    }
    setMergePath(p);
    onMergeOpenChange?.(true);
  };
  return (
    <div className={`py-dialog-backdrop ${isNativeWindow ? 'is-native-window' : ''}`}>
      <div
        className="py-conflict-dialog"
        style={
          isNativeWindow
            ? { position: 'relative', width: '100%', height: '100%', border: 'none', borderRadius: 0, boxShadow: 'none' }
            : {
                left: `${geom.x}px`,
                top: `${geom.y}px`,
                width: `${geom.w}px`,
                height: `${geom.h}px`
              }
        }
        onClick={e => e.stopPropagation()}
      >
        {!isNativeWindow && <ModalResizeHandles onResizeStart={handleResizePointerDown} />}
        <div
          className="mac-modal-header"
          data-tauri-drag-region={isNativeWindow ? 'true' : undefined}
          onPointerDown={isNativeWindow ? undefined : handleTitlePointerDown}
        >
          {!isNativeWindow && (
            <MacTrafficLights
              onClose={acting || busy ? undefined : onClose}
              onMaximize={toggleMaximize}
              isMaximized={isMaximized}
            />
          )}
          <span className="mac-title-centered">
            {t().conflict.title}
          </span>
        </div>
        <div className="conflict-subtitle">
          <div className="conflict-subtitle-lead">{subtitle}</div>
          {sequencer?.cherryPickSubject && (
            <div className="conflict-subtitle-subject">{sequencer.cherryPickSubject}</div>
          )}
        </div>
        {error && <div className="push-error" style={{ margin: '0 14px 8px' }}>{error}</div>}
        {notice && !error && <div className="conflict-notice">{notice}</div>}

        <div className="conflict-main">
          <div className="conflict-table-wrap">
            <div className="conflict-row conflict-head">
              <span className="conflict-cell name">{t().conflict.colName}</span>
              <span className="conflict-cell yours-head" title={yoursHead}>{yoursHead}</span>
              <span className="conflict-cell theirs-head" title={theirsHead}>{theirsHead}</span>
            </div>
            <div className="conflict-table">
              {remaining.length === 0 && (
                <div className="history-empty">{t().conflict.allResolved}</div>
              )}
              {groups
                ? groups.map(([dir, fs]) => (
                  <React.Fragment key={dir || '.'}>
                    <div className="conflict-group-head">{dir || '.'}</div>
                    {fs.map(renderRow)}
                  </React.Fragment>
                ))
                : sorted.map(renderRow)}
            </div>
            {selectedKind && (selectedKind === 'deleted_by_us' || selectedKind === 'deleted_by_them') && selectedFile && (
              <div className="conflict-delete-hint-bar">
                {selectedKind === 'deleted_by_us'
                  ? t().conflict.deletedByUs(selectedFile.path)
                  : t().conflict.deletedByThem(selectedFile.path)}{' '}
                {t().conflict.deleteHint}
              </div>
            )}
          </div>

          <div className="conflict-side-actions">
            <button className="py-btn" disabled={!selectedPath || acting || busy} onClick={() => void applySide('ours')}>
              {t().conflict.acceptYours}
            </button>
            <button className="py-btn" disabled={!selectedPath || acting || busy} onClick={() => void applySide('theirs')}>
              {t().conflict.acceptTheirs}
            </button>
            {/* Merge… 常亮（对齐 PyCharm）：删改冲突进去是一边空一边内容，
                逐块取空=删、取内容=留；全取空边则 Apply 时确认删除。 */}
            <button
              className="py-btn primary"
              disabled={!selectedPath || acting || busy}
              onClick={() => selectedPath && openMerge(selectedPath)}
            >
              {t().conflict.mergeDots}
            </button>
            {!selectedPath && <div className="conflict-select-hint">{t().conflict.selectHint}</div>}
          </div>
        </div>

        <div className="conflict-footer">
          <label className="conflict-group-check">
            <input
              type="checkbox"
              className="pycharm-checkbox"
              checked={groupByDir}
              onChange={e => setGroupByDir(e.target.checked)}
            />
            <span>{t().conflict.groupByDirectory}</span>
          </label>
          <button type="button" className="py-btn" onClick={onClose}>
            {t().common.close}
          </button>
        </div>
      </div>

      {mergePath && (
        <MergeToolDialog
          open
          isNativeWindow={isNativeWindow}
          repoPath={repoPath}
          path={mergePath}
          kind={kinds[mergePath] ?? 'content'}
          theirsTitle={
            head
              ? picking
                ? t().conflict.mergeTool.remotePaneCherry(head)
                : t().conflict.mergeTool.remotePane(head)
              : t().conflict.mergeTool.remotePane('')
          }
          localDetails={currentBranch || undefined}
          theirsDetails={
            picking
              ? [sequencer?.cherryPickAuthor, sequencer?.cherryPickSubject].filter(Boolean).join(' · ') ||
                head ||
                undefined
              : head || undefined
          }
          monacoTheme={monacoTheme}
          busy={busy}
          onClose={() => {
            setMergePath(null);
            onMergeOpenChange?.(false);
          }}
          onApplied={(path, message) => {
            setMergePath(null);
            onMergeOpenChange?.(false);
            // 对话框列表是打开时的快照：刷新后 App 会缩短它。
            afterAction(path, message);
          }}
        />
      )}
    </div>
  );
};
