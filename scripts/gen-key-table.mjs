#!/usr/bin/env node
// 从 vendored Ghostty 头文件生成前端键值表。
//
// 手写枚举数字极易出错，且症状是「按键无反应」：本项目曾因
// MINUS/PERIOD/QUOTE/SEMICOLON/SLASH 各偏 1、F2..F24 与整组 JIS 键缺失
// 而丢掉输入。这里以 event.h 为唯一来源，生成 keyValues.generated.ts。
//
// 用法：
//   node scripts/gen-key-table.mjs           # 重新生成
//   node scripts/gen-key-table.mjs --check   # 只校验是否漂移（CI）

import { readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const HEADER = resolve(
  ROOT,
  'src-tauri/vendor/ghostty-vt/include/ghostty/vt/key/event.h'
);
const OUTPUT = resolve(ROOT, 'src/ghostty-vt/keyValues.generated.ts');

/** 解析 `GhosttyKey` 枚举（隐式递增取值，跳过 MAX_VALUE 哨兵）。 */
function parseKeyEnum(header) {
  // 头文件里有多个同名 typedef（GhosttyKeyAction / GhosttyKey）。先定位
  // `} GhosttyKey;` 的结尾，再向前找最近的枚举起点，避免正则跨过前一个枚举。
  const end = header.indexOf('} GhosttyKey;');
  if (end < 0) {
    throw new Error('未在 event.h 中找到 GhosttyKey 枚举');
  }
  const start = header.lastIndexOf('typedef enum GHOSTTY_ENUM_TYPED', end);
  const brace = header.indexOf('{', start);
  if (start < 0 || brace < 0 || brace > end) {
    throw new Error('GhosttyKey 枚举起点解析失败');
  }

  // 去掉块注释与行注释后再按逗号切分：枚举项本身不含逗号表达式。
  const body = header
    .slice(brace + 1, end)
    .replace(/\/\*[\s\S]*?\*\//g, '')
    .replace(/\/\/[^\n]*/g, '');

  const entries = [];
  const seen = new Set();
  let next = 0;

  for (const raw of body.split(',')) {
    const item = raw.trim();
    if (!item) continue;

    const match = /^([A-Z0-9_]+)\s*(?:=\s*(.+))?$/.exec(item);
    if (!match) {
      throw new Error(`无法解析枚举项: ${item}`);
    }
    const [, cName, expr] = match;

    // 哨兵项不进入前端表。
    if (cName === 'GHOSTTY_KEY_MAX_VALUE') continue;

    if (expr !== undefined) {
      const value = expr.trim();
      if (value === 'GHOSTTY_ENUM_MAX_VALUE') continue;
      if (!/^\d+$/.test(value)) {
        throw new Error(`不支持的枚举表达式: ${cName} = ${value}`);
      }
      next = Number(value);
    }

    const prefix = 'GHOSTTY_KEY_';
    if (!cName.startsWith(prefix)) {
      throw new Error(`枚举项缺少 ${prefix} 前缀: ${cName}`);
    }
    const name = cName.slice(prefix.length);
    if (seen.has(name)) {
      throw new Error(`枚举项重名: ${name}`);
    }
    seen.add(name);
    entries.push([name, next]);
    next += 1;
  }

  if (entries.length === 0) {
    throw new Error('GhosttyKey 枚举为空');
  }
  return entries;
}

/** 按值排序后渲染 TypeScript 源文件（输出必须稳定，供 --check 比对）。 */
function render(entries) {
  const sorted = [...entries].sort((a, b) => a[1] - b[1] || a[0].localeCompare(b[0]));
  return [
    '// 由 scripts/gen-key-table.mjs 从 vendored 头文件生成，请勿手改。',
    '// 源：src-tauri/vendor/ghostty-vt/include/ghostty/vt/key/event.h（GhosttyKey 枚举）',
    '// 重新生成：npm run gen:key-table',
    '',
    'export const GHOSTTY_KEY_VALUES = {',
    ...sorted.map(([name, value]) => `  ${name}: ${value},`),
    '} as const;',
    ''
  ].join('\n');
}

const entries = parseKeyEnum(readFileSync(HEADER, 'utf8'));
const expected = render(entries);
const check = process.argv.includes('--check');

if (check) {
  // 按要求先生成到临时文件，再与仓库内文件逐字节比对。
  const tmp = resolve(tmpdir(), `keyValues.generated.${process.pid}.ts`);
  writeFileSync(tmp, expected);
  let actual = null;
  try {
    actual = readFileSync(OUTPUT, 'utf8');
  } catch {
    actual = null;
  } finally {
    rmSync(tmp, { force: true });
  }
  if (actual !== expected) {
    console.error(
      `${relative(ROOT, OUTPUT)} 与 event.h 不一致；请运行 npm run gen:key-table 后重新提交`
    );
    process.exit(1);
  }
  console.log(`键值表与 event.h 一致（${entries.length} 项）`);
} else {
  writeFileSync(OUTPUT, expected);
  console.log(`已生成 ${entries.length} 项键值 -> ${relative(ROOT, OUTPUT)}`);
}
