#!/usr/bin/env node
// 版本号单源检查：Cargo.toml 是唯一源，package.json 与 tauri.conf.json
// 必须与之对齐，前端经 vite define 注入（见 services/appVersion.ts）。
//
// 背景：三处曾漂移（package.json 1.0.0 vs Cargo/tauri.conf 0.1.0），
// 关于页显示的版本与实际产物对不上，bug 报告无法定位版本。
//
// 用法：node scripts/sync-version.mjs（只校验不对写，避免静默改版本）

import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');

const cargo = readFileSync(join(root, 'src-tauri/Cargo.toml'), 'utf8').match(
  /^version\s*=\s*"([^"]+)"/m
)?.[1];
const pkg = JSON.parse(readFileSync(join(root, 'package.json'), 'utf8')).version;
const conf = JSON.parse(readFileSync(join(root, 'src-tauri/tauri.conf.json'), 'utf8')).version;

let failures = 0;
const check = (name, got, want) => {
  if (got !== want) {
    console.error(`FAIL ${name} 版本 ${got} 与 Cargo.toml ${want} 不一致`);
    failures += 1;
  }
};

if (!cargo) {
  console.error('FAIL Cargo.toml 里没有读到 version');
  process.exit(1);
}
check('package.json', pkg, cargo);
check('tauri.conf.json', conf, cargo);

if (failures > 0) process.exit(1);
console.log(`ok 版本号单源对齐：${cargo}`);
