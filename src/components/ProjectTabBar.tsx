import React, { useLayoutEffect, useMemo, useRef, useState } from 'react';
import { getBadgeInitials, getProjectBadgeColor, formatTildePath } from '../services/pathUtils';
import { applyTabReorder } from '../services/tabOrder';
import { IconDoubleChevronRight, IconWindowClose } from './FileIcons';
import { t } from '../services/i18n';
import { useLanguageRefresh } from '../hooks/useLanguageRefresh';

export interface ProjectTab {
  path: string;
  name: string;
}

/** tab 状态桶：命令名走 activityCommandsRef（tooltip 用），不参与 memo 比较 */
export interface ProjectTabActivity {
  busy: boolean;
  outputting: boolean;
}

interface ProjectTabBarProps {
  tabs: ProjectTab[];
  activePath: string;
  /** 各项目控制台活动：缺席或 !busy=灰框；busy=静态绿框；busy+outputting=红色跑马灯 */
  activity: Record<string, ProjectTabActivity>;
  /** 命令名走 ref：tooltip 数据不参与渲染比较（activity 只存状态桶） */
  activityCommandsRef?: React.MutableRefObject<Record<string, string[]>>;
  onSelect?: (path: string) => void;
  onClose?: (path: string) => void;
  /** 按显示序的重排（from/to 都是 tab 条内的下标） */
  onReorder?: (from: number, to: number) => void;
}

/** 拖拽判定的位移阈值（px）：小于它视为点击，不进入重排。 */
const DRAG_THRESHOLD = 6;
/** 溢出按钮预留宽度（px）：统计可见 tab 数时从容器右缘扣掉。 */
const OVERFLOW_BTN_WIDTH = 30;

/**
 * 顶栏项目 tab 条（浏览器式）：点击切换项目、悬停 ×/中键关闭、拖拽重排，
 * 边框颜色外化各项目控制台的进程状态。放不下时先由 flex 压缩（CSS），
 * 仍放不下出现「≫」下拉列出剩余项目。
 *
 * memo：App 每次击键/终端输出都重算，tab 条只依赖 tabs/activity/回调；
 * 生效前提：App 侧回调全部 useStableCallback、activity 只在状态桶变化时换引用。
 */
export const ProjectTabBar = React.memo(function ProjectTabBar({
  tabs,
  activePath,
  activity,
  activityCommandsRef,
  onSelect,
  onClose,
  onReorder
}: ProjectTabBarProps) {
  useLanguageRefresh();
  const stripRef = useRef<HTMLDivElement>(null);
  const tabRefs = useRef<Array<HTMLDivElement | null>>([]);
  const [overflowCount, setOverflowCount] = useState(0);
  const lastWidthRef = useRef(0);

  // ---- 溢出检测：flex 压缩到底后仍放不下 → 裁掉尾部进「≫」下拉 ----
  // overflowCount 进依赖：重置为 0 后全量渲染重测一遍才能判定是否真的溢出。
  // 振荡防护：裁剪态放得下且宽度没变时保持不动（不回退全渲染），只有
  // 容器变宽或 tab 数变化才重新尝试完整渲染。
  const lastLenRef = useRef(0);
  useLayoutEffect(() => {
    const strip = stripRef.current;
    if (!strip) return;
    const measure = () => {
      const w = strip.clientWidth;
      const grew = w > lastWidthRef.current;
      lastWidthRef.current = w;
      if (tabs.length === 0) {
        setOverflowCount(0);
        return;
      }
      if (tabs.length !== lastLenRef.current) {
        lastLenRef.current = tabs.length;
        if (overflowCount !== 0) {
          // 先回到全量渲染（触发重渲），下一轮 pass 再判定溢出
          setOverflowCount(0);
          return;
        }
        // 已是全量渲染：直接按全量判定
      }
      const overflows = strip.scrollWidth > strip.clientWidth + 1;
      if (!overflows) {
        if (grew) setOverflowCount(0);
        return;
      }
      const containerRight = strip.getBoundingClientRect().right;
      let visible = 0;
      for (const el of tabRefs.current) {
        if (!el) continue;
        if (el.getBoundingClientRect().right <= containerRight - OVERFLOW_BTN_WIDTH) visible++;
        else break;
      }
      setOverflowCount(Math.max(tabs.length - visible, 1));
    };
    measure();
    const ro = new ResizeObserver(measure);
    ro.observe(strip);
    return () => ro.disconnect();
  }, [tabs, overflowCount]);

  // ---- 拖拽重排：pointer 跟手，位移过阈值进入拖拽，松手提交重排 ----
  // 被拖 tab 按 path 追踪（显示序在预览换位后会变化，path 才是稳定标识）；
  // 预览序 = 把被拖 tab 移到 dragOver 位置，指针跨过别的 tab 即换位。
  const [dragPath, setDragPath] = useState<string | null>(null);
  const [dragOver, setDragOver] = useState<number | null>(null);
  // 松手提交读 ref：pointermove 的 setState 与 pointerup 之间不保证重渲染，
  // 闭包里的 state 可能是旧的
  const dragOverRef = useRef<number | null>(null);
  const dragState = useRef<{ path: string; startX: number; pointerId: number; moved: boolean } | null>(null);
  const suppressClickRef = useRef(false);

  const handleTabPointerDown = (e: React.PointerEvent<HTMLDivElement>, index: number) => {
    if (e.button !== 0) return;
    dragState.current = { path: visibleTabs[index].path, startX: e.clientX, pointerId: e.pointerId, moved: false };
    // 出 tab 也能收到 move/up：capture 在 tab 元素上（无该 API 的环境退化为元素内监听）
    e.currentTarget.setPointerCapture?.(e.pointerId);
  };
  const handleTabPointerMove = (e: React.PointerEvent<HTMLDivElement>) => {
    const st = dragState.current;
    if (!st || e.pointerId !== st.pointerId) return;
    if (!st.moved && Math.abs(e.clientX - st.startX) < DRAG_THRESHOLD) return;
    st.moved = true;
    setDragPath(prev => (prev === st.path ? prev : st.path));
    // 目标位：指针所在 tab 的下标（命中矩形即可，跨过即换位）
    let over = dragOverRef.current ?? tabRefs.current.findIndex(el => el === e.currentTarget);
    tabRefs.current.forEach((el, i) => {
      if (!el) return;
      const rect = el.getBoundingClientRect();
      if (e.clientX >= rect.left && e.clientX <= rect.right) over = i;
    });
    if (dragOverRef.current !== over) {
      dragOverRef.current = over;
      setDragOver(over);
    }
  };
  const endDrag = (commit: boolean) => {
    const st = dragState.current;
    dragState.current = null;
    setDragPath(null);
    setDragOver(null);
    const over = dragOverRef.current;
    dragOverRef.current = null;
    if (!st) return;
    if (st.moved) {
      suppressClickRef.current = true;
      if (commit && over !== null) {
        const from = tabs.findIndex(t2 => t2.path === st.path);
        if (from >= 0 && from !== over) onReorder?.(from, over);
      }
    }
  };
  const handleTabPointerUp = () => endDrag(true);
  const handleTabPointerCancel = () => endDrag(false);

  const handleTabClick = (path: string) => {
    if (suppressClickRef.current) {
      suppressClickRef.current = false;
      return;
    }
    onSelect?.(path);
  };
  const handleTabAuxClick = (e: React.MouseEvent, path: string) => {
    // 中键关闭（浏览器习惯）；阻止默认避免触发自动滚动
    if (e.button === 1) {
      e.preventDefault();
      onClose?.(path);
    }
  };

  // 拖拽预览序：把被拖 tab 实时移到目标位（松手才把重排提交给父组件）
  const displayTabs = useMemo(() => {
    if (!dragPath || dragOver === null) return tabs;
    const from = tabs.findIndex(t2 => t2.path === dragPath);
    if (from < 0) return tabs;
    return applyTabReorder(tabs, from, dragOver);
  }, [tabs, dragPath, dragOver]);
  const visibleTabs = overflowCount > 0 ? displayTabs.slice(0, displayTabs.length - overflowCount) : displayTabs;
  const hiddenTabs = overflowCount > 0 ? displayTabs.slice(displayTabs.length - overflowCount) : [];

  const tabTitle = (tab: ProjectTab): string => {
    const a = activity[tab.path];
    const commands = activityCommandsRef?.current[tab.path] ?? [];
    const lines = [tab.name, formatTildePath(tab.path)];
    if (a?.busy) {
      lines.push(t().titleBar.tabRunningTooltip(commands.join(', ')));
    }
    return lines.join('\n');
  };

  // 溢出下拉里也标当前项目；溢出项有活动状态时按钮亮橙点提示
  const overflowHasActive = useMemo(
    () => hiddenTabs.some(t2 => t2.path === activePath),
    [hiddenTabs, activePath]
  );

  if (tabs.length === 0) {
    // 欢迎页/无项目：整条让位给窗口拖拽
    return <div className="project-tabbar project-tabbar-empty" data-tauri-drag-region="true" />;
  }

  return (
    <div className="project-tabbar" data-tauri-drag-region="false">
      <div className="project-tab-strip" ref={stripRef}>
        {visibleTabs.map((tab, i) => {
          const a = activity[tab.path];
          const stateClass = a?.busy ? (a.outputting ? 'tab-active-output' : 'tab-busy') : 'tab-idle';
          const isCurrent = tab.path === activePath;
          return (
            <div
              key={tab.path}
              ref={el => {
                tabRefs.current[i] = el;
              }}
              className={`project-tab ${stateClass} ${isCurrent ? 'current' : ''} ${tab.path === dragPath ? 'dragging' : ''}`}
              data-tauri-drag-region="false"
              title={tabTitle(tab)}
              onPointerDown={e => handleTabPointerDown(e, i)}
              onPointerMove={handleTabPointerMove}
              onPointerUp={handleTabPointerUp}
              onPointerCancel={handleTabPointerCancel}
              onClick={() => handleTabClick(tab.path)}
              onAuxClick={e => handleTabAuxClick(e, tab.path)}
            >
              {a?.busy && a.outputting && (
                <svg className="project-tab-marquee" aria-hidden="true" focusable="false">
                  <rect
                    className="project-tab-marquee-runner"
                    x="0"
                    y="0"
                    width="100%"
                    height="100%"
                    rx="5.5"
                    ry="5.5"
                    pathLength="100"
                  />
                </svg>
              )}
              <span
                className="project-tab-badge"
                style={{ background: getProjectBadgeColor(tab.name) }}
              >
                {getBadgeInitials(tab.name)}
              </span>
              <span className="project-tab-name">{tab.name}</span>
              <button
                type="button"
                className="project-tab-close"
                title={t().titleBar.closeProjectTab(tab.name)}
                aria-label={t().titleBar.closeProjectTab(tab.name)}
                data-tauri-drag-region="false"
                onClick={e => {
                  // 别让 × 触发 tab 的切换/拖拽收尾
                  e.stopPropagation();
                  onClose?.(tab.path);
                }}
                onPointerDown={e => e.stopPropagation()}
              >
                <IconWindowClose size={11} />
              </button>
            </div>
          );
        })}
        {overflowCount > 0 && (
          <div className={`project-tab-overflow ${overflowHasActive ? 'overflow-active' : ''}`} data-tauri-drag-region="false">
            <IconDoubleChevronRight size={12} />
            <div className="project-tab-overflow-menu" role="menu">
              {hiddenTabs.map(tab => {
                const a = activity[tab.path];
                const stateClass = a?.busy ? (a.outputting ? 'tab-active-output' : 'tab-busy') : 'tab-idle';
                return (
                  <div
                    key={tab.path}
                    className={`project-tab-overflow-item ${stateClass} ${tab.path === activePath ? 'current' : ''}`}
                    onClick={() => onSelect?.(tab.path)}
                    title={tabTitle(tab)}
                  >
                    <span className="project-tab-badge" style={{ background: getProjectBadgeColor(tab.name) }}>
                      {getBadgeInitials(tab.name)}
                    </span>
                    <span className="project-tab-name">{tab.name}</span>
                  </div>
                );
              })}
            </div>
          </div>
        )}
        {/* tab 之后的空白区保留窗口拖拽 */}
        <div className="project-tab-fill" data-tauri-drag-region="true" />
      </div>
    </div>
  );
});
