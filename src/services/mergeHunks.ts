// diff3 标记切块（纯函数，无任何 UI 依赖，可被 node verify 脚本直测）。
//
// 输入：git_merge_file_marked 的输出（merge-file -p --diff3，-L 标签任意）。
// 输出：冲突块在"当前文本"中的行范围 + 双方行 —— 中间格 decorations 高亮、
// gutter ≫/≪ 定位、≫/≪ splice 都以它为准。中间格是自由文本（可手改），
// 所以每次操作前重算，不缓存。

export interface MergeHunk {
  /** 块在当前结果文本中的行范围（0 起，end 不含） */
  start: number;
  end: number;
  ours: string[];
  theirs: string[];
  /** base 段行（无共同祖先时为空）：删除型块的结构化渲染用 */
  base: string[];
}

/** diff3 标记行识别（-L 标签任意，后端传 ours/base/theirs，前端只认符号列）。 */
export const isMarkStart = (l: string) => /^<{7}( |$)/.test(l);
export const isMarkBaseSep = (l: string) => /^\|{7}( |$)/.test(l);
export const isMarkMid = (l: string) => /^={7}$/.test(l);
export const isMarkEnd = (l: string) => /^>{7}( |$)/.test(l);

export const isMarkLine = (l: string) =>
  isMarkStart(l) || isMarkBaseSep(l) || isMarkMid(l) || isMarkEnd(l);

/** 当前结果文本 → 冲突块。手改中间格也没关系：每次操作前重算。 */
export function parseMergeHunks(text: string): MergeHunk[] {
  const lines = text.split('\n');
  const hunks: MergeHunk[] = [];
  let i = 0;
  while (i < lines.length) {
    if (!isMarkStart(lines[i])) {
      i++;
      continue;
    }
    const start = i;
    i++;
    const ours: string[] = [];
    while (i < lines.length && !isMarkBaseSep(lines[i]) && !isMarkMid(lines[i])) {
      ours.push(lines[i]);
      i++;
    }
    // 非 --diff3 风格（无 base 段）：直接是 =======，base 为空。
    const base: string[] = [];
    if (i < lines.length && isMarkBaseSep(lines[i])) {
      i++;
      while (i < lines.length && !isMarkMid(lines[i])) {
        base.push(lines[i]);
        i++;
      }
    }
    if (i < lines.length && isMarkMid(lines[i])) i++;
    const theirs: string[] = [];
    while (i < lines.length && !isMarkEnd(lines[i])) {
      theirs.push(lines[i]);
      i++;
    }
    if (i < lines.length && isMarkEnd(lines[i])) {
      i++;
      // 当块位于文件尾部且输入以换行符结尾（git merge-file 将文件的末尾换行置于 >>>>>>> theirs 之后）
      // 在 Monaco/PyCharm 编辑器行切分语义下，各非空边与 base 末尾均含该空行，吸纳之以保证末行对齐
      if (i === lines.length - 1 && lines[i] === '') {
        if (ours.length > 0) ours.push('');
        if (base.length > 0) base.push('');
        if (theirs.length > 0) theirs.push('');
        i++;
      }
    }
    hunks.push({ start, end: i, ours, theirs, base });
  }
  return hunks;
}

/** 无标记视图：中间格的初始文本 + 块在其中的位置（0 起，end 不含）。 */
export interface MergeViewHunk {
  start: number;
  end: number;
  ours: string[];
  theirs: string[];
  base: string[];
}
export interface MergeView {
  /** clean 行 + 每块初始显示（冲突块=ours 非空则 ours 否则 theirs；非冲突块=base，更改待应用），无任何标记行 */
  text: string;
  hunks: MergeViewHunk[];
}

/**
 * 带标记全文 → 无标记视图。PyCharm 中间格没有 `<<<<<<<` 行，且**非冲突更改
 * 初始不进结果**（实测：Result 只有 base 的 4 行，theirs 的新增 2 行留在右栏
 * 绿色待应用，"Apply non-conflicting changes" 按钮/侧栏标记按需吃入）。
 * 冲突块初始显示 ours（空则 theirs，如删改冲突）。对话框之后只维护视图坐标，
 * 用户敲字用 diffRegion 做前后缀对位，不再碰标记。
 */
export function buildMergeView(marked: string): MergeView {
  const lines = marked.split('\n');
  const parts = parseMergeHunks(marked);
  const out: string[] = [];
  const hunks: MergeViewHunk[] = [];
  let cur = 0;
  for (const h of parts) {
    out.push(...lines.slice(cur, h.start));
    const conflict = h.ours.length > 0 && h.theirs.length > 0;
    const show = conflict ? (h.ours.length > 0 ? h.ours : h.theirs) : h.base;
    const start = out.length;
    out.push(...show);
    hunks.push({ start, end: out.length, ours: h.ours, theirs: h.theirs, base: h.base });
    cur = h.end;
  }
  out.push(...lines.slice(cur));
  return { text: out.join('\n'), hunks };
}

/**
 * 空白忽略模式（对齐 PyCharm "Apply non-conflicting changes" 右侧下拉的
 * Do not ignore / Trim / Ignore / Ignore + empty lines / Ignore imports 档位）。
 * 缺省 undefined = none（旧行为，verify 脚本与所有旧调用不受影响）。
 */
export type WsMode = 'none' | 'trim' | 'space' | 'space-empty' | 'imports';

/** 单行归一（1:1 映射，长度不变 —— 对位结果的行号语义不被破坏）。 */
export function normWsLine(raw: string, mode?: WsMode): string {
  if (!mode || mode === 'none') return raw;
  if (mode === 'trim') return raw.trim();
  // space / space-empty / imports 先做空白折叠
  let l = raw.replace(/\s+/g, ' ').trim();
  if (mode === 'space-empty') {
    // "Ignore whitespaces and empty lines"：空行/纯空白行归一到同一哨兵，
    // 与非空行不相等 —— 行数保留，空行差异被消掉
    if (l === '') return '\u0000';
    return l;
  }
  if (mode === 'imports') {
    // 近似：import/include/use 行归一到同一哨兵（audit 注明静态近似）
    if (/^\s*(import\s|from\s+\S+\s+import\s|#include\b|use\s+\w)/.test(raw)) return '\u0001';
    return l;
  }
  return l;
}

const normAll = (lines: string[], mode?: WsMode): string[] =>
  !mode || mode === 'none' ? lines : lines.map(l => normWsLine(l, mode));

/** 新旧文本的前后缀对位：单区域变更（单次敲字/粘贴都是一个连续区）。 */
export function diffRegion(
  oldLines: string[],
  newLines: string[],
  ws?: WsMode
): { start: number; oldEnd: number; newEnd: number } {
  const o = normAll(oldLines, ws);
  const n = normAll(newLines, ws);
  let start = 0;
  while (start < o.length && start < n.length && o[start] === n[start]) start++;
  let oldEnd = o.length;
  let newEnd = n.length;
  while (oldEnd > start && newEnd > start && o[oldEnd - 1] === n[newEnd - 1]) {
    oldEnd--;
    newEnd--;
  }
  return { start, oldEnd, newEnd };
}

/**
 * 块内三方对齐：删除型块（恰一边为空）的结构化渲染用。
 * 对齐 base 与现存边：落在 LCS 里的行是 context（中性显示），其余是现存边
 * 独有的 delta（绿色待决）。用户在块内敲字后重算：改了 context 行 → 它不再
 * 命中 base，自动变绿；改回 → 自动变白。对齐自洽，无需维护偏移。
 */
export function alignAdded(base: string[], cur: string[], ws?: WsMode): boolean[] {
  const bs = normAll(base, ws);
  const cs = normAll(cur, ws);
  const n = bs.length;
  const m = cs.length;
  const dp: number[][] = Array.from({ length: n + 1 }, () => new Array<number>(m + 1).fill(0));
  for (let i = 1; i <= n; i++) {
    for (let j = 1; j <= m; j++) {
      dp[i][j] = bs[i - 1] === cs[j - 1] ? dp[i - 1][j - 1] + 1 : Math.max(dp[i - 1][j], dp[i][j - 1]);
    }
  }
  const added = new Array<boolean>(m).fill(true);
  let i = n;
  let j = m;
  while (i > 0 && j > 0) {
    if (bs[i - 1] === cs[j - 1]) {
      added[j - 1] = false;
      i--;
      j--;
    } else if (dp[i - 1][j] >= dp[i][j - 1]) {
      i--;
    } else {
      j--;
    }
  }
  return added;
}

/**
 * 侧栏逐行分类（该侧片段 vs base）：
 *   none = 与 base 相同（不染色，保持背景白）；
 *   add  = 纯新增行（绿色底色）；
 *   mod  = 替换/修改行（淡紫色底色）。
 * 采用工业级双端贪心剪枝（Myers / Patience 核心第一步）+ LCS 动态规划编辑图回溯，
 * 通用于任意文件、任意语言、任意变动模式（头插、尾增、中段多处交织增删改），非针对特定用例。
 */
export type SideLineKind = 'none' | 'add' | 'mod';

export function sideChanges(base: string[], cur: string[], ws?: WsMode): SideLineKind[] {
  const out: SideLineKind[] = new Array<SideLineKind>(cur.length).fill('none');
  const b = normAll(base, ws);
  const c = normAll(cur, ws);
  const n = b.length;
  const m = c.length;
  if (m === 0) return out;
  if (n === 0) {
    out.fill('add');
    return out;
  }

  // 1. 贪心剥离公共前缀（对齐 PyCharm 首尾匹配优先逻辑）
  let p = 0;
  while (p < n && p < m && b[p] === c[p]) p++;

  // 2. 剥离剩余部分的公共后缀
  let s = 0;
  while (s < n - p && s < m - p && b[n - 1 - s] === c[m - 1 - s]) s++;

  const bm = n - p - s;
  const cm = m - p - s;

  // 前缀和后缀与 base 相同，已在初始化时全置为 'none'
  if (cm === 0) return out;

  // 中段 base 无行留存，cur 中段全为纯新增
  if (bm === 0) {
    for (let k = 0; k < cm; k++) {
      out[p + k] = 'add';
    }
    return out;
  }

  // 中段较小或一般大小：利用 LCS 动态规划精准对位中段的每一处变动与保留
  const bMid = b.slice(p, p + bm);
  const cMid = c.slice(p, p + cm);

  // 防御极度超大块（> 2000x2000 格）回退为分段以防内存暴涨
  if (bm * cm > 4_000_000) {
    for (let k = 0; k < cm; k++) {
      out[p + k] = k < bm ? (cMid[k] === bMid[k] ? 'none' : 'mod') : 'add';
    }
    return out;
  }

  const dp: number[][] = Array.from({ length: bm + 1 }, () => new Array<number>(cm + 1).fill(0));
  for (let i = 1; i <= bm; i++) {
    for (let j = 1; j <= cm; j++) {
      dp[i][j] = bMid[i - 1] === cMid[j - 1] ? dp[i - 1][j - 1] + 1 : Math.max(dp[i - 1][j], dp[i][j - 1]);
    }
  }

  let i = bm;
  let j = cm;
  const edits: Array<{ type: 'keep' | 'delete' | 'insert'; curIdx: number }> = [];
  while (i > 0 || j > 0) {
    if (i > 0 && j > 0 && bMid[i - 1] === cMid[j - 1]) {
      edits.push({ type: 'keep', curIdx: j - 1 });
      i--;
      j--;
    } else if (j > 0 && (i === 0 || dp[i][j - 1] >= dp[i - 1][j])) {
      edits.push({ type: 'insert', curIdx: j - 1 });
      j--;
    } else {
      edits.push({ type: 'delete', curIdx: -1 });
      i--;
    }
  }
  edits.reverse();

  let k = 0;
  while (k < edits.length) {
    if (edits[k].type === 'keep') {
      k++;
      continue;
    }
    let delCount = 0;
    const insList: number[] = [];
    while (k < edits.length && edits[k].type !== 'keep') {
      if (edits[k].type === 'delete') delCount++;
      else insList.push(edits[k].curIdx);
      k++;
    }
    const kind: SideLineKind = delCount > 0 ? 'mod' : 'add';
    for (const curIdx of insList) {
      out[p + curIdx] = kind;
    }
  }

  return out;
}

/**
 * 块内容在整份侧栏文件中的定位（侧栏高亮用）。
 * 各块内容在对应侧文件里按顺序出现，用移动指针从头搜（精确连续匹配）；
 * 找不到（文件被外部改过 / 重复行歧义兜底）返回 null —— 宁可不染。
 */
export function locateBlocks(fileLines: string[], contents: string[][]): ([number, number] | null)[] {
  let p = 0;
  return contents.map((c, idx) => {
    if (c.length === 0) return null;
    for (let i = p; i + c.length <= fileLines.length; i++) {
      let ok = true;
      for (let k = 0; k < c.length; k++) {
        if (fileLines[i + k] !== c[k]) {
          ok = false;
          break;
        }
      }
      if (ok) {
        let end = i + c.length;
        // 通用尾部对齐：若该块是当前侧的最后一个非空块，且刚好覆盖到文件倒数第二行、最后一行是换行空行，
        // 则一并纳入该尾空行，避免尾行孤立断带
        const isLastHunk = contents.slice(idx + 1).every(rem => rem.length === 0);
        if (isLastHunk && end === fileLines.length - 1 && fileLines[end] === '') {
          end = fileLines.length;
        }
        const r: [number, number] = [i, end];
        p = end;
        return r;
      }
    }
    return null;
  });
}
