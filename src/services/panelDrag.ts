/**
 * 面板拖拽原语：Pointer Capture + DOM 直写，松手才交回 React state。
 *
 * 为什么用 setPointerCapture 而不是全屏遮罩 + window 监听：
 * 1. capture 之后所有后续指针事件（包括指针移出窗口外再松手）都会
 *    投递给把手元素。WKWebView 里鼠标在窗口外松开时，window 的
 *    mouseup 不触发 —— 旧的遮罩方案会漏掉 mouseup，留下一个
 *    z-index 999999 的全屏 div 永远拦截点击（用户感知为「拖了一下
 *    之后整个 app 点不动」），监听器也随之泄漏。
 * 2. pointercancel（系统手势抢占：触控板多指、通知中心呼出等）
 *    可以被感知，拖拽能优雅收尾而不是永久卡住。
 * 3. 不再需要遮罩来防止鼠标划过 canvas / monaco 丢事件 —— capture
 *    本身就保证了事件路由。
 *
 * 为什么拖拽期间直写 DOM 而不 setState：App.tsx 是无 memo 的单壳层，
 * mousemove 里每帧 setState 会重跑整棵树（TitleBar 分支过滤排序、
 * GitBottomPanel filteredCommits、statusCrumbs 全在路径上），感知为掉帧。
 * 这里 mousemove 只把尺寸写进目标元素 inline style 并按帧合并（rAF），
 * 松手一次性 setState 落盘。
 *
 * 拖拽期间外部 state 变化引发的重渲染不会吃掉直写值：React 对 style
 * 是逐键 diff，值没变的键不会重写。渲染值保持「上次提交的 state 尺寸」，
 * 与直写互不冲突（这也是渲染侧不要在拖拽中省略 width/height 键的原因
 * —— 省略键会让 React 删除 inline 宽高，面板塌缩闪跳）。
 */

/** 结构化的 React 指针事件子集（服务层不直接依赖 react 类型） */
export interface DragPointerEvent {
  clientX: number;
  clientY: number;
  /** React.PointerEvent 恒为鼠标主键 0 / 触摸 0；保留可选兼容旧调用 */
  button?: number;
  /** PointerEvent 标识，用于过滤拖拽期间的其余指针（多点触控） */
  pointerId: number;
  preventDefault: () => void;
  stopPropagation: () => void;
  currentTarget: EventTarget & Element;
}

interface DragHooks {
  /** 拖拽中（已按帧合并前的事件流），x/y 为视口坐标 */
  onMove?: (x: number, y: number) => void;
  /** 把手上方松手（含指针已移出窗口的情况——capture 保证送达） */
  onEnd?: (x: number, y: number) => void;
  /** 系统抢占指针（pointercancel）：按最后已知位置正常收尾 */
  onCancel?: (x: number, y: number) => void;
}

/**
 * 捕获式指针拖拽底层原语：把手元素 capture 本次指针，
 * 拖拽期间 body 光标固定为 cursor、禁用文本选择，结束后恢复。
 */
export function startPointerDrag(event: DragPointerEvent, cursor: string, hooks: DragHooks): void {
  if (event.button !== undefined && event.button !== 0) return;
  event.preventDefault();
  event.stopPropagation();

  const handle = event.currentTarget as HTMLElement;
  const pointerId = event.pointerId;
  // capture 成功后监听挂在把手元素上（capture 保证事件路由）；
  // 失败则退化为 window 级监听
  let listenTarget: Window | HTMLElement = handle;

  const prevCursor = document.body.style.cursor;
  const prevUserSelect = document.body.style.userSelect;
  document.body.style.cursor = cursor;
  document.body.style.userSelect = 'none';

  const cleanup = () => {
    listenTarget.removeEventListener('pointermove', onMove);
    listenTarget.removeEventListener('pointerup', onUp);
    listenTarget.removeEventListener('pointercancel', onCancel);
    listenTarget.removeEventListener('lostpointercapture', onLostCapture);
    document.body.style.cursor = prevCursor;
    document.body.style.userSelect = prevUserSelect;
  };
  const onMove = (ev: Event) => {
    const e = ev as PointerEvent;
    if (e.pointerId !== pointerId) return;
    hooks.onMove?.(e.clientX, e.clientY);
  };

  const onUp = (ev: Event) => {
    const e = ev as PointerEvent;
    if (e.pointerId !== pointerId) return;
    cleanup();
    hooks.onEnd?.(e.clientX, e.clientY);
  };

  const onCancel = (ev: Event) => {
    const e = ev as PointerEvent;
    if (e.pointerId !== pointerId) return;
    cleanup();
    hooks.onCancel?.(e.clientX, e.clientY);
  };

  // 捕获被系统收回：指针被抢占之外，把手元素在拖拽中被卸载（切项目/切工具窗口）
  // 也会触发这个事件，此时不会再有 pointerup/pointercancel —— 不接就走不到 cleanup，
  // body 上的光标与 userSelect 覆盖会永久残留。
  const onLostCapture = (ev: Event) => {
    const e = ev as PointerEvent;
    if (e.pointerId !== pointerId) return;
    cleanup();
    hooks.onCancel?.(e.clientX, e.clientY);
  };

  // capture 失败（合成事件、pointerId 无活动指针等极端环境）时退化为 window 级监听：
  // 仍能收到全局 pointermove/up，只是光标移出窗口外的松手可能丢——该路径仅存于
  // 不支持 Pointer Capture 的环境，真实 Tauri WKWebView 恒走 handle 捕获分支。
  try {
    handle.setPointerCapture(pointerId);
  } catch {
    listenTarget = window;
  }
  listenTarget.addEventListener('pointermove', onMove);
  listenTarget.addEventListener('pointerup', onUp);
  listenTarget.addEventListener('pointercancel', onCancel);
  listenTarget.addEventListener('lostpointercapture', onLostCapture);
}

export type DragAxis = 'x' | 'y';

export interface DragTarget {
  /** 拖拽期间实时改尺寸的元素（null = 只算值，不改 DOM） */
  element: HTMLElement | null;
  axis: DragAxis;
}

/**
 * 面板尺寸拖拽：mousemove 按帧合并直写目标元素宽/高，
 * 松手（或系统取消）以松手位置算出的最终值回调 onCommit。
 */
export function startPanelDrag(
  event: DragPointerEvent,
  target: DragTarget,
  cursor: string,
  compute: (clientX: number, clientY: number) => number,
  onCommit: (value: number) => void
): void {
  let pending: number | null = null;
  let frame: number | null = null;

  const apply = () => {
    frame = null;
    if (pending === null || !target.element) return;
    target.element.style[target.axis === 'x' ? 'width' : 'height'] = `${pending}px`;
  };

  const finish = (x: number, y: number) => {
    if (frame !== null) {
      cancelAnimationFrame(frame);
      frame = null;
    }
    pending = null;
    onCommit(compute(x, y));
  };

  startPointerDrag(event, cursor, {
    onMove: (x, y) => {
      pending = compute(x, y);
      if (frame === null) frame = requestAnimationFrame(apply);
    },
    onEnd: finish,
    onCancel: finish
  });
}
