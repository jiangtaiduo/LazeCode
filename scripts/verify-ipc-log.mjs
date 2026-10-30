#!/usr/bin/env node
// 回归：logIpc 单条截断。对齐 .console-line 的 pre-wrap + break-all ——
// 未截断时 MB 级单行会直接冻结 WebKit 布局（问题⑥的 Console 页签路径）。
import { logIpc, getIpcLog } from '../src/services/ipcLog.ts';

let failures = 0;
const assert = (cond, msg) => {
  if (cond) console.log(`  ok  ${msg}`);
  else {
    failures += 1;
    console.error(`FAIL  ${msg}`);
  }
};

const before = getIpcLog().length;
const huge = 'x'.repeat(100_000);
logIpc(huge, 'error');
const after = getIpcLog();
const mine = after[after.length - 1];
assert(after.length === before + 1, '日志条目正常追加');
assert(mine.text.length <= 4100, `超长条目被截断（实际 ${mine.text.length} 字）`);
assert(mine.text.endsWith('(已截断)'), '截断有明确标记');
assert(mine.level === 'error', '截断不改级别');

logIpc('短文本');
const tail = getIpcLog()[getIpcLog().length - 1];
assert(tail.text === '短文本', '普通条目原文保留');

if (failures) {
  console.error(`\n${failures} 项失败`);
  process.exit(1);
}
console.log('\n全部通过');
