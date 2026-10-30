#!/usr/bin/env node
// 校验 Rust `MenuLabels` 与前端字典的 `menu` 子树逐项一致。
//
// 为什么要这个脚本：菜单文案被复制了两份 —— 前端字典（ui 文案）与 Rust 常量
// 表（原生菜单，构造时同步进行、拿不到前端字典）。两份必然漂移，而漂移的后果
// 是「菜单某一项变成空串/旧语言」这类只在运行时可发现的问题。
//
// 本脚本把漂移变成 CI 里的一行失败。新增/修改菜单文案时同步两边即可。
//
// 用法：node --experimental-strip-types scripts/verify-menu-parity.mjs
//       （node 22.6+；本项目 `verify:wire` 已用同一机制直跑 TS）

import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');

// Rust 的 `pub const EN|ZH_CN: MenuLabels = MenuLabels { key: "value", ... };`
const RUST_TABLE_RE = /pub const (EN|ZH_CN): MenuLabels\s*=\s*MenuLabels\s*\{([^}]*)\};/g;
const RUST_ENTRY_RE = /(\w+):\s*"((?:[^"\\]|\\.)*)"/g;

/** 读 Rust 两张表。返回 { EN: {...}, ZH_CN: {...} }，键保持 snake_case。 */
function readRustTables() {
  const src = readFileSync(join(root, 'src-tauri/src/menu.rs'), 'utf8');
  const tables = {};
  for (const m of src.matchAll(RUST_TABLE_RE)) {
    const table = {};
    for (const e of m[2].matchAll(RUST_ENTRY_RE)) {
      table[e[1]] = e[2].replace(/\\"/g, '"');
    }
    tables[m[1]] = table;
  }
  return tables;
}

/** snake_case -> camelCase（Rust 键 -> TS 键）。 */
const toCamel = (k) => k.replace(/_([a-z])/g, (_, c) => c.toUpperCase());
/** camelCase -> snake_case（TS 键 -> Rust 键）。 */
const toSnake = (k) => k.replace(/([A-Z])/g, (_, c) => `_${c.toLowerCase()}`);

const en = await import(`file://${join(root, 'src/services/i18n/en.ts')}`);
const zhCN = await import(`file://${join(root, 'src/services/i18n/zh-CN.ts')}`);

const rustTables = readRustTables();
let failures = 0;

/** 单侧失败的记录。 */
function fail(msg) {
  console.error(`FAIL ${msg}`);
  failures += 1;
}

for (const [name, dict] of [
  ['EN', en.en],
  ['ZH_CN', zhCN.zhCN],
]) {
  const rust = rustTables[name];
  if (!rust) {
    fail(`${name}: 在 src-tauri/src/menu.rs 里找不到 ${name} 表`);
    continue;
  }
  const tsMenu = dict.menu;

  // 1. 前端字典里每一项，Rust 必须有且值相同。
  for (const [key, value] of Object.entries(tsMenu)) {
    const rk = toSnake(key);
    const rv = rust[rk];
    if (rv === undefined) {
      fail(`${name}.${key}：Rust 表缺字段 ${rk}（TS 有="${value}"）`);
      continue;
    }
    if (rv !== value) {
      fail(`${name}.${key}：两边不一致\n      Rust = "${rv}"\n      TS   = "${value}"`);
    }
  }

  // 2. Rust 表里每一项，前端字典必须有。防「改了 Rust 忘了改 TS」。
  for (const rk of Object.keys(rust)) {
    if (tsMenu[toCamel(rk)] === undefined) {
      fail(`${name}.${toCamel(rk)}：前端字典缺字段（Rust 有="${rust[rk]}"）`);
    }
  }

  // 3. 空串会让菜单项渲染成一条空白，用户看到"少了一项"。
  for (const [rk, rv] of Object.entries(rust)) {
    if (rv.trim() === '') fail(`${name}.${rk}：Rust 文案是空串`);
  }

  const n = Object.keys(tsMenu).length;
  console.log(`${failures === 0 ? 'ok' : '...'} ${name}: ${n} 项已比对`);
}

if (failures > 0) {
  console.error(`\n${failures} 处不一致。菜单文案有两份真相（前端字典 + Rust 常量表），`);
  console.error('请同步修改 src/services/i18n/{en,zh-CN}.ts 与 src-tauri/src/menu.rs。');
  process.exit(1);
}
console.log('\nok — MenuLabels 与前端字典两语言逐项一致');
