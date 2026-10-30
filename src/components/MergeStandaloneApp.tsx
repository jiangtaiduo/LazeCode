import React, { useState, useEffect, useCallback } from 'react';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { emit, listen } from '@tauri-apps/api/event';
import { invokeCommand } from '../services/tauriBridge';
import { MergeToolDialog } from './MergeToolDialog';
import { ensureMonacoReady, MONACO_THEME_BY_APP_THEME } from '../services/monacoSetup';
import { loadSettings } from '../services/appSettings';
import { applyThemeVariant } from '../main';
import type { ConflictKind } from '../types';

interface MergeToolParams {
  repo_path: string;
  file_path: string;
  kind?: string;
  theirs_title?: string;
  local_details?: string;
  theirs_details?: string;
}

export const MergeStandaloneApp: React.FC = () => {
  const [params, setParams] = useState<MergeToolParams | null>(null);
  const [loading, setLoading] = useState(true);
  const [monacoLoaded, setMonacoLoaded] = useState(false);
  const [monacoThemeName, setMonacoThemeName] = useState('ide-jetbrains-dark');

  useEffect(() => {
    let cancelled = false;
    const settings = loadSettings();
    const variant = settings.uiVariant ?? 1;
    const themeId = variant;
    const themeName = MONACO_THEME_BY_APP_THEME[themeId] ?? 'ide-jetbrains-dark';
    setMonacoThemeName(themeName);

    document.body.setAttribute('data-variant', String(variant));
    document.body.classList.toggle('platform-mac', true);

    Promise.all([
      applyThemeVariant(variant),
      ensureMonacoReady(themeId)
    ])
      .then(() => {
        if (!cancelled) setMonacoLoaded(true);
      })
      .catch(err => {
        console.error('Failed to initialize theme and Monaco in MergeStandaloneApp:', err);
        if (!cancelled) setMonacoLoaded(true);
      });

    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const p = await invokeCommand<MergeToolParams | null>('get_merge_tool_params');
        if (!cancelled && p) {
          setParams(p);
        }
      } catch (err) {
        console.error('Failed to load merge tool params:', err);
      } finally {
        if (!cancelled) setLoading(false);
      }
    })();

    const unlisten = listen<MergeToolParams>('ide_merge_tool_params_updated', e => {
      if (!cancelled && e.payload) {
        setParams(e.payload);
      }
    });

    return () => {
      cancelled = true;
      unlisten.then(u => u());
    };
  }, []);

  const handleClose = useCallback(async () => {
    try {
      await invokeCommand('close_window', { label: 'win_merge_tool' });
    } catch {
      try {
        const win = getCurrentWindow();
        await win.close();
      } catch (err) {
        console.error('Failed to close merge window:', err);
      }
    }
  }, []);

  const handleApplied = useCallback(async (path: string, message: string) => {
    try {
      await emit('ide_conflict_resolved', { path, message });
    } catch (err) {
      console.error('Failed to emit ide_conflict_resolved:', err);
    }
    try {
      await invokeCommand('close_window', { label: 'win_merge_tool' });
    } catch {
      try {
        const win = getCurrentWindow();
        await win.close();
      } catch (err) {
        console.error('Failed to close merge window after apply:', err);
      }
    }
  }, []);

  if (loading || !params || !monacoLoaded) {
    return (
      <div className="conflict-standalone-root" style={{ alignItems: 'center', justifyContent: 'center' }}>
        <div className="editor-lazy-spinner" />
        <div style={{ color: 'var(--text-muted)', fontSize: 13, marginTop: 12 }}>
          Loading Merge Tool...
        </div>
      </div>
    );
  }

  return (
    <div className="conflict-standalone-root">
      <MergeToolDialog
        open={true}
        isNativeWindow={true}
        repoPath={params.repo_path}
        path={params.file_path}
        kind={(params.kind as ConflictKind) ?? 'content'}
        theirsTitle={params.theirs_title || 'Changes from conflict'}
        localDetails={params.local_details}
        theirsDetails={params.theirs_details}
        monacoTheme={monacoThemeName}
        onClose={handleClose}
        onApplied={handleApplied}
      />
    </div>
  );
};
