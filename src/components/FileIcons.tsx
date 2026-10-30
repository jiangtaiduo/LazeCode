import React from 'react';

export const PythonIcon: React.FC<{ size?: number; className?: string }> = React.memo(({ size = 15, className = '' }) => (
  <svg width={size} height={size} viewBox="0 0 16 16" fill="none" className={className} style={{ flexShrink: 0 }}>
    {/* Blue upper part */}
    <path
      d="M7.88 1C4.42 1 4.6 2.5 4.6 2.5l.01 1.55h3.36v.48H3.24S1 4.28 1 7.74c0 3.47 1.95 3.35 1.95 3.35h1.16V9.45s-.06-1.95 1.93-1.95h3.32s1.88.03 1.88-1.85V3.34S11.5 1 7.88 1zm-1.8 1.15a.65.65 0 1 1 0 1.3.65.65 0 0 1 0-1.3z"
      fill="#387eb8"
    />
    {/* Yellow lower part */}
    <path
      d="M8.12 15c3.46 0 3.28-1.5 3.28-1.5l-.01-1.55H8.03v-.48h4.73S15 11.72 15 8.26c0-3.47-1.95-3.35-1.95-3.35h-1.16v1.64s.06 1.95-1.93 1.95H6.67s-1.88-.03-1.88 1.85v3.31S4.5 15 8.12 15zm1.8-1.15a.65.65 0 1 1 0-1.3.65.65 0 0 1 0 1.3z"
      fill="#ffe873"
    />
  </svg>
));

export const GitBranchIcon: React.FC<{ size?: number; color?: string; className?: string }> = React.memo(({
  size = 14,
  color = 'currentColor',
  className = ''
}) => (
  <svg width={size} height={size} viewBox="0 0 16 16" fill="none" className={className} style={{ flexShrink: 0 }}>
    <circle cx="4.5" cy="3.5" r="2" stroke={color} strokeWidth="1.5" fill="none" />
    <circle cx="4.5" cy="12.5" r="2" stroke={color} strokeWidth="1.5" fill="none" />
    <circle cx="11.5" cy="5.5" r="2" stroke={color} strokeWidth="1.5" fill="none" />
    <path d="M4.5 5.5v5" stroke={color} strokeWidth="1.5" strokeLinecap="round" />
    <path d="M4.5 7.5c0 2 2.5 3 7 3v-3" stroke={color} strokeWidth="1.5" strokeLinecap="round" />
  </svg>
));

/** 极简文件夹：中性灰蓝描边 + 极浅填充，与整体线条图标统一（弃用黄色块面） */
const FOLDER_STROKE = '#7d8794';
const FOLDER_FILL = 'rgba(125, 135, 148, 0.16)';

const FolderBody: React.FC<{ isOpen: boolean }> = ({ isOpen }) =>
  isOpen ? (
    <>
      {/* 掀开的盖片 */}
      <path
        d="M2 10.4V4.5c0-.72.58-1.3 1.3-1.3h2.8l1.6 1.6h4.9c.72 0 1.3.58 1.3 1.3v1.5H4.8c-.62 0-1.16.42-1.31 1.02L2 10.4Z"
        fill={FOLDER_FILL}
        stroke={FOLDER_STROKE}
        strokeWidth="1.1"
        strokeLinejoin="round"
      />
      {/* 前倾的主体 */}
      <path
        d="M4.9 8.3h8.9c.86 0 1.45.85 1.16 1.65l-1.05 2.85c-.19.5-.66.84-1.19.84H3.7c-.83 0-1.4-.83-1.13-1.61l.98-2.8c.18-.53.65-.93 1.35-.93Z"
        fill={FOLDER_FILL}
        stroke={FOLDER_STROKE}
        strokeWidth="1.1"
        strokeLinejoin="round"
      />
    </>
  ) : (
    <path
      d="M2 4.4c0-.77.63-1.4 1.4-1.4h2.9l1.6 1.6h4.7c.77 0 1.4.63 1.4 1.4v6c0 .77-.63 1.4-1.4 1.4H3.4c-.77 0-1.4-.63-1.4-1.4V4.4Z"
      fill={FOLDER_FILL}
      stroke={FOLDER_STROKE}
      strokeWidth="1.1"
      strokeLinejoin="round"
    />
  );

export const FolderIcon: React.FC<{ size?: number; color?: string; isOpen?: boolean }> = React.memo(({
  size = 15,
  isOpen = false
}) => (
  <svg width={size} height={size} viewBox="0 0 16 16" fill="none" style={{ flexShrink: 0 }}>
    <FolderBody isOpen={isOpen} />
  </svg>
));

/** 虚拟环境目录：极简文件夹 + 内部小圆环标记（替代黄色 Python 配色） */
export const VenvFolderIcon: React.FC<{ size?: number }> = React.memo(({ size = 15 }) => (
  <svg width={size} height={size} viewBox="0 0 16 16" fill="none" style={{ flexShrink: 0 }}>
    <FolderBody isOpen={false} />
    <circle cx="8" cy="9" r="2" stroke="#5b9bd5" strokeWidth="1.1" fill="none" />
  </svg>
));

export const DockerIcon: React.FC<{ size?: number }> = React.memo(({ size = 15 }) => (
  <svg width={size} height={size} viewBox="0 0 16 16" fill="none" style={{ flexShrink: 0 }}>
    <path
      d="M14.7 6.3c-.2-.1-.9-.2-1.6.2-.2-.5-.6-.9-1.2-1.2l-.4-.2-.2.4c-.4.8-.3 1.7.1 2.3-.6.4-1.6.3-2.6.3H1.3C1.1 8.8 1 9.8 1 10.8c0 2.9 2.4 4.2 5.3 4.2 3.6 0 6.6-1.8 7.6-5.4.8 0 1.8-.4 2.1-1.6v-.3l-.3-.4z"
      fill="#2496ed"
    />
    <path d="M2.5 5.5h1.5v1.5H2.5zm2 0H6v1.5H4.5zm2 0H8v1.5H6.5zm-2-2H6V5H4.5zm2 0H8V5H6.5zm2 0H10V5H8.5zm0 2H10v1.5H8.5z" fill="#2496ed" />
  </svg>
));

export const EnvFileIcon: React.FC<{ size?: number }> = React.memo(({ size = 15 }) => (
  <svg width={size} height={size} viewBox="0 0 16 16" fill="none" style={{ flexShrink: 0 }}>
    <rect x="2" y="2" width="12" height="12" rx="2" stroke="#e5c07b" strokeWidth="1.2" fill="none" />
    <line x1="4.5" y1="5.5" x2="11.5" y2="5.5" stroke="#e5c07b" strokeWidth="1.2" strokeLinecap="round" />
    <line x1="4.5" y1="8" x2="11.5" y2="8" stroke="#e5c07b" strokeWidth="1.2" strokeLinecap="round" />
    <line x1="4.5" y1="10.5" x2="8.5" y2="10.5" stroke="#e5c07b" strokeWidth="1.2" strokeLinecap="round" />
    <circle cx="6" cy="5.5" r="1.2" fill="#e5c07b" />
    <circle cx="10" cy="8" r="1.2" fill="#e5c07b" />
    <circle cx="6" cy="10.5" r="1.2" fill="#e5c07b" />
  </svg>
));

export const TsFileIcon: React.FC<{ size?: number }> = React.memo(({ size = 15 }) => (
  <svg width={size} height={size} viewBox="0 0 16 16" fill="none" style={{ flexShrink: 0 }}>
    <rect x="1.5" y="1.5" width="13" height="13" rx="2" fill="#3178c6" />
    <path d="M3.5 5.5h4v1.2H6.1v4.8H4.9V6.7H3.5V5.5zm5 2.5c.3-.3.8-.5 1.4-.5.8 0 1.4.4 1.4 1.1 0 .7-.4 1-1.1 1.2l-.4.1c-.5.1-.7.3-.7.6 0 .3.3.5.7.5.4 0 .8-.2 1-.4v1.1c-.3.2-.8.3-1.3.3-1 0-1.6-.5-1.6-1.3 0-.7.5-1.1 1.1-1.3l.4-.1c.4-.1.6-.3.6-.5 0-.3-.2-.4-.6-.4-.4 0-.7.2-.9.4V8z" fill="#fff" />
  </svg>
));

export const JsFileIcon: React.FC<{ size?: number }> = React.memo(({ size = 15 }) => (
  <svg width={size} height={size} viewBox="0 0 16 16" fill="none" style={{ flexShrink: 0 }}>
    <rect x="1.5" y="1.5" width="13" height="13" rx="2" fill="#f7df1e" />
    <path d="M5.5 10.2c0 .8.5 1.3 1.3 1.3.7 0 1.2-.4 1.2-1.1V5.5H9.2v4.8c0 1.4-1 2.2-2.4 2.2-1.3 0-2.3-.7-2.4-2.1l1.1-.2zm4.3-1.8c.4-.3.9-.5 1.5-.5.9 0 1.5.5 1.5 1.3 0 .8-.5 1.2-1.3 1.4l-.5.1c-.5.1-.7.3-.7.6 0 .3.3.6.8.6.5 0 .9-.2 1.2-.4v1.2c-.4.2-.9.3-1.5.3-1.2 0-1.9-.6-1.9-1.5 0-.8.6-1.3 1.3-1.5l.5-.1c.4-.1.6-.3.6-.6 0-.3-.3-.5-.7-.5-.4 0-.8.2-1.1.4V8.4z" fill="#000" />
  </svg>
));

export const HtmlFileIcon: React.FC<{ size?: number }> = React.memo(({ size = 15 }) => (
  <svg width={size} height={size} viewBox="0 0 16 16" fill="none" style={{ flexShrink: 0 }}>
    <path d="M2.5 2l1.1 12.5L8 15.5l4.4-1L13.5 2H2.5z" fill="#e44d26" />
    <path d="M8 3.1v11.2l3.4-.8.9-10.4H8z" fill="#f16529" />
    <path d="M5.2 5.2h5.6l-.2 2.1H5.4l.2 2.1h4.6l-.4 3.7-1.8.5-1.8-.5-.1-1.3H4.9l.2 2.4L8 14.8l2.9-.8.7-7.8H5.1l.1-1z" fill="#fff" />
  </svg>
));

export const CssFileIcon: React.FC<{ size?: number }> = React.memo(({ size = 15 }) => (
  <svg width={size} height={size} viewBox="0 0 16 16" fill="none" style={{ flexShrink: 0 }}>
    <path d="M2.5 2l1.1 12.5L8 15.5l4.4-1L13.5 2H2.5z" fill="#264de4" />
    <path d="M8 3.1v11.2l3.4-.8.9-10.4H8z" fill="#2965f1" />
    <path d="M5.2 5.2h5.6l-.2 2.1H7.8l.2 2.1h2.4l-.4 3.7-2 .5-2-.5-.1-1.3H4.7l.2 2.4L8 14.8l3.1-.8.7-7.8H5.1l.1-1z" fill="#fff" />
  </svg>
));

export const JsonFileIcon: React.FC<{ size?: number }> = React.memo(({ size = 15 }) => (
  <svg width={size} height={size} viewBox="0 0 16 16" fill="none" style={{ flexShrink: 0 }}>
    <rect x="2" y="2" width="12" height="12" rx="2" fill="#393b40" />
    <text x="8" y="11" fill="#e5c07b" fontSize="8" fontWeight="bold" textAnchor="middle" fontFamily="monospace">
      &#123;&#125;
    </text>
  </svg>
));

export const VueFileIcon: React.FC<{ size?: number }> = React.memo(({ size = 15 }) => (
  <svg width={size} height={size} viewBox="0 0 16 16" fill="none" style={{ flexShrink: 0 }}>
    <path d="M1.5 2.5h2.6L8 9.2l3.9-6.7h2.6L8 14.2 1.5 2.5z" fill="#41b883" />
    <path d="M5.4 2.5h1.8L8 4.4l.8-1.9h1.8L8 8 5.4 2.5z" fill="#35495e" />
  </svg>
));

export const TextFileIcon: React.FC<{ size?: number; color?: string }> = React.memo(({ size = 15, color = 'currentColor' }) => (
  <svg width={size} height={size} viewBox="0 0 16 16" fill="none" style={{ flexShrink: 0 }}>
    <path d="M3.5 2.5h6l3 3v8a1 1 0 0 1-1 1h-8a1 1 0 0 1-1-1v-10a1 1 0 0 1 1-1z" stroke={color} strokeWidth="1.2" fill="none" />
    <path d="M9.5 2.5v3h3" stroke={color} strokeWidth="1.2" fill="none" />
    <line x1="5.5" y1="7.5" x2="10.5" y2="7.5" stroke={color} strokeWidth="1" strokeLinecap="round" />
    <line x1="5.5" y1="9.5" x2="10.5" y2="9.5" stroke={color} strokeWidth="1" strokeLinecap="round" />
    <line x1="5.5" y1="11.5" x2="8.5" y2="11.5" stroke={color} strokeWidth="1" strokeLinecap="round" />
  </svg>
));

export const ShellFileIcon: React.FC<{ size?: number }> = React.memo(({ size = 15 }) => (
  <svg width={size} height={size} viewBox="0 0 16 16" fill="none" style={{ flexShrink: 0 }}>
    <rect x="2" y="2" width="12" height="12" rx="2" fill="#2d3748" />
    <path d="M4.5 5.5l2.5 2.5-2.5 2.5" stroke="#48bb78" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" />
    <line x1="8.5" y1="10.5" x2="11.5" y2="10.5" stroke="#48bb78" strokeWidth="1.5" strokeLinecap="round" />
  </svg>
));

export const RustFileIcon: React.FC<{ size?: number }> = React.memo(({ size = 15 }) => (
  <svg width={size} height={size} viewBox="0 0 16 16" fill="none" style={{ flexShrink: 0 }}>
    <circle cx="8" cy="8" r="6" stroke="#dea584" strokeWidth="1.5" fill="none" />
    <path d="M6 5.5h2.5c.8 0 1.5.5 1.5 1.2 0 .6-.5 1.1-1.1 1.2l1.6 2.6H8.8L7.4 8H7v2.5H6V5.5zm1 1.8h1.2c.4 0 .6-.2.6-.5s-.2-.5-.6-.5H7v1z" fill="#dea584" />
  </svg>
));

export const MarkdownFileIcon: React.FC<{ size?: number }> = React.memo(({ size = 15 }) => (
  <svg width={size} height={size} viewBox="0 0 16 16" fill="none" style={{ flexShrink: 0 }}>
    <text x="5" y="11" fill="#58a6ff" fontSize="9" fontWeight="bold" textAnchor="middle" fontFamily="sans-serif">M</text>
    <path d="M11 5.5v5M9.2 8.8l1.8 1.9 1.8-1.9" stroke="#58a6ff" strokeWidth="1.3" strokeLinecap="round" strokeLinejoin="round" />
  </svg>
));
export const GitLogStripeIcon: React.FC<{ size?: number; color?: string }> = React.memo(({ size = 18, color = 'currentColor' }) => (
  <svg width={size} height={size} viewBox="0 0 16 16" fill="none" style={{ flexShrink: 0 }}>
    <circle cx="4.8" cy="3.4" r="1.6" stroke={color} strokeWidth="1.35" fill="none" />
    <circle cx="4.8" cy="12.6" r="1.6" stroke={color} strokeWidth="1.35" fill="none" />
    <circle cx="11.6" cy="8" r="1.6" stroke={color} strokeWidth="1.35" fill="none" />
    <path d="M4.8 5v5.9" stroke={color} strokeWidth="1.35" strokeLinecap="round" />
    <path d="M6.4 3.6h2.9c1.25 0 2.3 1.05 2.3 2.3v0.55" stroke={color} strokeWidth="1.35" strokeLinecap="round" fill="none" />
  </svg>
));

export const TerminalStripeIcon: React.FC<{ size?: number; color?: string }> = React.memo(({ size = 18, color = 'currentColor' }) => (
  <svg width={size} height={size} viewBox="0 0 16 16" fill="none" style={{ flexShrink: 0 }}>
    <rect x="1.9" y="2.7" width="12.2" height="10.6" rx="1.7" stroke={color} strokeWidth="1.4" fill="none" />
    <path d="M4.7 6.1l2.4 2-2.4 2" stroke={color} strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round" fill="none" />
    <path d="M9.2 10.2h2.7" stroke={color} strokeWidth="1.4" strokeLinecap="round" />
  </svg>
));

export const IconGear: React.FC<{ size?: number; color?: string }> = React.memo(({ size = 16, color = 'currentColor' }) => (
  <svg width={size} height={size} viewBox="0 0 16 16" fill="none" style={{ flexShrink: 0 }}>
    <circle cx="8" cy="8" r="2.5" stroke={color} strokeWidth="1.3" fill="none" />
    <path
      d="M6.8 1.8h2.4l.35 1.5c.38.13.73.32 1.04.55l1.4-.6 1.7 1.7-.6 1.4c.23.31.42.66.55 1.04l1.5.35v2.4l-1.5.35c-.13.38-.32.73-.55 1.04l.6 1.4-1.7 1.7-1.4-.6c-.31.23-.66.42-1.04.55l-.35 1.5H6.8l-.35-1.5c-.38-.13-.73-.32-1.04-.55l-1.4.6-1.7-1.7.6-1.4c-.23-.31-.42-.66-.55-1.04l-1.5-.35V8.9l1.5-.35c.13-.38.32-.73.55-1.04l-.6-1.4 1.7-1.7 1.4.6c.31-.23.66-.42 1.04-.55l.35-1.5z"
      stroke={color}
      strokeWidth="1.2"
      strokeLinejoin="round"
      strokeLinecap="round"
      fill="none"
    />
  </svg>
));

export const SettingsStripeIcon: React.FC<{ size?: number; color?: string }> = React.memo(({ size = 18, color = 'currentColor' }) => (
  <IconGear size={size} color={color} />
));

export const GitFileIcon: React.FC<{ size?: number }> = React.memo(({ size = 15 }) => (
  <svg width={size} height={size} viewBox="0 0 16 16" fill="none" style={{ flexShrink: 0 }}>
    <rect x="2" y="2" width="12" height="12" rx="2" fill="#f05032" />
    <circle cx="6" cy="6" r="1.5" fill="#fff" />
    <circle cx="6" cy="10" r="1.5" fill="#fff" />
    <circle cx="10.5" cy="8" r="1.5" fill="#fff" />
    <path d="M6 6v4m0-2l4.5 0" stroke="#fff" strokeWidth="1.2" />
  </svg>
));

export const CommitStripeIcon: React.FC<{ size?: number; color?: string }> = React.memo(({ size = 18, color = 'currentColor' }) => (
  <svg width={size} height={size} viewBox="0 0 16 16" fill="none" style={{ flexShrink: 0 }}>
    <circle cx="8" cy="8" r="3.1" stroke={color} strokeWidth="1.4" fill="none" />
    <path d="M1.6 8h3.2M11.2 8h3.2" stroke={color} strokeWidth="1.4" strokeLinecap="round" />
  </svg>
));

export const ProjectStripeIcon: React.FC<{ size?: number; color?: string }> = React.memo(({ size = 18, color = 'currentColor' }) => (
  <svg width={size} height={size} viewBox="0 0 16 16" fill="none" style={{ flexShrink: 0 }}>
    <path
      d="M1.9 4.35c0-.8.65-1.45 1.45-1.45h2.8c.42 0 .82.18 1.1.5l.74.85c.28.32.68.5 1.1.5h4.36c.8 0 1.45.65 1.45 1.45v6.6c0 .8-.65 1.45-1.45 1.45H3.35c-.8 0-1.45-.65-1.45-1.45V4.35z"
      stroke={color}
      strokeWidth="1.4"
      strokeLinejoin="round"
      fill="none"
    />
    <path d="M2.1 6.55h11.8" stroke={color} strokeWidth="1.2" strokeLinecap="round" opacity="0.5" />
  </svg>
));

// ---------------------------------------------------------------------------
// 16×16 线性工具图标（stroke 1.4，随 currentColor / color 属性着色）
// ---------------------------------------------------------------------------

const outlineProps = (color: string) => ({
  stroke: color,
  strokeWidth: 1.4,
  strokeLinecap: 'round' as const,
  strokeLinejoin: 'round' as const,
  fill: 'none'
});

export const IconRefresh: React.FC<{ size?: number; color?: string }> = React.memo(({ size = 14, color = 'currentColor' }) => (
  <svg width={size} height={size} viewBox="0 0 16 16" fill="none" style={{ flexShrink: 0 }}>
    <path d="M14 8a6 6 0 1 1-6-6c1.68 0 3.29.67 4.49 1.83L14 5.33" {...outlineProps(color)} />
    <path d="M14 2v3.33h-3.33" {...outlineProps(color)} />
  </svg>
));

/** Show Diff：两个对比文档面板 */
export const IconDiff: React.FC<{ size?: number; color?: string }> = React.memo(({ size = 14, color = 'currentColor' }) => (
  <svg width={size} height={size} viewBox="0 0 16 16" fill="none" style={{ flexShrink: 0 }}>
    <rect x="2" y="2" width="5.2" height="12" rx="1" {...outlineProps(color)} />
    <rect x="8.8" y="2" width="5.2" height="12" rx="1" {...outlineProps(color)} />
    <path d="M3.6 5.5h2M3.6 8h2M3.6 10.5h2" {...outlineProps(color)} />
    <path d="M10.4 5.5h2M10.4 8h2M10.4 10.5h2" {...outlineProps(color)} />
  </svg>
));

/** Merge：两条汇入一条的提交图标记（合并提交行首） */
export const IconMerge: React.FC<{ size?: number; color?: string }> = React.memo(({ size = 12, color = 'currentColor' }) => (
  <svg width={size} height={size} viewBox="0 0 16 16" fill="none" style={{ flexShrink: 0 }}>
    <circle cx="4.2" cy="3.6" r="1.7" {...outlineProps(color)} />
    <circle cx="4.2" cy="12.4" r="1.7" {...outlineProps(color)} />
    <circle cx="11.8" cy="8" r="1.7" {...outlineProps(color)} />
    <path d="M4.2 5.3v5.4" {...outlineProps(color)} />
    <path d="M5.6 4.3c1 .6 1.6 1.5 2.3 2.4.7.9 1.4 1.5 2.4 1.8" {...outlineProps(color)} />
    <path d="M5.6 11.7c1-.6 1.6-1.5 2.3-2.4.7-.9 1.4-1.5 2.4-1.8" {...outlineProps(color)} />
  </svg>
));

export const IconPush: React.FC<{ size?: number; color?: string }> = React.memo(({ size = 14, color = 'currentColor' }) => (
  <svg width={size} height={size} viewBox="0 0 16 16" fill="none" style={{ flexShrink: 0 }}>
    <path d="M8 12.5V3.5" {...outlineProps(color)} />
    <path d="M4.5 7L8 3.5L11.5 7" {...outlineProps(color)} />
    <path d="M3 13.5h10" {...outlineProps(color)} />
  </svg>
));

export const IconPull: React.FC<{ size?: number; color?: string }> = React.memo(({ size = 14, color = 'currentColor' }) => (
  <svg width={size} height={size} viewBox="0 0 16 16" fill="none" style={{ flexShrink: 0 }}>
    <path d="M8 3.5V12.5" {...outlineProps(color)} />
    <path d="M4.5 9L8 12.5L11.5 9" {...outlineProps(color)} />
    <path d="M3 2.5h10" {...outlineProps(color)} />
  </svg>
));

/** Fetch：两支相向的斜箭头（远端 ↔ 本地） */
export const IconFetch: React.FC<{ size?: number; color?: string }> = React.memo(({ size = 14, color = 'currentColor' }) => (
  <svg width={size} height={size} viewBox="0 0 16 16" fill="none" style={{ flexShrink: 0 }}>
    <path d="M7.1 8.9L13 3" {...outlineProps(color)} />
    <path d="M9.7 3H13v3.3" {...outlineProps(color)} />
    <path d="M8.9 7.1L3 13" {...outlineProps(color)} />
    <path d="M3 9.7V13h3.3" {...outlineProps(color)} />
  </svg>
));

/** Update：左下斜箭头 */
export const IconUpdateArrow: React.FC<{ size?: number; color?: string }> = React.memo(({ size = 14, color = 'currentColor' }) => (
  <svg width={size} height={size} viewBox="0 0 16 16" fill="none" style={{ flexShrink: 0 }}>
    <path d="M12.5 3.5L4.2 11.8" {...outlineProps(color)} />
    <path d="M4.2 6.8V11.8H9.2" {...outlineProps(color)} />
  </svg>
));

export const IconStash: React.FC<{ size?: number; color?: string }> = React.memo(({ size = 14, color = 'currentColor' }) => (
  <svg width={size} height={size} viewBox="0 0 16 16" fill="none" style={{ flexShrink: 0 }}>
    <rect x="1.8" y="2.2" width="12.4" height="3.2" rx="0.8" {...outlineProps(color)} />
    <path d="M3.1 5.4v7.3a1.3 1.3 0 0 0 1.3 1.3h7.2a1.3 1.3 0 0 0 1.3-1.3V5.4" {...outlineProps(color)} />
    <path d="M8 12V8.7M6.7 10 8 11.3 9.3 10" {...outlineProps(color)} />
  </svg>
));

export const IconUnstash: React.FC<{ size?: number; color?: string }> = React.memo(({ size = 14, color = 'currentColor' }) => (
  <svg width={size} height={size} viewBox="0 0 16 16" fill="none" style={{ flexShrink: 0 }}>
    <rect x="1.8" y="2.2" width="12.4" height="3.2" rx="0.8" {...outlineProps(color)} />
    <path d="M3.1 5.4v7.3a1.3 1.3 0 0 0 1.3 1.3h7.2a1.3 1.3 0 0 0 1.3-1.3V5.4" {...outlineProps(color)} />
    <path d="M8 11.3V8M6.7 9.3 8 8l1.3 1.3" {...outlineProps(color)} />
  </svg>
));

/** 回滚/撤销：向左的弯箭头（参考 Rollback） */
export const IconRollback: React.FC<{ size?: number; color?: string }> = React.memo(({ size = 14, color = 'currentColor' }) => (
  <svg width={size} height={size} viewBox="0 0 16 16" fill="none" style={{ flexShrink: 0 }}>
    <path d="M5.5 3.5L2 7l3.5 3.5" {...outlineProps(color)} />
    <path d="M2 7h7.2a3.8 3.8 0 0 1 3.8 3.8v0a3.8 3.8 0 0 1-3.8 3.8H6.5" {...outlineProps(color)} />
  </svg>
));

export const IconHistory: React.FC<{ size?: number; color?: string }> = React.memo(({ size = 14, color = 'currentColor' }) => (
  <svg width={size} height={size} viewBox="0 0 16 16" fill="none" style={{ flexShrink: 0 }}>
    <circle cx="8" cy="8" r="6.2" {...outlineProps(color)} />
    <path d="M8 4.4V8l2.6 1.4" {...outlineProps(color)} />
  </svg>
));

export const IconAnnotate: React.FC<{ size?: number; color?: string }> = React.memo(({ size = 14, color = 'currentColor' }) => (
  <svg width={size} height={size} viewBox="0 0 16 16" fill="none" style={{ flexShrink: 0 }}>
    <circle cx="8" cy="4.8" r="2.6" {...outlineProps(color)} />
    <path d="M2.8 14v-1a3.5 3.5 0 0 1 3.5-3.5h3.4a3.5 3.5 0 0 1 3.5 3.5v1" {...outlineProps(color)} />
  </svg>
));

export const IconSearch: React.FC<{ size?: number; color?: string }> = React.memo(({ size = 13, color = 'currentColor' }) => (
  <svg width={size} height={size} viewBox="0 0 16 16" fill="none" style={{ flexShrink: 0 }}>
    <circle cx="7" cy="7" r="4.6" {...outlineProps(color)} />
    <path d="M10.4 10.4 13.8 13.8" {...outlineProps(color)} />
  </svg>
));

export const IconWarning: React.FC<{ size?: number; color?: string }> = React.memo(({ size = 13, color = 'currentColor' }) => (
  <svg width={size} height={size} viewBox="0 0 16 16" fill="none" style={{ flexShrink: 0 }}>
    <path d="M8 1.8 15 14H1L8 1.8z" fill={color} opacity="0.9" />
    <path d="M8 5.6v4.2" stroke="#2b2d30" strokeWidth="1.5" strokeLinecap="round" />
    <circle cx="8" cy="12" r="0.9" fill="#2b2d30" />
  </svg>
));

export const IconPlay: React.FC<{ size?: number; color?: string }> = React.memo(({ size = 12, color = 'currentColor' }) => (
  <svg width={size} height={size} viewBox="0 0 16 16" fill="none" style={{ flexShrink: 0 }}>
    <path d="M4.5 2.8v10.4L13 8 4.5 2.8z" fill={color} />
  </svg>
));

export const IconPlus: React.FC<{ size?: number; color?: string }> = React.memo(({ size = 14, color = 'currentColor' }) => (
  <svg width={size} height={size} viewBox="0 0 16 16" fill="none" style={{ flexShrink: 0 }}>
    <path d="M8 3v10M3 8h10" {...outlineProps(color)} />
  </svg>
));

export const IconChevronDown: React.FC<{ size?: number; color?: string }> = React.memo(({ size = 12, color = 'currentColor' }) => (
  <svg width={size} height={size} viewBox="0 0 16 16" fill="none" style={{ flexShrink: 0 }}>
    <path d="M4 6l4 4 4-4" {...outlineProps(color)} />
  </svg>
));

export const IconChevronUp: React.FC<{ size?: number; color?: string }> = React.memo(({ size = 12, color = 'currentColor' }) => (
  <svg width={size} height={size} viewBox="0 0 16 16" fill="none" style={{ flexShrink: 0 }}>
    <path d="M4 10l4-4 4 4" {...outlineProps(color)} />
  </svg>
));

export const IconChevronRight: React.FC<{ size?: number; color?: string }> = React.memo(({ size = 12, color = 'currentColor' }) => (
  <svg width={size} height={size} viewBox="0 0 16 16" fill="none" style={{ flexShrink: 0 }}>
    <path d="M6 4l4 4-4 4" {...outlineProps(color)} />
  </svg>
));

export const IconWindowMinimize: React.FC<{ size?: number; color?: string }> = React.memo(({ size = 12, color = 'currentColor' }) => (
  <svg width={size} height={size} viewBox="0 0 16 16" fill="none" style={{ flexShrink: 0 }}>
    <path d="M3.5 11h9" {...outlineProps(color)} />
  </svg>
));

export const IconWindowMaximize: React.FC<{ size?: number; color?: string }> = React.memo(({ size = 12, color = 'currentColor' }) => (
  <svg width={size} height={size} viewBox="0 0 16 16" fill="none" style={{ flexShrink: 0 }}>
    <rect x="3.5" y="3.5" width="9" height="9" rx="1" {...outlineProps(color)} />
  </svg>
));

export const IconWindowClose: React.FC<{ size?: number; color?: string }> = React.memo(({ size = 12, color = 'currentColor' }) => (
  <svg width={size} height={size} viewBox="0 0 16 16" fill="none" style={{ flexShrink: 0 }}>
    <path d="M4 4l8 8M12 4l-8 8" {...outlineProps(color)} />
  </svg>
));

/** 停靠右侧：面板右缘高亮竖条（工具窗口贴右语义） */
export const IconDockRight: React.FC<{ size?: number; color?: string }> = React.memo(({ size = 14, color = 'currentColor' }) => (
  <svg width={size} height={size} viewBox="0 0 16 16" fill="none" style={{ flexShrink: 0 }}>
    <rect x="2" y="3" width="12" height="10" rx="1.2" {...outlineProps(color)} />
    <path d="M9.6 3v10" {...outlineProps(color)} />
    <rect x="10.7" y="4.1" width="2.2" height="7.8" rx="0.6" fill={color} stroke="none" />
  </svg>
));

/** 停靠底部：面板下缘高亮横条（工具窗口贴底语义） */
export const IconDockBottom: React.FC<{ size?: number; color?: string }> = React.memo(({ size = 14, color = 'currentColor' }) => (
  <svg width={size} height={size} viewBox="0 0 16 16" fill="none" style={{ flexShrink: 0 }}>
    <rect x="2" y="3" width="12" height="10" rx="1.2" {...outlineProps(color)} />
    <path d="M2 9.6h12" {...outlineProps(color)} />
    <rect x="3.1" y="10.7" width="9.8" height="1.6" rx="0.5" fill={color} stroke="none" />
  </svg>
));

/** 拖拽把柄：两列圆点 */
export const IconGrip: React.FC<{ size?: number; color?: string }> = React.memo(({ size = 12, color = 'currentColor' }) => (
  <svg width={size} height={size} viewBox="0 0 16 16" fill="none" style={{ flexShrink: 0 }}>
    <circle cx="5.6" cy="3.6" r="1.25" fill={color} />
    <circle cx="5.6" cy="8" r="1.25" fill={color} />
    <circle cx="5.6" cy="12.4" r="1.25" fill={color} />
    <circle cx="10.4" cy="3.6" r="1.25" fill={color} />
    <circle cx="10.4" cy="8" r="1.25" fill={color} />
    <circle cx="10.4" cy="12.4" r="1.25" fill={color} />
  </svg>
));

export const IconCloud: React.FC<{ size?: number; color?: string }> = React.memo(({ size = 13, color = 'currentColor' }) => (
  <svg width={size} height={size} viewBox="0 0 16 16" fill="none" style={{ flexShrink: 0 }}>
    <path d="M11.5 12.5a3.2 3.2 0 0 0 .3-6.4 4.9 4.9 0 0 0-9.35 1.33A3.2 3.2 0 0 0 3.6 12.5h7.9z" {...outlineProps(color)} />
  </svg>
));

export const IconStar: React.FC<{ size?: number; color?: string }> = React.memo(({ size = 13, color = '#e3b341' }) => (
  <svg width={size} height={size} viewBox="0 0 16 16" fill="none" style={{ flexShrink: 0 }}>
    <path d="M8 1.8l1.9 3.85 4.25.62-3.07 3 .72 4.23L8 11.53l-3.8 1.97.72-4.23-3.07-3 4.25-.62L8 1.8z" fill={color} />
  </svg>
));

export const IconNewWindow: React.FC<{ size?: number; color?: string }> = React.memo(({ size = 14, color = 'currentColor' }) => (
  <svg width={size} height={size} viewBox="0 0 16 16" fill="none" style={{ flexShrink: 0 }}>
    <rect x="2" y="5.5" width="8.5" height="8.5" rx="1" {...outlineProps(color)} />
    <path d="M5.5 5.5V4a1 1 0 0 1 1-1H13a1 1 0 0 1 1 1v6.5a1 1 0 0 1-1 1h-1.5" {...outlineProps(color)} />
  </svg>
));

export const IconTag: React.FC<{ size?: number; color?: string }> = React.memo(({ size = 10, color = 'currentColor' }) => (
  <svg width={size} height={size} viewBox="0 0 16 16" fill="none" style={{ flexShrink: 0 }}>
    <path d="M2.5 7V3a.5.5 0 0 1 .5-.5h4a1 1 0 0 1 .71.29l5.79 5.8a1 1 0 0 1 0 1.41l-4 4a1 1 0 0 1-1.41 0l-5.8-5.8a1 1 0 0 1-.29-.7z" {...outlineProps(color)} />
    <circle cx="5.5" cy="5.5" r="1.1" fill={color} />
  </svg>
));

/** 贮藏架（Shelf）：带插槽的抽屉 */
export const IconShelf: React.FC<{ size?: number; color?: string }> = React.memo(({ size = 14, color = 'currentColor' }) => (
  <svg width={size} height={size} viewBox="0 0 16 16" fill="none" style={{ flexShrink: 0 }}>
    <rect x="2" y="2.5" width="12" height="11" rx="1.2" {...outlineProps(color)} />
    <path d="M2 10h12" {...outlineProps(color)} />
    <path d="M6.5 12.5h3" {...outlineProps(color)} />
  </svg>
));

/** 全部折叠：参考树折叠图标（上下两组向内指向的独立箭头，清晰分离不粘连成 X） */
export const IconCollapseAll: React.FC<{ size?: number; color?: string; strokeWidth?: number }> = React.memo(({
  size = 14,
  color = 'currentColor',
  strokeWidth = 1.3
}) => (
  <svg width={size} height={size} viewBox="0 0 16 16" fill="none" style={{ flexShrink: 0 }}>
    <path
      d="M3.5 3.2L8 6.6l4.5-3.4"
      stroke={color}
      strokeWidth={strokeWidth}
      strokeLinecap="round"
      strokeLinejoin="round"
    />
    <path
      d="M3.5 12.8L8 9.4l4.5 3.4"
      stroke={color}
      strokeWidth={strokeWidth}
      strokeLinecap="round"
      strokeLinejoin="round"
    />
  </svg>
));

/** 全部展开：参考树展开图标（上下两组向外张开的独立箭头，清晰分离不闭合成方块） */
export const IconExpandAll: React.FC<{ size?: number; color?: string; strokeWidth?: number }> = React.memo(({
  size = 14,
  color = 'currentColor',
  strokeWidth = 1.3
}) => (
  <svg width={size} height={size} viewBox="0 0 16 16" fill="none" style={{ flexShrink: 0 }}>
    <path
      d="M3.5 6L8 2.5l4.5 3.5"
      stroke={color}
      strokeWidth={strokeWidth}
      strokeLinecap="round"
      strokeLinejoin="round"
    />
    <path
      d="M3.5 10L8 13.5l4.5-3.5"
      stroke={color}
      strokeWidth={strokeWidth}
      strokeLinecap="round"
      strokeLinejoin="round"
    />
  </svg>
));

/** 按文件夹结构查看（Group by Directory，参考 Commit 工具栏） */
export const IconFolderTree: React.FC<{ size?: number; color?: string }> = React.memo(({ size = 14, color = 'currentColor' }) => (
  <svg width={size} height={size} viewBox="0 0 16 16" fill="none" style={{ flexShrink: 0 }}>
    <path d="M2 3.8h3.2l1.4 1.4h6.4a1 1 0 0 1 1 1v2.3" {...outlineProps(color)} />
    <path d="M2 3.8v8.4a1 1 0 0 0 1 1h10" {...outlineProps(color)} />
    <path d="M5.5 8.5h3.5M5.5 11.5h5.5" {...outlineProps(color)} />
  </svg>
));

/** 视图选项（View Options / 眼睛图标带角标）：参考 Commit 工具栏 */
export const IconViewOptions: React.FC<{ size?: number; color?: string; className?: string }> = React.memo(({
  size = 14,
  color = 'currentColor',
  className = ''
}) => (
  <svg width={size} height={size} viewBox="0 0 16 16" fill="none" className={className} style={{ flexShrink: 0 }}>
    <path
      d="M1.5 8s2.5-4.5 6.5-4.5S14.5 8 14.5 8s-2.5 4.5-6.5 4.5S1.5 8 1.5 8z"
      stroke={color}
      strokeWidth="1.3"
      strokeLinecap="round"
      strokeLinejoin="round"
    />
    <circle cx="8" cy="8" r="2" stroke={color} strokeWidth="1.3" />
    <path d="M12 12.5l2.5 2.5h-2.5v-2.5z" fill={color} />
  </svg>
));
export const IconEye = IconViewOptions;

export const IconMoreVert: React.FC<{ size?: number; color?: string }> = React.memo(({ size = 14, color = 'currentColor' }) => (
  <svg width={size} height={size} viewBox="0 0 16 16" fill="none" style={{ flexShrink: 0 }}>
    <circle cx="8" cy="3.5" r="1.25" fill={color} />
    <circle cx="8" cy="8" r="1.25" fill={color} />
    <circle cx="8" cy="12.5" r="1.25" fill={color} />
  </svg>
));
/** Shelve：托盘 + 向下箭头（参考 Commit 工具栏 Shelve 图标） */
export const IconShelveIn: React.FC<{ size?: number; color?: string }> = React.memo(({ size = 14, color = 'currentColor' }) => (
  <svg width={size} height={size} viewBox="0 0 16 16" fill="none" style={{ flexShrink: 0 }}>
    <path d="M2.5 9.2v3.3a1 1 0 0 0 1 1h9a1 1 0 0 0 1-1V9.2" {...outlineProps(color)} />
    <path d="M8 1.8v7.4" {...outlineProps(color)} />
    <path d="M5.2 6.4 8 9.2l2.8-2.8" {...outlineProps(color)} />
  </svg>
));

/** Unshelve：托盘 + 向上箭头（参考 Shelf 工具栏 Unshelve 图标） */
export const IconShelveOut: React.FC<{ size?: number; color?: string }> = React.memo(({ size = 14, color = 'currentColor' }) => (
  <svg width={size} height={size} viewBox="0 0 16 16" fill="none" style={{ flexShrink: 0 }}>
    <path d="M2.5 9.2v3.3a1 1 0 0 0 1 1h9a1 1 0 0 0 1-1V9.2" {...outlineProps(color)} />
    <path d="M8 9.2V1.8" {...outlineProps(color)} />
    <path d="M5.2 4.6 8 1.8l2.8 2.8" {...outlineProps(color)} />
  </svg>
));
export const IconDoubleChevronRight: React.FC<{ size?: number; color?: string; className?: string }> = React.memo(({
  size = 13,
  color = 'currentColor',
  className = ''
}) => (
  <svg width={size} height={size} viewBox="0 0 16 16" fill="none" className={className} style={{ flexShrink: 0 }}>
    <path d="M4 3.5L8.5 8L4 12.5" stroke={color} strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" />
    <path d="M8.5 3.5L13 8L8.5 12.5" stroke={color} strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" />
  </svg>
));

export const IconDoubleChevronLeft: React.FC<{ size?: number; color?: string; className?: string }> = React.memo(({
  size = 13,
  color = 'currentColor',
  className = ''
}) => (
  <svg width={size} height={size} viewBox="0 0 16 16" fill="none" className={className} style={{ flexShrink: 0 }}>
    <path d="M7.5 3.5L3 8L7.5 12.5" stroke={color} strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" />
    <path d="M12 3.5L7.5 8L12 12.5" stroke={color} strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" />
  </svg>
));

export const IconDoubleChevronBoth: React.FC<{ size?: number; color?: string; className?: string }> = React.memo(({
  size = 15,
  color = 'currentColor',
  className = ''
}) => (
  <svg width={size} height={size} viewBox="0 0 18 16" fill="none" className={className} style={{ flexShrink: 0 }}>
    <path d="M2.5 4.5L5.5 8L2.5 11.5" stroke={color} strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" />
    <path d="M6 4.5L9 8L6 11.5" stroke={color} strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" />
    <path d="M15.5 4.5L12.5 8L15.5 11.5" stroke={color} strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" />
    <path d="M12 4.5L9 8L12 11.5" stroke={color} strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" />
  </svg>
));

export const IconCross: React.FC<{ size?: number; color?: string; className?: string }> = React.memo(({
  size = 11,
  color = 'currentColor',
  className = ''
}) => (
  <svg width={size} height={size} viewBox="0 0 16 16" fill="none" className={className} style={{ flexShrink: 0 }}>
    <path d="M3.5 3.5L12.5 12.5M12.5 3.5L3.5 12.5" stroke={color} strokeWidth="2" strokeLinecap="round" />
  </svg>
));


export const IconArrowUp: React.FC<{ size?: number; color?: string; className?: string }> = React.memo(({
  size = 14,
  color = 'currentColor',
  className = ''
}) => (
  <svg width={size} height={size} viewBox="0 0 16 16" fill="none" className={className} style={{ flexShrink: 0 }}>
    <path d="M8 12.5V3.5M3.5 8L8 3.5 12.5 8" stroke={color} strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" />
  </svg>
));

export const IconArrowDown: React.FC<{ size?: number; color?: string; className?: string }> = React.memo(({
  size = 14,
  color = 'currentColor',
  className = ''
}) => (
  <svg width={size} height={size} viewBox="0 0 16 16" fill="none" className={className} style={{ flexShrink: 0 }}>
    <path d="M8 3.5V12.5M3.5 8L8 12.5 12.5 8" stroke={color} strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" />
  </svg>
));

export const IconArrowLeft: React.FC<{ size?: number; color?: string; className?: string }> = React.memo(({
  size = 14,
  color = 'currentColor',
  className = ''
}) => (
  <svg width={size} height={size} viewBox="0 0 16 16" fill="none" className={className} style={{ flexShrink: 0 }}>
    <path d="M12.5 8H3.5M8 3.5L3.5 8L8 12.5" stroke={color} strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" />
  </svg>
));

export const IconArrowRight: React.FC<{ size?: number; color?: string; className?: string }> = React.memo(({
  size = 14,
  color = 'currentColor',
  className = ''
}) => (
  <svg width={size} height={size} viewBox="0 0 16 16" fill="none" className={className} style={{ flexShrink: 0 }}>
    <path d="M3.5 8H12.5M8 3.5L12.5 8L8 12.5" stroke={color} strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" />
  </svg>
));

export const IconApplyNonConflict: React.FC<{ size?: number; color?: string; className?: string }> = React.memo(({
  size = 15,
  color = 'currentColor',
  className = ''
}) => (
  <svg width={size} height={size} viewBox="0 0 16 16" fill="none" className={className} style={{ flexShrink: 0 }}>
    <path d="M2.5 8h7M6.5 5l3 3-3 3" stroke={color} strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round" />
    <path d="M12.5 4v8" stroke={color} strokeWidth="1.4" strokeLinecap="round" strokeDasharray="1 1.5" />
    <circle cx="12.5" cy="4" r="0.75" fill={color} />
    <circle cx="12.5" cy="12" r="0.75" fill={color} />
  </svg>
));

export const IconMagicWand: React.FC<{ size?: number; color?: string; className?: string }> = React.memo(({
  size = 14,
  color = 'currentColor',
  className = ''
}) => (
  <svg width={size} height={size} viewBox="0 0 16 16" fill="none" className={className} style={{ flexShrink: 0 }}>
    <path d="M3 13L9.5 6.5" stroke={color} strokeWidth="1.5" strokeLinecap="round" />
    <path d="M9.5 6.5l1-1a1 1 0 0 1 1.4 0l.6.6a1 1 0 0 1 0 1.4l-1 1" stroke={color} strokeWidth="1.2" />
    <path d="M12 2v2M11 3h2M5 2.5l.5.5M14 6.5l-.5.5M8 1v1.5" stroke={color} strokeWidth="1.2" strokeLinecap="round" />
  </svg>
));

export const IconCloseSmall: React.FC<{ size?: number; color?: string; className?: string }> = React.memo(({
  size = 12,
  color = 'currentColor',
  className = ''
}) => (
  <svg width={size} height={size} viewBox="0 0 16 16" fill="none" className={className} style={{ flexShrink: 0 }}>
    <path d="M4 4l8 8M12 4l-8 8" stroke={color} strokeWidth="1.5" strokeLinecap="round" />
  </svg>
));

export const IconSyncArrows: React.FC<{ size?: number; color?: string; className?: string }> = React.memo(({
  size = 14,
  color = 'currentColor',
  className = ''
}) => (
  <svg width={size} height={size} viewBox="0 0 16 16" fill="none" className={className} style={{ flexShrink: 0 }}>
    <path d="M5 3v9M3 5.5l2-2.5 2 2.5" stroke={color} strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round" />
    <path d="M11 13V4M9 10.5l2 2.5 2-2.5" stroke={color} strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round" />
  </svg>
));
export const TomlFileIcon: React.FC<{ size?: number }> = React.memo(({ size = 15 }) => (
  <svg width={size} height={size} viewBox="0 0 16 16" fill="none" style={{ flexShrink: 0 }}>
    <rect x="2" y="2" width="12" height="12" rx="2" fill="#9c4221" />
    <text x="8" y="10.8" fill="#fff" fontSize="6.5" fontWeight="bold" textAnchor="middle" fontFamily="monospace">
      TOML
    </text>
  </svg>
));

export const YamlFileIcon: React.FC<{ size?: number }> = React.memo(({ size = 15 }) => (
  <svg width={size} height={size} viewBox="0 0 16 16" fill="none" style={{ flexShrink: 0 }}>
    <rect x="2" y="2" width="12" height="12" rx="2" fill="#cb171e" />
    <text x="8" y="11" fill="#fff" fontSize="7.5" fontWeight="bold" textAnchor="middle" fontFamily="sans-serif">
      YML
    </text>
  </svg>
));

export const SqlFileIcon: React.FC<{ size?: number }> = React.memo(({ size = 15 }) => (
  <svg width={size} height={size} viewBox="0 0 16 16" fill="none" style={{ flexShrink: 0 }}>
    <rect x="2" y="2" width="12" height="12" rx="2" fill="#2979ff" />
    <text x="8" y="11" fill="#fff" fontSize="8" fontWeight="bold" textAnchor="middle" fontFamily="sans-serif">
      SQL
    </text>
  </svg>
));

export const ImageFileIcon: React.FC<{ size?: number }> = React.memo(({ size = 15 }) => (
  <svg width={size} height={size} viewBox="0 0 16 16" fill="none" style={{ flexShrink: 0 }}>
    <rect x="2" y="2.5" width="12" height="11" rx="1.5" stroke="#a371f7" strokeWidth="1.2" fill="none" />
    <circle cx="5.5" cy="5.8" r="1.3" fill="#a371f7" />
    <path d="M3 12l3-3.5 2 2 3-4 2.5 3.5H3z" fill="#a371f7" opacity="0.8" />
  </svg>
));
export const XmlFileIcon: React.FC<{ size?: number }> = React.memo(({ size = 15 }) => (
  <svg width={size} height={size} viewBox="0 0 16 16" fill="none" style={{ flexShrink: 0 }}>
    <path d="M5 4.5L1.8 8L5 11.5" stroke="#e5975f" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round" />
    <path d="M11 4.5L14.2 8L11 11.5" stroke="#e5975f" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round" />
    <line x1="9.5" y1="3.5" x2="6.5" y2="12.5" stroke="#e5975f" strokeWidth="1.4" strokeLinecap="round" />
  </svg>
));

export const ImlFileIcon: React.FC<{ size?: number }> = React.memo(({ size = 15 }) => (
  <svg width={size} height={size} viewBox="0 0 16 16" fill="none" style={{ flexShrink: 0 }}>
    <rect x="2" y="3.5" width="12" height="9" rx="1.5" stroke="#8c8f96" strokeWidth="1.3" fill="none" />
    <circle cx="5" cy="8" r="1.1" fill="#8c8f96" />
    <line x1="7.5" y1="8" x2="11.5" y2="8" stroke="#8c8f96" strokeWidth="1.1" strokeLinecap="round" />
  </svg>
));

export const IgnoreFileIcon: React.FC<{ size?: number }> = React.memo(({ size = 15 }) => (
  <svg width={size} height={size} viewBox="0 0 16 16" fill="none" style={{ flexShrink: 0 }}>
    <circle cx="8" cy="8" r="5.5" stroke="#8c8f96" strokeWidth="1.3" fill="none" />
    <line x1="4.1" y1="4.1" x2="11.9" y2="11.9" stroke="#8c8f96" strokeWidth="1.3" strokeLinecap="round" />
  </svg>
));


// ---------------------------------------------------------------------------
// 静态单例 React Element 缓存（消除海量对象动态分配，触发 React Bailout 跳过 DOM 比对）
// ---------------------------------------------------------------------------
const CACHED_PYTHON_ICON = Object.freeze(<PythonIcon size={15} />);
const CACHED_TS_ICON = Object.freeze(<TsFileIcon size={15} />);
const CACHED_JS_ICON = Object.freeze(<JsFileIcon size={15} />);
const CACHED_HTML_ICON = Object.freeze(<HtmlFileIcon size={15} />);
const CACHED_CSS_ICON = Object.freeze(<CssFileIcon size={15} />);
const CACHED_JSON_ICON = Object.freeze(<JsonFileIcon size={15} />);
const CACHED_RUST_ICON = Object.freeze(<RustFileIcon size={15} />);
const CACHED_MD_ICON = Object.freeze(<MarkdownFileIcon size={15} />);
const CACHED_VUE_ICON = Object.freeze(<VueFileIcon size={15} />);
const CACHED_TOML_ICON = Object.freeze(<TomlFileIcon size={15} />);
const CACHED_YAML_ICON = Object.freeze(<YamlFileIcon size={15} />);
const CACHED_SQL_ICON = Object.freeze(<SqlFileIcon size={15} />);
const CACHED_IMAGE_ICON = Object.freeze(<ImageFileIcon size={15} />);
const CACHED_SHELL_ICON = Object.freeze(<ShellFileIcon size={15} />);
const CACHED_DOCKER_ICON = Object.freeze(<DockerIcon size={15} />);
const CACHED_ENV_ICON = Object.freeze(<EnvFileIcon size={15} />);
const CACHED_IGNORE_ICON = Object.freeze(<IgnoreFileIcon size={15} />);
const CACHED_XML_ICON = Object.freeze(<XmlFileIcon size={15} />);
const CACHED_IML_ICON = Object.freeze(<ImlFileIcon size={15} />);
const CACHED_TEXT_ICON = Object.freeze(<TextFileIcon size={15} />);
const CACHED_FOLDER_OPEN = Object.freeze(<FolderIcon size={15} isOpen={true} />);
const CACHED_FOLDER_CLOSED = Object.freeze(<FolderIcon size={15} isOpen={false} />);
const CACHED_VENV_FOLDER = Object.freeze(<VenvFolderIcon size={15} />);

/** O(1) 扩展名映射表 */
const EXTENSION_ICON_MAP: Record<string, React.ReactNode> = {
  py: CACHED_PYTHON_ICON,
  ts: CACHED_TS_ICON,
  tsx: CACHED_TS_ICON,
  js: CACHED_JS_ICON,
  jsx: CACHED_JS_ICON,
  html: CACHED_HTML_ICON,
  htm: CACHED_HTML_ICON,
  css: CACHED_CSS_ICON,
  scss: CACHED_CSS_ICON,
  less: CACHED_CSS_ICON,
  json: CACHED_JSON_ICON,
  rs: CACHED_RUST_ICON,
  md: CACHED_MD_ICON,
  markdown: CACHED_MD_ICON,
  vue: CACHED_VUE_ICON,
  toml: CACHED_TOML_ICON,
  yaml: CACHED_YAML_ICON,
  yml: CACHED_YAML_ICON,
  sql: CACHED_SQL_ICON,
  png: CACHED_IMAGE_ICON,
  jpg: CACHED_IMAGE_ICON,
  jpeg: CACHED_IMAGE_ICON,
  gif: CACHED_IMAGE_ICON,
  svg: CACHED_IMAGE_ICON,
  webp: CACHED_IMAGE_ICON,
  ico: CACHED_IMAGE_ICON,
  bmp: CACHED_IMAGE_ICON,
  sh: CACHED_SHELL_ICON,
  bash: CACHED_SHELL_ICON,
  zsh: CACHED_SHELL_ICON,
  xml: CACHED_XML_ICON,
  iml: CACHED_IML_ICON,
  gitignore: CACHED_IGNORE_ICON,
  gitattributes: CACHED_IGNORE_ICON,
  ignore: CACHED_IGNORE_ICON,
  dockerignore: CACHED_DOCKER_ICON,
};

/** 精确全名映射表（无视扩展名） */
const EXACT_NAME_ICON_MAP: Record<string, React.ReactNode> = {
  dockerfile: CACHED_DOCKER_ICON,
  '.dockerignore': CACHED_DOCKER_ICON,
  '.gitignore': CACHED_IGNORE_ICON,
  '.gitattributes': CACHED_IGNORE_ICON,
  '.env': CACHED_ENV_ICON,
  '.env.local': CACHED_ENV_ICON,
  '.env.development': CACHED_ENV_ICON,
  '.env.production': CACHED_ENV_ICON,
  '.env.test': CACHED_ENV_ICON,
};

/**
 * 根据文件名或扩展名返回精致的 IDE 风格图标（O(1) 查找 + 单例 React Element）
 */
export function renderFileIcon(filename: string, isDirectory: boolean = false, isOpen: boolean = false): React.ReactNode {
  if (isDirectory) {
    const lower = filename.toLowerCase();
    if (lower === '.venv' || lower === 'venv' || lower === 'env') {
      return CACHED_VENV_FOLDER;
    }
    return isOpen ? CACHED_FOLDER_OPEN : CACHED_FOLDER_CLOSED;
  }

  const lower = filename.toLowerCase();
  const exact = EXACT_NAME_ICON_MAP[lower];
  if (exact) return exact;

  if (lower.startsWith('.env') || lower.endsWith('.env')) {
    return CACHED_ENV_ICON;
  }

  const dotIdx = lower.lastIndexOf('.');
  if (dotIdx !== -1) {
    const ext = lower.slice(dotIdx + 1);
    const icon = EXTENSION_ICON_MAP[ext];
    if (icon) return icon;
  }

  if (lower.includes('dockerfile')) return CACHED_DOCKER_ICON;
  if (lower.endsWith('.ignore')) return CACHED_IGNORE_ICON;

  return CACHED_TEXT_ICON;
}

/** 垃圾桶：从最近打开的项目中移除 */
export const IconTrash: React.FC<{ size?: number; color?: string }> = React.memo(({ size = 13, color = 'currentColor' }) => (
  <svg width={size} height={size} viewBox="0 0 16 16" fill="none" style={{ flexShrink: 0 }}>
    <path d="M2.8 4.5h10.4" {...outlineProps(color)} />
    <path d="M6.2 4.5V3.3a1.2 1.2 0 0 1 1.2-1.2h1.2a1.2 1.2 0 0 1 1.2 1.2v1.2" {...outlineProps(color)} />
    <path d="M4.1 4.5l.6 8.2a1.3 1.3 0 0 0 1.3 1.2h4a1.3 1.3 0 0 0 1.3-1.2l.6-8.2" {...outlineProps(color)} />
    <path d="M6.5 7v3.9M9.5 7v3.9" {...outlineProps(color)} />
  </svg>
));

/** 在目录树中定位（Scroll from Source）：瞄准圈 */
export const IconLocate: React.FC<{ size?: number; color?: string }> = React.memo(({ size = 14, color = 'currentColor' }) => (
  <svg width={size} height={size} viewBox="0 0 16 16" fill="none" style={{ flexShrink: 0 }}>
    <circle cx="8" cy="8" r="4.4" {...outlineProps(color)} />
    <circle cx="8" cy="8" r="1.3" fill={color} stroke="none" />
    <path d="M8 1.4v2M8 12.6v2M1.4 8h2M12.6 8h2" {...outlineProps(color)} />
  </svg>
));

/** 自动定位开关（Always Select Opened File）：瞄准圈 + 右下跟随箭头，与单次定位区分 */
export const IconAutoLocate: React.FC<{ size?: number; color?: string }> = React.memo(({ size = 14, color = 'currentColor' }) => (
  <svg width={size} height={size} viewBox="0 0 16 16" fill="none" style={{ flexShrink: 0 }}>
    <circle cx="7" cy="7" r="4.2" {...outlineProps(color)} />
    <circle cx="7" cy="7" r="1.2" fill={color} stroke="none" />
    <path d="M10 10.2l3.4 3.4" {...outlineProps(color)} />
    <path d="M11.4 13.6h2.2v-2.2" {...outlineProps(color)} />
  </svg>
));

/** 标题栏操作继续/接受 (>>)：参考绿色操作按钮，纯净双箭头 */
export const IconOpContinue: React.FC<{ size?: number; color?: string; strokeWidth?: number }> = React.memo(({
  size = 13,
  color = 'currentColor',
  strokeWidth = 1.9
}) => (
  <svg width={size} height={size} viewBox="0 0 16 16" fill="none" style={{ flexShrink: 0 }}>
    <path
      d="M4.2 3.8L8.4 8l-4.2 4.2"
      stroke={color}
      strokeWidth={strokeWidth}
      strokeLinecap="round"
      strokeLinejoin="round"
    />
    <path
      d="M8.2 3.8L12.4 8l-4.2 4.2"
      stroke={color}
      strokeWidth={strokeWidth}
      strokeLinecap="round"
      strokeLinejoin="round"
    />
  </svg>
));

/** 标题栏操作中止 (X)：参考红色操作按钮，标准居中对角叉 */
export const IconOpAbort: React.FC<{ size?: number; color?: string; strokeWidth?: number }> = React.memo(({
  size = 13,
  color = 'currentColor',
  strokeWidth = 1.9
}) => (
  <svg width={size} height={size} viewBox="0 0 16 16" fill="none" style={{ flexShrink: 0 }}>
    <path
      d="M4 4l8 8M12 4l-8 8"
      stroke={color}
      strokeWidth={strokeWidth}
      strokeLinecap="round"
      strokeLinejoin="round"
    />
  </svg>
));

/** 提交比较侧边栏条带图标：三条横线 + 右向水平箭头（参考 截图） */
export const CompareStripeIcon: React.FC<{ size?: number; color?: string }> = React.memo(({ size = 16, color = 'currentColor' }) => (
  <svg width={size} height={size} viewBox="0 0 16 16" fill="none" style={{ flexShrink: 0 }}>
    <path d="M2.5 4.5h5.5M2.5 8h3.5M2.5 11.5h5.5" stroke={color} strokeWidth="1.3" strokeLinecap="round" />
    <path d="M6.5 8h6.5M10.5 5.5l2.5 2.5-2.5 2.5" stroke={color} strokeWidth="1.3" strokeLinecap="round" strokeLinejoin="round" />
  </svg>
));

/** 对比版本 / 交换对比版本：双向水平带端线箭头（参考 Compare Versions 菜单与工具栏） */
export const IconCompareVersions: React.FC<{ size?: number; color?: string }> = React.memo(({ size = 14, color = 'currentColor' }) => (
  <svg width={size} height={size} viewBox="0 0 16 16" fill="none" style={{ flexShrink: 0 }}>
    <path d="M2 3.5v4M2 5.5h10M9.5 3l2.5 2.5-2.5 2.5" stroke={color} strokeWidth="1.3" strokeLinecap="round" strokeLinejoin="round" />
    <path d="M14 8.5v4M14 10.5H4M6.5 8L4 10.5l2.5 2.5" stroke={color} strokeWidth="1.3" strokeLinecap="round" strokeLinejoin="round" />
  </svg>
));
export const IconSwap = IconCompareVersions;

/** 上下折叠/展开双向双尖角（参考 工具栏） */
export const IconChevronUpDown: React.FC<{ size?: number; color?: string }> = React.memo(({ size = 13, color = 'currentColor' }) => (
  <svg width={size} height={size} viewBox="0 0 16 16" fill="none" style={{ flexShrink: 0 }}>
    <path d="M4.5 6l3.5-3.5L11.5 6M4.5 10l3.5 3.5L11.5 10" stroke={color} strokeWidth="1.3" strokeLinecap="round" strokeLinejoin="round" />
  </svg>
));

/** 复制（Copy Revision Number 菜单图标） */
export const IconCopy: React.FC<{ size?: number; color?: string }> = React.memo(({ size = 13, color = 'currentColor' }) => (
  <svg width={size} height={size} viewBox="0 0 16 16" fill="none" style={{ flexShrink: 0 }}>
    <rect x="5.5" y="5.5" width="8" height="8" rx="1" stroke={color} strokeWidth="1.2" fill="none" />
    <path d="M10.5 5.5V3.5a1 1 0 0 0-1-1h-6a1 1 0 0 0-1 1v6a1 1 0 0 0 1 1h2" stroke={color} strokeWidth="1.2" strokeLinecap="round" />
  </svg>
));

/** Cherry-Pick 樱桃双果图标（参考 菜单） */
export const IconCherryPick: React.FC<{ size?: number; color?: string }> = React.memo(({ size = 14, color = 'currentColor' }) => (
  <svg width={size} height={size} viewBox="0 0 16 16" fill="none" style={{ flexShrink: 0 }}>
    <circle cx="5" cy="11.5" r="2.2" stroke={color} strokeWidth="1.2" />
    <circle cx="11" cy="11.5" r="2.2" stroke={color} strokeWidth="1.2" />
    <path d="M5 9.3c.8-3.5 3.8-5.5 4.8-6M11 9.3c-.6-3.2-2.8-5.5-3.8-6M9.8 3.3c1.2-.2 2.2.5 2.2.5" stroke={color} strokeWidth="1.2" strokeLinecap="round" />
  </svg>
));

/** 搜索框带下拉箭头（Q▾，参考 Git Log 搜索框） */
export const IconSearchDropdown: React.FC<{ size?: number; color?: string }> = React.memo(({ size = 12, color = 'currentColor' }) => (
  <svg width={size + 5} height={size} viewBox="0 0 17 12" fill="none" style={{ flexShrink: 0 }}>
    <circle cx="4.5" cy="4.5" r="3.5" stroke={color} strokeWidth="1.2" />
    <path d="M7 7l2.4 2.4" stroke={color} strokeWidth="1.2" strokeLinecap="round" />
    <path d="M11.5 4.5l1.8 2.2 1.8-2.2" stroke={color} strokeWidth="1.1" strokeLinecap="round" strokeLinejoin="round" />
  </svg>
));

/** 提交排序（参考 提交列表排序图标）：三条长短递减横线 + 左侧下箭头 */
export const IconSortCommits: React.FC<{ size?: number; color?: string }> = React.memo(({ size = 14, color = 'currentColor' }) => (
  <svg width={size} height={size} viewBox="0 0 16 16" fill="none" style={{ flexShrink: 0 }}>
    <path d="M3.5 2.5v9M1.8 9l1.7 2.5L5.2 9" stroke={color} strokeWidth="1.3" strokeLinecap="round" strokeLinejoin="round" />
    <path d="M7.5 4h6M7.5 7.5h4.5M7.5 11h3" stroke={color} strokeWidth="1.3" strokeLinecap="round" />
  </svg>
));

/** 提交图谱轨迹线（Commit Graph / Tracks）：主干节点 + 分支连线节点 */
export const IconCommitGraph: React.FC<{ size?: number; color?: string }> = React.memo(({ size = 14, color = 'currentColor' }) => (
  <svg width={size} height={size} viewBox="0 0 16 16" fill="none" style={{ flexShrink: 0 }}>
    <path d="M4.5 2v12" stroke={color} strokeWidth="1.2" />
    <circle cx="4.5" cy="4" r="1.8" fill={color} />
    <circle cx="4.5" cy="12" r="1.8" fill={color} />
    <path d="M4.5 5.5c3 0 7 1.5 7 3.5v5" stroke={color} strokeWidth="1.2" />
    <circle cx="11.5" cy="9" r="1.8" fill={color} />
  </svg>
));

/** 水平双向对比箭头（参考 Show Diff 菜单图标 ⇇） */
export const IconDiffHorizontal: React.FC<{ size?: number; color?: string }> = React.memo(({ size = 14, color = 'currentColor' }) => (
  <svg width={size} height={size} viewBox="0 0 16 16" fill="none" style={{ flexShrink: 0 }}>
    <path d="M5.5 3.5L2 7l3.5 3.5M2 7h12" {...outlineProps(color)} />
    <path d="M10.5 12.5L14 9l-3.5-3.5M14 9H2" {...outlineProps(color)} />
  </svg>
));

/** 对比在新标签页打开（Show Diff in a New Tab 菜单图标 ⇇→） */
export const IconDiffTab: React.FC<{ size?: number; color?: string }> = React.memo(({ size = 14, color = 'currentColor' }) => (
  <svg width={size} height={size} viewBox="0 0 16 16" fill="none" style={{ flexShrink: 0 }}>
    <path d="M5 4L2 7l3 3M2 7h9" {...outlineProps(color)} />
    <path d="M11 12L14 9l-3-3" {...outlineProps(color)} />
    <path d="M14 3.5v9" {...outlineProps(color)} />
  </svg>
));

/** 创建补丁图标（Create Patch...，带 + 的补丁文档） */
export const IconCreatePatch: React.FC<{ size?: number; color?: string }> = React.memo(({ size = 14, color = 'currentColor' }) => (
  <svg width={size} height={size} viewBox="0 0 16 16" fill="none" style={{ flexShrink: 0 }}>
    <path d="M3 2.5h6l4 4v7a1 1 0 0 1-1 1H3a1 1 0 0 1-1-1v-10a1 1 0 0 1 1-1z" {...outlineProps(color)} />
    <path d="M9 2.5v4h4" {...outlineProps(color)} />
    <path d="M5 10.5h4M7 8.5v4" {...outlineProps(color)} />
  </svg>
));

/** 退格/删除键图标（Delete... ⌫） */
export const IconDeleteKey: React.FC<{ size?: number; color?: string }> = React.memo(({ size = 14, color = 'currentColor' }) => (
  <svg width={size} height={size} viewBox="0 0 16 16" fill="none" style={{ flexShrink: 0 }}>
    <path d="M5.5 3.5L1.8 8l3.7 4.5h8a1 1 0 0 0 1-1v-7a1 1 0 0 0-1-1h-8z" {...outlineProps(color)} />
    <path d="M7.5 6l4 4M11.5 6l-4 4" {...outlineProps(color)} />
  </svg>
));
