#!/usr/bin/env node
// 校验 Git Log 长跨度折叠的三条不变式（见 src/services/logFolding.ts 文件头）：
//   1. 前缀单调：分页 loadMore 后旧的断开标记不被撤销（「蓝线从断开变连接」回归）；
//   2. interaction 行永不被省略（merge 曲线不断头）；
//   3. 布局前缀确定：追加提交不改旧行轨道分配。
//
// 夹具是合成提交列表（显式 parent 表），不依赖仓库状态：
//   h0 -> h1 -> m1(merge: [c3, P]) -> c3..c63(链, 其中 c50 可选为
//   merge: [c51, P] 制造 interaction) -> P(64, 根)
//
// 用法：node --experimental-strip-types scripts/verify-log-folding.mjs

import {
  computeGraphLayout,
  computeGraphBreaks,
  mergeGraphBreaks,
  GRAPH_BREAK_MIN_SPAN
} from '../src/services/logFolding.ts';

let failures = 0;
const assert = (cond, msg) => {
  if (cond) console.log(`  ok  ${msg}`);
  else {
    failures += 1;
    console.error(`FAIL  ${msg}`);
  }
};

/** 合成 GitCommit：只带布局/折叠实际读取的字段。 */
const commit = (hash, parents) => ({
  hash,
  shortHash: hash.slice(0, 8),
  message: `msg ${hash}`,
  author: 'tester',
  authorEmail: 't@example.com',
  date: '2026-01-01T00:00:00.000Z',
  graphTrack: 0,
  parents
});

/**
 * @param {number} total      总行数（P 放在 parentRow，默认 64，total > parentRow）
 * @param {number} interactionRow  >0 时该行改为 merge: [next, P]（制造 interaction）
 * @param {number} parentRow  根提交 P 所在行号；调大它 = 父提交离得更远，
 *                            pending ▼ 要多翻几页才解析（复现截图 1→2→3 流程）
 */
function makeCommits(total, interactionRow = -1, parentRow = 64) {
  const rows = [commit('h0', ['h1']), commit('h1', ['m1']), commit('m1', ['c3', 'P'])];
  const pRow = parentRow;
  for (let i = 3; i < total; i++) {
    if (i === pRow) {
      rows.push(commit('P', []));
      continue;
    }
    const next = i + 1 === pRow ? 'P' : `c${i + 1}`;
    const parents = [next];
    if (i === interactionRow) parents.push('P');
    rows.push(commit(`c${i}`, parents));
  }
  return rows;
}

const lane1 = c => computeGraphLayout([c]).size; // 占位避免误用
void lane1;

/** 某行是否在 lane1 被省略。 */
const omittedAt = (breaks, commits, idx, lane) =>
  breaks.get(commits[idx].hash)?.omitted?.has(lane) ?? false;

console.log('== 不变式 3：布局前缀确定 ==');
{
  const page1 = makeCommits(65).slice(0, 41);
  const full = makeCommits(65);
  const l1 = computeGraphLayout(page1);
  const l2 = computeGraphLayout(full);
  let same = true;
  for (const c of page1) if (JSON.stringify(l1.get(c.hash)) !== JSON.stringify(l2.get(c.hash))) same = false;
  assert(same, '第一页 41 行的 node/ins/straight/outs 在全量重算后逐字段不变');
}

console.log('== 页底 pending 折叠（截图 1：断开状态）==');
let page1Breaks;
let page1Commits;
{
  page1Commits = makeCommits(65).slice(0, 41);
  page1Breaks = computeGraphBreaks(page1Commits, computeGraphLayout(page1Commits));
  const at3 = page1Breaks.get('c3');
  assert(at3?.below?.some(b => b.lane === 1 && b.pending === true), 'row3 有 pending ▼（父提交未加载）');
  assert(omittedAt(page1Breaks, page1Commits, 10, 1), 'row10 lane1 省略（断开）');
  assert(omittedAt(page1Breaks, page1Commits, 40, 1), 'row40（页底）lane1 省略（断开直到列表末尾）');
  assert(!page1Breaks.get(page1Commits[40].hash)?.above?.length, '页底行没有 ▲（右端未闭合）');
}

console.log('== 不变式 1：分页合并后断开不撤销（截图 2/3 回归）==');
{
  const full = makeCommits(65);
  const fresh = computeGraphBreaks(full, computeGraphLayout(full));
  const merged = mergeGraphBreaks(page1Commits, page1Breaks, full, fresh);

  let preserved = true;
  for (let r = 4; r <= 39; r++) {
    if (!omittedAt(merged, full, r, 1)) preserved = false;
  }
  assert(preserved, '旧页 row4..39 的断开（省略）标记全部保留，断开→连接不可能发生');
  assert(merged.get(full[3].hash)?.below?.some(b => b.lane === 1), 'row3 ▼（段起点）保留');
  assert(
    omittedAt(merged, full, 40, 1) || merged.get(full[40].hash)?.above?.some(b => b.lane === 1),
    '旧页底 row40 仍处于断开态（省略或获得 ▲，二者都不会回到整线连接）'
  );

  const at3 = merged.get('c3');
  assert(at3?.below?.some(b => b.lane === 1 && b.pending !== true && b.peer.hash === 'P'),
    'row3 ▼ 刷新为真实父提交 P（箭头本身不消失）');
  assert(full[64].hash === 'P' && merged.get('P') === undefined, 'row64 是 P 本身，无折叠标记');
  assert(omittedAt(merged, full, 62, 1) && merged.get(full[63].hash)?.above?.some(b => b.lane === 1),
    '新页区间 41..62 省略 + row63 ▲（闭合箭头对完整）');
}

console.log('== 不变式 2：interaction 行永不断头 + 边界切分子段 ==');
{
  const page1Commits2 = makeCommits(65, 50).slice(0, 41);
  const b1 = computeGraphBreaks(page1Commits2, computeGraphLayout(page1Commits2));
  assert(omittedAt(b1, page1Commits2, 10, 1), '第一页（interaction 未加载）断开正常');

  const full = makeCommits(65, 50);
  const fresh = computeGraphBreaks(full, computeGraphLayout(full));
  const merged = mergeGraphBreaks(page1Commits2, b1, full, fresh);

  let preserved = true;
  for (let r = 4; r <= 39; r++) if (!omittedAt(merged, full, r, 1)) preserved = false;
  assert(preserved, 'interaction 随第二页落进同段后，旧断开仍全部保留（旧实现此处整段撤销）');
  assert(merged.get(full[3].hash)?.below?.some(b => b.lane === 1), 'row3 ▼（段起点）保留');

  assert(!omittedAt(merged, full, 50, 1), 'row50（merge 曲线汇入 lane1）不被省略，曲线不断头');
  assert(merged.get(full[49].hash)?.above?.some(b => b.lane === 1),
    'row49 有 ▲：interaction 边界把子段切在 49，箭头对接正确');
  assert(!omittedAt(merged, full, 51, 1), 'row51..63 不在本子段（13 行 < 阈值 25），保持整线');
  assert(GRAPH_BREAK_MIN_SPAN === 25, `折叠阈值为 25 行（当前 ${GRAPH_BREAK_MIN_SPAN}）`);
}

console.log('== pending 箭头点击：续页到解析为止（截图 1→2→3 测试流程回归）==');
{
  // 真实形态：P 放到 index 124，50 条/页时第一眼 N=50、第二页 N=100 都
  // 还见不到 P —— row3 ▼ 必须**连续多页保持 pending**（UI 的续页闭环据此
  // 一直加载），到 N=125 才解析成真父提交。旧 UI 只续一页就停（且从不
  // 跳转），点箭头落到了列表当时底部的那一行（截图 2 红箭头行）。
  const P_ROW = 124;
  let prev = null;
  const loadTo = n => {
    const cs = makeCommits(125, -1, P_ROW).slice(0, n);
    const fresh = computeGraphBreaks(cs, computeGraphLayout(cs));
    const merged = prev ? mergeGraphBreaks(prev.cs, prev.breaks, cs, fresh) : fresh;
    prev = { cs, breaks: merged };
    return { cs, merged };
  };
  for (const n of [50, 100]) {
    const { merged } = loadTo(n);
    const at3 = merged.get('c3');
    assert(at3?.below?.some(b => b.lane === 1 && b.pending === true),
      `加载到第 ${n} 条时 row3 ▼ 仍 pending（补一次页不够，必须继续续页）`);
  }
  const mid = prev;
  assert(omittedAt(mid.breaks, mid.cs, 49, 1), '两轮续页期间 row49 断开标记仍在（合并只加不撤）');

  const { cs, merged } = loadTo(125);
  const at3 = merged.get('c3');
  assert(at3?.below?.some(b => b.lane === 1 && b.pending !== true && b.peer.hash === 'P'),
    '第 125 条加载后 row3 ▼ 解析为真父提交 P（补跳目标，UI 此时才 jumpToCommit）');
  assert(omittedAt(merged, cs, 49, 1), '解析后旧断开仍保留，箭头语义从「继续加载」升级为「跳到 P」');
}

if (failures) {
  console.error(`\n${failures} 项失败`);
  process.exit(1);
}
console.log('\n全部通过');
