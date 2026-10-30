#!/usr/bin/env node
// 校验后端产生的错误码在前端字典里都有对应文案。
//
// 为什么需要：错误码协议把文案从前端搬到了后端码 + 前端字典两处
// （见 src-tauri/src/error_proto.rs 与 docs/i18n.md）。后端新增一个码而字典
// 没跟上时，describeBackendError 会走兜底、原样显示 "ERR:xxx|..."。
// 那个退化在 UI 上只是一句奇怪的话，不会崩、也不会被类型系统发现 ——
// 只有把两侧的清单拉出来比对才能挡住。
//
// 做法：从 Rust 源码里抓所有 coded("...") 的字面量，与 en/zh-CN 两本字典的
// backendErrors key 集合比对（码是 snake_case，字典是 camelCase，需转换）。
//
// 用法：node --experimental-strip-types scripts/verify-error-codes.mjs

import { readFileSync, readdirSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');

/** 抓 Rust 源码里所有 `coded("xxx", ...)` / `coded_display("xxx", ...)` 的码。 */
function rustCodes() {
  // 不要写死文件清单：新模块（file_encoding/extensions/file_watch/lsp…）
  // 里的 coded(...) 会被漏掉，字典缺 key 也能过门禁。
  const walk = (dir) => readdirSync(join(root, dir), { withFileTypes: true }).flatMap(entry => {
    const rel = `${dir}/${entry.name}`;
    return entry.isDirectory() ? walk(rel) : entry.name.endsWith('.rs') ? [rel] : [];
  });
  const files = walk('src-tauri/src');
  const codes = new Set();
  // coded("code", ...) 或 coded_display("code", ...)
  const re = /\bcoded(?:_display)?\(\s*"([a-z0-9_]+)"/g;
  for (const f of files) {
    let src;
    try {
      src = readFileSync(join(root, f), 'utf8');
    } catch {
      continue;
    }
    for (const m of src.matchAll(re)) codes.add(m[1]);
  }
  return codes;
}

const snakeToCamel = (c) => c.replace(/_([a-z0-9])/g, (_, ch) => ch.toUpperCase());

const en = await import(`file://${join(root, 'src/services/i18n/en.ts')}`);
const zh = await import(`file://${join(root, 'src/services/i18n/zh-CN.ts')}`);

const rust = rustCodes();
const enKeys = new Set(Object.keys(en.en.backendErrors));
const zhKeys = new Set(Object.keys(zh.zhCN.backendErrors));

let failures = 0;

/** 后端有码、字典缺 key —— 用户会看到裸的 ERR: 前缀。 */
for (const code of [...rust].sort()) {
  const key = snakeToCamel(code);
  if (!enKeys.has(key)) {
    console.error(`FAIL 后端码 ${code}（key ${key}）在 en 字典里没有文案`);
    failures += 1;
  }
  if (!zhKeys.has(key)) {
    console.error(`FAIL 后端码 ${code}（key ${key}）在 zh-CN 字典里没有文案`);
    failures += 1;
  }
}

/** 字典有 key、后端不产生 —— 死 key，应删除（避免字典里堆永不出现的条目）。 */
for (const key of [...enKeys].sort()) {
  const code = key.replace(/([A-Z])/g, (_, c) => `_${c.toLowerCase()}`);
  if (!rust.has(code)) {
    console.error(`WARN 字典 key ${key}（码 ${code}）后端不产生，可能是死 key`);
  }
}

// 两语言 key 集合必须一致（tsc 的 LocaleDict 已保证，这里双保险并给出可读输出）
const onlyEn = [...enKeys].filter((k) => !zhKeys.has(k));
const onlyZh = [...zhKeys].filter((k) => !enKeys.has(k));
if (onlyEn.length) {
  console.error(`FAIL 只在 en 有的 key: ${onlyEn.join(', ')}`);
  failures += onlyEn.length;
}
if (onlyZh.length) {
  console.error(`FAIL 只在 zh-CN 有的 key: ${onlyZh.join(', ')}`);
  failures += onlyZh.length;
}

// 静态串不能为空
for (const [name, dict] of [['en', en.en], ['zh-CN', zh.zhCN]]) {
  for (const [k, v] of Object.entries(dict.backendErrors)) {
    if (typeof v === 'string' && v.trim() === '') {
      console.error(`FAIL ${name}.${k} 是空串`);
      failures += 1;
    }
  }
}

console.log(`ok 后端码 ${rust.size} 个 / 字典 key ${enKeys.size} 个，两语言对齐`);

if (failures > 0) {
  console.error(`\n${failures} 处不一致。新增后端错误码时同步改 src/services/i18n/{en,zh-CN}.ts 的 backendErrors。`);
  process.exit(1);
}
console.log('ok — 后端错误码与前端字典完全对齐');
