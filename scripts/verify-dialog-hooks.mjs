#!/usr/bin/env node
// 对话框 hooks 顺序门禁：`if (!open) return null` 之后不许再调任何 hook。
//
// 为什么要这个脚本：React error #310（Rendered more hooks than during the
// previous render）—— open 翻转时前后两次渲染跑的 hook 数量不一样，整页直接
// 崩进错误边界（2026-10-14：ConflictDialog 的 groups useMemo 写在 early
// return 后面，点"解决冲突"即整页红屏）。tsc 和 eslint 都拦不住，只能扫文本。
//
// 规则：凡含 `if (!open) return null` 的 tsx，其后不许出现 useXxx( 调用。
// 回调内部的 `return null`（如 useMemo 里分组为空时）不算 —— 只认顶格的
// `if (!open) return null` 这一行。

import { readFileSync, readdirSync, statSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const srcDir = join(root, 'src');

function* walk(dir) {
  for (const e of readdirSync(dir)) {
    const p = join(dir, e);
    const st = statSync(p);
    if (st.isDirectory()) yield* walk(p);
    else if (/\.tsx$/.test(e)) yield p;
  }
}

let failures = 0;
for (const file of walk(srcDir)) {
  const lines = readFileSync(file, 'utf8').split('\n');
  const gateIdx = lines.findIndex(l => /^\s*if\s*\(!open\)\s*return null;/.test(l));
  if (gateIdx < 0) continue;
  lines.forEach((line, i) => {
    if (i <= gateIdx) return;
    if (/^\s*(\/\/|\*)/.test(line)) return;
    const m = line.match(/\buse[A-Z]\w*\s*\(/);
    if (m) {
      console.error(`FAIL ${file}:${i + 1} hook ${m[0]} 在 if (!open) return null 之后（React #310）`);
      failures += 1;
    }
  });
}
if (failures > 0) {
  console.error(`verify-dialog-hooks: ${failures} 处违规`);
  process.exit(1);
}
console.log('verify-dialog-hooks: ok');
