#!/usr/bin/env node
// 命令目录覆盖门禁（对齐 Zed「命令先有数据定义」的思想）。
//
// 挡两件事：
// 1. `COMMAND_CATALOG`（src/services/commandCatalog.ts）必须与 `TauriCommand`
//    联合类型 1:1 —— Rust 增删命令后只改一处，另一处会红。
// 2. 每条命令必须在 `simulateCommand`（src/services/tauriBridge.ts）里有显式
//    `case` 分支。`default: {} as T` 会静默吞掉新命令：浏览器预览里调一个
//    没 mock 的命令，调用方拿到 `{}` 继续跑，把"后端缺实现"伪装成"正常工作"。
//    显式分支可以是保守形状（`[]` / `{}` / `''`，与今天 default 行为一致），
//    但必须是作者有意选的 —— 新命令的分支形状不对，预览里立刻现形。
//
// 用法：node scripts/verify-command-catalog.mjs（已接入 npm run check）

import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');

const unionSrc = readFileSync(join(root, 'src/services/tauriCommands.ts'), 'utf8');
const union = [...unionSrc.matchAll(/'([a-z_0-9]+)'/g)].map((m) => m[1]).sort();

const catalogSrc = readFileSync(join(root, 'src/services/commandCatalog.ts'), 'utf8');
const catalog = [...catalogSrc.matchAll(/^\s{2}([a-z_0-9]+): \{/gm)].map((m) => m[1]).sort();

const bridgeSrc = readFileSync(join(root, 'src/services/tauriBridge.ts'), 'utf8')
  // 去掉注释，避免 `// case 'x':` 这种占位让门禁误判为已实现
  .replace(/\/\*[\s\S]*?\*\//g, '')
  .replace(/^\s*\/\/.*$/gm, '');
const cases = [...bridgeSrc.matchAll(/case '([a-z_0-9]+)'/g)].map((m) => m[1]);

let failures = 0;

for (const c of union.filter((c) => !catalog.includes(c))) {
  console.error(`FAIL 命令 '${c}' 在 COMMAND_CATALOG 里没有条目（标题/分类/风险补上）`);
  failures += 1;
}
for (const c of catalog.filter((c) => !union.includes(c))) {
  console.error(`FAIL CATALOG 条目 '${c}' 在 TauriCommand 里不存在（命令已删除？）`);
  failures += 1;
}
for (const c of union.filter((c) => !cases.includes(c))) {
  console.error(`FAIL 命令 '${c}' 在 simulateCommand 里没有 case 分支（浏览器预览会静默拿到 {}）`);
  failures += 1;
}

if (failures > 0) process.exit(1);
console.log(`ok 命令目录 ${catalog.length} 条与联合类型对齐，simulateCommand 全覆盖`);
