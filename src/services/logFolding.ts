/**
 * Git Log 长跨度折叠（log folding）纯逻辑层。
 *
 * 为什么独立成模块：`graphBreaks` 曾内联在 GitBottomPanel（800+ 行 TSX）里，
 * 无法回归测试，而它正是「滚动分页后蓝线从断开变连接」这类视觉 bug 的高发区
 * —— 判定输入是**已加载列表的前缀**，每次 loadMore 都重算，任何会回翻的
 * 判定都会被用户在一次滚动里看见两次。
 *
 * 核心不变式（scripts/verify-log-folding.mjs 逐条锁死）：
 * 1. **前缀单调**：旧行已有的 below/above/omitted 标记，在 mergeGraphBreaks
 *    合并后只增不减。根因级教训：旧实现按「整段内是否出现 interaction」
 *    二元判定并放弃**整段**折叠 —— N=50 页底时段内没有远端 merge，折叠成立
 *    （断开）；滚动 loadMore 后远端 merge 落进同一段，整段撤销（连接）。
 *    两层修复：
 *    a) interaction 行是**硬边界**，把长直通段切成子段各自判定（子段一旦
 *       成立其右端是已加载的父提交/边界行，天然固定）；
 *    b) 页底未闭合的段也折叠（omitted 覆盖到页底、不画 ▲、标 pending），
 *       后续分页通过 mergeGraphBreaks 把旧标记并入新结果 —— 只加不撤。
 * 2. **interaction 行永不被 omitted**：有 merge/fork 曲线的行必须保留完整
 *    竖线，否则 merge 曲线悬空断头。
 * 3. 折叠只针对**实际画出来的几何**（连续直通行），不是「同轨号两节点相隔
 *    多远」——后者在 `git log --branches` 的交错排序里会把无关提交配成一对。
 *
 * 布局层的**前缀确定性**是地基：computeGraphLayout 行 i 的输出只取决于
 * rows[0..i]，滚动追加不改旧行的 node/ins/straight/outs。
 */

import type { GitCommit } from '../types';

/** 一条轨道连续至少这么多行都是直通线（且子段内无 merge/fork 硬边界）才折叠。
 *  25 行 ≈ 一屏：日常分支与 merge 平滑连续，只对跨屏的超长历史空洞折叠。 */
export const GRAPH_BREAK_MIN_SPAN = 25;
/** 断开处空出来的高度（px）。太小看不出是断开的，太大又接不回去。 */
export const GRAPH_BREAK_GAP = 7;

/** 一行的全局轨道布局（对**全部已加载提交**做前缀确定性状态机分配）。 */
export interface GraphLayoutRow {
  node: number;
  hasInNode: boolean;
  ins: number[];
  straight: number[];
  outs: { from: number; to: number }[];
}

/** 一次折叠的端点箭头。`peer` 为跳转目标；`pending` 表示该分支的父提交行
 *  尚未加载（段延伸到分页底），点击触发继续加载而不是跳行。 */
export interface FoldArrowMark {
  lane: number;
  peer: GitCommit;
  span: number;
  tooltip: string;
  pending?: boolean;
}

/** 某一行在某条轨道上的断开信息。`above` = 顶部 ▲（线从空档后开始），
 *  `below` = 底部 ▼（线到空档前结束）；`omitted` = 该轨道竖线整行省略。 */
export interface GraphBreak {
  above?: FoldArrowMark[];
  below?: FoldArrowMark[];
  omitted?: Set<number>;
}

/** 全局统一 DAG 布局：对已加载的全量 commits 做轨道分配。
 *  **前缀确定性**：行 i 的输出只取决于 rows[0..i]，滚动加载追加行不会改变
 *  旧行的 node/ins/straight/outs —— 这是折叠层前缀单调性的地基。 */
export function computeGraphLayout(commits: GitCommit[]): Map<string, GraphLayoutRow> {
  const lanes: (string | null)[] = [];
  const map = new Map<string, GraphLayoutRow>();

  for (let i = 0; i < commits.length; i++) {
    const c = commits[i];
    const h = c.hash;
    let node: number;
    let hasInNode = false;
    const existingIdx = lanes.indexOf(h);
    if (existingIdx >= 0) {
      node = existingIdx;
      hasInNode = true;
    } else {
      const emptyIdx = lanes.indexOf(null);
      if (emptyIdx >= 0) {
        node = emptyIdx;
      } else {
        lanes.push(null);
        node = lanes.length - 1;
      }
    }

    const ins: number[] = [];
    const straight: number[] = [];
    for (let idx = 0; idx < lanes.length; idx++) {
      if (idx === node) continue;
      const l = lanes[idx];
      if (l === h) ins.push(idx);
      else if (l !== null) straight.push(idx);
    }

    const nextLanes = [...lanes];
    nextLanes[node] = null;
    for (const src of ins) nextLanes[src] = null;

    const outs: { from: number; to: number }[] = [];
    const parents = c.parents ?? [];
    for (let pIdx = 0; pIdx < parents.length; pIdx++) {
      const p = parents[pIdx];
      if (pIdx === 0) {
        nextLanes[node] = p;
        outs.push({ from: node, to: node });
      } else {
        const toIdx = nextLanes.indexOf(p);
        if (toIdx >= 0) {
          outs.push({ from: node, to: toIdx });
        } else {
          const emptyIdx = nextLanes.indexOf(null);
          let targetLane: number;
          if (emptyIdx >= 0) {
            targetLane = emptyIdx;
          } else {
            nextLanes.push(null);
            targetLane = nextLanes.length - 1;
          }
          nextLanes[targetLane] = p;
          outs.push({ from: node, to: targetLane });
        }
      }
    }

    lanes.length = nextLanes.length;
    for (let k = 0; k < nextLanes.length; k++) lanes[k] = nextLanes[k];

    map.set(h, { node, hasInNode, ins, straight, outs });
  }
  return map;
}

/** 轨道 side-interaction：其它分支在本行向 `lane` 汇入（merge 曲线）或本行的
 *  信号线分出到 `lane`（fork 曲线）。这些行必须整线保留，且是折叠子段的硬边界。 */
function sideInteraction(lay: GraphLayoutRow | undefined, lane: number): boolean {
  if (!lay) return false;
  if (lay.ins.includes(lane)) return true;
  return lay.outs.some(e => e.to === lane && e.from !== lane);
}

/**
 * 长跨度折叠与端点箭头（见文件头不变式 1/2）。
 *
 * 单趟逐轨道扫描：
 * - interaction 行永不作为段起点（它是边界，永远画整线）；
 * - 直通段右扫，遇「非直通」（父提交行）或「直通但 side-interaction」
 *   （边界行）切段；
 * - 段长 ≥ minSpan 才折叠：
 *   · 行 i 记 `below`(▼)，target=父提交行；父提交未加载 → pending，
 *     点击触发继续加载；
 *   · 中间行 `omitted`；右端已加载（边界或父提交）只到 j-1，行 j 记
 *     `above`(▲) 回跳子提交；页底未闭合 omitted 覆盖到 j、不画 ▲；
 * - 段的右端是**已加载行**（父提交/边界/页底），追加新行不改旧段判定，
 *   闭合状态的段天然前缀单调；页底段靠 mergeGraphBreaks 保旧标记。
 */
export function computeGraphBreaks(
  commits: GitCommit[],
  layout: Map<string, GraphLayoutRow>,
  minSpan: number = GRAPH_BREAK_MIN_SPAN
): Map<string, GraphBreak> {
  const out = new Map<string, GraphBreak>();
  const getEntry = (hash: string): GraphBreak => {
    let b = out.get(hash);
    if (!b) {
      b = {};
      out.set(hash, b);
    }
    return b;
  };
  if (commits.length < minSpan) return out;

  const straightOf = (idx: number, lane: number) =>
    layout.get(commits[idx].hash)?.straight.includes(lane) ?? false;
  const interactAt = (idx: number, lane: number) =>
    sideInteraction(layout.get(commits[idx].hash), lane);

  let maxLane = 0;
  for (const c of commits) {
    const lay = layout.get(c.hash);
    if (lay) maxLane = Math.max(maxLane, lay.node, ...lay.straight);
  }

  for (let lane = 0; lane <= maxLane; lane++) {
    let i = 0;
    while (i < commits.length) {
      // 段起点必须是普通直通行：非直通跳过，interaction 行是边界不能当段头
      if (!straightOf(i, lane) || interactAt(i, lane)) {
        i++;
        continue;
      }
      // 子段右扫：遇 interaction 行切段（边界行不属于本段）
      let j = i;
      let boundary: number | null = null;
      while (j + 1 < commits.length && straightOf(j + 1, lane) && !interactAt(j + 1, lane)) {
        j++;
      }
      if (j + 1 < commits.length && straightOf(j + 1, lane) && interactAt(j + 1, lane)) {
        boundary = j + 1;
      }
      const len = j - i + 1;

      if (len >= minSpan) {
        // 子提交：沿本轨道向上找到第一个非直通行（跳过边界/延续段）
        let cr = i - 1;
        while (cr >= 0 && straightOf(cr, lane)) cr--;
        const child = commits[Math.max(cr, 0)];

        // 父提交：从右端继续向右找第一个非直通行（跳过边界行）
        let p = j + 1;
        while (p < commits.length && straightOf(p, lane)) p++;
        const parent = p < commits.length ? commits[p] : null;
        const closedRight = boundary !== null || parent !== null;

        const downTooltip = parent
          ? `↓ 跳转到父提交: ${parent.shortHash} "${parent.message.slice(0, 30)}" (自本行起 ${p - i} 行前)`
          : '↓ 该分支延伸至更早提交，父提交尚未加载，点击继续加载';
        (getEntry(commits[i].hash).below ??= []).push({
          lane,
          peer: parent ?? child,
          span: len,
          tooltip: downTooltip,
          ...(parent === null ? { pending: true } : {})
        });

        const cut = closedRight ? j - 1 : j;
        for (let k = i + 1; k <= cut; k++) {
          (getEntry(commits[k].hash).omitted ??= new Set<number>()).add(lane);
        }

        if (closedRight) {
          const upTooltip = `↑ 跳转到子提交: ${child.shortHash} "${child.message.slice(0, 30)}" (自本行起 ${len} 行后)`;
          (getEntry(commits[j].hash).above ??= []).push({
            lane,
            peer: child,
            span: len,
            tooltip: upTooltip
          });
        }
      }
      i = (boundary !== null ? boundary : j) + 1;
    }
  }
  return out;
}

/** `prevCommits` 是否是 `nextCommits` 的前缀（同 hash 逐位相等、只在尾部追加）。 */
function isPrefixExtension(prevCommits: GitCommit[], nextCommits: GitCommit[]): boolean {
  if (prevCommits.length > nextCommits.length) return false;
  for (let k = 0; k < prevCommits.length; k++) {
    if (prevCommits[k].hash !== nextCommits[k].hash) return false;
  }
  return true;
}

/**
 * 分页合并：把上一页的折叠标记并入本次重算结果（不变式 1 的执行点）。
 *
 * 组件每次 loadMore 后用 fresh = computeGraphBreaks(全量) 重算，再调用本函数
 * 与旧结果合并：
 * - 旧结果在当前列表里只是前缀追加 → 保留；否则（切分支/重写历史）丢弃；
 * - 同一行同一轨道：**fresh 有箭头则 fresh 为准**（右端可能从页底推进到
 *   真父提交，箭头的 target/tooltip 需刷新）；其余旧标记（箭头、omitted）
 *   全部并入 —— 旧的断开永不被撤销。
 */
export function mergeGraphBreaks(
  prevCommits: GitCommit[] | null,
  prevBreaks: Map<string, GraphBreak> | null,
  nextCommits: GitCommit[],
  fresh: Map<string, GraphBreak>
): Map<string, GraphBreak> {
  if (!prevCommits || !prevBreaks || prevCommits.length === 0) return fresh;
  if (!isPrefixExtension(prevCommits, nextCommits)) return fresh;

  const merged = new Map(fresh);
  for (const [hash, old] of prevBreaks) {
    const fe = merged.get(hash);
    if (!fe) {
      merged.set(hash, old);
      continue;
    }
    // fresh 已在该行该轨道画了箭头 → 该轨道以 fresh 为准（含其 omitted 策略）
    const freshArrowLanes = new Set<number>();
    for (const m of fe.above ?? []) freshArrowLanes.add(m.lane);
    for (const m of fe.below ?? []) freshArrowLanes.add(m.lane);

    if (old.omitted) {
      const keep = new Set(fe.omitted ?? []);
      for (const l of old.omitted) if (!freshArrowLanes.has(l)) keep.add(l);
      if (keep.size) fe.omitted = keep;
    }
    for (const side of ['above', 'below'] as const) {
      const oldArr = old[side];
      if (!oldArr?.length) continue;
      const kept = oldArr.filter(m => !freshArrowLanes.has(m.lane));
      if (kept.length) fe[side] = [...(fe[side] ?? []), ...kept];
    }
  }
  return merged;
}
