#!/usr/bin/env node
// 切块纯函数回归：parseMergeHunks 是合并工具一切交互（高亮/≫≪/Apply）的地基，
// 行号错一位就是吃错块。用 node --experimental-strip-types 直测 TS 源码。
//
// 用法：node --experimental-strip-types scripts/verify-merge-hunks.mjs

import { parseMergeHunks, buildMergeView, diffRegion, alignAdded, locateBlocks, sideChanges, normWsLine } from '../src/services/mergeHunks.ts';

let failures = 0;
const check = (name, cond, extra = '') => {
  if (!cond) {
    console.error(`FAIL ${name} ${extra}`);
    failures += 1;
  }
};

// --diff3 标准形：clean 头 + 冲突块 + clean 尾
const diff3 = [
  'same',
  '<<<<<<< ours',
  'main',
  '||||||| base',
  'base',
  '=======',
  'side',
  '>>>>>>> theirs',
  'tail'
].join('\n');
let hs = parseMergeHunks(diff3);
check('diff3 一块', hs.length === 1, `got ${hs.length}`);
check('diff3 范围', hs[0]?.start === 1 && hs[0]?.end === 8, JSON.stringify(hs[0]));
check('diff3 ours', JSON.stringify(hs[0]?.ours) === JSON.stringify(['main']));
check('diff3 theirs', JSON.stringify(hs[0]?.theirs) === JSON.stringify(['side']));

// 无 base 风格（merge 非 --diff3 输出也认）
const plain = ['a', '<<<<<<< x', 'l1', '=======', 'r1', '>>>>>>> y', 'b'].join('\n');
hs = parseMergeHunks(plain);
check('无 base 一块', hs.length === 1 && hs[0].start === 1 && hs[0].end === 6);
check('无 base 内容', JSON.stringify(hs[0].ours) === '["l1"]' && JSON.stringify(hs[0].theirs) === '["r1"]');

// 干净文本：零块（Apply 放行条件）
check('干净零块', parseMergeHunks('a\nb\n').length === 0);
// 空边（删改冲突）：ours 为空数组，块仍在
const del = ['<<<<<<< ours', '||||||| base', 'gone', '=======', '>>>>>>> theirs'].join('\n');
hs = parseMergeHunks(del);
check('空边成块', hs.length === 1 && hs[0].ours.length === 0 && hs[0].theirs.length === 0);
// 多块 + 块间 clean 行不吞
const multi = ['<<<<<<< o', 'a', '=======', 'b', '>>>>>>> t', 'mid', '<<<<<<< o', 'c', '=======', 'd', '>>>>>>> t'].join('\n');
hs = parseMergeHunks(multi);
check('多块两块', hs.length === 2, `got ${hs.length}`);
check('多块第二块起点', hs[1]?.start === 6, `got ${hs[1]?.start}`);
// ≫ 模拟：splice 后重算块数减一（对 diff3 首块取 ours）
const d3 = parseMergeHunks(diff3);
const lines = diff3.split('\n');
const applied = [...lines.slice(0, d3[0].start), ...d3[0].ours, ...lines.slice(d3[0].end)].join('\n');
check('splice 方向', applied === ['same', 'main', 'tail'].join('\n'), JSON.stringify(applied));

// buildMergeView：无标记视图（PyCharm 中间格没有 <<<<<<< 行）
const view = buildMergeView(diff3);
check('视图无标记', !view.text.split('\n').some(l => /^(<{7}|\|{7}|={7}|>{7})/.test(l)), JSON.stringify(view.text));
check('视图初始为 ours', view.text === ['same', 'main', 'tail'].join('\n'), JSON.stringify(view.text));
check('视图块坐标', view.hunks.length === 1 && view.hunks[0].start === 1 && view.hunks[0].end === 2, JSON.stringify(view.hunks[0]));
const delView = buildMergeView(del);
// 非冲突块初始 = base（待应用，对齐 PyCharm 实测：Result 只有 base 行）
check('双空边视图为 base', delView.text === 'gone' && delView.hunks[0].start === 0 && delView.hunks[0].end === 1);
// 真 DU：两边都不进结果，初始 base = 'gone'（更改留在侧栏待应用）
const duOurs = ['<<<<<<< ours', 'mine', '||||||| base', 'gone', '=======', '>>>>>>> theirs'].join('\n');
const duTheirs = ['<<<<<<< ours', '||||||| base', 'gone', '=======', 'kept', '>>>>>>> theirs'].join('\n');
check('DU 初始为 base', buildMergeView(duOurs).text === 'gone');
const vt = buildMergeView(duTheirs);
check('UD 初始为 base', vt.text === 'gone' && vt.hunks[0].start === 0 && vt.hunks[0].end === 1, JSON.stringify(vt));
// diffRegion：块内敲字（old 5 行→new 6 行，第 3 行处 +1）
const r = diffRegion(['a', 'b', 'c', 'd', 'e'], ['a', 'b', 'X', 'c', 'd', 'e']);
check('对位起点', r.start === 2 && r.oldEnd === 2 && r.newEnd === 3, JSON.stringify(r));
// alignAdded：base 与现存边的 LCS，delta 标绿
check('parser 收 base', JSON.stringify(parseMergeHunks(diff3)[0].base) === JSON.stringify(['base']));
const duBase = ['# t', '', 'LazeCode', ''];
check(
  'DU 对齐',
  JSON.stringify(alignAdded(duBase, ['# t', '', 'LazeCode', '', '## dirty'])) === JSON.stringify([false, false, false, false, true])
);
check('全同全白', alignAdded(['a', 'b'], ['a', 'b']).every(v => !v));
check('全异全绿', alignAdded(['a'], ['x', 'y']).every(v => v));
// locateBlocks：块内容在侧栏文件里的连续定位（移动指针，重复行取前者之后）
check(
  '定位两块',
  JSON.stringify(locateBlocks(['x', 'a', 'b', 'y', 'a', 'b'], [['a', 'b'], ['a', 'b']])) === JSON.stringify([[1, 3], [4, 6]])
);
check('定位失败为 null', locateBlocks(['x'], [['q']])[0] === null);
check('空块不定位', locateBlocks(['x'], [[]])[0] === null);
// 录像现场：DU big block 整份文件 + base 对齐的 2 行新增（含一个空行）
const duBaseLines = ['# t', '', 'LazeCode', ''];
const duTheirsLines = ['# t', '', 'LazeCode', '', '## dirty', ''];
check(
  'DU 新增两行',
  alignAdded(duBaseLines, duTheirsLines).filter(Boolean).length === 2,
  JSON.stringify(alignAdded(duBaseLines, duTheirsLines))
);
check('DU 定位整份', JSON.stringify(locateBlocks(duTheirsLines, [duTheirsLines])[0]) === JSON.stringify([0, 6]));

// 尾部换行对齐：git merge-file 输出末尾为 >>>>>>> theirs\n 时，各非空边吸纳该空行
const eofMarked = [
  '<<<<<<< ours',
  '||||||| base',
  '# test-git-menu',
  '',
  'LazeCode Git 菜单功能测试项目。',
  '=======',
  '# test-git-menu',
  '',
  'LazeCode Git 菜单功能测试项目。',
  '',
  '## dirty-unstaged-line',
  '>>>>>>> theirs',
  ''
].join('\n');
const eofHunks = parseMergeHunks(eofMarked);
check('尾部块 base 包含末尾空行', eofHunks[0]?.base.length === 4, JSON.stringify(eofHunks[0]?.base));
check('尾部块 theirs 包含末尾空行', eofHunks[0]?.theirs.length === 6, JSON.stringify(eofHunks[0]?.theirs));
const eofView = buildMergeView(eofMarked);
check('尾部块视图范围覆盖 4 行', eofView.hunks[0]?.start === 0 && eofView.hunks[0]?.end === 4);
const eofTheirsFull = ['# test-git-menu', '', 'LazeCode Git 菜单功能测试项目。', '', '## dirty-unstaged-line', ''];
const eofLoc = locateBlocks(eofTheirsFull, [eofHunks[0].theirs]);
check('尾部块 theirs 完整定位 6 行', eofLoc[0]?.[0] === 0 && eofLoc[0]?.[1] === 6, JSON.stringify(eofLoc));
const eofKinds = sideChanges(eofHunks[0].base, eofTheirsFull.slice(eofLoc[0][0], eofLoc[0][1]));
check(
  '尾部块 1-4 行白，5-6 行绿',
  JSON.stringify(eofKinds) === JSON.stringify(['none', 'none', 'none', 'none', 'add', 'add']),
  JSON.stringify(eofKinds)
);

// sideChanges：侧栏片段 vs base 的逐行分类（add/mod/none —— 着色 + gutter 标记锚点）
check(
  'side 新增两行含空行',
  JSON.stringify(sideChanges(['# t', '', 'LazeCode'], ['# t', '', 'LazeCode', '', '## dirty'])) ===
    JSON.stringify(['none', 'none', 'none', 'add', 'add'])
);
check(
  'side 修改一行',
  JSON.stringify(sideChanges(['a', 'b'], ['a', 'B'])) === JSON.stringify(['none', 'mod'])
);
check('side 空侧全 none', sideChanges(['a', 'b'], []).length === 0);
check('side 中段追加', JSON.stringify(sideChanges(['a'], ['a', 'b', 'c'])) === JSON.stringify(['none', 'add', 'add']));
check(
  'side 中段交织新增与替换',
  JSON.stringify(sideChanges(['A', 'B', 'C', 'D', 'E'], ['A', 'new1', 'B', 'C', 'new2', 'E'])) ===
    JSON.stringify(['none', 'add', 'none', 'none', 'mod', 'none'])
);
check(
  'side 删除保持 none',
  JSON.stringify(sideChanges(['1', '2', '3', '4'], ['1', '4'])) ===
    JSON.stringify(['none', 'none'])
);
check(
  'side 纯新增全 add',
  JSON.stringify(sideChanges([], ['x', 'y'])) ===
    JSON.stringify(['add', 'add'])
);
// ws 归一：缺省 = 旧行为（none）；space 档消掉缩进差异
check('ws 缺省原样', normWsLine('  a b ') === '  a b ' && normWsLine('  a b ', 'none') === '  a b ');
check('ws space 折叠', normWsLine('\ta    b', 'space') === 'a b');
check('ws trim', normWsLine('  a b  ', 'trim') === 'a b');
check('ws 空行哨兵', normWsLine('   ', 'space-empty') === '\u0000' && normWsLine('', 'space-empty') === '\u0000');
// 空白档下 base 与缩进不同但内容相同的行对齐为 context
check('ws align 缩进差异不算新增', alignAdded(['a'], ['  a'], 'space').every(v => !v));
check('ws side 缩进差异不算 mod', JSON.stringify(sideChanges(['a'], ['  a'], 'space')) === JSON.stringify(['none']));

if (failures > 0) {
  console.error(`verify-merge-hunks: ${failures} 处失败`);
  process.exit(1);
}
console.log('verify-merge-hunks: ok');