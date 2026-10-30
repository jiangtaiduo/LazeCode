// 右键菜单的公共外壳：overlay（承接「点别处关闭」）+ 菜单本体。
//
// 为什么要 portal 到 body：菜单与 overlay 都是 position:fixed。CSS 规范下
// filter / backdrop-filter / transform 等会为 fixed 后代创建 containing block，
// 而本项目的液态毛玻璃主题（variant-3）给标题栏、Commit 抽屉、文件树抽屉、
// 底部 Git 面板都设了 backdrop-filter。菜单若留在这些容器内渲染：
//   1. top/left 变成相对该容器的坐标，菜单整体偏离鼠标；
//   2. overlay 的 inset:0 只覆盖容器自身，点容器外的编辑器不会命中它，
//      菜单因此不会消失；
//   3. 菜单逃出容器后被容器 overflow:hidden 裁剪，或被后绘制的兄弟面板盖住。
// portal 到 body 后 containing block 与层叠上下文都是根节点，三条同时消失，
// 且不依赖具体主题。坐标沿用 e.clientX/clientY（视口坐标），语义不变。
//
// 视口钳制（最终兜底）：菜单真实高度随内容变化（isDir、子菜单数量、终端
// 标签数、分支名长度都会改行数与折行），调用方只能用魔法数估算高度做粗略
// clamp（如 innerHeight - 470），必然出现溢出或被过度钳制。因此外壳在挂载后
// 自行测量并按视口钳制，调用方传的 style.top/style.left 仅作为锚点，最终
// 位置由本组件决定。各调用方原有的估算 clamp 可以保留：那只是无害的预钳制，
// 真实尺寸下的最终钳制统一在这里完成，调用方无需改动。

import React, { useLayoutEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';

/** 菜单与视口四边的最小间距：贴边会给「点到边界外=关闭」的判断留余地 */
const VIEWPORT_MARGIN = 8;

interface CtxMenuShellProps {
  /** 菜单本体；
   *  top/left 是调用方给的锚点坐标（已粗略 clamp 过），外壳会在此基础上
   *  测量实际尺寸并做最终钳制 */
  style: React.CSSProperties;
  /** 菜单容器额外 class（如 project-tree-ctx-menu / side-right） */
  className?: string;
  /**
   * 遮罩额外 class：供调用方在特定宿主（分支下拉）夺取点击权时调整遮罩
   * 层级，见 TitleBar 与 theme.css 的 branch-dropdown-open 规则。
   */
  overlayClassName?: string;
  onClose: () => void;
  children: React.ReactNode;
}

/** 钳制后的坐标；null = 尚未测量（首帧隐藏） */
interface ClampedPos { top: number; left: number }

export const CtxMenuShell: React.FC<CtxMenuShellProps> = ({ style, className, overlayClassName, onClose, children }) => {
  const menuRef = useRef<HTMLDivElement>(null);
  const [clamped, setClamped] = useState<ClampedPos | null>(null);
  // 仅用于避免 ResizeObserver 回调里重复 setState 导致渲染循环
  const lastClamped = useRef<ClampedPos | null>(null);

  const anchorTop = Number(style.top) || 0;
  const anchorLeft = Number(style.left) || 0;

  // Esc 关闭：菜单项都不抢 Esc，与原有各菜单的行为一致
  useLayoutEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.stopPropagation();
        onClose();
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [onClose]);

  // 挂载后测量 + 钳制。用 useLayoutEffect：在浏览器绘制前完成，配合下面的
  // visibility:hidden 首帧隐藏，不会出现「先错位再跳位」的闪烁。
  useLayoutEffect(() => {
    const el = menuRef.current;
    if (!el) return;

    const clamp = () => {
      // 用 offsetWidth/offsetHeight 而非 getBoundingClientRect：入场动画
      // （ctx-menu-slide 的 scale(0.985)）会污染 rect 的尺寸；而我们要的
      // 本来就只有盒子大小，位置由锚点算，不需要 rect 的坐标
      const w = el.offsetWidth;
      const h = el.offsetHeight;
      // 菜单比可用空间还高时（小屏 + 长菜单）取上边界：上缘贴 8px，
      // 下缘溢出是此时唯一可接受的取舍，比整体上移藏住锚点更可预期
      const maxTop = Math.max(VIEWPORT_MARGIN, window.innerHeight - h - VIEWPORT_MARGIN);
      const maxLeft = Math.max(VIEWPORT_MARGIN, window.innerWidth - w - VIEWPORT_MARGIN);
      const top = Math.min(Math.max(anchorTop, VIEWPORT_MARGIN), maxTop);
      const left = Math.min(Math.max(anchorLeft, VIEWPORT_MARGIN), maxLeft);
      const prev = lastClamped.current;
      if (prev && prev.top === top && prev.left === left) return;
      lastClamped.current = { top, left };
      setClamped({ top, left });
    };

    clamp();

    // 内容变化后重新测量：项目树菜单 isDir 切换、子菜单展开/收起、行数变化
    // 都会改高度，而 style.top/left 的依赖数组感知不到这些。ResizeObserver
    // 比 MutationObserver 更简单可靠（直接观测盒子尺寸，无需列举属性）。
    const ro = new ResizeObserver(clamp);
    ro.observe(el);
    // 视口缩放（改窗口大小）时锚点与新边界都失效，同样重测
    window.addEventListener('resize', clamp);

    return () => {
      ro.disconnect();
      window.removeEventListener('resize', clamp);
    };
  }, [anchorTop, anchorLeft]);

  // 测量完成前不显示：调用方的锚点可能让菜单整体在视口外，先藏住等钳制结果
  const menuStyle: React.CSSProperties = {
    ...style,
    ...(clamped ? { top: clamped.top, left: clamped.left } : null),
    visibility: clamped ? 'visible' : 'hidden'
  };

  return createPortal(
    <>
      <div
        className={`ctx-overlay${overlayClassName ? ` ${overlayClassName}` : ''}`}
        onClick={onClose}
        onContextMenu={e => {
          // 不能在别处再右键时 stopPropagation：业务菜单由 React 委托在 root
          // 冒泡派发（见 src/main.tsx 的 capture 守卫注释），这里只吃掉这次
          // 右键本身，让它既不开系统菜单也不重开新菜单。
          e.preventDefault();
          onClose();
        }}
      />
      <div ref={menuRef} className={`branch-ctx-menu${className ? ` ${className}` : ''}`} style={menuStyle}>
        {children}
      </div>
    </>,
    document.body
  );
};
