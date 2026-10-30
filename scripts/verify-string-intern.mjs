#!/usr/bin/env node
// 字符串驻留 + 探针快路径验证（运行: node scripts/verify-string-intern.mjs）。
//
// 覆盖：intern 同值同实例/空串/上限清空、internFileTree 就地驻留与引用保持、
// probeFastPath 三态（未变/变了/没了）与基线未知退化。

import {
  intern,
  internPoolSize,
  clearInternPoolForTest,
  internFileTree,
} from '../src/services/stringIntern.ts';
import { probeFastPath } from '../src/services/fileProbe.ts';

let failures = 0;
const assert = (cond, msg, extra) => {
  if (cond) console.log(`  ok  ${msg}`);
  else {
    failures += 1;
    console.error(`FAIL  ${msg}${extra !== undefined ? `（实际 ${JSON.stringify(extra)}）` : ''}`);
  }
};

// ---- intern ----
clearInternPoolForTest();
assert(intern('') === '', '空串原样返回');
const a = intern('/repo/src/App.tsx');
const b = intern('/repo/src/App.tsx');
assert(a === b, '同值返回同一实例（===）');
assert(internPoolSize() === 1, '同值只占一个表项', internPoolSize());
intern('/repo/other.ts');
assert(internPoolSize() === 2, '异值各占表项');

// ---- internFileTree ----
clearInternPoolForTest();
const tree = [
  { path: '/repo/src', name: 'src', children: [{ path: '/repo/src/App.tsx', name: 'App.tsx' }] },
];
const same = internFileTree(tree);
assert(same === tree, '返回同一引用（不复制树）');
assert(tree[0].children[0].path === intern('/repo/src/App.tsx'), '子节点路径已驻留');
const tree2 = [{ path: '/repo/src/App.tsx', name: 'App.tsx' }];
internFileTree(tree2);
assert(tree2[0].path === tree[0].children[0].path, '跨树同路径共享同一实例');

// ---- probeFastPath ----
const buf = { content: 'hi', diskHash: 'h-2', dirty: false, externalChanged: false, diskLen: 2, diskMtimeMs: 1000 };
assert(
  probeFastPath(buf, { exists: true, isFile: true, len: 2, mtimeMs: 1000 }) === 'unchanged',
  '三元组一致 → unchanged（零读盘）'
);
assert(
  probeFastPath(buf, { exists: true, isFile: true, len: 3, mtimeMs: 1000 }) === 'changed',
  '长度变 → changed（走全量读）'
);
assert(
  probeFastPath(buf, { exists: true, isFile: true, len: 2, mtimeMs: 1001 }) === 'changed',
  'mtime 变 → changed'
);
assert(
  probeFastPath(buf, { exists: true, isFile: true, len: 2, mtimeMs: null }) === 'changed',
  'mtime 未知 → changed（不能拿未知当未变）'
);
assert(
  probeFastPath(buf, { exists: false, isFile: false, len: 0, mtimeMs: null }) === 'missing',
  '不存在 → missing（走删除分支）'
);
const unknown = { content: 'hi', diskHash: '', dirty: false, externalChanged: false };
assert(
  probeFastPath(unknown, { exists: true, isFile: true, len: 2, mtimeMs: 1000 }) === 'changed',
  '基线未知（旧会话恢复）→ changed，退化为全量读'
);

if (failures > 0) {
  console.error(`\n${failures} 处失败`);
  process.exit(1);
}
console.log('ok 字符串驻留与探针快路径验证通过');
