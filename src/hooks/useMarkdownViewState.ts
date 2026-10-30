import { useState, useRef, useCallback, useEffect } from 'react';
import type { MarkdownViewMode } from '../components/MarkdownView';
import { useStableCallback } from './useStableCallback';

export interface UseMarkdownViewStateReturn {
  markdownViewModes: Record<string, MarkdownViewMode>;
  setMarkdownViewModes: React.Dispatch<React.SetStateAction<Record<string, MarkdownViewMode>>>;
  markdownScrollRatios: Record<string, number>;
  setMarkdownScrollRatios: React.Dispatch<React.SetStateAction<Record<string, number>>>;
  activeMarkdownMode: MarkdownViewMode;
  activeMarkdownScrollRatio: number;
  handleMarkdownScroll: (path: string, ratio: number) => void;
  markdownModeChangeStable: (mode: MarkdownViewMode) => void;
  markdownScrollChangeStable: (ratio: number) => void;
  flushMarkdownScroll: () => void;
}

/**
 * 管理 Markdown 文件的查看模式（源码/预览/分栏）与滚动同步状态。
 *
 * 滚动回写通过 rAF + 定时器防抖合并，避免用户滚动时高频触发全局重渲染。
 */
export function useMarkdownViewState(activeFilePath: string): UseMarkdownViewStateReturn {
  const [markdownViewModes, setMarkdownViewModes] = useState<Record<string, MarkdownViewMode>>({});
  const [markdownScrollRatios, setMarkdownScrollRatios] = useState<Record<string, number>>({});

  const scrollRatioTimeoutRef = useRef<number | null>(null);
  const pendingScrollRatioRef = useRef<{ path: string; ratio: number } | null>(null);

  const flushMarkdownScroll = useCallback(() => {
    if (scrollRatioTimeoutRef.current !== null) {
      window.clearTimeout(scrollRatioTimeoutRef.current);
      scrollRatioTimeoutRef.current = null;
    }
    const pending = pendingScrollRatioRef.current;
    pendingScrollRatioRef.current = null;
    if (!pending) return;
    setMarkdownScrollRatios(prev => {
      if (Math.abs((prev[pending.path] ?? 0) - pending.ratio) < 0.001) return prev;
      return { ...prev, [pending.path]: pending.ratio };
    });
  }, []);

  const handleMarkdownScroll = useCallback((path: string, ratio: number) => {
    pendingScrollRatioRef.current = { path, ratio: Math.max(0, Math.min(1, ratio)) };
    if (scrollRatioTimeoutRef.current !== null) {
      window.clearTimeout(scrollRatioTimeoutRef.current);
    }
    scrollRatioTimeoutRef.current = window.setTimeout(flushMarkdownScroll, 250);
  }, [flushMarkdownScroll]);

  useEffect(() => () => {
    flushMarkdownScroll();
  }, [flushMarkdownScroll]);

  const activeMarkdownMode: MarkdownViewMode = activeFilePath
    ? markdownViewModes[activeFilePath] ?? 'preview'
    : 'preview';

  const activeMarkdownScrollRatio = activeFilePath
    ? markdownScrollRatios[activeFilePath] ?? 0
    : 0;

  const markdownModeChangeStable = useStableCallback((mode: MarkdownViewMode) => {
    if (!activeFilePath) return;
    setMarkdownViewModes(prev => ({ ...prev, [activeFilePath]: mode }));
  });

  const markdownScrollChangeStable = useStableCallback((ratio: number) => {
    if (!activeFilePath) return;
    handleMarkdownScroll(activeFilePath, ratio);
  });

  return {
    markdownViewModes,
    setMarkdownViewModes,
    markdownScrollRatios,
    setMarkdownScrollRatios,
    activeMarkdownMode,
    activeMarkdownScrollRatio,
    handleMarkdownScroll,
    markdownModeChangeStable,
    markdownScrollChangeStable,
    flushMarkdownScroll,
  };
}
