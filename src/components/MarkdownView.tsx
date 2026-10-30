import { type CSSProperties, type PointerEvent, type ReactNode, useRef, useState } from 'react';
import { startPointerDrag } from '../services/panelDrag';
import { MarkdownPreview } from './MarkdownPreview';
import { t } from '../services/i18n';
import '../styles/markdown.css';

export type MarkdownViewMode = 'source' | 'preview' | 'split';

interface MarkdownViewProps {
  filePath: string;
  content: string;
  mode: MarkdownViewMode;
  onModeChange: (mode: MarkdownViewMode) => void;
  onOpenFile: (path: string) => void;
  source: ReactNode;
  scrollRatio: number;
  onScrollChange: (ratio: number) => void;
  /** 工作区根：透传给预览做链接越界判定 */
  workspaceRoot?: string;
}

type MarkdownModeDef = { id: MarkdownViewMode; label: () => string; title: () => string };

const MODES: MarkdownModeDef[] = [
  { id: 'source', label: () => t().markdownView.source, title: () => t().markdownView.sourceTitle },
  { id: 'preview', label: () => t().markdownView.preview, title: () => t().markdownView.previewTitle },
  { id: 'split', label: () => t().markdownView.split, title: () => t().markdownView.splitTitle }
];
const SPLIT_RESIZER_WIDTH = 8;
const MIN_SPLIT_PANE_WIDTH = 240;

export function MarkdownView({
  filePath,
  content,
  mode,
  onModeChange,
  onOpenFile,
  source,
  scrollRatio,
  onScrollChange,
  workspaceRoot,
}: MarkdownViewProps) {
  const [splitRatio, setSplitRatio] = useState(50);
  const bodyRef = useRef<HTMLDivElement>(null);
  const showSource = mode === 'source' || mode === 'split';
  const showPreview = mode === 'preview' || mode === 'split';

  const startResizeSplit = (event: PointerEvent<HTMLDivElement>) => {
    const body = bodyRef.current;
    if (!body) return;
    const rect = body.getBoundingClientRect();
    const availableWidth = Math.max(1, rect.width - SPLIT_RESIZER_WIDTH);
    const minWidth = Math.min(MIN_SPLIT_PANE_WIDTH, availableWidth / 2);
    const maxWidth = availableWidth - minWidth;
    const startX = event.clientX;
    const startWidth = availableWidth * splitRatio / 100;
    const computeWidth = (clientX: number) =>
      Math.max(minWidth, Math.min(maxWidth, startWidth + clientX - startX));
    const applyWidth = (width: number) => {
      body.style.gridTemplateColumns = `${width}px ${SPLIT_RESIZER_WIDTH}px minmax(0, 1fr)`;
    };
    const commitWidth = (clientX: number) => {
      setSplitRatio(computeWidth(clientX) / availableWidth * 100);
    };

    startPointerDrag(event, 'col-resize', {
      onMove: clientX => applyWidth(computeWidth(clientX)),
      onEnd: commitWidth,
      onCancel: commitWidth
    });
  };

  const splitStyle: CSSProperties | undefined = mode === 'split'
    ? {
        gridTemplateColumns: `minmax(0, calc(${splitRatio}% - ${SPLIT_RESIZER_WIDTH / 2}px)) ${SPLIT_RESIZER_WIDTH}px minmax(0, 1fr)`
      }
    : undefined;

  return (
    <div className="markdown-document-view">
      <div className="markdown-view-toolbar" data-tauri-drag-region="false">
        <span className="markdown-view-title">{t().markdownView.title}</span>
        <div className="markdown-mode-switcher" role="tablist" aria-label={t().markdownView.modeSwitcherLabel}>
          {MODES.map(item => (
            <button
              key={item.id}
              type="button"
              role="tab"
              aria-selected={mode === item.id}
              className={`markdown-mode-button${mode === item.id ? ' active' : ''}`}
              title={item.title()}
              onClick={() => onModeChange(item.id)}
            >
              {item.label()}
            </button>
          ))}
        </div>
      </div>
      <div
        ref={bodyRef}
        className={`markdown-view-body markdown-view-body-${mode}`}
        style={splitStyle}
      >
        <section className={`markdown-source-pane${showSource ? '' : ' markdown-pane-hidden'}`} aria-hidden={!showSource}>
          {source}
        </section>
        {mode === 'split' && (
          <div
            className="markdown-split-resizer"
            role="separator"
            aria-label={t().markdownView.resizerLabel}
            aria-orientation="vertical"
            aria-valuemin={0}
            aria-valuemax={100}
            aria-valuenow={Math.round(splitRatio)}
            onPointerDown={startResizeSplit}
            title={t().markdownView.resizerTitle}
          />
        )}
        <section className={`markdown-preview-pane${showPreview ? '' : ' markdown-pane-hidden'}`} aria-hidden={!showPreview}>
          <MarkdownPreview
            filePath={filePath}
            content={content}
            onOpenFile={onOpenFile}
            active={showPreview}
            scrollRatio={scrollRatio}
            onScrollChange={onScrollChange}
            workspaceRoot={workspaceRoot}
          />
        </section>
      </div>
    </div>
  );
}
