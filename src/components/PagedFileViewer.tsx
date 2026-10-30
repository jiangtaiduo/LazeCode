// 大文件分页查看器（P2 大文件模式的前端一半）。
//
// 1MB 以上的文件不进 Monaco：全量文本 + tokenization + decorations 三重
// 常驻，误触大日志足以拖慢整机。这里只持有**一页**（默认 200 行）：
// 翻页走 `read_file_range` 按需拉，跨 IPC 的永远只是一页。
//
// 只读：paged 缓冲的 dirty 恒为 false，保存/自动保存天然跳过；
// 外部变更检测走探针-only 分支（见 App.checkExternalChange），
// 从不为刷新而全量读盘。
//
// 注意 read_file_range 目前是"后端全读再切片"（见 file_range.rs）：
// 翻页时 Rust 侧有同文件体积的瞬时分配，但前端堆 + Monaco 开销恒定一页。
// 后端改真流式（按偏移读）之前，分页面向的是 1–8MB 段；8MB 以上仍走
// file_too_large 错误面板（守门不变）。

import { useState } from 'react';
import { t } from '../services/i18n';

/** 与 Rust FileRangeDto 对应（serde camelCase，见 file_range.rs） */
export interface FileRangeDto {
  path: string;
  startLine: number;
  lines: string[];
  totalLines: number;
  hasMore: boolean;
}

export interface PagedFileViewerProps {
  /** 当前页文本（后端已切好，原文透出） */
  pageText: string;
  /** 文件总行数（后端 FileRangeDto.totalLines） */
  totalLines: number;
  /** 当前页（1-based） */
  page: number;
  /** 每页行数 */
  pageSize: number;
  /** 文件字节数（横幅展示用，后端探针 len） */
  fileLen: number;
  /** 翻页在途（按钮禁用，防连点攒 IPC） */
  loading: boolean;
  /** 翻到某页（1-based，调用方钳制范围） */
  onPage: (page: number) => void;
  /** 逃生口：仍要完整打开（走正常 Monaco 路径，大文件自担风险） */
  onOpenFull: () => void;
}

function formatBytes(n: number): string {
  if (n >= 1024 * 1024 * 1024) return `${(n / 1024 / 1024 / 1024).toFixed(1)} GB`;
  if (n >= 1024 * 1024) return `${(n / 1024 / 1024).toFixed(1)} MB`;
  if (n >= 1024) return `${(n / 1024).toFixed(1)} KB`;
  return `${n} B`;
}

export function PagedFileViewer(props: PagedFileViewerProps): React.JSX.Element {
  const { pageText, totalLines, page, pageSize, fileLen, loading, onPage, onOpenFull } = props;
  const totalPages = Math.max(1, Math.ceil(totalLines / pageSize));
  const from = totalLines === 0 ? 0 : (page - 1) * pageSize + 1;
  const to = Math.min(totalLines, page * pageSize);
  const [jump, setJump] = useState('');

  const go = (p: number) => {
    if (loading) return;
    const clamped = Math.min(totalPages, Math.max(1, Math.floor(p) || 1));
    if (clamped !== page) onPage(clamped);
  };

  return (
    <div style={{ flex: 1, minHeight: 0, display: 'flex', flexDirection: 'column', overflow: 'hidden' }}>
      <div
        style={{
          padding: '6px 12px',
          fontSize: '12px',
          color: 'var(--text-muted)',
          borderBottom: '1px solid var(--border-color)',
          display: 'flex',
          alignItems: 'center',
          gap: '12px',
          flexWrap: 'wrap'
        }}
      >
        <span style={{ flex: 1, minWidth: '200px' }}>
          {t().editor.pagedBanner(formatBytes(fileLen), String(from), String(to))}
        </span>
        <span>{t().editor.pagedPageOf(String(page), String(totalPages))}</span>
        <button
          type="button"
          className="pycharm-btn"
          disabled={loading || page <= 1}
          onClick={() => go(page - 1)}
        >
          {t().editor.pagedPrev}
        </button>
        <button
          type="button"
          className="pycharm-btn"
          disabled={loading || page >= totalPages}
          onClick={() => go(page + 1)}
        >
          {t().editor.pagedNext}
        </button>
        <input
          value={jump}
          onChange={e => setJump(e.target.value)}
          onKeyDown={e => {
            if (e.key === 'Enter') {
              go(Number(jump));
              setJump('');
            }
          }}
          placeholder={`1–${totalPages}`}
          style={{ width: '72px' }}
          aria-label={t().editor.pagedGo}
        />
        <button
          type="button"
          className="pycharm-btn"
          disabled={loading}
          onClick={() => {
            go(Number(jump));
            setJump('');
          }}
        >
          {t().editor.pagedGo}
        </button>
        <button type="button" className="pycharm-btn" disabled={loading} onClick={onOpenFull}>
          {t().editor.pagedOpenFull}
        </button>
      </div>
      <pre
        style={{
          flex: 1,
          margin: 0,
          padding: '8px 12px',
          overflow: 'auto',
          fontFamily: 'JetBrains Mono, Menlo, Monaco, monospace',
          fontSize: '12px',
          lineHeight: '19px',
          whiteSpace: 'pre',
          userSelect: 'text'
        }}
      >
        {loading ? '…' : pageText}
      </pre>
    </div>
  );
}
