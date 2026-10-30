#!/usr/bin/env node
// 终端解析基准：原生 libghostty-vt vs @xterm/headless。
//
// 两者解析**同一份字节流**（由 Rust 侧 engine_bench_stream 生成并落盘，
// 本脚本用 FNV-1a 校验），在相同的 120x40 网格下计时。
//
// 三个口径，避免把测试框架开销误当成解析性能：
//   1. 整块写入：不含分块调度开销。
//   2. 8KB 批量入队：模拟 PTY 按 8KB 到达、调用方连续 write 后统一等待，
//      是 xterm.js 在真实应用中的常见用法。
//   3. 8KB 逐块 await：每次 write 都等待回调，会引入每个分块一次事件循环
//      调度；这不是解析能力，仅作为反例留在 JSON 里。
//
// 只比较解析器，不含渲染器与 IPC —— 这不是端到端 UI 基准。
//
// 用法：npm run bench:terminal

import { spawnSync } from 'node:child_process';
import { createRequire } from 'node:module';
import { readFileSync, writeFileSync, mkdirSync, existsSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import os from 'node:os';

const require = createRequire(import.meta.url);
const { Terminal } = require('@xterm/headless');
const xtermVersion = require('@xterm/headless/package.json').version;

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const TAURI_DIR = join(ROOT, 'src-tauri');
const BENCH_DIR = join(TAURI_DIR, 'target', 'bench');
const STREAM_PATH = join(BENCH_DIR, 'terminal-stream.bin');
const RESULT_PATH = join(BENCH_DIR, 'terminal-stack.json');

const COLS = 120;
const ROWS = 40;
const CHUNK = 8 * 1024;
const WARMUP_CHUNKS = 64;
const SCROLLBACK = 20_000;
const LINES = 120_000;

const MB = 1024 * 1024;
const EMPTY = new Uint8Array(0);

function fnv1a32(buffer) {
  let hash = 0x811c9dc5;
  for (let i = 0; i < buffer.length; i++) {
    hash ^= buffer[i];
    hash = Math.imul(hash, 0x01000193) >>> 0;
  }
  return hash.toString(16).padStart(8, '0');
}

function assert(condition, message) {
  if (!condition) throw new Error(message);
}

function run(command, args, options = {}) {
  const result = spawnSync(command, args, {
    cwd: options.cwd ?? ROOT,
    stdio: options.stdio ?? 'inherit',
    encoding: 'utf8',
    env: { ...process.env, ...(options.env ?? {}) },
  });
  if (result.status !== 0) {
    throw new Error(`${command} ${args.join(' ')} 退出码 ${result.status}`);
  }
  return result;
}

function buildNativeExample() {
  console.log('构建原生基准（release）…');
  run('cargo', ['build', '--release', '--example', 'bench_terminal'], {
    cwd: TAURI_DIR,
    stdio: ['ignore', 'inherit', 'inherit'],
  });
  const bin = join(TAURI_DIR, 'target', 'release', 'examples', 'bench_terminal');
  assert(existsSync(bin), `找不到基准二进制：${bin}`);
  return bin;
}

function runNativeBench(bin) {
  mkdirSync(BENCH_DIR, { recursive: true });
  const result = run(bin, [STREAM_PATH], {
    stdio: ['ignore', 'pipe', 'inherit'],
    env: { BENCH_LINES: String(LINES) },
  });
  const line = result.stdout.trim().split('\n').filter(Boolean).at(-1);
  assert(line, '原生基准没有输出 JSON');
  return JSON.parse(line);
}

function writeAsync(term, data) {
  return new Promise((resolvePromise, reject) => {
    const timer = setTimeout(() => reject(new Error('xterm.write 回调超时')), 30_000);
    term.write(data, () => {
      clearTimeout(timer);
      resolvePromise();
    });
  });
}

function newXterm() {
  return new Terminal({
    cols: COLS,
    rows: ROWS,
    scrollback: SCROLLBACK,
    convertEol: false,
  });
}

function chunksOf(payload, size) {
  const chunks = [];
  for (let offset = 0; offset < payload.length; offset += size) {
    chunks.push(payload.subarray(offset, offset + size));
  }
  return chunks;
}

async function warmup(term, chunks) {
  for (const chunk of chunks.slice(0, WARMUP_CHUNKS)) {
    await writeAsync(term, chunk);
  }
}

/**
 * 整块写入：一次 write 覆盖全部负载，不含分块调度开销。
 */
async function measureBulk(payload) {
  const term = newXterm();
  await warmup(term, chunksOf(payload, CHUNK));

  const started = process.hrtime.bigint();
  await writeAsync(term, payload);
  const elapsedSecs = Number(process.hrtime.bigint() - started) / 1e9;

  term.dispose();
  return { elapsedMs: elapsedSecs * 1000, mbPerS: payload.length / MB / elapsedSecs };
}

/**
 * 8KB 批量入队：连续 write 后只等一次队列排空。
 * 调用方不会因为每块都 await 而让出事件循环，xterm 的写缓冲可以合并处理。
 */
async function measureChunked(payload) {
  const term = newXterm();
  const chunks = chunksOf(payload, CHUNK);
  await warmup(term, chunks);

  const started = process.hrtime.bigint();
  for (const chunk of chunks) term.write(chunk);
  await writeAsync(term, EMPTY);
  const elapsedSecs = Number(process.hrtime.bigint() - started) / 1e9;
  term.dispose();

  return {
    elapsedMs: elapsedSecs * 1000,
    mbPerS: payload.length / MB / elapsedSecs,
    chunks: chunks.length,
  };
}

/**
 * 8KB 逐块 await：反例口径。每个分块都等一次回调，会强制走一次事件循环
 * 调度，数字反映的是调度开销而不是解析速度。
 */
async function measureSequential(payload) {
  const term = newXterm();
  const chunks = chunksOf(payload, CHUNK);
  await warmup(term, chunks);

  const started = process.hrtime.bigint();
  for (const chunk of chunks) await writeAsync(term, chunk);
  const elapsedSecs = Number(process.hrtime.bigint() - started) / 1e9;
  term.dispose();

  return {
    elapsedMs: elapsedSecs * 1000,
    mbPerS: payload.length / MB / elapsedSecs,
    chunks: chunks.length,
  };
}

async function repeat(fn, times = 3) {
  const runs = [];
  for (let index = 0; index < times; index++) runs.push(await fn());
  runs.sort((a, b) => a.mbPerS - b.mbPerS);
  const median = runs[Math.floor(runs.length / 2)];
  return { ...median, runsMbPerS: runs.map((run) => run.mbPerS) };
}

function formatNumber(value, digits = 2) {
  return Number(value).toFixed(digits);
}

async function main() {
  const bin = buildNativeExample();
  const native = runNativeBench(bin);

  const payload = readFileSync(STREAM_PATH);
  assert(payload.length === native.payload.bytes, '原生与 JS 的负载长度不一致');
  assert(fnv1a32(payload) === native.payload.fnv1a32, '原生与 JS 的负载哈希不一致');

  console.log(`负载：${formatNumber(payload.length / MB)} MB / ${native.payload.lines} 行 / fnv1a32=${native.payload.fnv1a32}`);

  const xtermChunked = await repeat(() => measureChunked(payload));
  const xtermBulk = await repeat(() => measureBulk(payload));
  const xtermSequential = await measureSequential(payload);

  const summary = {
    generatedAt: new Date().toISOString(),
    environment: {
      platform: `${os.platform()} ${os.release()} ${os.arch()}`,
      cpu: os.cpus()[0]?.model ?? 'unknown',
      node: process.version,
      xterm: xtermVersion,
      rustc: spawnSync('rustc', ['--version'], { encoding: 'utf8' }).stdout?.trim() ?? null,
    },
    payload: native.payload,
    config: {
      cols: COLS,
      rows: ROWS,
      chunkBytes: CHUNK,
      warmupChunks: WARMUP_CHUNKS,
      scrollback: SCROLLBACK,
      lines: LINES,
    },
    native: {
      parseChunked: native.parse,
      parseBulk: native.parseBulk,
      pipeline: native.pipeline,
      snapshot: native.snapshot,
    },
    xterm: {
      parseChunked: xtermChunked,
      parseBulk: xtermBulk,
      parseSequential: xtermSequential,
    },
    ratios: {
      chunked: native.parse.mb_per_s / xtermChunked.mbPerS,
      bulk: native.parseBulk.mb_per_s / xtermBulk.mbPerS,
    },
    note: '只比较解析器；xterm 不含 WebGL/DOM 渲染，原生不含 IPC 与 Canvas 绘制。逐块 await 口径是调度开销反例，不用于对比。',
  };

  mkdirSync(BENCH_DIR, { recursive: true });
  writeFileSync(RESULT_PATH, `${JSON.stringify(summary, null, 2)}\n`);

  console.log('');
  console.log('终端解析基准（同一份字节流，120x40）');
  console.log('─'.repeat(70));
  console.log('整块写入（无分块开销）');
  console.log(`  原生 libghostty-vt : ${formatNumber(native.parseBulk.mb_per_s, 1)} MB/s`);
  console.log(`  xterm.js headless  : ${formatNumber(xtermBulk.mbPerS, 1)} MB/s`);
  console.log(`  倍数               : ${formatNumber(summary.ratios.bulk)}x`);
  console.log('');
  console.log('8KB 批量入队（模拟 PTY 分块到达）');
  console.log(`  原生 libghostty-vt : ${formatNumber(native.parse.mb_per_s, 1)} MB/s`);
  console.log(`  xterm.js headless  : ${formatNumber(xtermChunked.mbPerS, 1)} MB/s`);
  console.log(`  倍数               : ${formatNumber(summary.ratios.chunked)}x`);
  console.log('');
  console.log(`原生解析+增量帧     : ${formatNumber(native.pipeline.mb_per_s, 1)} MB/s，${native.pipeline.frames} 帧，共 ${formatNumber(native.pipeline.diff_bytes / MB)} MB diff`);
  console.log(`原生快照            : ${formatNumber(native.snapshot.ms_per_frame, 3)} ms/帧（${formatNumber(native.snapshot.fps, 0)} FPS）`);
  console.log('');
  console.log(`反例：xterm 8KB 逐块 await = ${formatNumber(xtermSequential.mbPerS, 1)} MB/s——事件循环调度开销，不代表解析能力。`);
  console.log('');
  console.log(`结果已写入 ${RESULT_PATH}`);
  console.log('注意：解析器级对比，不含渲染与 IPC；不能据此推断端到端 UI 性能。');
}

main().catch((error) => {
  console.error(error.stack || error.message);
  process.exit(1);
});
