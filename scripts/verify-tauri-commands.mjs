#!/usr/bin/env node
// Tauri 命令名同步校验：Rust `#[tauri::command]` 与前端 `TauriCommand` 联合类型对齐。
//
// 为什么需要：`invokeCommand(cmd: string)` 里命令名拼错（`'git_ftech'`）
// 要到运行时点那条路才炸，tsc 看不见。联合类型让拼写错误变成编译错误。
//
// 做法：扫描 src-tauri/src/*.rs 里 `#[tauri::command]` 后紧跟的 `fn <name>`，
// 与 src/services/tauriCommands.ts 里的联合成员比对。两边差一个即 FAIL。
//
// 用法：
//   node scripts/verify-tauri-commands.mjs          # 只校验（npm run check 用）
//   node scripts/verify-tauri-commands.mjs --write  # 以 Rust 为准重写前端联合类型

import { readdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const outFile = join(root, 'src/services/tauriCommands.ts');

/** 扫出所有 #[tauri::command] 定义的命令名。 */
function rustCommands() {
  const dir = join(root, 'src-tauri/src');
  const names = new Set();
  // 属性与 fn 可能隔着 doc 注释/空行：逐行扫，见到属性后向下找第一个 `fn name`
  const fnRe = /^\s*pub\s+(?:async\s+)?fn\s+([a-z_0-9]+)\s*[<(]/;
  for (const f of readdirSync(dir)) {
    if (!f.endsWith('.rs')) continue;
    const lines = readFileSync(join(dir, f), 'utf8').split('\n');
    let armed = false;
    for (const line of lines) {
      if (line.includes('#[tauri::command]')) {
        armed = true;
        continue;
      }
      if (!armed) continue;
      const m = line.match(fnRe);
      if (m) {
        names.add(m[1]);
        armed = false;
      } else if (line.trim() !== '' && !line.trim().startsWith('///') && !line.trim().startsWith('//') && !line.trim().startsWith('#[')) {
        // 属性与 fn 之间只允许注释/空行/其它属性，其它内容说明跟丢了，复位
        armed = false;
      }
    }
  }
  return [...names].sort();
}

/** 从前端联合类型文件里抓成员。 */
function frontendCommands() {
  const src = readFileSync(outFile, 'utf8');
  return [...src.matchAll(/'([a-z_0-9]+)'/g)].map((m) => m[1]).sort();
}

function render(commands) {
  const members = commands.map((c) => `  | '${c}'`).join('\n');
  return `// 由 scripts/verify-tauri-commands.mjs --write 生成，勿手改。
// 源：src-tauri/src/*.rs 里全部 \`#[tauri::command]\`。
// Rust 侧新增/删除命令后跑 \`--write\` 重生，CI 用默认模式校验两者对齐。
export type TauriCommand =
${members};
`;
}

const rust = rustCommands();

if (process.argv.includes('--write')) {
  writeFileSync(outFile, render(rust));
  console.log(`ok 写入 ${outFile}，共 ${rust.length} 个命令`);
  process.exit(0);
}

let frontend;
try {
  frontend = frontendCommands();
} catch {
  console.error(`FAIL 读不到 ${outFile}，先跑一次 --write 生成`);
  process.exit(1);
}

// 声明了命令却忘了加进 generate_handler!，编译能过、运行时才报
// "command not found"。在同一个门禁里补上这一列。
const libSrc = readFileSync(join(root, 'src-tauri/src/lib.rs'), 'utf8');
const handlerStart = libSrc.indexOf('generate_handler![');
const handlerEnd = libSrc.indexOf('])', handlerStart);
const handlerSrc = handlerStart >= 0 && handlerEnd > handlerStart
  ? libSrc.slice(handlerStart, handlerEnd)
  : '';
const registered = new Set(
  handlerSrc
    .split('\n')
    .map(line => line.match(/([a-z_0-9]+)\s*,?\s*$/)?.[1])
    .filter(Boolean)
);

let failures = 0;
for (const c of rust.filter((c) => !registered.has(c))) {
  console.error(`FAIL Rust 命令 '${c}' 未注册进 generate_handler!`);
  failures += 1;
}
for (const c of rust.filter((c) => !frontend.includes(c))) {
  console.error(`FAIL Rust 命令 '${c}' 在前端 TauriCommand 里没有成员（跑 --write 同步）`);
  failures += 1;
}
for (const c of frontend.filter((c) => !rust.includes(c))) {
  console.error(`FAIL 前端成员 '${c}' 在 Rust 侧不存在（命令已删除？跑 --write 同步）`);
  failures += 1;
}
if (failures > 0) process.exit(1);
console.log(`ok Tauri 命令 ${rust.length} 个，前后端对齐`);
