// 文件树窗口化：纯几何计算，供 ProjectDrawer 与单测共用。
//
// 树行高固定（CSS: .tree-node-row 22px / .tree-root-item 24px /
// .tree-external-root 24px + margin-top 4px），所以窗口边界可以按前缀和
// 二分查找，不需要逐行测量。行高若改，必须同步 TREE_* 常量与这里。

export interface TreeWindow {
  /** 首个渲染行（含） */
  start: number;
  /** 末个渲染行（不含） */
  end: number;
  /** 顶部占位高度 */
  topPad: number;
  /** 底部占位高度 */
  bottomPad: number;
}

/** 由每行高度生成前缀和：offsets[i] = 第 i 行顶部，offsets[n] = 总高度。 */
export function buildTreeOffsets(heights: readonly number[]): number[] {
  const offsets = new Array<number>(heights.length + 1);
  let acc = 0;
  for (let i = 0; i < heights.length; i += 1) {
    offsets[i] = acc;
    acc += heights[i];
  }
  offsets[heights.length] = acc;
  return offsets;
}

/** 第一个「底边 > y」的行号；y 超出总高时返回行数。 */
function firstRowBottomAfter(offsets: readonly number[], y: number): number {
  let lo = 0;
  let hi = offsets.length - 1;
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    if (offsets[mid + 1] <= y) lo = mid + 1;
    else hi = mid;
  }
  return lo;
}

/** 第一个「顶边 >= y」的行号；y 超出总高时返回行数。 */
function firstRowTopAtOrAfter(offsets: readonly number[], y: number): number {
  let lo = 0;
  let hi = offsets.length - 1;
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    if (offsets[mid] < y) lo = mid + 1;
    else hi = mid;
  }
  return lo;
}

/**
 * 计算应挂载的行区间与上下占位高度。
 *
 * 窗口至少覆盖视口，并且始终返回至少一行：行数少于视口时也走同一条路径，
 * 避免「空树/一行树」出现独立的空态分支。`overscan` 保留两屏余量，
 * 快速滚动时合成线程先看到空白的情况会被吸收。
 */
export function computeTreeWindow(
  offsets: readonly number[],
  totalHeight: number,
  scrollTop: number,
  viewportHeight: number,
  overscan: number
): TreeWindow {
  const count = offsets.length - 1;
  if (count <= 0) return { start: 0, end: 0, topPad: 0, bottomPad: 0 };

  const top = Math.max(0, scrollTop);
  const viewport = Math.max(0, viewportHeight);
  const firstVisible = firstRowBottomAfter(offsets, top);
  const start = Math.max(0, Math.min(count - 1, firstVisible - Math.max(0, overscan)));
  const firstAfterViewport = firstRowTopAtOrAfter(offsets, top + viewport);
  const end = Math.max(start + 1, Math.min(count, firstAfterViewport + Math.max(0, overscan)));

  return {
    start,
    end,
    topPad: offsets[start],
    bottomPad: Math.max(0, totalHeight - offsets[end])
  };
}
