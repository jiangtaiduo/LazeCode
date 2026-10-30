import React, { useState, useEffect, useCallback } from 'react';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { emit, listen } from '@tauri-apps/api/event';
import { invokeCommand } from '../services/tauriBridge';
import { ConflictDialog } from './ConflictDialog';
import { loadSettings } from '../services/appSettings';
import { applyThemeVariant } from '../main';
import type { GitChangedFile, SequencerState, GitRepoInfo } from '../types';

export const ConflictStandaloneApp: React.FC = () => {
  const [repoPath, setRepoPath] = useState<string>('');
  const [files, setFiles] = useState<GitChangedFile[]>([]);
  const [currentBranch, setCurrentBranch] = useState<string>('');
  const [sequencer, setSequencer] = useState<SequencerState | null>(null);
  const [loading, setLoading] = useState(true);

  // 初始化拉取仓库路径与冲突上下文
  const reloadData = useCallback(async (path: string) => {
    try {
      const [changedList, repoInfo, seq] = await Promise.all([
        invokeCommand<GitChangedFile[]>('git_status', { repoPath: path }),
        invokeCommand<GitRepoInfo>('git_repo_info', { repoPath: path }).catch(() => ({ isRepo: false, root: '', branch: '' })),
        invokeCommand<SequencerState>('git_sequencer_state', { repoPath: path }).catch(() => null)
      ]);
      setFiles((changedList ?? []).filter(f => f.status === 'C'));
      setCurrentBranch(repoInfo?.branch ?? '');
      setSequencer(seq);
    } catch (err) {
      console.error('Failed to load conflict status:', err);
    }
  }, []);

  useEffect(() => {
    const settings = loadSettings();
    const variant = settings.uiVariant ?? 1;
    document.body.setAttribute('data-variant', String(variant));
    document.body.classList.toggle('platform-mac', true);
    void applyThemeVariant(variant);

    let cancelled = false;
    (async () => {
      try {
        let path = await invokeCommand<string | null>('get_conflict_repo_path');
        if (!path) {
          path = localStorage.getItem('ide_conflict_repo_path') || '';
        }
        if (cancelled) return;
        if (path) {
          setRepoPath(path);
          try {
            localStorage.setItem('ide_conflict_repo_path', path);
          } catch {
            // ignore
          }
          await reloadData(path);
        }
      } finally {
        if (!cancelled) setLoading(false);
      }
    })();

    return () => {
      cancelled = true;
    };
  }, [reloadData]);

  const handleClose = useCallback(async () => {
    try {
      await emit('ide_conflict_closed');
    } catch {
      // ignore
    }
    try {
      await invokeCommand('close_window', { label: 'win_conflicts' });
    } catch {
      try {
        const win = getCurrentWindow();
        await win.close();
      } catch (err) {
        console.error('Failed to close conflict window:', err);
      }
    }
  }, []);

  const handleAllResolved = useCallback(async (message: string) => {
    try {
      await emit('ide_all_conflicts_resolved', { message });
    } catch {
      // ignore
    }
    try {
      await invokeCommand('close_window', { label: 'win_conflicts' });
    } catch {
      try {
        const win = getCurrentWindow();
        await win.close();
      } catch (err) {
        console.error('Failed to close conflict window after all resolved:', err);
      }
    }
  }, []);

  // 监听来自独立解决冲突窗口（win_merge_tool）的单文件解决通知
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    listen<{ path: string; message: string }>('ide_conflict_resolved', async e => {
      if (repoPath) {
        try {
          const changedList = await invokeCommand<GitChangedFile[]>('git_status', { repoPath });
          const remaining = (changedList ?? []).filter(f => f.status === 'C');
          setFiles(remaining);
          if (remaining.length === 0) {
            await handleAllResolved(e.payload.message || 'All conflicts resolved');
          }
        } catch (err) {
          console.error('Failed to refresh conflicts after resolution:', err);
        }
      }
    }).then(u => {
      unlisten = u;
    });
    return () => {
      unlisten?.();
    };
  }, [repoPath, handleAllResolved]);


  const handleFileResolved = useCallback(
    async (path: string, message: string) => {
      try {
        await emit('ide_conflict_resolved', { path, message });
      } catch {
        // ignore
      }
      if (repoPath) {
        await reloadData(repoPath);
      }
    },
    [repoPath, reloadData]
  );

  const handleOpenMergeExternal = useCallback(
    async (params: { path: string; kind: string; theirsTitle: string; localDetails?: string; theirsDetails?: string }) => {
      if (!repoPath) return;
      try {
        await invokeCommand('open_merge_tool_window', {
          params: {
            repo_path: repoPath,
            file_path: params.path,
            kind: params.kind,
            theirs_title: params.theirsTitle,
            local_details: params.localDetails ?? null,
            theirs_details: params.theirsDetails ?? null,
          }
        });
      } catch (err) {
        console.error('Failed to open merge tool window:', err);
      }
    },
    [repoPath]
  );

  if (loading) {
    return (
      <div className="conflict-standalone-root" style={{ alignItems: 'center', justifyContent: 'center' }}>
        <div style={{ color: 'var(--text-muted)', fontSize: 13 }}>Loading conflicts...</div>
      </div>
    );
  }

  return (
    <div className="conflict-standalone-root">
      <ConflictDialog
        open={true}
        isNativeWindow={true}
        repoPath={repoPath}
        files={files}
        currentBranch={currentBranch}
        sequencer={sequencer}
        monacoTheme="ide-jetbrains-dark"
        onClose={handleClose}
        onAllResolved={handleAllResolved}
        onFileResolved={handleFileResolved}
        onOpenMergeExternal={handleOpenMergeExternal}
      />
    </div>
  );
};
