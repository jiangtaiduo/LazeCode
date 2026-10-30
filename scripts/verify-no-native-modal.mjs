#!/usr/bin/env node
// 禁止 window.prompt / window.confirm / alert 回潮。
//
// 为什么要这个脚本：wry 0.55.1 的 WKUIDelegate 没有实现任何 JS 面板
//（无 runJavaScriptAlertPanel / ConfirmPanel / TextInputPanel），
// prompt/confirm 在 webview 里直接返回 null、连框都不弹 —— 凡走它们的菜单项
// 点下去都是"没有任何反应"，且静默 return、无 toast、无日志，极难排查。
// 文本输入一律走 TextPromptDialog（App.appPrompt bridge），确认走
// @tauri-apps/plugin-dialog。本脚本把回潮变成 CI 里的一行失败。

import { readFileSync, readdirSync, statSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const srcDir = join(root, 'src');

const BANNED = [
  /(?<![\w$.])window\.prompt\s*\(/,
  /(?<![\w$.])window\.confirm\s*\(/,
  /(?<![\w$.])window\.alert\s*\(/,
  /from\s+['"]\.\/services\/nativeModal['"]|from\s+['"]\.\.\/services\/nativeModal['"]/,
];

function* walk(dir) {
  for (const e of readdirSync(dir)) {
    const p = join(dir, e);
    const st = statSync(p);
    if (st.isDirectory()) yield* walk(p);
    else if (/\.(ts|tsx)$/.test(e)) yield p;
  }
}

let failures = 0;
for (const file of walk(srcDir)) {
  const lines = readFileSync(file, 'utf8').split('\n');
  lines.forEach((line, i) => {
    // 注释行不算（审计文档里会提到这些名字）
    const code = line.replace(/^\s*(\/\/|\*)/, '');
    if (/^\s*(\/\/|\*)/.test(line)) return;
    for (const re of BANNED) {
      if (re.test(code)) {
        console.error(`FAIL ${file}:${i + 1}: ${line.trim()}（JS 原生面板在 wry 里无实现，见 TextPromptDialog 头注）`);
        failures += 1;
      }
    }
  });
}

if (failures > 0) {
  console.error(`verify-no-native-modal: ${failures} 处违规`);
  process.exit(1);
}
console.log('verify-no-native-modal: ok（src 内无 window.prompt/confirm/alert、无 nativeModal 引用）');
