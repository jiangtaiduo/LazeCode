import type { CSSProperties } from 'react';

export interface TerminalA11yMirrorProps {
  /** 可见视口的纯文本（由 `frameToA11yText` 生成） */
  text: string;
  /** 屏幕阅读器读到的区域名称，如「终端输出」 */
  label: string;
  className?: string;
}

/**
 * 仅供辅助技术的终端文本镜像。
 *
 * Canvas 对屏幕阅读器是不透明的，因此把同帧文本放进一个视觉隐藏的
 * `role="log"` 容器。`aria-live="off"` 表示内容更新不主动播报（终端每帧
 * 都在变，实时播报会把输出刷屏），用户可主动导航到该区域阅读。
 *
 * 视觉隐藏用绝对定位 + 1px 尺寸 + clip-path：不参与布局，也不会被
 * `display:none` / `visibility:hidden` 或 `aria-hidden` 移出无障碍树。
 * `tabIndex={-1}` 让它可按程序聚焦但不进入 Tab 顺序，`pointer-events:none`
 * 避免这 1px 挡住画布上的点击。
 */
const hiddenStyle: CSSProperties = {
  position: 'absolute',
  width: 1,
  height: 1,
  overflow: 'hidden',
  clipPath: 'inset(50%)',
  whiteSpace: 'pre',
  pointerEvents: 'none'
};

export function TerminalA11yMirror({ text, label, className }: TerminalA11yMirrorProps) {
  return (
    <div
      role="log"
      aria-live="off"
      aria-label={label}
      tabIndex={-1}
      className={className}
      style={hiddenStyle}
    >
      {text}
    </div>
  );
}
